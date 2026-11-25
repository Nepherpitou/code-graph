//! Snapshot-to-snapshot audit diff.
//!
//! This module deliberately knows nothing about Git. It compares two persisted
//! snapshots and accepts only generic selectors and an optional path list.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use crate::analysis::analyze;
use crate::graph::{EdgeKind, GraphEdge, GraphNode};
use crate::store::{GraphSummary, Store};

use super::args::{AuditDiffArgs, AuditDiffOrder, AuditDiffStatus, AuditDiffView, AuditFormat};
use super::audit_views::{self, ViewInput};
use super::audit::{
    analysis_options, fit, metric, metric_json, pad, parse_kind, print_json, snapshot_json,
};

#[derive(Clone, Debug)]
struct DiffContext {
    store: Store,
    from: GraphSummary,
    to: GraphSummary,
}

pub(super) async fn run(args: AuditDiffArgs) -> Result<ExitCode> {
    if args.min_delta.is_some() && matches!(args.order_by, AuditDiffOrder::Name) {
        bail!("--min-delta needs a metric --order-by, not name");
    }
    let context = open_context(&args).await?;
    let warnings = comparability_warnings(&context.from, &context.to)?;
    for warning in &warnings {
        eprintln!("[code-graph] warning: {warning}");
    }

    let old_nodes = context.store.all_nodes(context.from.id).await?;
    let new_nodes = context.store.all_nodes(context.to.id).await?;
    let data = match_nodes(&old_nodes, &new_nodes);

    let kind = args.kind.as_deref().map(parse_kind).transpose()?;
    let changed_files = match &args.changed_files {
        Some(path) => Some(load_changed_files(path)?),
        None => None,
    };

    let mut rows = data.rows;
    if !args.all_metrics {
        rows.retain(|row| {
            matches!(row.status, DiffStatus::Added | DiffStatus::Removed)
                || !row.flags.is_empty()
                || row
                    .delta
                    .as_ref()
                    .is_some_and(MetricDelta::has_meaningful_change)
        });
    }
    rows.retain(|row| row_matches(row, &args, kind, changed_files.as_ref()));
    rows.retain(|row| row_passes_min_delta(row, &args));
    sort_rows(&mut rows, &args);

    if args.view != AuditDiffView::Symbols {
        let old_edges = context.store.all_edges(context.from.id).await?;
        let new_edges = context.store.all_edges(context.to.id).await?;
        let view_input = ViewInput {
            from: &context.from,
            to: &context.to,
            old_nodes: &old_nodes,
            new_nodes: &new_nodes,
            old_edges: &old_edges,
            new_edges: &new_edges,
            rows: &rows,
            changed_files: changed_files.as_ref(),
            verify_calls: !args.no_verify_calls,
            args: &args,
        };
        return audit_views::run(args.view, &view_input).await;
    }

    let total_rows = rows.len();
    let limit = args.limit.clamp(1, 1000);
    let shown_rows: Vec<&NodeDiff> = rows.iter().take(limit).collect();

    let edges = if args.edges {
        let old_edges = context.store.all_edges(context.from.id).await?;
        let new_edges = context.store.all_edges(context.to.id).await?;
        Some(diff_edges(&old_edges, &new_edges, &data.node_map))
    } else {
        None
    };

    match args.format {
        AuditFormat::Json => {
            let rows_json = shown_rows
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            let mut payload = json!({
                "kind": "audit.diff",
                "from": snapshot_json(&context.from),
                "to": snapshot_json(&context.to),
                "summary": summary_json(&context.from, &context.to),
                "orderBy": order_label(args.order_by),
                "ascending": args.ascending,
                "totalRows": total_rows,
                "shownRows": shown_rows.len(),
                "truncated": total_rows > shown_rows.len(),
                "rows": rows_json,
                "warnings": warnings,
            });
            if let Some(edges) = &edges {
                payload["edges"] = json!({
                    "added": edges.added.iter().map(edge_json).collect::<Vec<_>>(),
                    "removed": edges.removed.iter().map(edge_json).collect::<Vec<_>>(),
                });
            }
            print_json(&payload)?;
        }
        AuditFormat::Text => {
            print_text(&TextReport {
                context: &context,
                rows: &rows,
                shown_rows: &shown_rows,
                total_rows,
                args: &args,
                edges: edges.as_ref(),
            });
        }
    }

    if args.exit_code && total_rows > 0 {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

async fn open_context(args: &AuditDiffArgs) -> Result<DiffContext> {
    let db = args
        .db
        .clone()
        .unwrap_or_else(|| args.root.join(".code-graph").join("code-graph.db"));

    if args.refresh {
        if args.to.is_some() {
            bail!("--refresh creates the target snapshot; it cannot be combined with --to");
        }
        if !args.root.is_dir() {
            bail!("project root is not a directory: {}", args.root.display());
        }
        let baseline = if db.exists() {
            let store = Store::open(&db).await?;
            match args.from.as_deref() {
                Some(selector) => Some(resolve_selector(&store, selector).await?),
                None => store.latest_graph().await?,
            }
        } else {
            None
        };
        let Some(baseline) = baseline else {
            bail!(
                "diff needs a baseline snapshot; database {} has no snapshots.\nRun `code-graph audit {} --refresh` first.",
                db.display(),
                args.root.display()
            );
        };

        eprintln!("[code-graph] analyzing {} ...", args.root.display());
        let report = analyze(analysis_options(&args.root)).await?;
        let store = Store::open(&db).await?;
        let graph_id = store.insert_report(&report).await?;
        let target = store
            .graph(graph_id)
            .await?
            .ok_or_else(|| anyhow!("snapshot #{graph_id} was stored but cannot be read back"))?;
        eprintln!(
            "[code-graph] stored snapshot #{graph_id} in {} ({} ms)",
            db.display(),
            report.meta.duration_ms
        );
        ensure_distinct(&baseline, &target)?;
        return Ok(DiffContext {
            store,
            from: baseline,
            to: target,
        });
    }

    if !db.exists() {
        bail!(
            "snapshot database not found: {}\nRun `code-graph audit {} --refresh` first.",
            db.display(),
            args.root.display()
        );
    }
    let store = Store::open(&db).await?;
    let from = resolve_selector(&store, args.from.as_deref().unwrap_or("previous")).await?;
    let to = resolve_selector(&store, args.to.as_deref().unwrap_or("latest")).await?;
    ensure_distinct(&from, &to)?;
    Ok(DiffContext { store, from, to })
}

fn ensure_distinct(from: &GraphSummary, to: &GraphSummary) -> Result<()> {
    if from.id == to.id {
        bail!("cannot diff snapshot #{} with itself", from.id);
    }
    Ok(())
}

async fn resolve_selector(store: &Store, selector: &str) -> Result<GraphSummary> {
    if selector.eq_ignore_ascii_case("latest") {
        return store
            .latest_graph()
            .await?
            .ok_or_else(|| anyhow!("no snapshots in database"));
    }
    if selector.eq_ignore_ascii_case("previous") {
        return store.previous_graph().await?.ok_or_else(|| {
            anyhow!(
                "no previous snapshot; database needs at least two snapshots.\nRun `code-graph audit <root> --refresh` to create another one."
            )
        });
    }
    let id: i64 = selector.parse().map_err(|_| {
        anyhow!("snapshot must be a numeric id, \"latest\", or \"previous\", got {selector:?}")
    })?;
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
        bail!("snapshot #{id} not found; database has no snapshots");
    }
    bail!("snapshot #{id} not found; available: {available}")
}

fn comparability_warnings(from: &GraphSummary, to: &GraphSummary) -> Result<Vec<String>> {
    if from.meta.language != to.meta.language {
        bail!(
            "cannot diff snapshots with different languages: {} (#{}) → {} (#{})",
            from.meta.language,
            from.id,
            to.meta.language,
            to.id
        );
    }
    let mut warnings = Vec::new();
    if from.meta.root != to.meta.root {
        warnings.push(format!(
            "roots differ: {} → {}; matching uses relative paths",
            from.meta.root, to.meta.root
        ));
    }
    if from.meta.options != to.meta.options {
        warnings
            .push("analysis options differ; metric deltas may reflect configuration".to_string());
    }
    if from.meta.truncated || to.meta.truncated {
        warnings.push("one or both snapshots are truncated; deltas may be incomplete".to_string());
    }
    if from.meta.constants_crawled < from.meta.constants_total
        || to.meta.constants_crawled < to.meta.constants_total
    {
        warnings.push(format!(
            "constant crawl truncated ({} of {} on base, {} of {} on head); dead-code deltas are unreliable",
            from.meta.constants_crawled,
            from.meta.constants_total,
            to.meta.constants_crawled,
            to.meta.constants_total
        ));
    }
    if from.meta.version != to.meta.version {
        warnings.push(format!(
            "tool versions differ: {} → {}",
            from.meta.version, to.meta.version
        ));
    }
    Ok(warnings)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricSet {
    priority: f64,
    cyclomatic: u32,
    sloc: u32,
    loc: u32,
    params: u32,
    fan_in: u32,
    fan_out: u32,
    call_fan_in: u32,
    call_fan_out: u32,
    data_fan_in: u32,
    data_fan_out: u32,
    transitive_fan_in: u32,
    transitive_fan_out: u32,
    instability: f64,
    ifc: f64,
    hotspot: f64,
    betweenness: f64,
    result_ignored_ratio: f64,
    scc_size: u32,
    dead_code_candidate: bool,
    in_cycle: bool,
}

impl MetricSet {
    fn from_node(node: &GraphNode) -> Self {
        Self {
            priority: node.metrics.refactor_priority,
            cyclomatic: node.cyclomatic,
            sloc: node.sloc,
            loc: node.loc,
            params: node.params,
            fan_in: node.metrics.fan_in,
            fan_out: node.metrics.fan_out,
            call_fan_in: node.metrics.call_fan_in,
            call_fan_out: node.metrics.call_fan_out,
            data_fan_in: node.metrics.data_fan_in,
            data_fan_out: node.metrics.data_fan_out,
            transitive_fan_in: node.metrics.transitive_fan_in,
            transitive_fan_out: node.metrics.transitive_fan_out,
            instability: node.metrics.instability,
            ifc: node.metrics.ifc,
            hotspot: node.metrics.hotspot,
            betweenness: node.metrics.betweenness,
            result_ignored_ratio: node.metrics.result_ignored_ratio,
            scc_size: node.metrics.scc_size,
            dead_code_candidate: node.metrics.dead_code_candidate,
            in_cycle: node.metrics.in_cycle,
        }
    }

    fn value(&self, order: AuditDiffOrder) -> f64 {
        match order {
            AuditDiffOrder::Priority => self.priority,
            AuditDiffOrder::Complexity => self.cyclomatic as f64,
            AuditDiffOrder::Sloc => self.sloc as f64,
            AuditDiffOrder::Loc => self.loc as f64,
            AuditDiffOrder::BlastRadius => self.transitive_fan_in as f64,
            AuditDiffOrder::TransitiveFanOut => self.transitive_fan_out as f64,
            AuditDiffOrder::FanIn => self.fan_in as f64,
            AuditDiffOrder::FanOut => self.fan_out as f64,
            AuditDiffOrder::CallFanIn => self.call_fan_in as f64,
            AuditDiffOrder::CallFanOut => self.call_fan_out as f64,
            AuditDiffOrder::DataFanIn => self.data_fan_in as f64,
            AuditDiffOrder::DataFanOut => self.data_fan_out as f64,
            AuditDiffOrder::Ifc => self.ifc,
            AuditDiffOrder::Betweenness => self.betweenness,
            AuditDiffOrder::Hotspot => self.hotspot,
            AuditDiffOrder::Params => self.params as f64,
            AuditDiffOrder::Name => 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricDelta {
    priority: f64,
    cyclomatic: i64,
    sloc: i64,
    loc: i64,
    params: i64,
    fan_in: i64,
    fan_out: i64,
    call_fan_in: i64,
    call_fan_out: i64,
    data_fan_in: i64,
    data_fan_out: i64,
    transitive_fan_in: i64,
    transitive_fan_out: i64,
    instability: f64,
    ifc: f64,
    hotspot: f64,
    betweenness: f64,
    result_ignored_ratio: f64,
    scc_size: i64,
}

impl MetricDelta {
    fn between(old: &MetricSet, new: &MetricSet) -> Self {
        Self {
            priority: new.priority - old.priority,
            cyclomatic: i64::from(new.cyclomatic) - i64::from(old.cyclomatic),
            sloc: i64::from(new.sloc) - i64::from(old.sloc),
            loc: i64::from(new.loc) - i64::from(old.loc),
            params: i64::from(new.params) - i64::from(old.params),
            fan_in: i64::from(new.fan_in) - i64::from(old.fan_in),
            fan_out: i64::from(new.fan_out) - i64::from(old.fan_out),
            call_fan_in: i64::from(new.call_fan_in) - i64::from(old.call_fan_in),
            call_fan_out: i64::from(new.call_fan_out) - i64::from(old.call_fan_out),
            data_fan_in: i64::from(new.data_fan_in) - i64::from(old.data_fan_in),
            data_fan_out: i64::from(new.data_fan_out) - i64::from(old.data_fan_out),
            transitive_fan_in: i64::from(new.transitive_fan_in) - i64::from(old.transitive_fan_in),
            transitive_fan_out: i64::from(new.transitive_fan_out)
                - i64::from(old.transitive_fan_out),
            instability: new.instability - old.instability,
            ifc: new.ifc - old.ifc,
            hotspot: new.hotspot - old.hotspot,
            betweenness: new.betweenness - old.betweenness,
            result_ignored_ratio: new.result_ignored_ratio - old.result_ignored_ratio,
            scc_size: i64::from(new.scc_size) - i64::from(old.scc_size),
        }
    }

    fn value(&self, order: AuditDiffOrder) -> f64 {
        match order {
            AuditDiffOrder::Priority => self.priority,
            AuditDiffOrder::Complexity => self.cyclomatic as f64,
            AuditDiffOrder::Sloc => self.sloc as f64,
            AuditDiffOrder::Loc => self.loc as f64,
            AuditDiffOrder::BlastRadius => self.transitive_fan_in as f64,
            AuditDiffOrder::TransitiveFanOut => self.transitive_fan_out as f64,
            AuditDiffOrder::FanIn => self.fan_in as f64,
            AuditDiffOrder::FanOut => self.fan_out as f64,
            AuditDiffOrder::CallFanIn => self.call_fan_in as f64,
            AuditDiffOrder::CallFanOut => self.call_fan_out as f64,
            AuditDiffOrder::DataFanIn => self.data_fan_in as f64,
            AuditDiffOrder::DataFanOut => self.data_fan_out as f64,
            AuditDiffOrder::Ifc => self.ifc,
            AuditDiffOrder::Betweenness => self.betweenness,
            AuditDiffOrder::Hotspot => self.hotspot,
            AuditDiffOrder::Params => self.params as f64,
            AuditDiffOrder::Name => 0.0,
        }
    }

    fn has_meaningful_change(&self) -> bool {
        self.priority.abs() >= 1e-9
            || [self.cyclomatic, self.sloc, self.loc, self.params]
                .iter()
                .any(|value| *value != 0)
    }

    fn is_zero(&self) -> bool {
        let counts = [
            self.cyclomatic,
            self.sloc,
            self.loc,
            self.params,
            self.fan_in,
            self.fan_out,
            self.call_fan_in,
            self.call_fan_out,
            self.data_fan_in,
            self.data_fan_out,
            self.transitive_fan_in,
            self.transitive_fan_out,
            self.scc_size,
        ];
        let scores = [
            self.priority,
            self.instability,
            self.ifc,
            self.hotspot,
            self.betweenness,
            self.result_ignored_ratio,
        ];
        counts.iter().all(|value| *value == 0)
            && scores.iter().all(|value| value.abs() < 1e-9)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum DiffStatus {
    Changed,
    Added,
    Removed,
    Moved,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NodeDiff {
    pub(super) status: DiffStatus,
    matched_by: &'static str,
    id_changed: bool,
    pub(super) qname: String,
    pub(super) kind: String,
    pub(super) file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    old: Option<MetricSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new: Option<MetricSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<MetricDelta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    flags: Vec<&'static str>,
}

struct DiffData {
    rows: Vec<NodeDiff>,
    node_map: HashMap<String, String>,
}

fn match_nodes(old_nodes: &[GraphNode], new_nodes: &[GraphNode]) -> DiffData {
    let mut rows = Vec::new();
    let mut node_map = HashMap::new();
    let mut old_used: HashSet<String> = HashSet::new();
    let mut new_used: HashSet<String> = HashSet::new();

    let new_by_id: HashMap<&str, &GraphNode> = new_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for old_node in old_nodes {
        let Some(new_node) = new_by_id.get(old_node.id.as_str()) else {
            continue;
        };
        if let Some(row) = paired_row(old_node, new_node, DiffStatus::Changed, "id") {
            rows.push(row);
        }
        old_used.insert(old_node.id.clone());
        new_used.insert(new_node.id.clone());
        node_map.insert(old_node.id.clone(), new_node.id.clone());
    }

    let mut old_remaining = remaining(old_nodes, &old_used);
    let mut new_remaining = remaining(new_nodes, &new_used);
    let old_by_file = unique_by(&old_remaining, |node| {
        (
            node.file.clone(),
            node.kind.as_str().to_string(),
            node.qname.clone(),
        )
    });
    let new_by_file = unique_by(&new_remaining, |node| {
        (
            node.file.clone(),
            node.kind.as_str().to_string(),
            node.qname.clone(),
        )
    });
    for old_node in old_remaining.iter().copied() {
        let key = (
            old_node.file.clone(),
            old_node.kind.as_str().to_string(),
            old_node.qname.clone(),
        );
        if !old_by_file.contains_key(&key) {
            continue;
        }
        let Some(new_node) = new_by_file.get(&key).copied() else {
            continue;
        };
        if let Some(row) = paired_row(old_node, new_node, DiffStatus::Changed, "file-qname") {
            rows.push(row);
        }
        old_used.insert(old_node.id.clone());
        new_used.insert(new_node.id.clone());
        node_map.insert(old_node.id.clone(), new_node.id.clone());
    }

    old_remaining = remaining(old_nodes, &old_used);
    new_remaining = remaining(new_nodes, &new_used);
    let old_by_qname = unique_by(&old_remaining, |node| {
        (node.kind.as_str().to_string(), node.qname.clone())
    });
    let new_by_qname = unique_by(&new_remaining, |node| {
        (node.kind.as_str().to_string(), node.qname.clone())
    });
    for old_node in old_remaining.iter().copied() {
        let key = (old_node.kind.as_str().to_string(), old_node.qname.clone());
        if !old_by_qname.contains_key(&key) {
            continue;
        }
        let Some(new_node) = new_by_qname.get(&key).copied() else {
            continue;
        };
        let status = if old_node.file == new_node.file {
            DiffStatus::Changed
        } else {
            DiffStatus::Moved
        };
        if let Some(row) = paired_row(old_node, new_node, status, "qname") {
            rows.push(row);
        }
        old_used.insert(old_node.id.clone());
        new_used.insert(new_node.id.clone());
        node_map.insert(old_node.id.clone(), new_node.id.clone());
    }

    for old_node in old_nodes {
        if old_used.contains(&old_node.id) {
            continue;
        }
        rows.push(single_row(old_node, DiffStatus::Removed));
    }
    for new_node in new_nodes {
        if new_used.contains(&new_node.id) {
            continue;
        }
        rows.push(single_row(new_node, DiffStatus::Added));
    }

    DiffData { rows, node_map }
}

fn remaining<'a>(nodes: &'a [GraphNode], used: &HashSet<String>) -> Vec<&'a GraphNode> {
    nodes
        .iter()
        .filter(|node| !used.contains(&node.id))
        .collect()
}

fn unique_by<'a, K, F>(nodes: &[&'a GraphNode], key: F) -> HashMap<K, &'a GraphNode>
where
    K: Eq + std::hash::Hash + Clone,
    F: Fn(&GraphNode) -> K,
{
    let mut map = HashMap::new();
    let mut duplicate = HashSet::new();
    for node in nodes {
        match map.entry(key(node)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(*node);
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                duplicate.insert(entry.key().clone());
            }
        }
    }
    for key in duplicate {
        map.remove(&key);
    }
    map
}

fn paired_row(
    old: &GraphNode,
    new: &GraphNode,
    status: DiffStatus,
    matched_by: &'static str,
) -> Option<NodeDiff> {
    let old_set = MetricSet::from_node(old);
    let new_set = MetricSet::from_node(new);
    let delta = MetricDelta::between(&old_set, &new_set);
    let mut flags = Vec::new();
    if old.id != new.id {
        flags.push("signature-changed");
    }
    if old.file != new.file {
        flags.push("moved");
    }
    if !old.metrics.dead_code_candidate && new.metrics.dead_code_candidate {
        flags.push("dead-code-started");
    }
    if old.metrics.dead_code_candidate && !new.metrics.dead_code_candidate {
        flags.push("dead-code-ended");
    }
    if !old.metrics.in_cycle && new.metrics.in_cycle {
        flags.push("cycle-entered");
    }
    if old.metrics.in_cycle && !new.metrics.in_cycle {
        flags.push("cycle-left");
    }
    if delta.priority > 0.0 {
        flags.push("priority-increased");
    } else if delta.priority < 0.0 {
        flags.push("priority-decreased");
    }
    if delta.is_zero() && flags.is_empty() {
        return None;
    }
    Some(NodeDiff {
        status,
        matched_by,
        id_changed: old.id != new.id,
        qname: new.qname.clone(),
        kind: new.kind.as_str().to_string(),
        file: new.file.clone(),
        from_id: Some(old.id.clone()),
        to_id: Some(new.id.clone()),
        old: Some(old_set),
        new: Some(new_set),
        delta: Some(delta),
        flags,
    })
}

fn single_row(node: &GraphNode, status: DiffStatus) -> NodeDiff {
    let set = MetricSet::from_node(node);
    let (old, new) = if status == DiffStatus::Added {
        (None, Some(set))
    } else {
        (Some(set), None)
    };
    NodeDiff {
        status,
        matched_by: "none",
        id_changed: false,
        qname: node.qname.clone(),
        kind: node.kind.as_str().to_string(),
        file: node.file.clone(),
        from_id: if status == DiffStatus::Removed {
            Some(node.id.clone())
        } else {
            None
        },
        to_id: if status == DiffStatus::Added {
            Some(node.id.clone())
        } else {
            None
        },
        old,
        new,
        delta: None,
        flags: Vec::new(),
    }
}

fn row_matches(
    row: &NodeDiff,
    args: &AuditDiffArgs,
    kind: Option<crate::graph::NodeKind>,
    changed_files: Option<&HashSet<String>>,
) -> bool {
    if !args.status.is_empty()
        && !args
            .status
            .iter()
            .any(|status| status_matches(*status, row.status))
    {
        return false;
    }
    if let Some(kind) = kind {
        if row.kind != kind.as_str() {
            return false;
        }
    }
    if let Some(file) = args.file.as_deref() {
        if !row.file.contains(file) {
            return false;
        }
    }
    if let Some(changed) = changed_files {
        if !changed.iter().any(|path| path_matches(path, &row.file)) {
            return false;
        }
    }
    true
}

fn status_matches(filter: AuditDiffStatus, status: DiffStatus) -> bool {
    match filter {
        AuditDiffStatus::Changed => status == DiffStatus::Changed,
        AuditDiffStatus::Added => status == DiffStatus::Added,
        AuditDiffStatus::Removed => status == DiffStatus::Removed,
        AuditDiffStatus::Moved => status == DiffStatus::Moved,
    }
}

fn load_changed_files(path: &Path) -> Result<HashSet<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("cannot read changed-files list {}", path.display()))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(normalize_path)
        .collect())
}

