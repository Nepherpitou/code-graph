//! Human-facing architecture audit over a persisted snapshot.
//!
//! This command deliberately stays below the smell layer: it ranks existing
//! metrics, groups factual graph structures (SCCs), and lists existing
//! dead-code candidates. It does not invent thresholds or verdicts.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::analysis::{analyze, AnalyzeOptions};
use crate::graph::{EdgeKind, GraphEdge, GraphNode, NodeKind};
use crate::query::QueryService;
use crate::store::{GraphSummary, Store};

use super::args::{
    AuditArgs, AuditCommand, AuditCommon, AuditCyclesArgs, AuditDeadArgs,
    AuditFocusArgs, AuditFormat, AuditMetricsArgs, AuditOrder, AuditSearchArgs, AuditTopArgs,
};
use super::audit_diff;

pub(super) async fn run(args: AuditArgs) -> Result<ExitCode> {
    let mut common = args.common;
    if let Some(path) = args.path {
        common.root = path;
    }

    match args.command {
        None => overview(common).await?,
        Some(AuditCommand::Top(args)) => top(args).await?,
        Some(AuditCommand::Focus(args)) => focus(args).await?,
        Some(AuditCommand::Cycles(args)) => cycles(args).await?,
        Some(AuditCommand::DeadCode(args)) => dead_code(args).await?,
        Some(AuditCommand::Search(args)) => search(args).await?,
        Some(AuditCommand::Metrics(args)) => metrics(args)?,
        Some(AuditCommand::Diff(args)) => return audit_diff::run(args).await,
    }
    Ok(ExitCode::SUCCESS)
}

struct AuditContext {
    store: Store,
    graph_id: i64,
    summary: GraphSummary,
}

pub(super) fn analysis_options(root: &Path) -> AnalyzeOptions {
    AnalyzeOptions {
        root: root.to_path_buf(),
        lang: None,
        server_cmd: None,
        include_external: false,
        include_constants: true,
        ref_fallback: true,
        max_nodes: 20_000,
        max_constants: 500,
        exclude: Vec::new(),
        verbose: false,
        quiet: true,
    }
}

async fn open_context(common: &AuditCommon) -> Result<AuditContext> {
    let root = common.root.clone();
    let db = common
        .db
        .clone()
        .unwrap_or_else(|| root.join(".code-graph").join("code-graph.db"));

    if let Some(selector) = &common.snapshot {
        if !db.exists() {
            bail!(
                "snapshot database not found: {}\nRun `code-graph audit {}` first.",
                db.display(),
                root.display()
            );
        }
        let store = Store::open(&db).await?;
        let summary = resolve_snapshot(&store, selector).await?;
        return Ok(AuditContext {
            store,
            graph_id: summary.id,
            summary,
        });
    }

    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }

    if !common.refresh && db.exists() {
        let store = Store::open(&db).await?;
        if let Some(summary) = store.latest_graph().await? {
            return Ok(AuditContext {
                graph_id: summary.id,
                summary,
                store,
            });
        }
    }

    eprintln!("[code-graph] analyzing {} ...", root.display());
    let report = analyze(analysis_options(&root)).await?;
    let store = Store::open(&db).await?;
    let graph_id = store.insert_report(&report).await?;
    let summary = store
        .graph(graph_id)
        .await?
        .ok_or_else(|| anyhow!("snapshot #{graph_id} was stored but cannot be read back"))?;
    eprintln!(
        "[code-graph] stored snapshot #{graph_id} in {} ({} ms)",
        db.display(),
        report.meta.duration_ms
    );
    Ok(AuditContext {
        store,
        graph_id,
        summary,
    })
}

async fn resolve_snapshot(store: &Store, selector: &str) -> Result<GraphSummary> {
    if selector.eq_ignore_ascii_case("latest") {
        return store
            .latest_graph()
            .await?
            .ok_or_else(|| anyhow!("no snapshots in database"));
    }
    let id: i64 = selector
        .parse()
        .map_err(|_| anyhow!("snapshot must be a numeric id or \"latest\", got {selector:?}"))?;
    if let Some(summary) = store.graph(id).await? {
        return Ok(summary);
    }
    let available = store
        .graphs()
        .await?
        .iter()
        .map(|graph| graph.id.to_string())
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    if available.is_empty() {
        bail!("snapshot #{id} not found; database has no snapshots")
    }
    bail!("snapshot #{id} not found; available: {available}")
}

async fn load_nodes(context: &AuditContext) -> Result<Vec<GraphNode>> {
    context.store.all_nodes(context.graph_id).await
}

async fn load_edges(context: &AuditContext) -> Result<Vec<GraphEdge>> {
    context.store.all_edges(context.graph_id).await
}

