//! In-memory traversal over a persisted snapshot: slices, search, shortest paths,
//! and impact reachability. SQLite stores the graph; BFS runs here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::RwLock;

mod entry;
mod overview;

pub use entry::{rank_entry_points, EntryPoint};
pub use overview::{build_overview, file_slice, FileSlice, Overview};

use crate::graph::{EdgeKind, GraphEdge, GraphNode};
use crate::store::Store;

#[derive(Debug, Default, Clone)]
pub struct GraphSlice {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusSlice {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub entry: Option<String>,
    pub exit: Option<String>,
    /// Node IDs on the shortest entry → selected → exit route.
    pub route: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct EntrySlice {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
}

#[derive(Debug, Default, Clone)]
pub struct PathResult {
    pub found: bool,
    pub path: Vec<String>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeFilter {
    Call,
    All,
}

impl EdgeFilter {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("all") => EdgeFilter::All,
            _ => EdgeFilter::Call,
        }
    }
}

struct IndexedEdge {
    id: i64,
    source: usize,
    target: usize,
}

fn select_with_budget(selected: &mut HashSet<usize>, node: usize, limit: usize) {
    if selected.len() < limit {
        selected.insert(node);
    }
}

#[derive(Default)]
struct BfsTree {
    distance: Vec<usize>,
    parent: Vec<Option<(usize, i64)>>,
}

fn bfs_calls(index: &GraphIndex, root: usize, reverse: bool) -> BfsTree {
    let n = index.keys.len();
    let mut tree = BfsTree {
        distance: vec![usize::MAX; n],
        parent: vec![None; n],
    };
    let adjacency = if reverse {
        &index.in_call
    } else {
        &index.out_call
    };
    tree.distance[root] = 0;
    let mut queue = VecDeque::new();
    queue.push_back(root);
    while let Some(node) = queue.pop_front() {
        let next_distance = tree.distance[node] + 1;
        for edge_index in &adjacency[node] {
            let edge = &index.edges[*edge_index];
            let next = if reverse { edge.source } else { edge.target };
            if tree.distance[next] != usize::MAX {
                continue;
            }
            tree.distance[next] = next_distance;
            tree.parent[next] = Some((node, edge.id));
            queue.push_back(next);
        }
    }
    tree
}

fn bfs_undirected(index: &GraphIndex, root: usize, max_depth: usize) -> Vec<usize> {
    let mut distance = vec![usize::MAX; index.keys.len()];
    distance[root] = 0;
    let mut queue = VecDeque::new();
    queue.push_back(root);
    while let Some(node) = queue.pop_front() {
        let next_distance = distance[node] + 1;
        if next_distance > max_depth {
            continue;
        }
        for edge_index in index.out_all[node].iter().chain(index.in_all[node].iter()) {
            let edge = &index.edges[*edge_index];
            let next = if edge.source == node {
                edge.target
            } else {
                edge.source
            };
            if next == node || distance[next] != usize::MAX {
                continue;
            }
            distance[next] = next_distance;
            queue.push_back(next);
        }
    }
    distance
}

fn path_toward_root(tree: &BfsTree, node: usize, root: usize) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cursor = node;
    loop {
        path.push(cursor);
        if cursor == root {
            break;
        }
        let Some((next, _)) = tree.parent[cursor] else {
            break;
        };
        cursor = next;
    }
    path
}

/// Reachable nodes sorted by distance, then by score, then by index.
fn ranked_candidates(
    index: &GraphIndex,
    distance: &[usize],
    score: impl Fn(&GraphNode) -> u32,
) -> Vec<(usize, usize, u32)> {
    let mut candidates: Vec<(usize, usize, u32)> = distance
        .iter()
        .enumerate()
        .filter(|(_, distance)| **distance != usize::MAX)
        .map(|(node, distance)| (*distance, node, score(&index.nodes[node])))
        .collect();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.1.cmp(&right.1))
    });
    candidates
}

/// Selects ranked nodes whose distance stays within the cutoff.
fn select_within(
    selected: &mut HashSet<usize>,
    ranked: &[(usize, usize, u32)],
    max_distance: usize,
    limit: usize,
) {
    for (distance, node, _) in ranked {
        if *distance > max_distance {
            break;
        }
        select_with_budget(selected, *node, limit);
    }
}

