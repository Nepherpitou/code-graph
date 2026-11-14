use crate::graph::GraphNode;

use super::model::GraphTopEntry;

pub(super) fn top_by<F>(
    nodes: &[GraphNode],
    project: &[usize],
    limit: usize,
    value: F,
) -> Vec<GraphTopEntry>
where
    F: Fn(&GraphNode) -> f64,
{
    let mut rows: Vec<&GraphNode> = project.iter().map(|index| &nodes[*index]).collect();
    rows.sort_by(|a, b| {
        value(b)
            .partial_cmp(&value(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.into_iter()
        .take(limit)
        .map(|node| GraphTopEntry {
            id: node.id.clone(),
            name: node.qname.clone(),
            file: node.file.clone(),
            value: value(node),
        })
        .collect()
}