pub(super) fn snapshot_json(summary: &GraphSummary) -> Value {
    json!({
        "id": summary.id,
        "createdAt": summary.created_at,
        "root": summary.meta.root,
        "language": summary.meta.language,
        "generatedAt": summary.meta.generated_at,
        "files": summary.meta.files,
        "symbols": summary.meta.symbols,
        "truncated": summary.meta.truncated,
    })
}

pub(super) fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

fn text_header(summary: &GraphSummary) {
    let project = Path::new(&summary.meta.root)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&summary.meta.root);
    println!(
        "{} · {} · snapshot #{}",
        project, summary.meta.language, summary.id
    );
    println!(
        "{} files · {} nodes ({} callables, {} constants) · {} call edges · {} data edges",
        summary.meta.files,
        summary.stats.nodes,
        summary.stats.functions,
        summary.stats.constants,
        summary.stats.edges,
        summary.stats.data_edges
    );
    println!("root: {}", summary.meta.root);
}

pub(super) fn fit(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut clipped = value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    clipped.push('…');
    clipped
}

pub(super) fn pad(value: &str, width: usize) -> String {
    let value = fit(value, width);
    format!("{value:<width$}")
}

pub(super) fn metric(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else if value.abs() < 10.0 {
        format!("{value:.3}")
    } else if value.abs() < 100.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.1}")
    }
}

pub(super) fn metric_json(value: f64) -> f64 {
    let scale = if value.fract().abs() < f64::EPSILON {
        1.0
    } else if value.abs() < 10.0 {
        1000.0
    } else if value.abs() < 100.0 {
        100.0
    } else {
        10.0
    };
    (value * scale).round() / scale
}

fn node_brief(node: &GraphNode) -> Value {
    json!({
        "id": node.id,
        "name": node.name,
        "qname": node.qname,
        "kind": node.kind.as_str(),
        "file": node.file,
        "line": node.line,
        "endLine": node.end_line,
        "loc": node.loc,
    })
}

fn node_row(node: &GraphNode, value: f64) -> Value {
    json!({
        "id": node.id,
        "name": node.name,
        "qname": node.qname,
        "kind": node.kind.as_str(),
        "file": node.file,
        "line": node.line,
        "value": metric_json(value),
        "fanIn": node.metrics.fan_in,
        "fanOut": node.metrics.fan_out,
        "callFanIn": node.metrics.call_fan_in,
        "callFanOut": node.metrics.call_fan_out,
        "dataFanIn": node.metrics.data_fan_in,
        "dataFanOut": node.metrics.data_fan_out,
        "transitiveFanIn": node.metrics.transitive_fan_in,
        "transitiveFanOut": node.metrics.transitive_fan_out,
        "callIn": node.metrics.call_in,
        "callOut": node.metrics.call_out,
        "moduleRefs": node.metrics.module_refs,
        "cyclomatic": node.cyclomatic,
        "loc": node.loc,
        "sloc": node.sloc,
        "params": node.params,
        "hotspot": metric_json(node.metrics.hotspot),
        "ifc": metric_json(node.metrics.ifc),
        "betweenness": metric_json(node.metrics.betweenness),
        "instability": metric_json(node.metrics.instability),
        "resultIgnoredRatio": metric_json(node.metrics.result_ignored_ratio),
        "scc": node.metrics.scc,
        "sccSize": node.metrics.scc_size,
        "inCycle": node.metrics.in_cycle,
        "deadCodeCandidate": node.metrics.dead_code_candidate,
        "refactorPriority": metric_json(node.metrics.refactor_priority),
    })
}

fn dead_candidates<'a>(nodes: &'a [GraphNode], file: Option<&str>) -> Vec<&'a GraphNode> {
    let mut candidates: Vec<&GraphNode> = nodes
        .iter()
        .filter(|node| node.metrics.dead_code_candidate)
        .filter(|node| file.is_none_or(|file| node.file.contains(file)))
        .collect();
    candidates.sort_by(|left, right| {
        right
            .loc
            .cmp(&left.loc)
            .then_with(|| right.metrics.call_out.cmp(&left.metrics.call_out))
            .then_with(|| left.qname.cmp(&right.qname))
    });
    candidates
}