/// Spends budget on the route from the BFS root to `node`, root side first, and
/// returns the route walking from `node` back towards the root.
fn select_route(
    selected: &mut HashSet<usize>,
    tree: &BfsTree,
    node: usize,
    root: usize,
    limit: usize,
) -> Vec<usize> {
    let path = path_toward_root(tree, node, root);
    // Root-to-node order keeps the budget concentrated near the selected node
    // when the path itself is larger than the limit.
    for step in path.iter().rev() {
        select_with_budget(selected, *step, limit);
    }
    path
}

/// Nearest ranked node that passes `candidate`, together with the budgeted route
/// from the BFS root down to it.
fn nearest_route(
    selected: &mut HashSet<usize>,
    index: &GraphIndex,
    ranked: &[(usize, usize, u32)],
    tree: &BfsTree,
    root: usize,
    limit: usize,
    candidate: impl Fn(&GraphNode) -> bool,
) -> Option<(usize, usize, Vec<usize>)> {
    let (distance, node, _) = ranked
        .iter()
        .find(|(_, node, _)| candidate(&index.nodes[*node]))?;
    Some((
        *distance,
        *node,
        select_route(selected, tree, *node, root, limit),
    ))
}

/// Adds the nodes exactly two undirected hops away.
fn select_second_level(
    selected: &mut HashSet<usize>,
    index: &GraphIndex,
    root: usize,
    limit: usize,
) {
    for (node, distance) in bfs_undirected(index, root, 2).iter().enumerate() {
        if *distance == 2 {
            select_with_budget(selected, node, limit);
        }
    }
}

/// Everything the focus response needs from the selected node set.
struct SelectedSlice {
    keys: Vec<String>,
    edge_ids: Vec<i64>,
    route: Vec<String>,
}

/// Node keys, inner edge ids and the entry → selected → exit route.
fn selected_slice(
    index: &GraphIndex,
    selected: &HashSet<usize>,
    entry_path: &[usize],
    exit_path: &[usize],
) -> SelectedSlice {
    let keys = selected
        .iter()
        .filter_map(|node| index.keys.get(*node).cloned())
        .collect();
    let mut edge_ids = HashSet::new();
    for node in selected {
        for edge_index in index.out_all[*node]
            .iter()
            .chain(index.in_all[*node].iter())
        {
            let edge = &index.edges[*edge_index];
            if selected.contains(&edge.source) && selected.contains(&edge.target) {
                edge_ids.insert(edge.id);
            }
        }
    }
    SelectedSlice {
        keys,
        edge_ids: edge_ids.into_iter().collect(),
        route: route_keys(index, entry_path, exit_path),
    }
}

/// Node keys on the entry → selected → exit route, without duplicates.
fn route_keys(index: &GraphIndex, entry_path: &[usize], exit_path: &[usize]) -> Vec<String> {
    let mut route = Vec::new();
    let mut seen = HashSet::new();
    for node in entry_path.iter().chain(exit_path.iter().rev().skip(1)) {
        if let Some(key) = index.keys.get(*node) {
            if seen.insert(key.clone()) {
                route.push(key.clone());
            }
        }
    }
    route
}

fn direct_neighbors(index: &GraphIndex, root: usize) -> Vec<usize> {
    let mut direct = Vec::new();
    for edge_index in index.out_all[root].iter().chain(index.in_all[root].iter()) {
        let edge = &index.edges[*edge_index];
        let next = if edge.source == root {
            edge.target
        } else {
            edge.source
        };
        if next != root {
            direct.push(next);
        }
    }
    direct.sort_unstable();
    direct.dedup();
    direct
}

struct GraphIndex {
    /// Decoded payloads, positionally aligned with `keys` and the adjacency lists.
    nodes: Vec<GraphNode>,
    keys: Vec<String>,
    key_to_idx: HashMap<String, usize>,
    edges: Vec<IndexedEdge>,
    out_call: Vec<Vec<usize>>,
    in_call: Vec<Vec<usize>>,
    out_all: Vec<Vec<usize>>,
    in_all: Vec<Vec<usize>>,
}

