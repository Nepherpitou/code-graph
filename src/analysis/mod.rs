//! Analysis feature: discover files, start the LSP server, drive the graph
//! builder, compute metrics, and assemble the report.

mod options;
mod summaries;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use url::Url;

use crate::graph::{parse_symbols, Builder, RawSymbol};
use crate::languages::{adapter, detect_language, file_language_id, Adapter, LanguageId};
use crate::lsp::{self, LspClient};
use crate::metrics;
use crate::progress::{silent_callback, ProgressCallback, Reporter};
use crate::report::{GraphMeta, GraphReport};
use crate::source::{discover, DiscoverOptions, SourceFile};

pub use options::AnalyzeOptions;

async fn wait_until_ready(
    client: &LspClient,
    adapter: &Adapter,
    capabilities: &Value,
    builder: &Builder,
    reporter: &Reporter,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some((sample_name, sample_uri, sample_position)) = builder.readiness_sample() else {
        return warnings;
    };
    reporter.emit(
        "ready",
        format!("checking {} server readiness", adapter.id.as_str()),
    );
    let mut ready = false;
    if adapter.id == LanguageId::Rust {
        // rust-analyzer answers empty call hierarchy while the crate graph loads;
        // asking for one real item is the only reliable readiness check.
        for _ in 0..600 {
            match client
                .prepare_call_hierarchy_quick(&sample_uri, sample_position)
                .await
            {
                Ok(items) if !items.is_empty() => {
                    ready = true;
                    break;
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Let the reference index settle before crawling references.
        tokio::time::sleep(Duration::from_millis(250)).await;
        if !ready {
            warnings.push(
                "rust-analyzer did not answer call hierarchy within 60s; results may be incomplete"
                    .to_string(),
            );
        }
    } else if capabilities.get("workspaceSymbolProvider").is_some() {
        for _ in 0..3 {
            if let Ok(count) = client.workspace_symbol_count(&sample_name).await {
                if count > 0 {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // tsserver may answer an arbitrary workspace/symbol query with an empty
        // list even when the project is loaded; call hierarchy is the real check.
    }
    warnings
}

pub async fn analyze(options: AnalyzeOptions) -> Result<GraphReport> {
    analyze_with_progress(options, silent_callback()).await
}

pub async fn analyze_with_progress(
    options: AnalyzeOptions,
    callback: ProgressCallback,
) -> Result<GraphReport> {
    let reporter = Reporter::new(options.quiet, callback);
    let started = Instant::now();
    let root = std::fs::canonicalize(&options.root)
        .map_err(|error| anyhow!("cannot resolve {}: {error}", options.root.display()))?;
    let language = options
        .lang
        .or_else(|| detect_language(&root))
        .ok_or_else(|| anyhow!("could not detect a supported language; pass --lang"))?;
    let adapter = adapter(language);

    reporter.emit(
        "scan",
        format!(
            "language: {}; scanning {}",
            adapter.id.as_str(),
            root.display()
        ),
    );
    let discovered = discover(
        &root,
        adapter,
        &DiscoverOptions {
            excludes: options.exclude.clone(),
            ..DiscoverOptions::default()
        },
    );
    if discovered.is_empty() {
        return Err(anyhow!(
            "no {} source files found under {}",
            adapter.id.as_str(),
            root.display()
        ));
    }
    reporter.emit(
        "scan",
        format!("parsing {} files with tree-sitter", discovered.len()),
    );
    let source_file_count = discovered.len();
    let mut sources: HashMap<String, SourceFile> = HashMap::new();
    for source in discovered {
        sources.insert(source.rel.clone(), source);
    }

    let (command, server_args) = if let Some(custom) = &options.server_cmd {
        let mut parts = custom.split_whitespace();
        let command = parts.next().unwrap_or(custom).to_string();
        (command, parts.map(str::to_string).collect())
    } else {
        resolve_server_command(adapter.server, adapter.args, &root)
    };

    reporter.emit(
        "lsp",
        format!("starting LSP server: {command} {}", server_args.join(" ")),
    );
    let (client, initialize) = lsp::start(
        root.clone(),
        command.clone(),
        server_args.clone(),
        options.verbose,
    )
    .await
    .map_err(|error| anyhow!("{error}"))?;

    let capabilities = initialize
        .get("capabilities")
        .cloned()
        .unwrap_or(Value::Null);
    let mut local_warnings = Vec::new();
    if capabilities.get("callHierarchyProvider").is_none() {
        local_warnings.push(
            "LSP server does not advertise call hierarchy; call edges will be empty".to_string(),
        );
    }
    if capabilities.get("documentSymbolProvider").is_none() {
        local_warnings.push("LSP server does not advertise document symbols".to_string());
    }

    reporter.emit("lsp", format!("opening {} documents", sources.len()));
    let mut open_failures = 0usize;
    for source in sources.values() {
        let uri = Url::from_file_path(&source.abs)
            .map(|url| url.to_string())
            .unwrap_or_default();
        if client
            .open_document(&uri, file_language_id(&source.rel), &source.text)
            .await
            .is_err()
        {
            open_failures += 1;
        }
    }
    if open_failures > 0 {
        local_warnings.push(format!(
            "failed to open {open_failures} documents in the LSP server"
        ));
    }

    let mut files: Vec<(String, String)> = sources
        .values()
        .map(|source| {
            (
                source.rel.clone(),
                Url::from_file_path(&source.abs)
                    .map(|url| url.to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));

    reporter.emit("symbols", "collecting document symbols");
    let client_ref = &client;
    let mut symbol_results: Vec<(String, Result<Vec<RawSymbol>, String>)> =
        stream::iter(files.clone().into_iter().map(|(rel, uri)| async move {
            let result = client_ref
                .document_symbols(&uri)
                .await
                .map(|value| parse_symbols(&value))
                .map_err(|error| error.to_string());
            (rel, result)
        }))
        .buffer_unordered(8)
        .collect()
        .await;
    symbol_results.sort_by(|left, right| left.0.cmp(&right.0));

    let empty = symbol_results
        .iter()
        .filter(|(_, result)| {
            result
                .as_ref()
                .map(|symbols| symbols.is_empty())
                .unwrap_or(false)
        })
        .count();
    if empty == symbol_results.len() && symbol_results.len() > 1 {
        reporter.emit(
            "symbols",
            "all symbol queries were empty; retrying after server warm-up",
        );
        tokio::time::sleep(Duration::from_millis(800)).await;
        symbol_results = stream::iter(files.clone().into_iter().map(|(rel, uri)| async move {
            let result = client_ref
                .document_symbols(&uri)
                .await
                .map(|value| parse_symbols(&value))
                .map_err(|error| error.to_string());
            (rel, result)
        }))
        .buffer_unordered(8)
        .collect()
        .await;
        symbol_results.sort_by(|left, right| left.0.cmp(&right.0));
    }

    let symbol_count: usize = symbol_results
        .iter()
        .map(|(_, result)| result.as_ref().map(Vec::len).unwrap_or(0))
        .sum();

    let mut builder = Builder::new(
        root.clone(),
        adapter,
        sources,
        options.max_nodes,
        options.include_external,
        options.include_constants,
    );
    for (rel, result) in &symbol_results {
        match result {
            Ok(symbols) => {
                for symbol in symbols {
                    builder.add_symbol(symbol, rel, "", false, None);
                }
            }
            Err(error) => local_warnings.push(format!("documentSymbol failed for {rel}: {error}")),
        }
    }
    reporter.emit(
        "symbols",
        format!(
            "discovered {} nodes from {symbol_count} symbols",
            builder.node_count()
        ),
    );

    local_warnings
        .extend(wait_until_ready(&client, adapter, &capabilities, &builder, &reporter).await);

    reporter.emit("calls", "walking call hierarchy");
    builder.walk_calls(&client, &reporter).await;
    builder.add_callback_edges();

    if options.include_constants {
        builder
            .crawl_constants(
                &client,
                options.max_constants,
                adapter.id == LanguageId::Rust,
                &reporter,
            )
            .await;
    }

    if options.ref_fallback {
        builder
            .fallback_references(&client, adapter.id == LanguageId::Rust, &reporter)
            .await;
    }

    let output = builder.finish();
    let truncated = output.truncated;
    let mut nodes = output.nodes;
    let edges = output.edges;
    let mut warnings = output.warnings;

    reporter.emit("metrics", "computing metrics");
    let (stats, top) = metrics::compute(&mut nodes, &edges);
    let files = summaries::build_file_summaries(&nodes, &edges);

    warnings.extend(local_warnings);
    if truncated {
        warnings.push(format!(
            "node limit of {} reached; graph is partial",
            options.max_nodes
        ));
    }
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));

    let node_count = nodes.iter().filter(|node| !node.external).count();
    let edge_count = edges.iter().filter(|edge| !edge.external).count();
    let meta = GraphMeta {
        tool: "code-graph".to_string(),
        version: "0.1.0".to_string(),
        generated_at: now_iso8601(),
        root: root.to_string_lossy().to_string(),
        language: adapter.id.as_str().to_string(),
        server: format!("{command} {}", server_args.join(" "))
            .trim()
            .to_string(),
        duration_ms: started.elapsed().as_millis(),
        files: source_file_count,
        symbols: symbol_count,
        node_count,
        edge_count,
        truncated,
        options: json!({
            "lang": options.lang.map(|id| id.as_str()).unwrap_or("auto"),
            "includeExternal": options.include_external,
            "includeConstants": options.include_constants,
            "refFallback": options.ref_fallback,
            "maxNodes": options.max_nodes,
            "maxConstants": options.max_constants,
            "exclude": options.exclude.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>(),
        }),
    };

    client.stop().await;
    Ok(GraphReport {
        meta,
        nodes,
        edges,
        files,
        stats,
        top,
        warnings,
    })
}

fn resolve_server_command(command: &str, args: &[&str], root: &Path) -> (String, Vec<String>) {
    let base = command.split_whitespace().next().unwrap_or(command);
    let mut candidates = vec![root.join("node_modules").join(".bin").join(base)];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("node_modules").join(".bin").join(base));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..4 {
            let Some(current) = dir else { break };
            candidates.push(current.join("node_modules").join(".bin").join(base));
            dir = current.parent().map(Path::to_path_buf);
        }
    }
    let executable = candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.to_string_lossy().to_string())
        .unwrap_or_else(|| command.to_string());
    let args = if command.split_whitespace().count() > 1 {
        command
            .split_whitespace()
            .skip(1)
            .map(str::to_string)
            .collect()
    } else {
        args.iter().map(|arg| arg.to_string()).collect()
    };
    (executable, args)
}

pub(crate) fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Convert to a UTC timestamp without pulling in chrono.
    let days = secs / 86_400;
    let seconds_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}