fn normalize_path(path: &str) -> String {
    let mut path = path.replace('\\', "/");
    if let Some(rest) = path.strip_prefix("./") {
        path = rest.to_string();
    }
    path
}

fn path_matches(changed: &str, file: &str) -> bool {
    let file = normalize_path(file);
    changed == file || changed.ends_with(&format!("/{file}"))
}

fn row_passes_min_delta(row: &NodeDiff, args: &AuditDiffArgs) -> bool {
    let Some(min) = args.min_delta else {
        return true;
    };
    match row.status {
        DiffStatus::Changed | DiffStatus::Moved => row
            .delta
            .as_ref()
            .map(|delta| delta.value(args.order_by).abs() >= min)
            .unwrap_or(false),
        DiffStatus::Added | DiffStatus::Removed => true,
    }
}

fn sort_rows(rows: &mut [NodeDiff], args: &AuditDiffArgs) {
    rows.sort_by(|left, right| {
        if matches!(args.order_by, AuditDiffOrder::Name) {
            return if args.ascending {
                left.qname.cmp(&right.qname)
            } else {
                right.qname.cmp(&left.qname)
            }
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| status_rank(left.status).cmp(&status_rank(right.status)));
        }
        let left_value = sort_value(left, args.order_by);
        let right_value = sort_value(right, args.order_by);
        let ordering = match (left_value, right_value) {
            (Some(left), Some(right)) if args.ascending => left.partial_cmp(&right),
            (Some(left), Some(right)) => right.partial_cmp(&left),
            (Some(_), None) => Some(Ordering::Less),
            (None, Some(_)) => Some(Ordering::Greater),
            (None, None) => Some(Ordering::Equal),
        }
        .unwrap_or(Ordering::Equal);
        ordering
            .then_with(|| left.qname.cmp(&right.qname))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| status_rank(left.status).cmp(&status_rank(right.status)))
    });
}

