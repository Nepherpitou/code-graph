//! Entry-point selection for the overview graph.
//!
//! An entry point is a project callable with no incoming call edges and no
//! test/spec attribution. Ranking uses the already-computed transitive fan-out:
//! how many callables the entry can reach. Depth is intentionally not part of
//! the score; it is only a tie-breaker, because a long chain of trivial wrappers
//! is less useful than broad reach.

use serde::Serialize;

use crate::graph::{GraphNode, NodeKind};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    pub id: String,
    pub name: String,
    pub file: String,
    pub reach: u32,
    pub height: u32,
    pub fan_out: u32,
}

#[derive(Debug, Clone)]
pub struct EntryRanking {
    pub points: Vec<EntryPoint>,
    pub total: usize,
}

pub fn rank_entry_points(nodes: &[GraphNode], limit: usize) -> EntryRanking {
    let limit = limit.clamp(1, 20);
    let mut candidates: Vec<&GraphNode> = nodes
        .iter()
        .filter(|node| is_entry_candidate(node))
        .collect();
    let total = candidates.len();
    candidates.sort_by(|left, right| {
        right
            .metrics
            .transitive_fan_out
            .cmp(&left.metrics.transitive_fan_out)
            .then_with(|| right.metrics.height.cmp(&left.metrics.height))
            .then_with(|| right.metrics.fan_out.cmp(&left.metrics.fan_out))
            .then_with(|| left.qname.cmp(&right.qname))
    });
    let points = candidates
        .into_iter()
        .take(limit)
        .map(|node| EntryPoint {
            id: node.id.clone(),
            name: node.qname.clone(),
            file: node.file.clone(),
            reach: node.metrics.transitive_fan_out,
            height: node.metrics.height,
            fan_out: node.metrics.fan_out,
        })
        .collect();
    EntryRanking { points, total }
}

pub(crate) fn is_exit_candidate(node: &GraphNode) -> bool {
    node.kind.is_callable()
        && node.kind != NodeKind::Constructor
        && !node.external
        && !node.synthetic
        && node.metrics.call_out == 0
        && !is_test_node(node)
}

pub(crate) fn is_entry_candidate(node: &GraphNode) -> bool {
    node.kind.is_callable()
        && node.kind != NodeKind::Constructor
        && !node.external
        && !node.synthetic
        && node.metrics.call_in == 0
        && node.metrics.transitive_fan_out > 0
        && !is_test_node(node)
}

fn is_test_node(node: &GraphNode) -> bool {
    let file = node.file.as_str();
    if file
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__" | "spec"))
    {
        return true;
    }
    if file.contains(".test.") || file.contains(".spec.") {
        return true;
    }
    if file.ends_with("tests.rs") || file.ends_with("test.rs") {
        return true;
    }
    let member = node
        .qname
        .rsplit('.')
        .next()
        .unwrap_or(node.qname.as_str())
        .to_lowercase();
    member.starts_with("test") || member.starts_with("spec") || member.starts_with("bench")
}
