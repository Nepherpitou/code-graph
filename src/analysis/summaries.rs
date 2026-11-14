use std::collections::HashMap;

use crate::graph::{EdgeKind, FileSummary, GraphEdge, GraphNode};

pub(super) fn build_file_summaries(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<FileSummary> {
    let mut summaries: HashMap<String, FileSummary> = HashMap::new();
    for node in nodes {
        if node.external {
            continue;
        }
        let entry = summaries
            .entry(node.file.clone())
            .or_insert_with(|| FileSummary {
                file: node.file.clone(),
                nodes: 0,
                call_in: 0,
                call_out: 0,
                constants_used: 0,
                dead_code: 0,
            });
        entry.nodes += 1;
        entry.constants_used += node.metrics.constants_used;
        if node.metrics.dead_code_candidate {
            entry.dead_code += 1;
        }
    }
    for edge in edges {
        if edge.kind != EdgeKind::Call || edge.external {
            continue;
        }
        let source_file = nodes
            .iter()
            .find(|node| node.id == edge.source)
            .map(|node| node.file.clone());
        let target_file = nodes
            .iter()
            .find(|node| node.id == edge.target)
            .map(|node| node.file.clone());
        if let Some(file) = source_file {
            if let Some(entry) = summaries.get_mut(&file) {
                entry.call_out += edge.sites;
            }
        }
        if let Some(file) = target_file {
            if let Some(entry) = summaries.get_mut(&file) {
                entry.call_in += edge.sites;
            }
        }
    }
    let mut files: Vec<FileSummary> = summaries.into_values().collect();
    files.sort_by(|a, b| b.nodes.cmp(&a.nodes).then(a.file.cmp(&b.file)));
    files
}
