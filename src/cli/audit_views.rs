//! Atomic views over a snapshot diff.
//!
//! Every view answers one question about the difference between two persisted
//! snapshots and prints either a small table or JSON rows. Nothing here knows
//! about Git, pull requests, or comment formatting: callers pass a list of
//! changed files and compose whatever they need from the output.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::ExitCode;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::graph::{EdgeKind, GraphEdge, GraphNode, NodeKind};
use crate::store::GraphSummary;

use super::args::{AuditDiffArgs, AuditDiffView, AuditFormat};
use super::audit::{pad, print_json, snapshot_json};
use super::audit_diff::{DiffStatus, NodeDiff};

/// Which snapshot pair a view has to explain, and what the caller considers changed.
pub(super) struct ViewInput<'a> {
    pub(super) from: &'a GraphSummary,
    pub(super) to: &'a GraphSummary,
    pub(super) old_nodes: &'a [GraphNode],
    pub(super) new_nodes: &'a [GraphNode],
    pub(super) old_edges: &'a [GraphEdge],
    pub(super) new_edges: &'a [GraphEdge],
    pub(super) rows: &'a [NodeDiff],
    pub(super) changed_files: Option<&'a HashSet<String>>,
    pub(super) verify_calls: bool,
    pub(super) args: &'a AuditDiffArgs,
}

impl ViewInput<'_> {
    fn limit(&self) -> usize {
        self.args.limit.clamp(1, 1000)
    }
}

/// Why a call edge was left out of the dependency views.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Bucket {
    Override,
    Test,
    Wiring,
    Duplicate,
    Resignature,
    Unverified,
}

impl Bucket {
    fn label(self) -> &'static str {
        match self {
            Bucket::Override => "override",
            Bucket::Test => "test",
            Bucket::Wiring => "wiring",
            Bucket::Duplicate => "duplicate",
            Bucket::Resignature => "resignature",
            Bucket::Unverified => "unverified",
        }
    }
}

struct EdgeView<'a> {
    src: &'a GraphNode,
    dst: &'a GraphNode,
    sites: u32,
}

struct Graphs<'a> {
    old_by_id: HashMap<&'a str, &'a GraphNode>,
    new_by_id: HashMap<&'a str, &'a GraphNode>,
    new_by_key: HashMap<(&'a str, &'a str), &'a GraphNode>,
    old_pairs: HashSet<(&'a str, &'a str)>,
    old_keys: HashSet<(&'a str, &'a str, &'a str, &'a str)>,
    new_pairs: HashSet<(&'a str, &'a str)>,
}

impl<'a> Graphs<'a> {
    fn new(input: &'a ViewInput<'a>) -> Self {
        let old_by_id: HashMap<&str, &GraphNode> =
            input.old_nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let new_by_id = input.new_nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let new_by_key = input
            .new_nodes
            .iter()
            .map(|n| ((n.file.as_str(), n.qname.as_str()), n))
            .collect();
        let old_pairs = input
            .old_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Call)
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect();
        let new_pairs = input
            .new_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Call)
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect();
        let old_keys = input
            .old_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Call)
            .filter_map(|e| {
                let src = old_by_id.get(e.source.as_str())?;
                let dst = old_by_id.get(e.target.as_str())?;
                Some((
                    src.file.as_str(),
                    src.qname.as_str(),
                    dst.file.as_str(),
                    dst.qname.as_str(),
                ))
            })
            .collect();
        Self {
            old_by_id,
            new_by_id,
            new_by_key,
            old_pairs,
            old_keys,
            new_pairs,
        }
    }

    fn node(&self, id: &str) -> Option<&'a GraphNode> {
        self.new_by_id.get(id).copied()
    }
}

fn is_abstract(node: &GraphNode) -> bool {
    node.declaration.trim_start().starts_with("abstract")
}

fn is_test_file(path: &str) -> bool {
    path.ends_with(".spec.ts") || path.contains("/test/")
}

fn is_framework_file(path: &str) -> bool {
    const SUFFIXES: [&str; 5] = [
        ".controller.ts",
        ".module.ts",
        ".provider.ts",
        ".guard.ts",
        ".interceptor.ts",
    ];
    SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
}

fn is_callable(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Function | NodeKind::Method)
}

/// Conventional containers that hold modules rather than being one.
const CONTAINERS: [&str; 6] = ["src", "lib", "app", "pkg", "crates", "packages"];

/// The module a file belongs to: its first path segment after the usual
/// source roots, or `root` for files that sit directly in one.
fn area(path: &str) -> &str {
    let mut rest = path;
    while let Some((head, tail)) = rest.split_once('/') {
        if CONTAINERS.contains(&head) {
            rest = tail;
            continue;
        }
        return head;
    }
    "root"
}

/// Same-named methods are linked by the LSP only for overrides and their siblings.
fn is_override_link(src: &GraphNode, dst: &GraphNode, new_pairs: &HashSet<(&str, &str)>) -> bool {
    if src.name != dst.name {
        return false;
    }
    if is_abstract(src) || is_abstract(dst) {
        return true;
    }
    new_pairs.contains(&(dst.id.as_str(), src.id.as_str()))
}

