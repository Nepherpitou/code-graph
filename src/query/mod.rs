//! In-memory traversal over a persisted snapshot: slices, search, shortest paths,
//! and impact reachability. SQLite stores the graph; BFS runs here.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

mod overview;

pub use overview::{build_overview, file_slice, FileSlice, Overview};

use crate::graph::{EdgeKind, GraphEdge, GraphNode};
use crate::store::Store;

#[derive(Debug, Default, Clone)]
pub struct GraphSlice {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
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