fn cycle_groups(nodes: &[GraphNode], edges: &[GraphEdge], min_size: usize) -> Vec<CycleGroup> {
    let mut by_scc: BTreeMap<i64, Vec<&GraphNode>> = BTreeMap::new();
    for node in nodes {
        if node.metrics.in_cycle && node.metrics.scc_size >= min_size as u32 {
            by_scc.entry(node.metrics.scc).or_default().push(node);
        }
    }
    let mut groups: Vec<CycleGroup> = by_scc
        .into_iter()
        .filter_map(|(scc, members)| {
            if members.len() < min_size {
                return None;
            }
            let mut members: Vec<GraphNode> = members.into_iter().cloned().collect();
            members.sort_by(|left, right| left.qname.cmp(&right.qname));
            let ids: std::collections::HashSet<&str> =
                members.iter().map(|node| node.id.as_str()).collect();
            let internal: Vec<GraphEdge> = edges
                .iter()
                .filter(|edge| {
                    edge.kind == EdgeKind::Call
                        && ids.contains(edge.source.as_str())
                        && ids.contains(edge.target.as_str())
                })
                .cloned()
                .collect();
            Some(CycleGroup {
                scc,
                nodes: members,
                edges: internal,
            })
        })
        .collect();
    groups.sort_by(|left, right| {
        right
            .nodes
            .len()
            .cmp(&left.nodes.len())
            .then_with(|| left.nodes[0].qname.cmp(&right.nodes[0].qname))
            .then_with(|| left.scc.cmp(&right.scc))
    });
    groups
}

#[derive(Debug)]
struct CycleGroup {
    scc: i64,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

fn cycle_json(group: &CycleGroup) -> Value {
    json!({
        "scc": group.scc,
        "size": group.nodes.len(),
        "nodes": group.nodes.iter().map(node_brief).collect::<Vec<_>>(),
        "edges": group.edges.iter().map(|edge| json!({
            "source": edge.source,
            "target": edge.target,
            "sites": edge.sites,
        })).collect::<Vec<_>>(),
    })
}

fn cycle_label(group: &CycleGroup) -> String {
    group
        .nodes
        .iter()
        .map(|node| node.qname.as_str())
        .collect::<Vec<_>>()
        .join(" ↔ ")
}

fn print_cycle_edges(group: &CycleGroup) {
    if group.edges.is_empty() {
        return;
    }
    let by_id: HashMap<&str, &GraphNode> = group
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    println!("  Internal calls:");
    for edge in group.edges.iter().take(4) {
        let source = by_id
            .get(edge.source.as_str())
            .map(|node| node.qname.as_str())
            .unwrap_or(&edge.source);
        let target = by_id
            .get(edge.target.as_str())
            .map(|node| node.qname.as_str())
            .unwrap_or(&edge.target);
        println!("    {} → {} ({} sites)", source, target, edge.sites);
    }
    if group.edges.len() > 4 {
        println!("    … {} more edges", group.edges.len() - 4);
    }
}

async fn overview(common: AuditCommon) -> Result<()> {
    let context = open_context(&common).await?;
    let nodes = load_nodes(&context).await?;
    let cycles = cycle_groups(&nodes, &[], 2);
    let dead = dead_candidates(&nodes, None);

    match common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.overview",
                "snapshot": snapshot_json(&context.summary),
                "stats": &context.summary.stats,
                "top": &context.summary.top,
                "cycles": cycles.iter().take(3).map(cycle_json).collect::<Vec<_>>(),
                "deadCode": dead.iter().take(5).map(|node| node_brief(node)).collect::<Vec<_>>(),
                "warnings": &context.summary.warnings,
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            println!("Architecture");
            println!(
                "  cycles              {} (largest {})",
                context.summary.stats.cycles, context.summary.stats.largest_cycle
            );
            println!(
                "  dead-code candidates {} (heuristic, not proof)",
                context.summary.stats.dead_code
            );
            println!("  isolated callables  {}", context.summary.stats.isolated);
            println!(
                "  cross-file calls    {} ({:.1}%)",
                context.summary.stats.cross_file_edges,
                context.summary.stats.cross_file_ratio * 100.0
            );
            println!(
                "  ignored results     {:.1}%",
                context.summary.stats.result_ignored_ratio * 100.0
            );
            if context.summary.meta.truncated {
                println!("  warning             graph is truncated");
            }

            println!();
            println!("Top by hotspot score");
            let hotspot_rows = context
                .summary
                .top
                .hotspot
                .iter()
                .filter(|row| row.value > 0.0)
                .take(5)
                .collect::<Vec<_>>();
            if hotspot_rows.is_empty() {
                println!("  no ranked callables");
            } else {
                println!(
                    "  {:>3}  {} {} hotspot",
                    "#",
                    pad("symbol", 34),
                    pad("file", 40)
                );
                for (index, row) in hotspot_rows.iter().enumerate() {
                    println!(
                        "  {:>3}  {} {} {:>7}",
                        index + 1,
                        pad(&row.name, 34),
                        pad(&row.file, 40),
                        metric(row.value)
                    );
                }
            }

            println!();
            println!("Cycles");
            if cycles.is_empty() {
                println!("  none");
            } else {
                for group in cycles.iter().take(3) {
                    println!(
                        "  {}  ({} nodes · SCC {})",
                        cycle_label(group),
                        group.nodes.len(),
                        group.scc
                    );
                }
            }

            println!();
            println!("Dead-code candidates (heuristic, not proof)");
            if dead.is_empty() {
                println!("  none");
            } else {
                for node in dead.iter().take(5) {
                    println!(
                        "  {}  {}  loc {}",
                        pad(&node.qname, 34),
                        pad(&node.file, 40),
                        node.loc
                    );
                }
                if dead.len() > 5 {
                    println!("  … {} more", dead.len() - 5);
                }
            }

            if !context.summary.warnings.is_empty() {
                println!();
                println!("Warnings");
                for warning in &context.summary.warnings {
                    println!("  - {warning}");
                }
            }

            println!();
            println!("Next");
            println!("  code-graph audit top --order-by complexity");
            println!("  code-graph audit focus <symbol>");
            println!("  code-graph audit cycles");
        }
    }
    Ok(())
}