fn sort_value(row: &NodeDiff, order: AuditDiffOrder) -> Option<f64> {
    match order {
        AuditDiffOrder::Name => None,
        _ => match row.status {
            DiffStatus::Changed | DiffStatus::Moved => {
                row.delta.as_ref().map(|delta| delta.value(order))
            }
            DiffStatus::Added => row.new.as_ref().map(|value| value.value(order)),
            DiffStatus::Removed => row.old.as_ref().map(|value| value.value(order)),
        },
    }
}

fn status_rank(status: DiffStatus) -> u8 {
    match status {
        DiffStatus::Changed => 0,
        DiffStatus::Moved => 1,
        DiffStatus::Added => 2,
        DiffStatus::Removed => 3,
    }
}

fn order_label(order: AuditDiffOrder) -> &'static str {
    match order {
        AuditDiffOrder::Priority => "priority",
        AuditDiffOrder::Complexity => "cyclomatic",
        AuditDiffOrder::Sloc => "sloc",
        AuditDiffOrder::Loc => "loc",
        AuditDiffOrder::BlastRadius => "transitive-in",
        AuditDiffOrder::TransitiveFanOut => "transitive-out",
        AuditDiffOrder::FanIn => "fan-in",
        AuditDiffOrder::FanOut => "fan-out",
        AuditDiffOrder::CallFanIn => "call-fan-in",
        AuditDiffOrder::CallFanOut => "call-fan-out",
        AuditDiffOrder::DataFanIn => "data-fan-in",
        AuditDiffOrder::DataFanOut => "data-fan-out",
        AuditDiffOrder::Ifc => "ifc",
        AuditDiffOrder::Betweenness => "betweenness",
        AuditDiffOrder::Hotspot => "hotspot",
        AuditDiffOrder::Params => "params",
        AuditDiffOrder::Name => "name",
    }
}

