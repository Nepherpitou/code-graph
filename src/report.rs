//! The JSON report contract shared with the web UI.

use serde::{Deserialize, Serialize};

use crate::graph::{FileSummary, GraphEdge, GraphNode};
use crate::metrics::{GraphStats, GraphTop};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMeta {
    pub tool: String,
    pub version: String,
    pub generated_at: String,
    pub root: String,
    pub language: String,
    pub server: String,
    pub duration_ms: u128,
    pub files: usize,
    pub symbols: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub truncated: bool,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphReport {
    pub meta: GraphMeta,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub files: Vec<FileSummary>,
    pub stats: GraphStats,
    pub top: GraphTop,
    pub warnings: Vec<String>,
}