impl GraphIndex {
    async fn load(store: &Store, graph_id: i64) -> Result<Self> {
        let node_rows = store.nodes_with_ids(graph_id).await?;
        let mut nodes = Vec::with_capacity(node_rows.len());
        let mut keys = Vec::with_capacity(node_rows.len());
        let mut id_to_idx: HashMap<i64, usize> = HashMap::with_capacity(node_rows.len());
        for (position, (row_id, node)) in node_rows.into_iter().enumerate() {
            keys.push(node.id.clone());
            id_to_idx.insert(row_id, position);
            nodes.push(node);
        }
        let key_to_idx: HashMap<String, usize> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index))
            .collect();
        let rows = store.edge_rows(graph_id).await?;
        let n = keys.len();
        let mut edges = Vec::with_capacity(rows.len());
        let mut out_call = vec![Vec::new(); n];
        let mut in_call = vec![Vec::new(); n];
        let mut out_all = vec![Vec::new(); n];
        let mut in_all = vec![Vec::new(); n];

        for row in rows {
            let (Some(&source), Some(&target)) =
                (id_to_idx.get(&row.source_id), id_to_idx.get(&row.target_id))
            else {
                continue;
            };
            let kind = match row.kind.as_str() {
                "data" => EdgeKind::Data,
                _ => EdgeKind::Call,
            };
            let edge_index = edges.len();
            edges.push(IndexedEdge {
                id: row.id,
                source,
                target,
            });
            out_all[source].push(edge_index);
            in_all[target].push(edge_index);
            if kind == EdgeKind::Call {
                out_call[source].push(edge_index);
                in_call[target].push(edge_index);
            }
        }

        Ok(Self {
            nodes,
            keys,
            key_to_idx,
            edges,
            out_call,
            in_call,
            out_all,
            in_all,
        })
    }

    fn adjacency(&self, filter: EdgeFilter, reverse: bool) -> &[Vec<usize>] {
        match (filter, reverse) {
            (EdgeFilter::Call, false) => &self.out_call,
            (EdgeFilter::Call, true) => &self.in_call,
            (EdgeFilter::All, false) => &self.out_all,
            (EdgeFilter::All, true) => &self.in_all,
        }
    }
}

fn entry_levels(
    index: &GraphIndex,
    roots: &[String],
    depth: usize,
    limit: usize,
) -> (HashSet<usize>, bool) {
    let mut visited = HashSet::with_capacity(limit.min(index.keys.len()));
    // Spend the budget root by root: each entry point and then its direct
    // callees, before moving to the next entry point. Second-level callees
    // are only considered after every root and first level had a chance.
    let mut first_level: Vec<usize> = Vec::new();
    let mut truncated = false;
    for root in roots {
        let Some(&node) = index.key_to_idx.get(root) else {
            continue;
        };
        if visited.contains(&node) {
            continue;
        }
        if visited.len() >= limit {
            truncated = true;
            break;
        }
        visited.insert(node);
        for edge in &index.out_call[node] {
            let next = index.edges[*edge].target;
            if visited.contains(&next) {
                continue;
            }
            if visited.len() >= limit {
                truncated = true;
                continue;
            }
            visited.insert(next);
            first_level.push(next);
        }
    }
    let mut current_level = first_level;
    for _ in 2..=depth {
        let mut next_level: Vec<usize> = Vec::new();
        for node in current_level {
            for edge in &index.out_call[node] {
                let next = index.edges[*edge].target;
                if visited.contains(&next) {
                    continue;
                }
                if visited.len() >= limit {
                    truncated = true;
                    continue;
                }
                visited.insert(next);
                next_level.push(next);
            }
        }
        if next_level.is_empty() {
            break;
        }
        current_level = next_level;
    }
    (visited, truncated)
}

fn entry_edge_ids(index: &GraphIndex, visited: &HashSet<usize>) -> HashSet<i64> {
    let mut edge_ids = HashSet::with_capacity(visited.len().saturating_mul(2));
    for node in visited {
        for edge in &index.out_call[*node] {
            let edge = &index.edges[*edge];
            if visited.contains(&edge.target) {
                edge_ids.insert(edge.id);
            }
        }
    }
    edge_ids
}