fn int_summary(old: i64, new: i64) -> Value {
    json!({ "from": old, "to": new, "delta": new - old })
}

fn float_summary(old: f64, new: f64) -> Value {
    json!({
        "from": metric_json(old),
        "to": metric_json(new),
        "delta": metric_json(new - old),
    })
}

fn summary_json(from: &GraphSummary, to: &GraphSummary) -> Value {
    json!({
        "files": int_summary(from.meta.files as i64, to.meta.files as i64),
        "symbols": int_summary(from.meta.symbols as i64, to.meta.symbols as i64),
        "nodes": int_summary(from.stats.nodes as i64, to.stats.nodes as i64),
        "functions": int_summary(from.stats.functions as i64, to.stats.functions as i64),
        "constants": int_summary(from.stats.constants as i64, to.stats.constants as i64),
        "edges": int_summary(from.stats.edges as i64, to.stats.edges as i64),
        "dataEdges": int_summary(from.stats.data_edges as i64, to.stats.data_edges as i64),
        "externalEdges": int_summary(from.stats.external_edges as i64, to.stats.external_edges as i64),
        "cycles": int_summary(from.stats.cycles as i64, to.stats.cycles as i64),
        "nodesInCycles": int_summary(from.stats.nodes_in_cycles as i64, to.stats.nodes_in_cycles as i64),
        "largestCycle": int_summary(from.stats.largest_cycle as i64, to.stats.largest_cycle as i64),
        "deadCode": int_summary(from.stats.dead_code as i64, to.stats.dead_code as i64),
        "isolated": int_summary(from.stats.isolated as i64, to.stats.isolated as i64),
        "maxDepth": int_summary(from.stats.max_depth as i64, to.stats.max_depth as i64),
        "maxHeight": int_summary(from.stats.max_height as i64, to.stats.max_height as i64),
        "crossFileEdges": int_summary(from.stats.cross_file_edges as i64, to.stats.cross_file_edges as i64),
        "density": float_summary(from.stats.density, to.stats.density),
        "avgFanIn": float_summary(from.stats.avg_fan_in, to.stats.avg_fan_in),
        "avgFanOut": float_summary(from.stats.avg_fan_out, to.stats.avg_fan_out),
        "crossFileRatio": float_summary(from.stats.cross_file_ratio, to.stats.cross_file_ratio),
        "resultIgnoredRatio": float_summary(from.stats.result_ignored_ratio, to.stats.result_ignored_ratio),
    })
}