impl AuditOrder {
    pub(super) fn label(self) -> &'static str {
        match self {
            AuditOrder::Hotspot => "hotspot",
            AuditOrder::Priority => "priority",
            AuditOrder::Complexity => "cyclomatic",
            AuditOrder::BlastRadius => "transitive-in",
            AuditOrder::FanIn => "fan-in",
            AuditOrder::FanOut => "fan-out",
            AuditOrder::CallFanIn => "call-fan-in",
            AuditOrder::CallFanOut => "call-fan-out",
            AuditOrder::DataFanIn => "data-fan-in",
            AuditOrder::DataFanOut => "data-fan-out",
            AuditOrder::Ifc => "ifc",
            AuditOrder::Betweenness => "betweenness",
            AuditOrder::Loc => "loc",
            AuditOrder::Sloc => "sloc",
            AuditOrder::Params => "params",
        }
    }

    pub(super) fn value(self, node: &GraphNode) -> f64 {
        match self {
            AuditOrder::Hotspot => node.metrics.hotspot,
            AuditOrder::Priority => node.metrics.refactor_priority,
            AuditOrder::Complexity => node.cyclomatic as f64,
            AuditOrder::BlastRadius => node.metrics.transitive_fan_in as f64,
            AuditOrder::FanIn => node.metrics.fan_in as f64,
            AuditOrder::FanOut => node.metrics.fan_out as f64,
            AuditOrder::CallFanIn => node.metrics.call_fan_in as f64,
            AuditOrder::CallFanOut => node.metrics.call_fan_out as f64,
            AuditOrder::DataFanIn => node.metrics.data_fan_in as f64,
            AuditOrder::DataFanOut => node.metrics.data_fan_out as f64,
            AuditOrder::Ifc => node.metrics.ifc,
            AuditOrder::Betweenness => node.metrics.betweenness,
            AuditOrder::Loc => node.loc as f64,
            AuditOrder::Sloc => node.sloc as f64,
            AuditOrder::Params => node.params as f64,
        }
    }
}

pub(super) fn parse_kind(value: &str) -> Result<NodeKind> {
    match value {
        "function" | "fn" => Ok(NodeKind::Function),
        "method" => Ok(NodeKind::Method),
        "constructor" | "ctor" => Ok(NodeKind::Constructor),
        "constant" | "const" => Ok(NodeKind::Constant),
        "variable" | "var" => Ok(NodeKind::Variable),
        "other" => Ok(NodeKind::Other),
        other => bail!(
            "unknown kind {other:?}; expected function, method, constructor, constant, variable, or other"
        ),
    }
}

