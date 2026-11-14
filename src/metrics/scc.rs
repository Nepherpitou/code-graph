use std::collections::{HashSet, VecDeque};

pub(super) fn strongly_connected(project: &[usize], adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut indices: Vec<i64> = vec![-1; n];
    let mut low: Vec<i64> = vec![0; n];
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut result: Vec<Vec<usize>> = Vec::new();
    let mut next_index = 0i64;

    for start in project {
        if indices[*start] >= 0 {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(*start, 0)];
        while let Some(&(node, edge_index)) = work.last() {
            if edge_index == 0 {
                indices[node] = next_index;
                low[node] = next_index;
                next_index += 1;
                stack.push(node);
                on_stack[node] = true;
            }
            if edge_index < adjacency[node].len() {
                let next = adjacency[node][edge_index];
                if let Some(frame) = work.last_mut() {
                    frame.1 += 1;
                }
                if indices[next] < 0 {
                    work.push((next, 0));
                } else if on_stack[next] {
                    low[node] = low[node].min(indices[next]);
                }
                continue;
            }
            if low[node] == indices[node] {
                let mut component = Vec::new();
                loop {
                    let popped = stack.pop().unwrap();
                    on_stack[popped] = false;
                    component.push(popped);
                    if popped == node {
                        break;
                    }
                }
                result.push(component);
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
        }
    }
    result
}

pub(super) fn topological_order(adjacency: &[HashSet<usize>]) -> Vec<usize> {
    let mut indegree = vec![0usize; adjacency.len()];
    for targets in adjacency {
        for target in targets {
            indegree[*target] += 1;
        }
    }
    let mut queue: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter(|(_, value)| **value == 0)
        .map(|(index, _)| index)
        .collect();
    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for target in &adjacency[node] {
            indegree[*target] -= 1;
            if indegree[*target] == 0 {
                queue.push_back(*target);
            }
        }
    }
    order
}