struct TextReport<'a> {
    context: &'a DiffContext,
    rows: &'a [NodeDiff],
    shown_rows: &'a [&'a NodeDiff],
    total_rows: usize,
    args: &'a AuditDiffArgs,
    edges: Option<&'a EdgeDiff>,
}

fn print_text(report: &TextReport<'_>) {
    let TextReport {
        context,
        rows,
        shown_rows,
        total_rows,
        args,
        edges,
    } = report;
    println!("snapshot #{} → #{}", context.from.id, context.to.id);
    print_snapshot("from", &context.from);
    print_snapshot("to", &context.to);
    if context.from.meta.root != context.to.meta.root {
        println!(
            "roots: {} → {} (matched by relative paths)",
            context.from.meta.root, context.to.meta.root
        );
    }
    println!();
    print_summary(&context.from, &context.to);
    println!();
    println!(
        "Sorted by {} ({})",
        order_label(args.order_by),
        if args.ascending {
            "ascending"
        } else {
            "descending"
        }
    );

    if rows.is_empty() {
        println!();
        println!(
            "No metric changes between snapshot #{} and #{}.",
            context.from.id, context.to.id
        );
        return;
    }

    let changed_count = rows
        .iter()
        .filter(|row| matches!(row.status, DiffStatus::Changed | DiffStatus::Moved))
        .count();
    let added_count = rows
        .iter()
        .filter(|row| row.status == DiffStatus::Added)
        .count();
    let removed_count = rows
        .iter()
        .filter(|row| row.status == DiffStatus::Removed)
        .count();

    if changed_count > 0 {
        println!();
        println!("Changed ({changed_count})");
        print_change_header(args.order_by);
        for row in shown_rows
            .iter()
            .filter(|row| matches!(row.status, DiffStatus::Changed | DiffStatus::Moved))
        {
            print_change_row(row, args.order_by);
        }
    }
    if added_count > 0 {
        println!();
        println!("Added ({added_count})");
        print_single_header("new");
        for row in shown_rows
            .iter()
            .filter(|row| row.status == DiffStatus::Added)
        {
            print_single_row(row, "new");
        }
    }
    if removed_count > 0 {
        println!();
        println!("Removed ({removed_count})");
        print_single_header("old");
        for row in shown_rows
            .iter()
            .filter(|row| row.status == DiffStatus::Removed)
        {
            print_single_row(row, "old");
        }
    }
    if let Some(edges) = edges {
        println!();
        println!("Edges: +{} / -{}", edges.added.len(), edges.removed.len());
    }
    if *total_rows > shown_rows.len() {
        println!();
        println!(
            "shown {} of {} rows · use --limit to see more",
            shown_rows.len(),
            total_rows
        );
    }
}