async fn top(args: AuditTopArgs) -> Result<()> {
    let context = open_context(&args.common).await?;
    let nodes = load_nodes(&context).await?;
    let kind = args.kind.as_deref().map(parse_kind).transpose()?;
    let limit = args.limit.clamp(1, 200);
    let default_data_rank = kind.is_none() && matches!(args.order_by, AuditOrder::DataFanIn);

    let mut rows: Vec<&GraphNode> = nodes
        .iter()
        .filter(|node| !node.external)
        .filter(|node| match kind {
            Some(kind) => node.kind == kind,
            None if default_data_rank => node.kind.is_data(),
            None => node.kind.is_callable(),
        })
        .filter(|node| {
            args.file
                .as_deref()
                .is_none_or(|file| node.file.contains(file))
        })
        .filter(|node| args.min.is_none_or(|min| args.order_by.value(node) >= min))
        .collect();

    rows.sort_by(|left, right| {
        let left_value = args.order_by.value(left);
        let right_value = args.order_by.value(right);
        let ordering = if args.ascending {
            left_value.partial_cmp(&right_value)
        } else {
            right_value.partial_cmp(&left_value)
        };
        ordering
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.qname.cmp(&right.qname))
            .then_with(|| left.id.cmp(&right.id))
    });
    rows.truncate(limit);

    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.top",
                "snapshot": snapshot_json(&context.summary),
                "orderBy": args.order_by.label(),
                "rows": rows.iter()
                    .map(|node| node_row(node, args.order_by.value(node)))
                    .collect::<Vec<_>>(),
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            if rows.is_empty() {
                println!("No callables match this filter.");
                return Ok(());
            }
            println!(
                "Top {} by {} ({})",
                rows.len(),
                args.order_by.label(),
                if args.ascending {
                    "ascending"
                } else {
                    "descending"
                }
            );
            if args.min.is_some() {
                println!(
                    "  filter: {} ≥ {}",
                    args.order_by.label(),
                    args.min.unwrap_or_default()
                );
            }
            let show_in = !matches!(
                args.order_by,
                AuditOrder::FanIn | AuditOrder::CallFanIn | AuditOrder::DataFanIn
            );
            let show_out = !matches!(
                args.order_by,
                AuditOrder::FanOut | AuditOrder::CallFanOut | AuditOrder::DataFanOut
            );
            let show_cyclo = !matches!(args.order_by, AuditOrder::Complexity);
            let show_loc = !matches!(args.order_by, AuditOrder::Loc | AuditOrder::Sloc);

            print!(
                "  {:>3}  {} {} {:>8}",
                "#",
                pad("symbol", 32),
                pad("file", 38),
                args.order_by.label()
            );
            if show_in {
                print!(" {:>5}", "in");
            }
            if show_out {
                print!(" {:>5}", "out");
            }
            if show_cyclo {
                print!(" {:>6}", "cyclo");
            }
            if show_loc {
                print!(" {:>5}", "loc");
            }
            println!();
            for (index, node) in rows.iter().enumerate() {
                print!(
                    "  {:>3}  {} {} {:>8}",
                    index + 1,
                    pad(&node.qname, 32),
                    pad(&node.file, 38),
                    metric(args.order_by.value(node))
                );
                if show_in {
                    print!(" {:>5}", node.metrics.fan_in);
                }
                if show_out {
                    print!(" {:>5}", node.metrics.fan_out);
                }
                if show_cyclo {
                    print!(" {:>6}", node.cyclomatic);
                }
                if show_loc {
                    print!(" {:>5}", node.loc);
                }
                println!();
            }
            println!();
            println!("  metric: {}", args.order_by.label());
            println!("  next:   code-graph audit focus <symbol>");
        }
    }
    Ok(())
}

fn resolve_focus(args: &AuditFocusArgs, nodes: &[GraphNode]) -> Result<GraphNode> {
    let file = args.file.as_deref();
    let line = args.line;
    let eligible = |node: &GraphNode| {
        file.is_none_or(|file| node.file.contains(file))
            && line.is_none_or(|line| node.line <= line && line <= node.end_line)
    };

    if let Some(id) = args.id.as_deref() {
        return nodes
            .iter()
            .find(|node| node.id == id && eligible(node))
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "no symbol with id {id:?}{}",
                    file.map(|file| format!(" in {file}")).unwrap_or_default()
                )
            });
    }
    if let Some(qname) = args.qname.as_deref() {
        return nodes
            .iter()
            .find(|node| node.qname == qname && eligible(node))
            .cloned()
            .ok_or_else(|| anyhow!("no symbol with qname {qname:?}"));
    }

    let selector = args
        .selector
        .as_deref()
        .ok_or_else(|| anyhow!("provide a symbol, or use --id / --qname"))?;
    let mut matches: Vec<&GraphNode> = nodes
        .iter()
        .filter(|node| eligible(node))
        .filter(|node| node.id == selector || node.qname == selector || node.name == selector)
        .collect();

    if matches.is_empty() {
        matches = nodes
            .iter()
            .filter(|node| eligible(node))
            .filter(|node| node.qname.ends_with(selector))
            .collect();
    }
    match matches.as_slice() {
        [] => bail!("no symbol matches {selector:?}"),
        [node] => Ok((*node).clone()),
        many => {
            let mut candidates = many
                .iter()
                .take(8)
                .map(|node| format!("  {}  {}:{}", node.qname, node.file, node.line))
                .collect::<Vec<_>>()
                .join("\n");
            if many.len() > 8 {
                candidates.push_str(&format!("\n  … {} more", many.len() - 8));
            }
            bail!(
                "multiple symbols match {selector:?}:\n{candidates}\nuse --file, --line, --qname, or --id"
            )
        }
    }
}

#[derive(Clone)]
struct Relation {
    node: GraphNode,
    sites: u32,
}

