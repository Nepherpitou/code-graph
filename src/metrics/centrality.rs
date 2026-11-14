use std::collections::{HashSet, VecDeque};

pub(super) fn page_rank(project: &[usize], adjacency: &[HashSet<usize>]) -> Vec<f64> {
    let n = adjacency.len();
    let mut rank = vec![0.0; n];
    if project.is_empty() {
        return rank;
    }
    let initial = 1.0 / project.len() as f64;
    for index in project {
        rank[*index] = initial;
    }
    let damping = 0.85;
    let mut next = vec![0.0; n];
    for _ in 0..100 {
        let mut dangling = 0.0;
        for index in project {
            if adjacency[*index].is_empty() {
                dangling += rank[*index];
            }
        }
        let base =
            (1.0 - damping) / project.len() as f64 + damping * dangling / project.len() as f64;
        for index in project {
            next[*index] = base;
        }
        for index in project {
            let targets = &adjacency[*index];
            if targets.is_empty() {
                continue;
            }
            let share = damping * rank[*index] / targets.len() as f64;
            for target in targets {
                next[*target] += share;
            }
        }
        let mut delta = 0.0;
        for index in project {
            delta += (next[*index] - rank[*index]).abs();
            rank[*index] = next[*index];
        }
        if delta < 1e-9 {
            break;
        }
    }
    rank
}

pub(super) fn betweenness(
    project: &[usize],
    callable: &[bool],
    adjacency: &[HashSet<usize>],
) -> Vec<f64> {
    let n = adjacency.len();
    let mut centrality = vec![0.0f64; n];
    let sources: Vec<usize> = project
        .iter()
        .copied()
        .filter(|index| callable[*index])
        .collect();
    for source in sources {
        let mut stack: Vec<usize> = Vec::new();
        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut sigma = vec![0.0f64; n];
        let mut distance = vec![-1i64; n];
        sigma[source] = 1.0;
        distance[source] = 0;
        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(source);
        while let Some(node) = queue.pop_front() {
            stack.push(node);
            for next in &adjacency[node] {
                if distance[*next] < 0 {
                    distance[*next] = distance[node] + 1;
                    queue.push_back(*next);
                }
                if distance[*next] == distance[node] + 1 {
                    sigma[*next] += sigma[node];
                    predecessors[*next].push(node);
                }
            }
        }
        let mut dependency = vec![0.0f64; n];
        while let Some(node) = stack.pop() {
            for previous in &predecessors[node] {
                let contribution =
                    (sigma[*previous] / sigma[node].max(f64::EPSILON)) * (1.0 + dependency[node]);
                dependency[*previous] += contribution;
            }
            if node != source {
                centrality[node] += dependency[node];
            }
        }
    }
    centrality
}