fn print_snapshot(label: &str, summary: &GraphSummary) {
    println!(
        "{:<4}  #{:<3}  {}  · tool {}  · {} files / {} nodes",
        label,
        summary.id,
        summary.created_at,
        summary.meta.version,
        summary.meta.files,
        summary.stats.nodes
    );
}

fn print_summary(from: &GraphSummary, to: &GraphSummary) {
    println!("Summary");
    print_summary_int("files", from.meta.files as i64, to.meta.files as i64);
    print_summary_int("nodes", from.stats.nodes as i64, to.stats.nodes as i64);
    print_summary_int(
        "callables",
        from.stats.functions as i64,
        to.stats.functions as i64,
    );
    print_summary_int("edges", from.stats.edges as i64, to.stats.edges as i64);
    print_summary_int(
        "data edges",
        from.stats.data_edges as i64,
        to.stats.data_edges as i64,
    );
    print_summary_int("cycles", from.stats.cycles as i64, to.stats.cycles as i64);
    print_summary_int(
        "dead code",
        from.stats.dead_code as i64,
        to.stats.dead_code as i64,
    );
}

fn print_summary_int(label: &str, old: i64, new: i64) {
    println!(
        "  {:<12} {:>7} → {:<7} {}",
        label,
        old,
        new,
        signed_int(new - old)
    );
}