fn relations(nodes: &[GraphNode], edges: &[GraphEdge], id: &str, incoming: bool) -> Vec<Relation> {
    let by_id: HashMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut grouped: HashMap<String, Relation> = HashMap::new();
    for edge in edges {
        if edge.kind != EdgeKind::Call {
            continue;
        }
        let other_id = if incoming {
            if edge.target != id {
                continue;
            }
            &edge.source
        } else {
            if edge.source != id {
                continue;
            }
            &edge.target
        };
        let Some(other) = by_id.get(other_id.as_str()) else {
            continue;
        };
        grouped
            .entry(other.id.clone())
            .and_modify(|relation| relation.sites += edge.sites)
            .or_insert_with(|| Relation {
                node: (*other).clone(),
                sites: edge.sites,
            });
    }
    let mut relations: Vec<Relation> = grouped.into_values().collect();
    relations.sort_by(|left, right| {
        right
            .sites
            .cmp(&left.sites)
            .then_with(|| left.node.qname.cmp(&right.node.qname))
    });
    relations
}

fn relation_json(relation: &Relation) -> Value {
    json!({
        "id": relation.node.id,
        "qname": relation.node.qname,
        "file": relation.node.file,
        "line": relation.node.line,
        "kind": relation.node.kind.as_str(),
        "sites": relation.sites,
        "fanIn": relation.node.metrics.fan_in,
        "fanOut": relation.node.metrics.fan_out,
        "hotspot": metric_json(relation.node.metrics.hotspot),
    })
}

fn print_relations(label: &str, relations: &[Relation], limit: usize) {
    println!("{} ({})", label, relations.len());
    if relations.is_empty() {
        println!("  none");
        return;
    }
    for relation in relations.iter().take(limit) {
        println!(
            "  {} {} {:>3} sites",
            pad(&relation.node.qname, 32),
            pad(&relation.node.file, 38),
            relation.sites
        );
    }
    if relations.len() > limit {
        println!("  … {} more", relations.len() - limit);
    }
}

fn route_name(route: &[String], nodes: &HashMap<&str, &GraphNode>) -> Vec<String> {
    route
        .iter()
        .map(|id| {
            nodes
                .get(id.as_str())
                .map(|node| node.qname.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect()
}

async fn focus(args: AuditFocusArgs) -> Result<()> {
    let context = open_context(&args.common).await?;
    let nodes = load_nodes(&context).await?;
    let node = resolve_focus(&args, &nodes)?;
    let edges = load_edges(&context).await?;
    let callers = relations(&nodes, &edges, &node.id, true);
    let callees = relations(&nodes, &edges, &node.id, false);
    let limit = args.limit.clamp(1, 50);

    let query = QueryService::new(context.store.clone());
    let focus_slice = query.focus_slice(context.graph_id, &node.id, 180).await?;
    let by_id: HashMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let route = route_name(&focus_slice.route, &by_id);

    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.focus",
                "snapshot": snapshot_json(&context.summary),
                "data": {
                    "node": node_row(&node, node.metrics.hotspot),
                    "metrics": &node.metrics,
                    "callers": callers.iter().take(limit).map(relation_json).collect::<Vec<_>>(),
                    "callees": callees.iter().take(limit).map(relation_json).collect::<Vec<_>>(),
                    "route": {
                        "entry": focus_slice.entry,
                        "exit": focus_slice.exit,
                        "ids": focus_slice.route,
                        "path": route,
                    },
                },
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            println!("{}", node.qname);
            println!(
                "{}:{} · {} · {}",
                node.file,
                node.line,
                node.kind.as_str(),
                if node.exported {
                    "exported"
                } else {
                    "internal"
                }
            );
            if node.metrics.in_cycle {
                println!(
                    "cycle: SCC {} ({} callables)",
                    node.metrics.scc, node.metrics.scc_size
                );
            }
            println!();
            println!("Metrics");
            println!(
                "  priority       {:>8}",
                metric(node.metrics.refactor_priority)
            );
            println!("  hotspot        {:>8}", metric(node.metrics.hotspot));
            println!("  fan-in         {:>8}", node.metrics.fan_in);
            println!("  fan-out        {:>8}", node.metrics.fan_out);
            println!(
                "  call-in/out    {:>8}",
                format!("{}/{}", node.metrics.call_fan_in, node.metrics.call_fan_out)
            );
            println!(
                "  data-in/out    {:>8}",
                format!("{}/{}", node.metrics.data_fan_in, node.metrics.data_fan_out)
            );
            println!("  transitive-in  {:>8}", node.metrics.transitive_fan_in);
            println!("  transitive-out {:>8}", node.metrics.transitive_fan_out);
            println!("  cyclomatic     {:>8}", node.cyclomatic);
            println!("  loc            {:>8}", node.loc);
            println!("  sloc           {:>8}", node.sloc);
            println!("  params         {:>8}", node.params);
            println!("  ifc            {:>8}", metric(node.metrics.ifc));
            println!("  betweenness    {:>8}", metric(node.metrics.betweenness));
            println!("  instability    {:>8}", metric(node.metrics.instability));
            println!(
                "  ignored result {:>7.1}%",
                node.metrics.result_ignored_ratio * 100.0
            );
            println!();
            print_relations("Callers", &callers, limit);
            println!();
            print_relations("Callees", &callees, limit);
            println!();
            println!("Route");
            if focus_slice.entry.is_none() && focus_slice.exit.is_none() {
                println!("  no entry/exit route found");
            } else {
                if let Some(entry) = focus_slice.entry.as_deref() {
                    let entry = by_id
                        .get(entry)
                        .map(|node| node.qname.as_str())
                        .unwrap_or(entry);
                    println!("  entry  {entry}");
                }
                if let Some(exit) = focus_slice.exit.as_deref() {
                    let exit = by_id
                        .get(exit)
                        .map(|node| node.qname.as_str())
                        .unwrap_or(exit);
                    println!("  exit   {exit}");
                }
                if route.is_empty() {
                    println!("  path   (route not included)");
                } else {
                    println!("  path   {}", route.join(" → "));
                }
            }
            println!();
            println!("Next");
            println!(
                "  code-graph audit top --order-by blast-radius --file {}",
                node.file
            );
            println!("  code-graph audit cycles");
        }
    }
    Ok(())
}

async fn cycles(args: AuditCyclesArgs) -> Result<()> {
    let context = open_context(&args.common).await?;
    let nodes = load_nodes(&context).await?;
    let edges = load_edges(&context).await?;
    let groups = cycle_groups(&nodes, &edges, args.min_size.max(2));
    let limit = args.limit.clamp(1, 100);
    let shown = &groups[..groups.len().min(limit)];

    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.cycles",
                "snapshot": snapshot_json(&context.summary),
                "cycles": shown.iter().map(cycle_json).collect::<Vec<_>>(),
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            if shown.is_empty() {
                println!("No cycles found.");
            } else {
                println!("Cycles · {} found", groups.len());
                for (index, group) in shown.iter().enumerate() {
                    println!(
                        "\nCycle {} · {} nodes · SCC {}",
                        index + 1,
                        group.nodes.len(),
                        group.scc
                    );
                    println!("  {}", cycle_label(group));
                    for node in &group.nodes {
                        println!(
                            "    {}  {}:{}",
                            pad(&node.qname, 32),
                            pad(&node.file, 38),
                            node.line
                        );
                    }
                    print_cycle_edges(group);
                }
            }
            println!();
            println!("Cycles are SCCs with more than one callable; not every cycle is a bug.");
        }
    }
    Ok(())
}

