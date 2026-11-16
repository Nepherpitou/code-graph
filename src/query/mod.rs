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

fn ranked_candidates(
    distance: &[usize],
    keys: &[String],
    node_by_key: &HashMap<&str, &GraphNode>,
    max_distance: usize,
    predicate: impl Fn(&GraphNode) -> bool,
    score: impl Fn(&GraphNode) -> u32,
) -> Vec<(usize, usize, u32)> {
    let mut candidates: Vec<(usize, usize, u32)> = distance
        .iter()
        .enumerate()
        .filter(|(_, distance)| **distance != usize::MAX && **distance <= max_distance)
        .filter_map(|(node, distance)| {
            let candidate = node_by_key.get(keys[node].as_str())?;
            predicate(candidate).then_some((*distance, node, score(candidate)))
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.1.cmp(&right.1))
    });
    candidates
}

fn select_ranked(selected: &mut HashSet<usize>, candidates: &[(usize, usize, u32)], limit: usize) {
    for (_, node, _) in candidates {
        select_with_budget(selected, *node, limit);
    }
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
        let node_rows = store.node_index(graph_id).await?;
        let keys: Vec<String> = node_rows.iter().map(|(_, key)| key.clone()).collect();
        let key_to_idx: HashMap<String, usize> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index))
            .collect();
        let id_to_idx: HashMap<i64, usize> = node_rows
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
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
        let mut visited: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        for root in roots {
            if let Some(&node) = index.key_to_idx.get(root) {
                if visited.insert(node) {
                    queue.push_back((node, 0));
                }
            }
        }
        let mut truncated = false;
        while let Some((node, distance)) = queue.pop_front() {
            if distance >= depth {
                continue;
            }
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
                queue.push_back((next, distance + 1));
            }
        }
        let keys: Vec<String> = visited
            .iter()
            .filter_map(|node| index.keys.get(*node).cloned())
            .collect();
        let mut edge_ids = HashSet::new();
        for node in &visited {
            for edge in &index.out_call[*node] {
                let edge = &index.edges[*edge];
                if visited.contains(&edge.target) {
                    edge_ids.insert(edge.id);
                }
            }
        }
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
        let nodes = self.store.all_nodes(graph_id).await?;
        let node_by_key: HashMap<&str, &GraphNode> =
            nodes.iter().map(|node| (node.id.as_str(), node)).collect();
        let mut selected = HashSet::new();
        select_with_budget(&mut selected, root_idx, limit);

        // 1. Direct neighbours, both incoming and outgoing, all edge kinds.
        for next in direct_neighbors(&index, root_idx) {
            select_with_budget(&mut selected, next, limit);
        }

        // Reverse call BFS: distance from the selected node to each caller.
        let reverse = bfs_calls(&index, root_idx, true);
        let distance = &reverse.distance;

        let entries = ranked_candidates(
            distance,
            &index.keys,
            &node_by_key,
            usize::MAX,
            entry::is_entry_candidate,
            |node| node.metrics.transitive_fan_out,
        );

        // 2. Add the caller path from the nearest entry point to the selection.
        let mut entry_distance = None;
        let mut entry_node = None;
        let mut entry_path = Vec::new();
        if let Some((nearest_distance, entry_idx, _)) = entries.first().copied() {
            entry_distance = Some(nearest_distance);
            entry_node = Some(entry_idx);
            let path = path_toward_root(&reverse, entry_idx, root_idx);
            // Root-to-entry order keeps the budget concentrated near the
            // selected node when the path itself is larger than the limit.
            for node in path.iter().rev() {
                select_with_budget(&mut selected, *node, limit);
            }
            entry_path = path;
        }

        // Forward call BFS: shortest callee paths from the selected node.
        let forward = bfs_calls(&index, root_idx, false);
        let out_distance = &forward.distance;

        let exits = ranked_candidates(
            out_distance,
            &index.keys,
            &node_by_key,
            usize::MAX,
            entry::is_exit_candidate,
            |node| node.metrics.height,
        );

        // 3. Add the callee path from the selection to the nearest exit.
        let mut exit_distance = None;
        let mut exit_node = None;
        let mut exit_path = Vec::new();
        if let Some((nearest_distance, exit_idx, _)) = exits.first().copied() {
            exit_distance = Some(nearest_distance);
            exit_node = Some(exit_idx);
            let path = path_toward_root(&forward, exit_idx, root_idx);
            // Root-to-exit order, same budget rule as the caller path.
            for node in path.iter().rev() {
                select_with_budget(&mut selected, *node, limit);
            }
            exit_path = path;
        }

        // 4. Second-level neighbours in the undirected slice.
        let undirected = bfs_undirected(&index, root_idx, 2);
        let mut second: Vec<(usize, usize)> = undirected
            .iter()
            .enumerate()
            .filter(|(_, distance)| **distance == 2)
            .map(|(node, _)| node)
            .map(|node| (node, 0))
            .collect();
        second.sort_unstable();
        for (node, _) in second {
            select_with_budget(&mut selected, node, limit);
        }

        // 5. Remaining callers up to the nearest entry point. When the
        // component has no entry candidate, use the whole caller closure (the
        // limit still applies).
        let ancestor_limit = entry_distance.unwrap_or(usize::MAX);
        let ancestors = ranked_candidates(
            distance,
            &index.keys,
            &node_by_key,
            ancestor_limit,
            |_| true,
            |node| node.metrics.transitive_fan_out,
        );
        select_ranked(&mut selected, &ancestors, limit);

        // 6. Remaining callees up to the nearest exit point.
        let descendant_limit = exit_distance.unwrap_or(usize::MAX);
        let descendants = ranked_candidates(
            out_distance,
            &index.keys,
            &node_by_key,
            descendant_limit,
            |_| true,
            |node| node.metrics.height,
        );
        select_ranked(&mut selected, &descendants, limit);

        let keys: Vec<String> = selected
            .iter()
            .filter_map(|node| index.keys.get(*node).cloned())
            .collect();
        let mut edge_ids = HashSet::new();
        for node in &selected {
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
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self
            .store
            .edges_by_ids(graph_id, &edge_ids.into_iter().collect::<Vec<_>>())
            .await?;
        let mut route_keys = Vec::new();
        let mut route_seen = HashSet::new();
        for node in entry_path.iter().chain(exit_path.iter().rev().skip(1)) {
            if let Some(key) = index.keys.get(*node) {
                if route_seen.insert(key.clone()) {
                    route_keys.push(key.clone());
                }
            }
        }
        Ok(FocusSlice {
            nodes,
            edges,
            entry: entry_node.and_then(|node| index.keys.get(node).cloned()),
            exit: exit_node.and_then(|node| index.keys.get(node).cloned()),
            route: route_keys,
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
        let mut visited: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
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
        let keys: Vec<String> = visited
            .iter()
            .filter_map(|node| index.keys.get(*node).cloned())
            .collect();
        let mut edge_ids = HashSet::new();
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
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self
            .store
            .edges_by_ids(graph_id, &edge_ids.into_iter().collect::<Vec<_>>())
            .await?;
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
        let keys: Vec<String> = visited
            .iter()
            .filter_map(|node| index.keys.get(*node).cloned())
            .collect();
        let mut edge_ids = HashSet::new();
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
        let nodes = self.store.nodes_by_keys(graph_id, &keys).await?;
        let edges = self
            .store
            .edges_by_ids(graph_id, &edge_ids.into_iter().collect::<Vec<_>>())
            .await?;
        Ok(GraphSlice { nodes, edges })
    }
}