fn print_change_header(order: AuditDiffOrder) {
    let label = if matches!(order, AuditDiffOrder::Name) {
        "name".to_string()
    } else {
        format!("Δ{}", order_label(order))
    };
    print!("  {:<7} {:>12}", "status", label);
    if !matches!(order, AuditDiffOrder::Complexity) {
        print!(" {:>8}", "Δcyclo");
    }
    if !matches!(order, AuditDiffOrder::Sloc) {
        print!(" {:>8}", "Δsloc");
    }
    println!("  {:<34} file", "symbol");
}

fn print_change_row(row: &NodeDiff, order: AuditDiffOrder) {
    let Some(delta) = &row.delta else {
        return;
    };
    let value = if matches!(order, AuditDiffOrder::Name) {
        "—".to_string()
    } else {
        signed(delta.value(order))
    };
    print!("  {:<7} {:>12}", status_label(row.status), fit(&value, 12));
    if !matches!(order, AuditDiffOrder::Complexity) {
        print!(" {:>8}", signed_int(delta.cyclomatic));
    }
    if !matches!(order, AuditDiffOrder::Sloc) {
        print!(" {:>8}", signed_int(delta.sloc));
    }
    println!("  {:<34} {}", pad(&row.qname, 34), fit(&row.file, 44));
}

fn print_single_header(label: &str) {
    println!(
        "  {:<7} {:>10} {:>6} {:>6}  {:<34} file",
        "status", label, "cyclo", "sloc", "symbol"
    );
}

fn print_single_row(row: &NodeDiff, label: &str) {
    let set = if label == "new" {
        row.new.as_ref()
    } else {
        row.old.as_ref()
    };
    let Some(set) = set else {
        return;
    };
    println!(
        "  {:<7} {:>10} {:>6} {:>6}  {:<34} {}",
        status_label(row.status),
        metric(set.priority),
        metric(set.cyclomatic as f64),
        metric(set.sloc as f64),
        pad(&row.qname, 34),
        fit(&row.file, 44)
    );
}

fn status_label(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Changed => "changed",
        DiffStatus::Added => "added",
        DiffStatus::Removed => "removed",
        DiffStatus::Moved => "moved",
    }
}

fn signed(value: f64) -> String {
    if value > 0.0 {
        format!("+{}", metric(value))
    } else {
        metric(value)
    }
}

fn signed_int(value: i64) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EdgeJson {
    source: String,
    target: String,
    kind: &'static str,
    sites: u32,
}

struct EdgeDiff {
    added: Vec<EdgeJson>,
    removed: Vec<EdgeJson>,
}

fn edge_json(edge: &EdgeJson) -> Value {
    json!({
        "source": edge.source,
        "target": edge.target,
        "kind": edge.kind,
        "sites": edge.sites,
    })
}

fn diff_edges(
    old_edges: &[GraphEdge],
    new_edges: &[GraphEdge],
    node_map: &HashMap<String, String>,
) -> EdgeDiff {
    let old_by_key = edge_map(old_edges, node_map);
    let new_by_key = edge_map(new_edges, node_map);
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for (key, edge) in &new_by_key {
        if !old_by_key.contains_key(key) {
            added.push(edge_json_from(edge, key));
        }
    }
    for (key, edge) in &old_by_key {
        if !new_by_key.contains_key(key) {
            removed.push(edge_json_from(edge, key));
        }
    }
    added.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.kind.cmp(right.kind))
    });
    removed.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.kind.cmp(right.kind))
    });
    EdgeDiff { added, removed }
}

