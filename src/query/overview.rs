//! File/directory overview and file slices derived from a persisted graph.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::graph::{GraphEdge, GraphNode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub path: String,
    pub nodes: usize,
    pub callables: usize,
    pub constants: usize,
    pub cycle_nodes: usize,
    pub dead_code: usize,
    pub hotspot: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub calls: u32,
    pub data: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub group: String,
    pub total_nodes: usize,
    pub nodes: Vec<OverviewNode>,
    pub edges: Vec<OverviewEdge>,
}

#[derive(Debug, Default)]
pub struct FileSlice {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total: usize,
    pub truncated: bool,
}

fn cluster_for(file: &str, group: &str) -> (String, String) {
    if group == "file" {
        return (format!("file:{file}"), file.to_string());
    }
    let path = if !file.contains('/') {
        ".".to_string()
    } else {
        let parts: Vec<&str> = file.split('/').collect();
        if parts.first() == Some(&"src") && parts.len() > 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            parts[0].to_string()
        }
    };
    (format!("dir:{path}"), path)
}

fn in_scope(file: &str, path: &str, group: &str) -> bool {
    if group == "file" {
        return file == path;
    }
    if path == "." {
        return !file.contains('/');
    }
    file == path || file.starts_with(&format!("{path}/"))
}

pub fn build_overview(nodes: &[GraphNode], edges: &[GraphEdge], group: &str) -> Overview {
    let group = if group == "file" { "file" } else { "dir" };
    let mut clusters: HashMap<String, OverviewNode> = HashMap::new();
    let mut node_cluster: HashMap<&str, String> = HashMap::new();

    for node in nodes.iter().filter(|node| !node.external) {
        let (id, path) = cluster_for(&node.file, group);
        node_cluster.insert(&node.id, id.clone());
        let cluster = clusters.entry(id.clone()).or_insert_with(|| OverviewNode {
            id,
            label: path.clone(),
            kind: group.to_string(),
            path,
            nodes: 0,
            callables: 0,
            constants: 0,
            cycle_nodes: 0,
            dead_code: 0,
            hotspot: 0.0,
        });
        cluster.nodes += 1;
        if node.kind.is_callable() {
            cluster.callables += 1;
        } else if node.kind.is_data() {
            cluster.constants += 1;
        }
        if node.metrics.in_cycle {
            cluster.cycle_nodes += 1;
        }
        if node.metrics.dead_code_candidate {
            cluster.dead_code += 1;
        }
        cluster.hotspot = cluster.hotspot.max(node.metrics.hotspot);
    }

    let mut edge_counts: HashMap<(String, String), OverviewEdge> = HashMap::new();
    for edge in edges.iter().filter(|edge| !edge.external) {
        let (Some(source), Some(target)) = (
            node_cluster.get(edge.source.as_str()),
            node_cluster.get(edge.target.as_str()),
        ) else {
            continue;
        };
        if source == target {
            continue;
        }
        let key = (source.clone(), target.clone());
        let entry = edge_counts.entry(key).or_insert_with(|| OverviewEdge {
            id: format!("overview:{source}->{target}"),
            source: source.clone(),
            target: target.clone(),
            calls: 0,
            data: 0,
            total: 0,
        });
        if edge.kind == crate::graph::EdgeKind::Call {
            entry.calls += edge.sites;
        } else {
            entry.data += edge.sites;
        }
        entry.total = entry.calls + entry.data;
    }

    let mut nodes: Vec<OverviewNode> = clusters.into_values().collect();
    nodes.sort_by(|left, right| left.path.cmp(&right.path));
    let mut edges: Vec<OverviewEdge> = edge_counts.into_values().collect();
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    Overview {
        group: group.to_string(),
        total_nodes: node_cluster.len(),
        nodes,
        edges,
    }
}

pub fn file_slice(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    path: &str,
    group: &str,
    limit: usize,
) -> FileSlice {
    let group = if group == "file" { "file" } else { "dir" };
    let limit = limit.clamp(1, 5000);
    let mut selected: Vec<GraphNode> = nodes
        .iter()
        .filter(|node| !node.external && in_scope(&node.file, path, group))
        .cloned()
        .collect();
    let total = selected.len();
    selected.sort_by(|left, right| {
        let left_degree = left.metrics.fan_in + left.metrics.fan_out;
        let right_degree = right.metrics.fan_in + right.metrics.fan_out;
        right_degree
            .cmp(&left_degree)
            .then_with(|| left.qname.cmp(&right.qname))
    });
    selected.truncate(limit);
    let ids: HashSet<&str> = selected.iter().map(|node| node.id.as_str()).collect();
    let edges = edges
        .iter()
        .filter(|edge| {
            !edge.external
                && ids.contains(edge.source.as_str())
                && ids.contains(edge.target.as_str())
        })
        .cloned()
        .collect();
    FileSlice {
        truncated: selected.len() < total,
        nodes: selected,
        edges,
        total,
    }
}