/// Whether the call site line actually names the callee.
///
/// The LSP resolves a bare identifier to any declaration with that name, so a
/// destructured local like `const { created } = ...` can produce an edge to a
/// function `created` that the line never calls. The source text is the only
/// cheap guard; when the analyzed tree is gone, the edge is kept.
fn line_calls(
    root: &str,
    file: &str,
    name: &str,
    line: u32,
    cache: &mut HashMap<String, Vec<String>>,
) -> bool {
    let path = format!("{root}/{file}");
    let lines = cache_entries(cache, &path);
    if lines.is_empty() || line == 0 {
        // The analyzed tree is gone (database-only usage) or the edge has no
        // recorded call site: keep the edge.
        return true;
    }
    let line = line as usize;
    let start = line.saturating_sub(3).min(lines.len());
    let end = line.min(lines.len());
    let window = lines[start..end].join(" ");
    window.contains(&format!("{name}("))
        || window.contains(&format!("{name} ("))
        || window.contains(&format!(".{name}"))
}

fn cache_entries<'a>(cache: &'a mut HashMap<String, Vec<String>>, path: &str) -> &'a [String] {
    if !cache.contains_key(path) {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        cache.insert(path.to_string(), contents.lines().map(str::to_string).collect());
    }
    cache.get(path).map(Vec::as_slice).unwrap_or(&[])
}

fn site_line(edge: &GraphEdge) -> u32 {
    edge.call_sites.iter().map(|site| site.line).min().unwrap_or(0)
}

/// Call edges added by the target snapshot, split into usable ones and buckets.
fn new_call_edges<'a>(
    input: &'a ViewInput<'a>,
    graphs: &Graphs<'a>,
) -> (Vec<EdgeView<'a>>, HashMap<Bucket, Vec<EdgeView<'a>>>) {
    let mut candidates: Vec<(EdgeView<'a>, &'a GraphEdge)> = Vec::new();
    let mut dropped: HashMap<Bucket, Vec<EdgeView<'a>>> = HashMap::new();
    for edge in input.new_edges {
        if edge.kind != EdgeKind::Call {
            continue;
        }
        if graphs
            .old_pairs
            .contains(&(edge.source.as_str(), edge.target.as_str()))
        {
            continue;
        }
        let (Some(src), Some(dst)) = (
            graphs.new_by_id.get(edge.source.as_str()).copied(),
            graphs.new_by_id.get(edge.target.as_str()).copied(),
        ) else {
            continue;
        };
        let view = EdgeView {
            src,
            dst,
            sites: edge.sites,
        };
        if is_override_link(src, dst, &graphs.new_pairs) {
            dropped.entry(Bucket::Override).or_default().push(view);
        } else if is_test_file(&src.file) || is_test_file(&dst.file) {
            dropped.entry(Bucket::Test).or_default().push(view);
        } else if matches!(src.kind, NodeKind::Constructor) || matches!(dst.kind, NodeKind::Constructor)
        {
            dropped.entry(Bucket::Wiring).or_default().push(view);
        } else {
            candidates.push((view, edge));
        }
    }

    let mut by_site: HashMap<(&str, u32), Vec<usize>> = HashMap::new();
    for (index, (_view, edge)) in candidates.iter().enumerate() {
        by_site
            .entry((edge.source.as_str(), site_line(edge)))
            .or_default()
            .push(index);
    }
    let mut real = Vec::new();
    let mut duplicate = Vec::new();
    let mut resignature = Vec::new();
    let mut unverified = Vec::new();
    let mut cache: HashMap<String, Vec<String>> = HashMap::new();
    let root = input.to.meta.root.clone();
    let mut seen = vec![false; candidates.len()];
    for group in by_site.values() {
        let abstract_names: HashSet<&str> = group
            .iter()
            .map(|&index| candidates[index].0.dst)
            .filter(|dst| is_abstract(dst))
            .map(|dst| dst.name.as_str())
            .collect();
        for &index in group {
            if seen[index] {
                continue;
            }
            seen[index] = true;
            let (view, edge) = &candidates[index];
            if !is_abstract(view.dst) && abstract_names.contains(view.dst.name.as_str()) {
                duplicate.push(EdgeView {
                    src: view.src,
                    dst: view.dst,
                    sites: view.sites,
                });
                continue;
            }
            let key = (
                view.src.file.as_str(),
                view.src.qname.as_str(),
                view.dst.file.as_str(),
                view.dst.qname.as_str(),
            );
            if graphs.old_keys.contains(&key) {
                resignature.push(EdgeView {
                    src: view.src,
                    dst: view.dst,
                    sites: view.sites,
                });
                continue;
            }
            if input.verify_calls
                && !line_calls(&root, &view.src.file, &view.dst.name, site_line(edge), &mut cache)
            {
                unverified.push(EdgeView {
                    src: view.src,
                    dst: view.dst,
                    sites: view.sites,
                });
                continue;
            }
            real.push(EdgeView {
                src: view.src,
                dst: view.dst,
                sites: view.sites,
            });
        }
    }
    dropped.insert(Bucket::Duplicate, duplicate);
    dropped.insert(Bucket::Resignature, resignature);
    dropped.insert(Bucket::Unverified, unverified);
    (real, dropped)
}

