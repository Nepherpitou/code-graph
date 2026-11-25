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

use crate::graph::conventions::{call_pairs, wiring_exclusion, Bucket, Conventions};
use crate::graph::{EdgeKind, GraphEdge, GraphNode, NodeKind};
use crate::store::GraphSummary;

use super::args::{AuditDiffArgs, AuditDiffView, AuditFormat};
use super::audit::{pad, print_json, snapshot_json};
use super::audit_diff::{DiffStatus, NodeDiff};

mod views;

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

struct EdgeView<'a> {
    src: &'a GraphNode,
    dst: &'a GraphNode,
    sites: u32,
}

/// Analyzed root plus the line cache of the call-site text check.
struct Verification {
    root: String,
    cache: HashMap<String, Vec<String>>,
}

impl Verification {
    fn new(root: &str) -> Self {
        Self {
            root: root.to_string(),
            cache: HashMap::new(),
        }
    }

    fn calls(&mut self, src_file: &str, callee: &str, line: u32) -> bool {
        line_calls(&self.root, src_file, callee, line, &mut self.cache)
    }
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

/// The earliest call site line of an edge, used for grouping and the text check.
fn site_line(edge: &GraphEdge) -> u32 {
    edge.call_sites.iter().map(|site| site.line).min().unwrap_or(0)
}

/// Call edges added by the target snapshot, split into usable ones and buckets.
fn new_call_edges<'a>(
    input: &'a ViewInput<'a>,
    graphs: &Graphs<'a>,
) -> (Vec<EdgeView<'a>>, HashMap<Bucket, Vec<EdgeView<'a>>>) {
    let conventions = Conventions::of(&input.to.meta.language);
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
        match wiring_exclusion(src, dst, &graphs.new_pairs, conventions) {
            Some(bucket) => dropped.entry(bucket).or_default().push(view),
            None => candidates.push((view, edge)),
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
    let mut verify = input
        .verify_calls
        .then(|| Verification::new(&input.to.meta.root));
    let mut seen = vec![false; candidates.len()];
    for group in by_site.values() {
        let abstract_names: HashSet<&str> = group
            .iter()
            .map(|&index| candidates[index].0.dst)
            .filter(|dst| conventions.is_abstract(dst))
            .map(|dst| dst.name.as_str())
            .collect();
        for &index in group {
            if seen[index] {
                continue;
            }
            seen[index] = true;
            let (view, edge) = &candidates[index];
            if !conventions.is_abstract(view.dst) && abstract_names.contains(view.dst.name.as_str()) {
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
            if let Some(context) = verify.as_mut() {
                if !context.calls(&view.src.file, &view.dst.name, site_line(edge)) {
                    unverified.push(EdgeView {
                        src: view.src,
                        dst: view.dst,
                        sites: view.sites,
                    });
                    continue;
                }
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

/// Call edges of one snapshot that carry real dependencies: overrides, tests,
/// DI wiring and — when a verification context is given — call sites whose text
/// does not name the callee are removed.
fn graph_call_edges<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    mut verify: Option<&mut Verification>,
    conventions: &Conventions,
) -> Vec<EdgeView<'a>> {
    let by_id: HashMap<&str, &GraphNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let pairs = call_pairs(edges);
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
        if wiring_exclusion(src, dst, &pairs, conventions).is_some() {
            continue;
        }
        if let Some(context) = verify.as_mut() {
            if !context.calls(&src.file, &dst.name, site_line(edge)) {
                continue;
            }
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

fn adjacency<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    verify: Option<&mut Verification>,
    conventions: &Conventions,
) -> (Adjacency<'a>, Adjacency<'a>) {
    let mut forward: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
    let mut reverse: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in graph_call_edges(nodes, edges, verify, conventions) {
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

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Mean cyclomatic of the touched callables, split so that both columns of a
/// row describe the same symbols: `(changed_before, changed_after, added_after)`.
fn touched_complexity(input: &ViewInput<'_>) -> (f64, f64, f64) {
    let old_by_key: HashMap<(&str, &str), &GraphNode> = input
        .old_nodes
        .iter()
        .map(|node| ((node.file.as_str(), node.qname.as_str()), node))
        .collect();
    let new_by_key: HashMap<(&str, &str), &GraphNode> = input
        .new_nodes
        .iter()
        .map(|node| ((node.file.as_str(), node.qname.as_str()), node))
        .collect();
    let mut changed_before = Vec::new();
    let mut changed_after = Vec::new();
    let mut added_after = Vec::new();
    for row in input.rows {
        if row.kind != NodeKind::Function.as_str() && row.kind != NodeKind::Method.as_str() {
            continue;
        }
        let key = (row.file.as_str(), row.qname.as_str());
        match row.status {
            DiffStatus::Added => {
                if let Some(node) = new_by_key.get(&key) {
                    added_after.push(f64::from(node.cyclomatic));
                }
            }
            DiffStatus::Changed | DiffStatus::Moved => {
                if let Some(node) = new_by_key.get(&key) {
                    changed_after.push(f64::from(node.cyclomatic));
                }
                if let Some(node) = old_by_key.get(&key) {
                    changed_before.push(f64::from(node.cyclomatic));
                }
            }
            DiffStatus::Removed => {}
        }
    }
    (
        mean(&changed_before),
        mean(&changed_after),
        mean(&added_after),
    )
}

fn totals(nodes: &[GraphNode], conventions: &Conventions) -> (f64, f64, f64, i64) {
    let mut cyclomatic = 0.0;
    let mut loc = 0.0;
    let mut priority = 0.0;
    let mut dead = 0i64;
    for node in nodes {
        cyclomatic += f64::from(node.cyclomatic);
        loc += f64::from(node.loc);
        priority += node.metrics.refactor_priority;
        if node.metrics.dead_code_candidate && !conventions.is_test_node(node) {
            dead += 1;
        }
    }
    (cyclomatic, loc, priority, dead)
}

struct TarjanState<'a> {
    index: HashMap<&'a str, usize>,
    low: HashMap<&'a str, usize>,
    stack: Vec<&'a str>,
    on_stack: HashSet<&'a str>,
    counter: usize,
    out: Vec<Vec<&'a str>>,
}

fn sccs<'a>(
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    verify: Option<&mut Verification>,
    conventions: &Conventions,
) -> Vec<Vec<&'a str>> {
    let (forward, _) = adjacency(nodes, edges, verify, conventions);
    let mut state = TarjanState {
        index: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        on_stack: HashSet::new(),
        counter: 0,
        out: Vec::new(),
    };
    let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for root in ids {
        if state.index.contains_key(root) {
            continue;
        }
        strongconnect(root, &forward, &mut state);
    }
    state.out
}

fn strongconnect<'a>(
    node: &'a str,
    forward: &HashMap<&'a str, Vec<&'a str>>,
    state: &mut TarjanState<'a>,
) {
    let TarjanState {
        index,
        low,
        stack,
        on_stack,
        counter,
        out,
    } = state;
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
        AuditDiffView::Summary => views::summary(input),
        AuditDiffView::Uses => views::uses(input, &graphs),
        AuditDiffView::Dependencies => views::dependencies(input, &graphs),
        AuditDiffView::Callers => views::callers(input, &graphs),
        AuditDiffView::Affected => views::affected(input, &graphs),
        AuditDiffView::Areas => views::areas(input),
        AuditDiffView::FilteredEdges => views::filtered_edges(input, &graphs),
        AuditDiffView::Cycles => views::cycles(input),
        AuditDiffView::DeadCode => views::dead_code(input),
        AuditDiffView::Reach => views::reach(input, &graphs),
        AuditDiffView::FanIn => views::fan_in(input, &graphs),
        AuditDiffView::Similar => views::similar(input, &graphs),
        AuditDiffView::SilentFiles => views::silent_files(input),
        AuditDiffView::Symbols => bail!("symbols view is handled by the diff itself"),
    }
}
















#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::conventions::is_override_link;
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
    fn conventions_follow_the_snapshot_language() {
        let ts = Conventions::of("typescript");
        let rust = Conventions::of("rust");
        assert!(ts.is_framework_file("src/users/users.controller.ts"));
        assert!(!rust.is_framework_file("src/users/users.controller.ts"));
        assert!(ts.is_test_file("src/users/users.spec.ts"));
        assert!(!rust.is_test_file("src/users/users.spec.ts"));
        assert!(rust.is_test_node(&node("t#f", "tests.case", NodeKind::Function, "src/lib.rs", "fn case(")));
        assert!(!rust.is_test_node(&node("f#f", "case", NodeKind::Function, "src/lib.rs", "fn case(")));
        assert!(ts.is_abstract(&node("a#f", "f", NodeKind::Method, "a.ts", "abstract f(")));
        assert!(!rust.is_abstract(&node("a#f", "f", NodeKind::Method, "a.rs", "abstract f(")));
        assert_eq!(ts.silent_reason("src/x/user.dto.ts"), "dto-schema");
        assert_eq!(rust.silent_reason("src/x/user.dto.ts"), "no-callable-delta");
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
        assert!(is_override_link(&first, &second, &pairs, Conventions::of("typescript")));

        let controller = node("c#delete", "delete", NodeKind::Method, "c.ts", "async delete(id: string) {");
        let service = node("s#delete", "delete", NodeKind::Method, "s.ts", "async delete(id: string) {");
        assert!(!is_override_link(&controller, &service, &HashSet::new(), Conventions::of("typescript")));

        let base = node("base#run", "run", NodeKind::Method, "ports.ts", "abstract run(): void;");
        assert!(is_override_link(&base, &first, &HashSet::new(), Conventions::of("typescript")));
    }

    #[test]
    fn sccs_find_mutual_recursion() {
        let nodes = vec![
            node("a#f", "a.f", NodeKind::Function, "a.ts", "function f() {"),
            node("b#g", "b.g", NodeKind::Function, "b.ts", "function g() {"),
            node("c#h", "c.h", NodeKind::Function, "c.ts", "function h() {"),
        ];
        let edges = vec![edge("a#f", "b#g"), edge("b#g", "a#f")];
        let cycles = sccs(&nodes, &edges, None, Conventions::of("typescript"));
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    fn similar_sets<'a>(entries: &[(&'a str, &[&'a str])]) -> HashMap<&'a str, HashSet<&'a str>> {
        entries
            .iter()
            .map(|(id, deps)| (*id, deps.iter().copied().collect()))
            .collect()
    }

    fn node_index(nodes: &[GraphNode]) -> HashMap<&str, &GraphNode> {
        nodes.iter().map(|node| (node.id.as_str(), node)).collect()
    }

    fn touched(ids: &[&'static str]) -> HashMap<&'static str, DiffStatus> {
        ids.iter().map(|id| (*id, DiffStatus::Changed)).collect()
    }

    #[test]
    fn similar_reports_the_closest_partner_of_a_touched_symbol() {
        let nodes = vec![
            node("a#f", "a", NodeKind::Method, "f.ts", "a() {"),
            node("b#g", "b", NodeKind::Method, "g.ts", "b() {"),
        ];
        let sets = similar_sets(&[
            ("a#f", &["x", "y", "z", "q"][..]),
            ("b#g", &["x", "y", "z", "w"][..]),
        ]);
        let rows = views::find_similar(
            &sets,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f"]),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].shared, 3);
        assert!((rows[0].containment - 0.75).abs() < 1e-9);
        assert_eq!(rows[0].b.1, "b");
        assert_eq!(rows[0].shared_calls, vec!["x", "y", "z"]);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_none());
    }

    #[test]
    fn similar_skips_leaves_and_direct_calls() {
        let leaves = similar_sets(&[("a#f", &["x", "y"][..]), ("b#g", &["x", "y"][..])]);
        let nodes = vec![
            node("a#f", "a", NodeKind::Method, "f.ts", "a() {"),
            node("b#g", "b", NodeKind::Method, "g.ts", "b() {"),
        ];
        assert!(views::find_similar(
            &leaves,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f"]),
        )
        .is_empty());

        let caller = similar_sets(&[
            ("a#f", &["x", "y", "z", "b#g"][..]),
            ("b#g", &["x", "y", "z"][..]),
        ]);
        assert!(views::find_similar(
            &caller,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f"]),
        )
        .is_empty());
    }

    #[test]
    fn similar_skips_pairs_that_already_matched_in_the_base() {
        let nodes = vec![
            node("a#f", "a", NodeKind::Method, "f.ts", "a() {"),
            node("b#g", "b", NodeKind::Method, "g.ts", "b() {"),
        ];
        let sets = similar_sets(&[
            ("a#f", &["x", "y", "z"][..]),
            ("b#g", &["x", "y", "z"][..]),
        ]);
        let old: HashSet<(&str, &str)> = [("h.ts", "x"), ("h.ts", "y"), ("h.ts", "z")]
            .into_iter()
            .collect();
        let mut base = HashMap::new();
        base.insert(("f.ts", "a"), old.clone());
        base.insert(("g.ts", "b"), old);
        assert!(views::find_similar(
            &sets,
            &node_index(&nodes),
            &base,
            &HashMap::new(),
            &touched(&["a#f"]),
        )
        .is_empty());
    }

    #[test]
    fn similar_keeps_only_the_best_partner_and_one_row_per_pair() {
        let nodes = vec![
            node("a#f", "a", NodeKind::Method, "f.ts", "a() {"),
            node("b#g", "b", NodeKind::Method, "g.ts", "b() {"),
            node("c#h", "c", NodeKind::Method, "h.ts", "c() {"),
        ];
        let sets = similar_sets(&[
            ("a#f", &["x", "y", "z", "q"][..]),
            ("b#g", &["x", "y", "z"][..]),
            ("c#h", &["x", "y", "z", "q"][..]),
        ]);
        let rows = views::find_similar(
            &sets,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f"]),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].b.1, "c");

        let both = views::find_similar(
            &sets,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f", "c#h"]),
        );
        assert_eq!(both.len(), 1);
    }

    #[test]
    fn similar_accepts_half_overlap_and_rejects_less() {
        let nodes = vec![
            node("a#f", "a", NodeKind::Method, "f.ts", "a() {"),
            node("b#g", "b", NodeKind::Method, "g.ts", "b() {"),
        ];
        let half = similar_sets(&[
            ("a#f", &["x", "y", "z", "m", "n", "o"][..]),
            ("b#g", &["x", "y", "z"][..]),
        ]);
        assert_eq!(
            views::find_similar(
                &half,
                &node_index(&nodes),
                &HashMap::new(),
                &HashMap::new(),
                &touched(&["a#f"]),
            )
            .len(),
            1
        );

        let less = similar_sets(&[
            ("a#f", &["x", "y", "z", "m", "n", "o", "t"][..]),
            ("b#g", &["x", "y", "z", "p", "q", "r", "s"][..]),
        ]);
        assert!(views::find_similar(
            &less,
            &node_index(&nodes),
            &HashMap::new(),
            &HashMap::new(),
            &touched(&["a#f"]),
        )
        .is_empty());
    }

    #[test]
    fn dependency_graph_keeps_one_id_per_port_name_and_skips_framework_files() {
        let nodes = vec![
            node("c#f", "f", NodeKind::Function, "c.ts", "function f() {"),
            node("d#g", "g", NodeKind::Function, "d.ts", "function g() {"),
            node("k#h", "h", NodeKind::Function, "c.controller.ts", "h() {"),
            node("p#mark", "mark", NodeKind::Method, "ports/p.ts", "abstract mark(): void;"),
            node("i#mark", "mark", NodeKind::Method, "infra/i.ts", "mark(): void {"),
        ];
        let edges = vec![
            edge("c#f", "p#mark"),
            edge("c#f", "i#mark"),
            edge("d#g", "p#mark"),
            edge("k#h", "p#mark"),
        ];
        let sets = views::dependency_graph(&nodes, &edges, None, Conventions::of("typescript"));
        assert_eq!(sets["c#f"].len(), 1);
        assert!(sets["c#f"].contains("p#mark"));
        assert_eq!(sets["d#g"].len(), 1);
        assert!(!sets.contains_key("k#h"));
    }

    #[test]
    fn fan_in_flags_only_real_choke_points() {
        assert!(views::absorbed_callers(1, 4, 2));
        assert!(!views::absorbed_callers(0, 3, 1));
        assert!(!views::absorbed_callers(3, 4, 3));
    }

    #[test]
    fn transitive_fan_out_is_a_cli_value() {
        use clap::ValueEnum;
        let diff = crate::cli::args::AuditDiffOrder::TransitiveFanOut;
        assert_eq!(diff.to_possible_value().unwrap().get_name(), "transitive-fan-out");
        let top = crate::cli::args::AuditOrder::TransitiveFanOut;
        assert_eq!(top.to_possible_value().unwrap().get_name(), "transitive-fan-out");
    }

    #[test]
    fn reach_stops_at_the_start_node() {
        let nodes = vec![
            node("a#f", "f", NodeKind::Function, "a.ts", "function f() {"),
            node("b#g", "g", NodeKind::Function, "b.ts", "function g() {"),
            node("c#h", "h", NodeKind::Function, "c.ts", "function h() {"),
        ];
        let edges = vec![edge("a#f", "b#g"), edge("b#g", "c#h")];
        let (forward, _) = adjacency(&nodes, &edges, None, Conventions::of("typescript"));
        let reached = reach_from("a#f", &forward);
        assert_eq!(reached.len(), 2);
        assert!(!reached.contains("a#f"));
    }
}
