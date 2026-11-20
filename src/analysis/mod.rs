//! Analysis feature: discover files, start the LSP server, drive the graph
//! builder, compute metrics, and assemble the report.

mod options;
mod summaries;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use url::Url;

use crate::graph::{parse_symbols, Builder, RawSymbol};
use crate::languages::{
    adapter, detect_language, file_language_id, Adapter, LanguageId, Readiness, ReadyCtx,
    ServerKind,
};
use crate::lsp::{self, LspClient};
use crate::metrics;
use crate::progress::{silent_callback, ProgressCallback, Reporter};
use crate::report::{GraphMeta, GraphReport};
use crate::source::{discover, DiscoverOptions, SourceFile};

pub use options::AnalyzeOptions;

async fn collect_symbols(
    client: &LspClient,
    files: &[(String, String)],
    reporter: &Reporter,
) -> Vec<(String, Result<Vec<RawSymbol>, String>)> {
    let query = |files: Vec<(String, String)>| async move {
        stream::iter(files.into_iter().map(|(rel, uri)| async move {
            let result = client
                .document_symbols(&uri)
                .await
                .map(|value| parse_symbols(&value))
                .map_err(|error| error.to_string());
            (rel, result)
        }))
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await
    };

    let mut results = query(files.to_vec()).await;
    results.sort_by(|left, right| left.0.cmp(&right.0));
    let empty = results
        .iter()
        .filter(|(_, result)| {
            result
                .as_ref()
                .map(|symbols| symbols.is_empty())
                .unwrap_or(false)
        })
        .count();
    if empty == results.len() && results.len() > 1 {
        reporter.emit(
            "symbols",
            "all symbol queries were empty; retrying after server warm-up",
        );
        tokio::time::sleep(Duration::from_millis(800)).await;
        results = query(files.to_vec()).await;
        results.sort_by(|left, right| left.0.cmp(&right.0));
    }
    results
}

fn discover_sources(
    root: &Path,
    adapter: &Adapter,
    options: &AnalyzeOptions,
    reporter: &Reporter,
) -> Result<(HashMap<String, SourceFile>, usize)> {
    reporter.emit(
        "scan",
        format!(
            "language: {}; scanning {}",
            adapter.id.as_str(),
            root.display()
        ),
    );
    let discovered = discover(
        root,
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
    let count = discovered.len();
    let sources = discovered
        .into_iter()
        .map(|source| (source.rel.clone(), source))
        .collect();
    Ok((sources, count))
}

async fn run_builder(
    builder: &mut Builder,
    client: &LspClient,
    adapter: &Adapter,
    options: &AnalyzeOptions,
    reporter: &Reporter,
) {
    reporter.emit("calls", "walking call hierarchy");
    builder.walk_calls(client, reporter).await;
    builder.add_callback_edges();

    if options.include_constants {
        builder
            .crawl_constants(
                client,
                options.max_constants,
                adapter.id == LanguageId::Rust,
                reporter,
            )
            .await;
    }
    if options.ref_fallback {
        builder
            .fallback_references(client, adapter.id == LanguageId::Rust, reporter)
            .await;
    }
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

    let (sources, source_file_count) = discover_sources(&root, adapter, &options, &reporter)?;

    let (command, server_args) = if let Some(custom) = &options.server_cmd {
        let mut parts = custom.split_whitespace();
        let command = parts.next().unwrap_or(custom).to_string();
        (command, parts.map(str::to_string).collect())
    } else if adapter.id == LanguageId::TypeScript {
        resolve_typescript_server(&root).unwrap_or_else(|| {
            resolve_server_command(adapter.server.command(), adapter.server.args(), &root)
        })
    } else {
        resolve_server_command(adapter.server.command(), adapter.server.args(), &root)
    };
    // The behaviour follows the program that actually starts, not `--lang`:
    // `--server-cmd "tsgo --lsp --stdio"` on TypeScript keeps tsgo's policy.
    // A custom command may be wrapped (`npx … tsgo`, a proxy script), so the
    // whole line is what names the server.
    let server_kind = ServerKind::resolve(options.server_cmd.as_deref().unwrap_or(&command));

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
    let symbol_results = collect_symbols(&client, &files, &reporter).await;
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

    let samples = builder.readiness_samples();
    let workspace = adapter
        .root_markers
        .iter()
        .any(|marker| root.join(marker).exists());
    let readiness = server_kind
        .wait_ready(ReadyCtx {
            client: &client,
            samples: &samples,
            reporter: &reporter,
            budget: Duration::from_secs(60),
            workspace,
        })
        .await;
    if let Readiness::TimedOut { pending } = readiness {
        local_warnings.push(format!(
            "{} did not finish within 60s ({}); results may be incomplete",
            adapter.id.as_str(),
            pending.join(", ")
        ));
    }

    run_builder(&mut builder, &client, adapter, &options, &reporter).await;

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

fn resolve_typescript_server(root: &Path) -> Option<(String, Vec<String>)> {
    // Prefer the official Go implementation. `tsrs` is faster but beta, so it
    // is only selected when `tsgo` is absent.
    for (name, args) in [
        ("tsgo", ["--lsp", "--stdio"]),
        ("tsrs", ["--lsp", "-stdio"]),
    ] {
        if let Some(path) = find_executable(name, root) {
            return Some((
                path.to_string_lossy().to_string(),
                args.iter().map(|arg| arg.to_string()).collect(),
            ));
        }
    }
    None
}

fn find_executable(name: &str, root: &Path) -> Option<PathBuf> {
    let mut executors: Vec<PathBuf> = vec![root.join("node_modules/.bin/").join(name)];
    if let Ok(cwd) = std::env::current_dir() {
        executors.push(cwd.join("node_modules/.bin/").join(name));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..4 {
            let Some(current) = dir else { break };
            executors.push(current.join("node_modules/.bin/").join(name));
            dir = current.parent().map(Path::to_path_buf);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            executors.push(dir.join(name));
        }
    }
    executors.into_iter().find(|candidate| candidate.is_file())
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