async fn dead_code(args: AuditDeadArgs) -> Result<()> {
    let context = open_context(&args.common).await?;
    let nodes = load_nodes(&context).await?;
    let candidates = dead_candidates(&nodes, args.file.as_deref());
    let limit = args.limit.clamp(1, 200);
    let shown: Vec<&GraphNode> = candidates.iter().take(limit).copied().collect();

    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.dead_code",
                "snapshot": snapshot_json(&context.summary),
                "candidates": shown.iter().map(|node| node_row(node, node.loc as f64)).collect::<Vec<_>>(),
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            println!("Dead-code candidates ({})", candidates.len());
            if shown.is_empty() {
                println!("  none");
            } else {
                println!(
                    "  {:>3}  {} {} {:>5} {:>5} {:>6}",
                    "#",
                    pad("symbol", 32),
                    pad("file", 38),
                    "loc",
                    "in",
                    "refs"
                );
                for (index, node) in shown.iter().enumerate() {
                    println!(
                        "  {:>3}  {} {} {:>5} {:>5} {:>6}",
                        index + 1,
                        pad(&node.qname, 32),
                        pad(&node.file, 38),
                        node.loc,
                        node.metrics.call_in,
                        node.metrics.module_refs
                    );
                }
                if candidates.len() > shown.len() {
                    println!("  … {} more", candidates.len() - shown.len());
                }
            }
            println!();
            println!("Heuristic: no incoming calls, no module references, not exported, not a constructor, not in a cycle.");
            println!("Test helpers can still appear when the analyzer could not mark them as entry-like.");
            println!("Treat this as a candidate list, not proof.");
        }
    }
    Ok(())
}