/// Call edges of one snapshot with overrides, tests and wiring removed.
fn real_call_edges<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
) -> Vec<EdgeView<'a>> {
    let by_id: HashMap<&str, &GraphNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let pairs: HashSet<(&str, &str)> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Call)
        .map(|e| (e.source.as_str(), e.target.as_str()))
        .collect();
    let mut out = Vec::new();
    for edge in edges {
        if edge.kind != EdgeKind::Call {
            continue;
        }
        let (Some(src), Some(dst)) = (
            by_id.get(edge.source.as_str()).copied(),
            by_id.get(edge.target.as_str()).copied(),
        ) else {
            continue;
        };
        if is_override_link(src, dst, &pairs)
            || is_test_file(&src.file)
            || is_test_file(&dst.file)
            || matches!(src.kind, NodeKind::Constructor)
            || matches!(dst.kind, NodeKind::Constructor)
        {
            continue;
        }
        out.push(EdgeView {
            src,
            dst,
            sites: edge.sites,
        });
    }
    out
}

type Adjacency<'a> = HashMap<&'a str, Vec<&'a str>>;

fn adjacency<'a>(nodes: &'a [GraphNode], edges: &'a [GraphEdge]) -> (Adjacency<'a>, Adjacency<'a>) {
    let mut forward: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
    let mut reverse: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in real_call_edges(nodes, edges) {
        if !known.contains(edge.src.id.as_str()) || !known.contains(edge.dst.id.as_str()) {
            continue;
        }
        forward
            .entry(edge.src.id.as_str())
            .or_default()
            .push(edge.dst.id.as_str());
        reverse
            .entry(edge.dst.id.as_str())
            .or_default()
            .push(edge.src.id.as_str());
    }
    (forward, reverse)
}

fn reach_from<'a>(start: &'a str, forward: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    seen.insert(start);
    queue.push_back(start);
    while let Some(current) = queue.pop_front() {
        for next in forward.get(current).into_iter().flatten() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen.remove(start);
    seen
}

fn touched_ids<'a>(input: &'a ViewInput<'a>, graphs: &Graphs<'a>) -> HashMap<&'a str, DiffStatus> {
    let changed = input.changed_files;
    let mut out = HashMap::new();
    for row in input.rows {
        if !matches!(row.status, DiffStatus::Added | DiffStatus::Changed | DiffStatus::Moved) {
            continue;
        }
        if row.kind != NodeKind::Function.as_str() && row.kind != NodeKind::Method.as_str() {
            continue;
        }
        if let Some(files) = changed {
            if !files.contains(&row.file) {
                continue;
            }
        }
        if let Some(node) = graphs.new_by_key.get(&(row.file.as_str(), row.qname.as_str())) {
            out.insert(node.id.as_str(), row.status);
        }
    }
    out
}

fn require_changed_files<'a>(input: &'a ViewInput<'a>, view: &str) -> Result<&'a HashSet<String>> {
    match input.changed_files {
        Some(files) => Ok(files),
        None => bail!("--view {view} needs --changed-files"),
    }
}

fn envelope(view: AuditDiffView, input: &ViewInput<'_>, rows: Value, extra: Value) -> Value {
    let mut payload = json!({
        "kind": "audit.view",
        "view": view_label(view),
        "from": snapshot_json(input.from),
        "to": snapshot_json(input.to),
        "rows": rows,
        "changedFiles": input.changed_files.map(|files| files.len()),
        "limit": input.limit(),
        "truncated": false,
    });
    if let (Value::Object(payload_map), Value::Object(extra_map)) = (&mut payload, extra) {
        for (key, value) in extra_map {
            payload_map.insert(key, value);
        }
    }
    payload
}

fn view_label(view: AuditDiffView) -> &'static str {
    match view {
        AuditDiffView::Summary => "summary",
        AuditDiffView::Symbols => "symbols",
        AuditDiffView::Uses => "uses",
        AuditDiffView::Dependencies => "dependencies",
        AuditDiffView::Callers => "callers",
        AuditDiffView::Affected => "affected",
        AuditDiffView::Areas => "areas",
        AuditDiffView::FilteredEdges => "filtered-edges",
        AuditDiffView::Cycles => "cycles",
        AuditDiffView::DeadCode => "dead-code",
        AuditDiffView::SilentFiles => "silent-files",
    }
}

fn callables(nodes: &[GraphNode]) -> Vec<&GraphNode> {
    nodes
        .iter()
        .filter(|node| is_callable(node.kind) && !node.synthetic)
        .collect()
}

fn mean_cyclomatic(nodes: &[GraphNode]) -> f64 {
    let list = callables(nodes);
    if list.is_empty() {
        return 0.0;
    }
    list.iter().map(|node| f64::from(node.cyclomatic)).sum::<f64>() / list.len() as f64
}

fn totals(nodes: &[GraphNode]) -> (f64, f64, f64, i64) {
    let mut cyclomatic = 0.0;
    let mut loc = 0.0;
    let mut priority = 0.0;
    let mut dead = 0i64;
    for node in nodes {
        cyclomatic += f64::from(node.cyclomatic);
        loc += f64::from(node.loc);
        priority += node.metrics.refactor_priority;
        if node.metrics.dead_code_candidate {
            dead += 1;
        }
    }
    (cyclomatic, loc, priority, dead)
}