fn edge_map<'a>(
    edges: &'a [GraphEdge],
    node_map: &HashMap<String, String>,
) -> HashMap<(String, String, &'static str), &'a GraphEdge> {
    edges
        .iter()
        .map(|edge| {
            let source = node_map
                .get(&edge.source)
                .cloned()
                .unwrap_or_else(|| edge.source.clone());
            let target = node_map
                .get(&edge.target)
                .cloned()
                .unwrap_or_else(|| edge.target.clone());
            ((source, target, edge_kind(edge.kind)), edge)
        })
        .collect()
}

fn edge_json_from(edge: &GraphEdge, key: &(String, String, &'static str)) -> EdgeJson {
    EdgeJson {
        source: key.0.clone(),
        target: key.1.clone(),
        kind: key.2,
        sites: edge.sites,
    }
}

fn edge_kind(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Call => "call",
        EdgeKind::Data => "data",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{NodeKind, NodeMetrics, ResultUsage};

    fn node(id: &str, qname: &str, file: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            name: qname.rsplit('.').next().unwrap_or(qname).to_string(),
            qname: qname.to_string(),
            kind: NodeKind::Function,
            file: file.to_string(),
            line: 1,
            end_line: 10,
            loc: 10,
            sloc: 8,
            params: 2,
            cyclomatic: 4,
            exported: false,
            external: false,
            lazy: false,
            synthetic: false,
            is_entry_like: false,
            declaration: String::new(),
            metrics: NodeMetrics {
                fan_in: 1,
                fan_out: 2,
                refactor_priority: 10.0,
                ..NodeMetrics::default()
            },
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Call,
            sites: 1,
            result_used: ResultUsage::Unknown,
            external: false,
            inferred: None,
            call_sites: Vec::new(),
        }
    }

    #[test]
    fn matches_exact_and_reports_delta() {
        let old = vec![node("a", "a", "src/a.ts")];
        let mut new_node = node("a", "a", "src/a.ts");
        new_node.cyclomatic = 6;
        new_node.sloc = 12;
        new_node.metrics.refactor_priority = 14.0;
        let data = match_nodes(&old, &[new_node]);
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].status, DiffStatus::Changed);
        assert_eq!(data.rows[0].matched_by, "id");
        assert_eq!(data.rows[0].delta.as_ref().unwrap().cyclomatic, 2);
        assert_eq!(data.rows[0].delta.as_ref().unwrap().sloc, 4);
        assert_eq!(data.node_map.get("a"), Some(&"a".to_string()));
    }

    #[test]
    fn matches_signature_change_by_file_and_qname() {
        let old = vec![node("old-id", "Service.run", "src/service.ts")];
        let new = vec![node("new-id", "Service.run", "src/service.ts")];
        let data = match_nodes(&old, &new);
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].status, DiffStatus::Changed);
        assert_eq!(data.rows[0].matched_by, "file-qname");
        assert!(data.rows[0].id_changed);
    }

    #[test]
    fn matches_unique_moved_node() {
        let old = vec![node("old-id", "Service.run", "src/service.ts")];
        let new = vec![node("new-id", "Service.run", "src/service-run.ts")];
        let data = match_nodes(&old, &new);
        assert_eq!(data.rows.len(), 1);
        assert_eq!(data.rows[0].status, DiffStatus::Moved);
        assert_eq!(data.rows[0].matched_by, "qname");
    }

    #[test]
    fn duplicate_qname_is_not_moved() {
        let old = vec![
            node("a", "Service.run", "src/a.ts"),
            node("b", "Service.run", "src/b.ts"),
        ];
        let new = vec![node("c", "Service.run", "src/c.ts")];
        let data = match_nodes(&old, &new);
        assert!(data.rows.iter().all(|row| row.status != DiffStatus::Moved));
        assert_eq!(
            data.rows
                .iter()
                .filter(|row| row.status == DiffStatus::Added)
                .count(),
            1
        );
        assert_eq!(
            data.rows
                .iter()
                .filter(|row| row.status == DiffStatus::Removed)
                .count(),
            2
        );
    }

    #[test]
    fn edge_diff_uses_matched_endpoint_ids() {
        let old_edges = vec![edge("e1", "old-a", "old-b")];
        let new_edges = vec![edge("e2", "new-a", "new-b")];
        let mut map = HashMap::new();
        map.insert("old-a".to_string(), "new-a".to_string());
        map.insert("old-b".to_string(), "new-b".to_string());
        let diff = diff_edges(&old_edges, &new_edges, &map);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn changed_file_suffix_matches_subdirectory_analysis() {
        assert!(path_matches("web/src/a.ts", "src/a.ts"));
        assert!(path_matches("src/a.ts", "src/a.ts"));
        assert!(!path_matches("src/a.ts", "src/b.ts"));
    }

    #[test]
    fn graph_only_drift_is_not_meaningful() {
        let old = MetricSet::from_node(&node("a", "a", "src/a.ts"));
        let mut new = old.clone();
        new.hotspot += 0.1;
        let delta = MetricDelta::between(&old, &new);
        assert!(!delta.has_meaningful_change());
    }
}