pub struct QueryService {
    store: Store,
    indexes: RwLock<HashMap<i64, Arc<GraphIndex>>>,
}

impl QueryService {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            indexes: RwLock::new(HashMap::new()),
        }
    }

    async fn index(&self, graph_id: i64) -> Result<Arc<GraphIndex>> {
        if let Some(index) = self.indexes.read().await.get(&graph_id) {
            return Ok(index.clone());
        }
        let index = Arc::new(GraphIndex::load(&self.store, graph_id).await?);
        self.indexes.write().await.insert(graph_id, index.clone());
        Ok(index)
    }

    pub async fn bootstrap(&self, graph_id: i64, limit: usize) -> Result<GraphSlice> {
        let index = self.index(graph_id).await?;
        let limit = limit.clamp(1, 5000);
        // Small graphs are sent whole. For larger ones, reserve half the budget
        // for one-hop neighbors; otherwise top-ranked roots would be isolated.
        let total = self.store.node_count(graph_id).await?;
        let root_limit = if total <= limit {
            total
        } else {
            (limit / 2).max(1)
        };
        let roots = self.store.top_nodes(graph_id, root_limit).await?;
        self.slice_from_keys(&index, graph_id, &roots, 1, limit)
            .await
    }

    pub async fn subgraph(
        &self,
        graph_id: i64,
        roots: &[String],
        depth: usize,
        limit: usize,
    ) -> Result<GraphSlice> {
        let index = self.index(graph_id).await?;
        self.slice_from_keys(&index, graph_id, roots, depth, limit)
            .await
    }

    /// Outgoing call edges only. Entry-point roots must not pull in their
    /// external callers through the reverse adjacency used by neighborhood
    /// slices.
    pub async fn entry_slice(
        &self,
        graph_id: i64,
        roots: &[String],
        depth: usize,
        limit: usize,
    ) -> Result<EntrySlice> {
        let index = self.index(graph_id).await?;
        let depth = depth.clamp(1, 8);
        let limit = limit.clamp(1, 5000);
        let (visited, truncated) = entry_levels(&index, roots, depth, limit);
        let mut keys: Vec<String> = Vec::with_capacity(visited.len());
        keys.extend(
            visited
                .iter()
                .filter_map(|node| index.keys.get(*node).cloned()),
        );
        let edge_ids = entry_edge_ids(&index, &visited);
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self
            .store
            .edges_by_ids(graph_id, &edge_ids.into_iter().collect::<Vec<_>>())
            .await?;
        Ok(EntrySlice {
            nodes,
            edges,
            truncated,
        })
    }

    /// Focused exploration slice.
    ///
    /// Priority order:
    /// 1. the selected node and all direct neighbours;
    /// 2. the shortest caller path to the nearest entry point;
    /// 3. the shortest callee path to the nearest exit point;
    /// 4. all undirected neighbours at exactly two hops;
    /// 5. remaining callers up to the entry point, then callees up to the
    ///    exit point (the limit still applies).
    ///
    /// The two shortest paths together form the execution route through the
    /// selected node. The budget is spent on context around that route instead
    /// of unrelated entry-point branches.
    pub async fn focus_slice(&self, graph_id: i64, root: &str, limit: usize) -> Result<FocusSlice> {
        let index = self.index(graph_id).await?;
        let Some(&root_idx) = index.key_to_idx.get(root) else {
            return Ok(FocusSlice::default());
        };
        let limit = limit.clamp(1, 5000);
        let mut selected = HashSet::with_capacity(limit.min(index.keys.len()));
        select_with_budget(&mut selected, root_idx, limit);

        // 1. Direct neighbours, both incoming and outgoing, all edge kinds.
        for next in direct_neighbors(&index, root_idx) {
            select_with_budget(&mut selected, next, limit);
        }

        // Reverse call BFS: distance from the selected node to each caller.
        // Ranked once: the nearest entry is the first matching candidate and the
        // remaining ancestors keep that order, so step 5 reuses the list.
        let reverse = bfs_calls(&index, root_idx, true);
        let callers = ranked_candidates(&index, &reverse.distance, |node| {
            node.metrics.transitive_fan_out
        });
        // 2. Nearest entry point plus the caller path down to the selection.
        let entry = nearest_route(
            &mut selected,
            &index,
            &callers,
            &reverse,
            root_idx,
            limit,
            entry::is_entry_candidate,
        );

        // Forward call BFS: shortest callee paths from the selected node.
        let forward = bfs_calls(&index, root_idx, false);
        let callees = ranked_candidates(&index, &forward.distance, |node| node.metrics.height);
        // 3. Nearest exit point plus the callee path from the selection.
        let exit = nearest_route(
            &mut selected,
            &index,
            &callees,
            &forward,
            root_idx,
            limit,
            entry::is_exit_candidate,
        );

        // 4. Second-level neighbours in the undirected slice.
        select_second_level(&mut selected, &index, root_idx, limit);

        // 5. Remaining callers up to the nearest entry point. When the component
        // has no entry candidate, the whole caller closure is used instead (the
        // limit still applies).
        let ancestor_limit = entry
            .as_ref()
            .map_or(usize::MAX, |(distance, _, _)| *distance);
        select_within(&mut selected, &callers, ancestor_limit, limit);

        // 6. Remaining callees up to the nearest exit point.
        let descendant_limit = exit
            .as_ref()
            .map_or(usize::MAX, |(distance, _, _)| *distance);
        select_within(&mut selected, &callees, descendant_limit, limit);

        let output = selected_slice(
            &index,
            &selected,
            entry
                .as_ref()
                .map_or(&[][..], |(_, _, path)| path.as_slice()),
            exit.as_ref()
                .map_or(&[][..], |(_, _, path)| path.as_slice()),
        );
        let nodes = self.store.nodes_by_keys(graph_id, &output.keys).await?;
        let edges = self.store.edges_by_ids(graph_id, &output.edge_ids).await?;
        Ok(FocusSlice {
            nodes,
            edges,
            entry: entry.and_then(|(_, node, _)| index.keys.get(node).cloned()),
            exit: exit.and_then(|(_, node, _)| index.keys.get(node).cloned()),
            route: output.route,
        })
    }

    async fn slice_from_keys(
        &self,
        index: &GraphIndex,
        graph_id: i64,
        roots: &[String],
        depth: usize,
        limit: usize,
    ) -> Result<GraphSlice> {
        let depth = depth.min(8);
        let limit = limit.clamp(1, 5000);
        let capacity = limit.min(index.keys.len());
        let mut visited: HashSet<usize> = HashSet::with_capacity(capacity);
        let mut queue: VecDeque<(usize, usize)> = VecDeque::with_capacity(capacity);
        for root in roots {
            if let Some(&node) = index.key_to_idx.get(root) {
                if visited.insert(node) {
                    queue.push_back((node, 0));
                }
            }
        }
        while let Some((node, distance)) = queue.pop_front() {
            if distance >= depth || visited.len() >= limit {
                continue;
            }
            for edge in index.out_all[node].iter().chain(index.in_all[node].iter()) {
                let edge = &index.edges[*edge];
                let next = if edge.source == node {
                    edge.target
                } else {
                    edge.source
                };
                if visited.insert(next) {
                    if visited.len() >= limit {
                        break;
                    }
                    queue.push_back((next, distance + 1));
                }
            }
        }
        let mut keys: Vec<String> = Vec::with_capacity(visited.len());
        keys.extend(
            visited
                .iter()
                .filter_map(|node| index.keys.get(*node).cloned()),
        );
        let mut edge_ids: HashSet<i64> = HashSet::with_capacity(visited.len().saturating_mul(2));
        for node in &visited {
            for edge in index.out_all[*node]
                .iter()
                .chain(index.in_all[*node].iter())
            {
                let edge = &index.edges[*edge];
                if visited.contains(&edge.source) && visited.contains(&edge.target) {
                    edge_ids.insert(edge.id);
                }
            }
        }
        let mut edge_id_list: Vec<i64> = Vec::with_capacity(edge_ids.len());
        edge_id_list.extend(edge_ids);
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self.store.edges_by_ids(graph_id, &edge_id_list).await?;
        Ok(GraphSlice { nodes, edges })
    }

    pub async fn path(
        &self,
        graph_id: i64,
        from: &str,
        to: &str,
        filter: EdgeFilter,
        max_depth: usize,
    ) -> Result<PathResult> {
        let index = self.index(graph_id).await?;
        let (Some(&from_idx), Some(&to_idx)) =
            (index.key_to_idx.get(from), index.key_to_idx.get(to))
        else {
            return Ok(PathResult::default());
        };
        if from_idx == to_idx {
            let nodes = self
                .store
                .nodes_by_keys(graph_id, &[from.to_string()])
                .await?;
            return Ok(PathResult {
                found: true,
                path: vec![from.to_string()],
                nodes,
                edges: Vec::new(),
            });
        }
        let max_depth = max_depth.clamp(1, 64);
        let adjacency = index.adjacency(filter, false);
        let mut distance = vec![usize::MAX; index.keys.len()];
        let mut parent: Vec<Option<(usize, i64)>> = vec![None; index.keys.len()];
        let mut queue = VecDeque::new();
        distance[from_idx] = 0;
        queue.push_back(from_idx);
        let mut found = false;
        while let Some(node) = queue.pop_front() {
            if distance[node] >= max_depth {
                continue;
            }
            for edge_index in &adjacency[node] {
                let edge = &index.edges[*edge_index];
                let next = if edge.source == node {
                    edge.target
                } else {
                    edge.source
                };
                if distance[next] != usize::MAX {
                    continue;
                }
                distance[next] = distance[node] + 1;
                parent[next] = Some((node, edge.id));
                if next == to_idx {
                    found = true;
                    break;
                }
                queue.push_back(next);
            }
            if found {
                break;
            }
        }
        if !found {
            return Ok(PathResult::default());
        }
        let mut path_indices = vec![to_idx];
        let mut edge_ids = Vec::new();
        let mut cursor = to_idx;
        while cursor != from_idx {
            let Some((previous, edge_id)) = parent[cursor] else {
                return Ok(PathResult::default());
            };
            edge_ids.push(edge_id);
            cursor = previous;
            path_indices.push(cursor);
        }
        path_indices.reverse();
        edge_ids.reverse();
        let path: Vec<String> = path_indices
            .iter()
            .filter_map(|node| index.keys.get(*node).cloned())
            .collect();
        let nodes = self.store.nodes_by_keys(graph_id, &path).await?;
        let edges = self.store.edges_by_ids_ordered(graph_id, &edge_ids).await?;
        Ok(PathResult {
            found: true,
            path,
            nodes,
            edges,
        })
    }

    pub async fn impact(
        &self,
        graph_id: i64,
        node: &str,
        depth: usize,
        reverse: bool,
    ) -> Result<GraphSlice> {
        let index = self.index(graph_id).await?;
        let Some(&start) = index.key_to_idx.get(node) else {
            return Ok(GraphSlice::default());
        };
        let depth = depth.clamp(1, 8);
        let limit = 5000;
        let adjacency = index.adjacency(EdgeFilter::Call, reverse);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back((start, 0));
        while let Some((current, distance)) = queue.pop_front() {
            if distance >= depth || visited.len() >= limit {
                continue;
            }
            for edge_index in &adjacency[current] {
                let edge = &index.edges[*edge_index];
                let next = if edge.source == current {
                    edge.target
                } else {
                    edge.source
                };
                if visited.insert(next) {
                    if visited.len() >= limit {
                        break;
                    }
                    queue.push_back((next, distance + 1));
                }
            }
        }
        let mut keys: Vec<String> = Vec::with_capacity(visited.len());
        keys.extend(
            visited
                .iter()
                .filter_map(|node| index.keys.get(*node).cloned()),
        );
        let mut edge_ids: HashSet<i64> = HashSet::with_capacity(visited.len().saturating_mul(2));
        for node in &visited {
            for edge in index.out_all[*node]
                .iter()
                .chain(index.in_all[*node].iter())
            {
                let edge = &index.edges[*edge];
                if visited.contains(&edge.source) && visited.contains(&edge.target) {
                    edge_ids.insert(edge.id);
                }
            }
        }
        let mut edge_id_list: Vec<i64> = Vec::with_capacity(edge_ids.len());
        edge_id_list.extend(edge_ids);
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self.store.edges_by_ids(graph_id, &edge_id_list).await?;
        Ok(GraphSlice { nodes, edges })
    }
}