fn search_class(node: &GraphNode) -> String {
    if let Some((class, _)) = node.qname.rsplit_once('.') {
        return class.to_string();
    }
    Path::new(&node.file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(node.file.as_str())
        .to_string()
}

async fn search(args: AuditSearchArgs) -> Result<()> {
    let context = open_context(&args.common).await?;
    let kind = args.kind.as_deref().map(parse_kind).transpose()?;
    let limit = args.limit.clamp(1, 100);
    let kind_filter = kind.map(NodeKind::as_str);
    let nodes = context
        .store
        .search_nodes(context.graph_id, &args.query, kind_filter, limit)
        .await?;

    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.search",
                "snapshot": snapshot_json(&context.summary),
                "query": args.query,
                "nodes": nodes.iter().map(|node| node_row(node, node.metrics.hotspot)).collect::<Vec<_>>(),
            }))?;
        }
        AuditFormat::Text => {
            text_header(&context.summary);
            println!();
            println!("Search {:#?} · {} matches", args.query, nodes.len());
            if nodes.is_empty() {
                println!("  no matches");
            } else {
                for (index, node) in nodes.iter().enumerate() {
                    if index > 0 {
                        println!();
                    }
                    println!("  [{}] {}", index + 1, node.qname);
                    println!(
                        "      kind: {:<12} class: {}",
                        node.kind.as_str(),
                        search_class(node)
                    );
                    println!("      file: {}:{}", node.file, node.line);
                    println!(
                        "      fan-in {} · fan-out {} · reach {} · depth {} · height {} · cc {} · loc {} · hotspot {}",
                        node.metrics.fan_in,
                        node.metrics.fan_out,
                        node.metrics.transitive_fan_out,
                        node.metrics.depth,
                        node.metrics.height,
                        node.cyclomatic,
                        node.loc,
                        metric(node.metrics.hotspot)
                    );
                    let mut tags = Vec::new();
                    if node.is_entry_like {
                        tags.push("entry");
                    }
                    if node.metrics.in_cycle {
                        tags.push("cycle");
                    }
                    if node.metrics.dead_code_candidate {
                        tags.push("dead-code candidate");
                    }
                    if node.synthetic {
                        tags.push("synthetic");
                    }
                    if !tags.is_empty() {
                        println!("      tags: {}", tags.join(" · "));
                    }
                    println!("      id: {}", node.id);
                }
            }
            println!();
            println!("Next: code-graph audit focus <symbol>");
        }
    }
    Ok(())
}

fn metric_definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "priority",
            "absolute refactoring priority: complexity * impact * call instability",
        ),
        (
            "hotspot",
            "relative centrality triage score; not an absolute complexity measure",
        ),
        (
            "complexity",
            "cyclomatic complexity: 1 + decision points in the callable body",
        ),
        (
            "blast-radius",
            "transitive fan-in: callables that can reach this node through call edges",
        ),
        (
            "fan-in / fan-out",
            "total distinct call + data neighbours; call sites count multiplicity separately",
        ),
        (
            "call / data fan",
            "the same degree split by edge kind; call is the architecture view",
        ),
        (
            "sloc",
            "non-empty, non-comment lines in the declaration range",
        ),
        (
            "ifc",
            "information-flow complexity: LOC + (call-fan-in × call-fan-out)²",
        ),
        (
            "betweenness",
            "how often the node lies on shortest paths; a bottleneck signal",
        ),
        (
            "instability",
            "call-fan-out / (call-fan-in + call-fan-out); 0 = depended on, 1 = depends on others",
        ),
        (
            "ignored result",
            "ignored call sites / call sites with known result usage",
        ),
        (
            "cycle",
            "strongly connected component with more than one callable",
        ),
        (
            "dead-code candidate",
            "no incoming calls, no module references, not exported, not a constructor, not in a cycle",
        ),
    ]
}

fn metrics(args: AuditMetricsArgs) -> Result<()> {
    match args.common.format {
        AuditFormat::Json => {
            print_json(&json!({
                "kind": "audit.metrics",
                "metrics": metric_definitions()
                    .into_iter()
                    .map(|(name, description)| json!({ "name": name, "description": description }))
                    .collect::<Vec<_>>(),
            }))?;
        }
        AuditFormat::Text => {
            println!("Metrics used by audit");
            for (name, description) in metric_definitions() {
                println!("  {:<18} {}", name, description);
            }
            println!();
            println!("These are definitions, not smell thresholds.");
            println!("`audit top --order-by <metric>` ranks the existing values.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::NodeKind;

    #[test]
    fn kind_aliases_parse() {
        assert_eq!(parse_kind("function").unwrap(), NodeKind::Function);
        assert_eq!(parse_kind("fn").unwrap(), NodeKind::Function);
        assert_eq!(parse_kind("method").unwrap(), NodeKind::Method);
        assert_eq!(parse_kind("ctor").unwrap(), NodeKind::Constructor);
        assert!(parse_kind("not-a-kind").is_err());
    }

    #[test]
    fn small_metrics_keep_three_decimals() {
        assert_eq!(metric(0.5521), "0.552");
        assert_eq!(metric(12.34), "12.34");
        assert_eq!(metric(123.4), "123.4");
        assert_eq!(metric(2.0), "2");
    }
}