fn sccs<'a>(nodes: &'a [GraphNode], edges: &'a [GraphEdge]) -> Vec<Vec<&'a str>> {
    let (forward, _) = adjacency(nodes, edges);
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut low: HashMap<&str, usize> = HashMap::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut on_stack: HashSet<&str> = HashSet::new();
    let mut counter = 0usize;
    let mut out = Vec::new();
    let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for root in ids {
        if index.contains_key(root) {
            continue;
        }
        strongconnect(root, &forward, &mut index, &mut low, &mut stack, &mut on_stack, &mut counter, &mut out);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn strongconnect<'a>(
    node: &'a str,
    forward: &HashMap<&'a str, Vec<&'a str>>,
    index: &mut HashMap<&'a str, usize>,
    low: &mut HashMap<&'a str, usize>,
    stack: &mut Vec<&'a str>,
    on_stack: &mut HashSet<&'a str>,
    counter: &mut usize,
    out: &mut Vec<Vec<&'a str>>,
) {
    let mut work: Vec<(&'a str, usize)> = vec![(node, 0)];
    while let Some((current, edge_index)) = work.pop() {
        if edge_index == 0 {
            index.insert(current, *counter);
            low.insert(current, *counter);
            *counter += 1;
            stack.push(current);
            on_stack.insert(current);
        }
        let edges = forward.get(current).map(Vec::as_slice).unwrap_or(&[]);
        if edge_index < edges.len() {
            let next = edges[edge_index];
            work.push((current, edge_index + 1));
            if !index.contains_key(next) {
                work.push((next, 0));
            } else if on_stack.contains(next) {
                let candidate = index[next];
                let entry = low.entry(current).or_insert(candidate);
                if candidate < *entry {
                    *entry = candidate;
                }
            }
            continue;
        }
        if low[current] == index[current] {
            let mut component = Vec::new();
            while let Some(top) = stack.pop() {
                on_stack.remove(top);
                component.push(top);
                if top == current {
                    break;
                }
            }
            if component.len() > 1 {
                out.push(component);
            }
        }
        if let Some((parent, _)) = work.last().copied() {
            let candidate = low[current];
            let entry = low.entry(parent).or_insert(candidate);
            if candidate < *entry {
                *entry = candidate;
            }
        }
    }
}

fn format_float(value: f64) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn print_table(headers: &[&str], rows: &[Vec<String>], widths: &[usize]) {
    let mut line = String::new();
    for (index, header) in headers.iter().enumerate() {
        line.push_str(&pad(header, widths[index]));
    }
    println!("  {line}");
    for row in rows {
        let mut line = String::new();
        for (index, cell) in row.iter().enumerate() {
            line.push_str(&pad(cell, widths[index]));
        }
        println!("  {line}");
    }
}

pub(super) async fn run(view: AuditDiffView, input: &ViewInput<'_>) -> Result<ExitCode> {
    let graphs = Graphs::new(input);
    match view {
        AuditDiffView::Summary => summary(input),
        AuditDiffView::Uses => uses(input, &graphs),
        AuditDiffView::Dependencies => dependencies(input, &graphs),
        AuditDiffView::Callers => callers(input, &graphs),
        AuditDiffView::Affected => affected(input, &graphs),
        AuditDiffView::Areas => areas(input),
        AuditDiffView::FilteredEdges => filtered_edges(input, &graphs),
        AuditDiffView::Cycles => cycles(input),
        AuditDiffView::DeadCode => dead_code(input),
        AuditDiffView::SilentFiles => silent_files(input),
        AuditDiffView::Symbols => bail!("symbols view is handled by the diff itself"),
    }
}

fn summary(input: &ViewInput<'_>) -> Result<ExitCode> {
    let old_real = real_call_edges(input.old_nodes, input.old_edges);
    let new_real = real_call_edges(input.new_nodes, input.new_edges);
    let cross = |edges: &[EdgeView<'_>]| {
        edges
            .iter()
            .filter(|edge| area(&edge.src.file) != area(&edge.dst.file))
            .count()
    };
    let (old_cx, old_loc, old_priority, old_dead) = totals(input.old_nodes);
    let (new_cx, new_loc, new_priority, new_dead) = totals(input.new_nodes);
    let old_cycles = sccs(input.old_nodes, input.old_edges).len();
    let new_cycles = sccs(input.new_nodes, input.new_edges).len();

    let raw_calls = |edges: &[GraphEdge]| edges.iter().filter(|edge| edge.kind == EdgeKind::Call).count();
    let rows = vec![
        ("nodes", input.from.stats.nodes as f64, input.to.stats.nodes as f64),
        (
            "callables",
            callables(input.old_nodes).len() as f64,
            callables(input.new_nodes).len() as f64,
        ),
        (
            "call edges",
            raw_calls(input.old_edges) as f64,
            raw_calls(input.new_edges) as f64,
        ),
        ("cross-module calls", cross(&old_real) as f64, cross(&new_real) as f64),
        ("cyclomatic, total", old_cx, new_cx),
        ("cyclomatic/callable", mean_cyclomatic(input.old_nodes), mean_cyclomatic(input.new_nodes)),
        ("loc", old_loc, new_loc),
        ("refactor priority", old_priority, new_priority),
        ("real cycles", old_cycles as f64, new_cycles as f64),
        ("dead-code candidates", old_dead as f64, new_dead as f64),
    ];
    match input.args.format {
        AuditFormat::Json => {
            let rows_json: Vec<Value> = rows
                .iter()
                .map(|(label, before, after)| {
                    json!({
                        "metric": label,
                        "from": before,
                        "to": after,
                        "delta": after - before,
                    })
                })
                .collect();
            print_json(&json!({
                "kind": "audit.view",
                "view": "summary",
                "from": snapshot_json(input.from),
                "to": snapshot_json(input.to),
                "rows": rows_json,
                "rawCycles": {"from": input.from.stats.cycles, "to": input.to.stats.cycles},
            }))?;
        }
        AuditFormat::Text => {
            println!("snapshot #{} → #{}", input.from.id, input.to.id);
            println!();
            let table: Vec<Vec<String>> = rows
                .iter()
                .map(|(label, before, after)| {
                    vec![
                        (*label).to_string(),
                        format_float(*before),
                        format_float(*after),
                        format_float(after - before),
                    ]
                })
                .collect();
            print_table(&["metric", "before", "after", "delta"], &table, &[24, 12, 12, 12]);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn uses(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let (real, _) = new_call_edges(input, graphs);
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for edge in &real {
        let dst = edge.dst;
        if dst.id.is_empty() {
            continue;
        }
        let still_exists = graphs.old_by_id.contains_key(dst.id.as_str());
        if !still_exists {
            continue;
        }
        if !is_callable(dst.kind) || dst.synthetic {
            continue;
        }
        if is_test_file(&dst.file) || is_framework_file(&dst.file) {
            continue;
        }
        if dst.file.starts_with("common/decorators/")
            || dst.file.starts_with("common/pipes/")
            || dst.file.starts_with("common/dto/")
        {
            continue;
        }
        *counts.entry(dst.id.as_str()).or_default() += 1;
    }
    let mut rows: Vec<(&GraphNode, u32)> = counts
        .into_iter()
        .filter_map(|(id, count)| graphs.old_by_id.get(id).map(|node| (*node, count)))
        .collect();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.qname.cmp(&right.0.qname)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::Uses,
        &["existing symbol", "new calls"],
        &[52, 10],
        shown
            .iter()
            .map(|(node, count)| {
                (
                    vec![node.qname.clone(), count.to_string()],
                    json!({
                        "symbol": node.qname,
                        "file": node.file,
                        "newCalls": count,
                    }),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn dependencies(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let (real, _) = new_call_edges(input, graphs);
    let mut counts: HashMap<(&str, &str), u32> = HashMap::new();
    for edge in &real {
        let from = area(&edge.src.file);
        let to = area(&edge.dst.file);
        if from != to {
            *counts.entry((from, to)).or_default() += 1;
        }
    }
    let mutual: Vec<(&str, &str)> = counts
        .iter()
        .filter(|((from, to), _)| counts.contains_key(&(*to, *from)) && from < to)
        .map(|((from, to), _)| (*from, *to))
        .collect();
    let mut rows: Vec<((&str, &str), u32)> = counts.into_iter().collect();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::Dependencies,
        &["from", "to", "new calls"],
        &[24, 24, 10],
        shown
            .iter()
            .map(|((from, to), count)| {
                let mutual = rows
                    .iter()
                    .any(|((other_from, other_to), _)| *other_from == *to && *other_to == *from);
                (
                    vec![format!("{from}/"), format!("{to}/"), count.to_string()],
                    json!({
                        "from": from,
                        "to": to,
                        "newCalls": count,
                        "mutual": mutual,
                    }),
                )
            })
            .collect(),
        json!({
            "totalRows": rows.len(),
            "truncated": rows.len() > shown.len(),
            "mutual": mutual.iter().map(|(from, to)| json!([from, to])).collect::<Vec<_>>(),
        }),
    )
}

fn callers(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let changed = require_changed_files(input, "callers")?;
    let (real, _) = new_call_edges(input, graphs);
    let mut rows: Vec<EdgeView<'_>> = real
        .into_iter()
        .filter(|edge| changed.contains(&edge.dst.file) && !changed.contains(&edge.src.file))
        .collect();
    rows.sort_by(|left, right| {
        left.src
            .qname
            .cmp(&right.src.qname)
            .then_with(|| left.dst.qname.cmp(&right.dst.qname))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::Callers,
        &["existing caller", "now calls"],
        &[44, 44],
        shown
            .iter()
            .map(|edge| {
                (
                    vec![edge.src.qname.clone(), edge.dst.qname.clone()],
                    json!({
                        "caller": edge.src.qname,
                        "callerFile": edge.src.file,
                        "callee": edge.dst.qname,
                        "calleeFile": edge.dst.file,
                        "sites": edge.sites,
                    }),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

struct AffectedRow<'a> {
    node: &'a GraphNode,
    changed: u32,
    added: u32,
    all_callees: usize,
    direct: bool,
}

fn affected(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let changed_files = require_changed_files(input, "affected")?;
    let touched = touched_ids(input, graphs);
    let (forward, reverse) = adjacency(input.new_nodes, input.new_edges);

    let mut counts: HashMap<&str, (u32, u32)> = HashMap::new();
    for (id, status) in &touched {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        seen.insert(id);
        queue.push_back(id);
        while let Some(current) = queue.pop_front() {
            for caller in reverse.get(current).into_iter().flatten() {
                if seen.insert(caller) {
                    queue.push_back(caller);
                }
            }
        }
        let bucket = *status == DiffStatus::Added;
        for ancestor in seen {
            if ancestor == *id {
                continue;
            }
            let Some(node) = graphs.node(ancestor) else {
                continue;
            };
            if changed_files.contains(&node.file) {
                continue;
            }
            let entry = counts.entry(ancestor).or_default();
            if bucket {
                entry.1 += 1;
            } else {
                entry.0 += 1;
            }
        }
    }

    let direct: HashSet<&str> = counts
        .keys()
        .copied()
        .filter(|id| forward.get(id).is_some_and(|callees| {
            callees.iter().any(|callee| touched.contains_key(callee))
        }))
        .collect();

    let mut rows: Vec<AffectedRow<'_>> = counts
        .iter()
        .filter_map(|(id, (changed, added))| {
            let node = graphs.node(id)?;
            Some(AffectedRow {
                node,
                changed: *changed,
                added: *added,
                all_callees: reach_from(id, &forward).len(),
                direct: direct.contains(id),
            })
        })
        .collect();
    rows.sort_by(|left, right| {
        let left_share = f64::from(left.changed) / left.all_callees.max(1) as f64;
        let right_share = f64::from(right.changed) / right.all_callees.max(1) as f64;
        right_share
            .partial_cmp(&left_share)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.node
                    .metrics
                    .refactor_priority
                    .partial_cmp(&right.node.metrics.refactor_priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .reverse()
            })
            .then_with(|| left.node.qname.cmp(&right.node.qname))
    });

    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    let changed_total = counts.values().filter(|(changed, _)| *changed > 0).count();
    let added_only = counts.values().filter(|(changed, _)| *changed == 0).count();
    output_rows(
        input,
        AuditDiffView::Affected,
        &["existing symbol", "changed", "added", "share", "priority", "direct"],
        &[44, 8, 6, 8, 10, 6],
        shown
            .iter()
            .map(|row| {
                let share = f64::from(row.changed) / row.all_callees.max(1) as f64;
                (
                    vec![
                        row.node.qname.clone(),
                        row.changed.to_string(),
                        if row.added == 0 { String::new() } else { row.added.to_string() },
                        format!("{:.0}%", share * 100.0),
                        format_float(row.node.metrics.refactor_priority),
                        if row.direct { "yes".into() } else { String::new() },
                    ],
                    json!({
                        "symbol": row.node.qname,
                        "file": row.node.file,
                        "changedCallees": row.changed,
                        "addedCallees": row.added,
                        "allCallees": row.all_callees,
                        "share": share,
                        "priority": row.node.metrics.refactor_priority,
                        "direct": row.direct,
                    }),
                )
            })
            .collect(),
        json!({
            "totalRows": rows.len(),
            "affectedByChanged": changed_total,
            "affectedByAddedOnly": added_only,
            "direct": direct.len(),
            "truncated": rows.len() > shown.len(),
        }),
    )
}

struct AreaRow<'a> {
    name: &'a str,
    added: u32,
    removed: u32,
    changed: u32,
    old_mean: f64,
    new_mean: f64,
    loc_delta: i64,
}

fn nodes_in_area<'a>(nodes: &'a [GraphNode], name: &str) -> Vec<&'a GraphNode> {
    nodes
        .iter()
        .filter(|node| area(&node.file) == name && !node.synthetic)
        .collect()
}

fn mean_over(nodes: &[&GraphNode]) -> f64 {
    if nodes.is_empty() {
        return 0.0;
    }
    nodes.iter().map(|node| f64::from(node.cyclomatic)).sum::<f64>() / nodes.len() as f64
}

fn areas(input: &ViewInput<'_>) -> Result<ExitCode> {
    let changed = require_changed_files(input, "areas")?;
    let mut counts: HashMap<&str, (u32, u32, u32)> = HashMap::new();
    for row in input.rows {
        if !changed.contains(&row.file) {
            continue;
        }
        let entry = counts.entry(area(&row.file)).or_default();
        match row.status {
            DiffStatus::Added => entry.0 += 1,
            DiffStatus::Removed => entry.1 += 1,
            DiffStatus::Changed | DiffStatus::Moved => entry.2 += 1,
        }
    }
    let mut rows: Vec<AreaRow<'_>> = counts
        .into_iter()
        .map(|(name, (added, removed, changed))| {
            let old_nodes = nodes_in_area(input.old_nodes, name);
            let new_nodes = nodes_in_area(input.new_nodes, name);
            let old_callables: Vec<&GraphNode> =
                old_nodes.iter().copied().filter(|node| is_callable(node.kind)).collect();
            let new_callables: Vec<&GraphNode> =
                new_nodes.iter().copied().filter(|node| is_callable(node.kind)).collect();
            let loc_delta: i64 = new_nodes.iter().map(|node| i64::from(node.loc)).sum::<i64>()
                - old_nodes.iter().map(|node| i64::from(node.loc)).sum::<i64>();
            AreaRow {
                name,
                added,
                removed,
                changed,
                old_mean: mean_over(&old_callables),
                new_mean: mean_over(&new_callables),
                loc_delta,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .added
            .cmp(&left.added)
            .then_with(|| left.name.cmp(right.name))
    });
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::Areas,
        &["area", "added", "removed", "changed", "cyclomatic/callable", "loc delta"],
        &[24, 7, 8, 8, 20, 10],
        shown
            .iter()
            .map(|row| {
                let cx = if (row.old_mean - row.new_mean).abs() < 0.005 {
                    format!("{:.2}", row.new_mean)
                } else {
                    format!("{:.2} → {:.2}", row.old_mean, row.new_mean)
                };
                (
                    vec![
                        format!("{}/", row.name),
                        row.added.to_string(),
                        row.removed.to_string(),
                        row.changed.to_string(),
                        cx,
                        format!("{:+}", row.loc_delta),
                    ],
                    json!({
                        "area": row.name,
                        "added": row.added,
                        "removed": row.removed,
                        "changed": row.changed,
                        "cyclomaticBefore": row.old_mean,
                        "cyclomaticAfter": row.new_mean,
                        "locDelta": row.loc_delta,
                    }),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn filtered_edges(input: &ViewInput<'_>, graphs: &Graphs<'_>) -> Result<ExitCode> {
    let (real, dropped) = new_call_edges(input, graphs);
    let total: usize = input
        .new_edges
        .iter()
        .filter(|edge| {
            edge.kind == EdgeKind::Call
                && !graphs
                    .old_pairs
                    .contains(&(edge.source.as_str(), edge.target.as_str()))
        })
        .count();
    let order = [
        Bucket::Override,
        Bucket::Test,
        Bucket::Wiring,
        Bucket::Duplicate,
        Bucket::Resignature,
        Bucket::Unverified,
    ];
    let mut rows: Vec<(&str, usize)> = order
        .iter()
        .map(|bucket| (bucket.label(), dropped.get(bucket).map(Vec::len).unwrap_or(0)))
        .collect();
    rows.push(("used", real.len()));
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::FilteredEdges,
        &["reason", "edges"],
        &[20, 8],
        shown
            .iter()
            .map(|(reason, count)| {
                (
                    vec![(*reason).to_string(), count.to_string()],
                    json!({"reason": reason, "edges": count}),
                )
            })
            .collect(),
        json!({"newCallEdges": total, "used": real.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn cycles(input: &ViewInput<'_>) -> Result<ExitCode> {
    let old: HashSet<Vec<&str>> = sccs(input.old_nodes, input.old_edges)
        .into_iter()
        .map(|component| {
            let mut members: Vec<&str> = component
                .iter()
                .filter_map(|id| input.old_nodes.iter().find(|n| n.id == *id))
                .map(|n| n.qname.as_str())
                .collect();
            members.sort_unstable();
            members
        })
        .collect();
    let new_cycles = sccs(input.new_nodes, input.new_edges);
    let mut rows: Vec<(Vec<&str>, &'static str)> = Vec::new();
    let mut seen: HashSet<Vec<&str>> = HashSet::new();
    for component in &new_cycles {
        let mut members: Vec<&str> = component
            .iter()
            .filter_map(|id| input.new_nodes.iter().find(|n| n.id == *id))
            .map(|n| n.qname.as_str())
            .collect();
        members.sort_unstable();
        seen.insert(members.clone());
        rows.push((members.clone(), if old.contains(&members) { "existing" } else { "added" }));
    }
    for members in old {
        if !seen.contains(&members) {
            let mut members = members.clone();
            members.sort_unstable();
            rows.push((members, "removed"));
        }
    }
    rows.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0)));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::Cycles,
        &["status", "size", "members"],
        &[10, 6, 120],
        shown
            .iter()
            .map(|(members, status)| {
                (
                    vec![(*status).to_string(), members.len().to_string(), members.join(", ")],
                    json!({"status": status, "size": members.len(), "members": members}),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn dead_code(input: &ViewInput<'_>) -> Result<ExitCode> {
    let was_dead: HashSet<(&str, &str)> = input
        .old_nodes
        .iter()
        .filter(|node| node.metrics.dead_code_candidate)
        .map(|node| (node.file.as_str(), node.qname.as_str()))
        .collect();
    let mut rows: Vec<&GraphNode> = input
        .new_nodes
        .iter()
        .filter(|node| node.metrics.dead_code_candidate)
        .filter(|node| !node.qname.starts_with("tests.") && !is_test_file(&node.file))
        .filter(|node| !was_dead.contains(&(node.file.as_str(), node.qname.as_str())))
        .collect();
    rows.sort_by(|left, right| left.qname.cmp(&right.qname));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::DeadCode,
        &["symbol", "kind", "file", "framework"],
        &[40, 10, 40, 10],
        shown
            .iter()
            .map(|node| {
                (
                    vec![
                        node.qname.clone(),
                        node.kind.as_str().to_string(),
                        node.file.clone(),
                        if is_framework_file(&node.file) { "yes".into() } else { String::new() },
                    ],
                    json!({
                        "symbol": node.qname,
                        "kind": node.kind.as_str(),
                        "file": node.file,
                        "framework": is_framework_file(&node.file),
                    }),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn silent_files(input: &ViewInput<'_>) -> Result<ExitCode> {
    let changed = require_changed_files(input, "silent-files")?;
    let touched: HashSet<&str> = input.rows.iter().map(|row| row.file.as_str()).collect();
    let mut rows: Vec<(&str, &'static str)> = changed
        .iter()
        .filter(|file| !touched.contains(file.as_str()))
        .map(|file| {
            let reason = if file.ends_with(".spec.ts") {
                "test"
            } else if file.contains(".dto") || file.contains("schema") {
                "dto-schema"
            } else if file.contains("/ports/") {
                "port"
            } else if file.ends_with(".module.ts") {
                "di-module"
            } else {
                "no-callable-delta"
            };
            (file.as_str(), reason)
        })
        .collect();
    rows.sort_by(|left, right| left.0.cmp(right.0));
    let limit = input.limit();
    let shown = rows.iter().take(limit).collect::<Vec<_>>();
    output_rows(
        input,
        AuditDiffView::SilentFiles,
        &["file", "reason"],
        &[64, 18],
        shown
            .iter()
            .map(|(file, reason)| {
                (
                    vec![(*file).to_string(), (*reason).to_string()],
                    json!({"file": file, "reason": reason}),
                )
            })
            .collect(),
        json!({"totalRows": rows.len(), "truncated": rows.len() > shown.len()}),
    )
}

fn output_rows(
    input: &ViewInput<'_>,
    view: AuditDiffView,
    headers: &[&str],
    widths: &[usize],
    rows: Vec<(Vec<String>, Value)>,
    extra: Value,
) -> Result<ExitCode> {
    let displayed = rows.len();
    let total = extra
        .get("totalRows")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(displayed);
    match input.args.format {
        AuditFormat::Json => {
            let rows_json: Vec<Value> = rows.into_iter().map(|(_, json_row)| json_row).collect();
            let mut payload = envelope(view, input, Value::Array(rows_json), extra);
            if let Some(object) = payload.as_object_mut() {
                object.insert("headers".into(), json!(headers));
            }
            print_json(&payload)?;
        }
        AuditFormat::Text => {
            println!("snapshot #{} → #{}", input.from.id, input.to.id);
            println!();
            let table: Vec<Vec<String>> = rows.into_iter().map(|(cells, _)| cells).collect();
            print_table(headers, &table, widths);
            if total > displayed {
                println!();
                println!("shown {displayed} of {total} rows · use --limit to see more");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ResultUsage;

    fn node(id: &str, name: &str, kind: NodeKind, file: &str, declaration: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            name: name.to_string(),
            qname: name.to_string(),
            kind,
            file: file.to_string(),
            line: 1,
            end_line: 2,
            loc: 1,
            sloc: 1,
            params: 0,
            cyclomatic: 1,
            exported: false,
            external: false,
            lazy: false,
            synthetic: false,
            is_entry_like: false,
            declaration: declaration.to_string(),
            metrics: crate::graph::NodeMetrics::default(),
        }
    }

    fn edge(source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            id: format!("call:{source}->{target}"),
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Call,
            sites: 1,
            result_used: ResultUsage::All,
            external: false,
            inferred: None,
            call_sites: vec![crate::graph::CallSite {
                file: String::new(),
                line: 10,
                character: 4,
                result_used: Some(true),
            }],
        }
    }

    #[test]
    fn areas_skip_source_roots() {
        assert_eq!(area("src/cli/audit_views.rs"), "cli");
        assert_eq!(area("crates/graph/src/lib.rs"), "graph");
        assert_eq!(area("src/main.rs"), "root");
        assert_eq!(area("ai-chat/application/x.ts"), "ai-chat");
    }

    #[test]
    fn sibling_overrides_are_not_real_calls() {
        let first = node("a#run", "run", NodeKind::Method, "a.ts", "run(): void {");
        let second = node("b#run", "run", NodeKind::Method, "b.ts", "run(): void {");
        let mut pairs = HashSet::new();
        pairs.insert(("a#run", "b#run"));
        pairs.insert(("b#run", "a#run"));
        assert!(is_override_link(&first, &second, &pairs));

        let controller = node("c#delete", "delete", NodeKind::Method, "c.ts", "async delete(id: string) {");
        let service = node("s#delete", "delete", NodeKind::Method, "s.ts", "async delete(id: string) {");
        assert!(!is_override_link(&controller, &service, &HashSet::new()));

        let base = node("base#run", "run", NodeKind::Method, "ports.ts", "abstract run(): void;");
        assert!(is_override_link(&base, &first, &HashSet::new()));
    }

    #[test]
    fn sccs_find_mutual_recursion() {
        let nodes = vec![
            node("a#f", "a.f", NodeKind::Function, "a.ts", "function f() {"),
            node("b#g", "b.g", NodeKind::Function, "b.ts", "function g() {"),
            node("c#h", "c.h", NodeKind::Function, "c.ts", "function h() {"),
        ];
        let edges = vec![edge("a#f", "b#g"), edge("b#g", "a#f")];
        let cycles = sccs(&nodes, &edges);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    #[test]
    fn reach_stops_at_the_start_node() {
        let nodes = vec![
            node("a#f", "f", NodeKind::Function, "a.ts", "function f() {"),
            node("b#g", "g", NodeKind::Function, "b.ts", "function g() {"),
            node("c#h", "h", NodeKind::Function, "c.ts", "function h() {"),
        ];
        let edges = vec![edge("a#f", "b#g"), edge("b#g", "c#h")];
        let (forward, _) = adjacency(&nodes, &edges);
        let reached = reach_from("a#f", &forward);
        assert_eq!(reached.len(), 2);
        assert!(!reached.contains("a#f"));
    }
}
