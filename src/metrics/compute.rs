//! Core metric computation: degrees, reachability, SCC condensation, centrality,
//! dead-code heuristics, and ranked tables.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeMetrics};

use super::centrality::{betweenness, page_rank};
use super::model::{GraphStats, GraphTop, GraphTopEntry};
use super::scc::{strongly_connected, topological_order};
use super::top::top_by;

const TRANSITIVE_NODE_LIMIT: usize = 3000;
const BETWEENNESS_WORK_LIMIT: usize = 6_000_000;

pub fn compute(nodes: &mut [GraphNode], edges: &[GraphEdge]) -> (GraphStats, GraphTop) {
    let project: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| !node.external)
        .map(|(index, _)| index)
        .collect();
    let id_to_idx: HashMap<String, usize> = project
        .iter()
        .map(|index| (nodes[*index].id.clone(), *index))
        .collect();
    let n = nodes.len();

    for index in &project {
        let module_refs = nodes[*index].metrics.module_refs;
        let external_fan_out = nodes[*index].metrics.external_fan_out;
        nodes[*index].metrics = NodeMetrics {
            module_refs,
            external_fan_out,
            ..NodeMetrics::default()
        };
    }

    let mut out_adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut in_adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut data_out: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut data_in: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut callable = vec![false; n];
    for index in &project {
        callable[*index] = matches!(
            nodes[*index].kind,
            NodeKind::Function | NodeKind::Method | NodeKind::Constructor
        );
    }

    let mut call_out = vec![0u32; n];
    let mut call_in = vec![0u32; n];
    let mut known_sites = vec![0u32; n];
    let mut ignored_sites = vec![0u32; n];

    for edge in edges {
        if edge.external {
            continue;
        }
        let (Some(&source), Some(&target)) =
            (id_to_idx.get(&edge.source), id_to_idx.get(&edge.target))
        else {
            continue;
        };
        match edge.kind {
            EdgeKind::Call => {
                out_adj[source].insert(target);
                in_adj[target].insert(source);
                call_out[source] += edge.sites;
                call_in[target] += edge.sites;
                let known = edge
                    .call_sites
                    .iter()
                    .filter(|site| site.result_used.is_some())
                    .count() as u32;
                let used = edge
                    .call_sites
                    .iter()
                    .filter(|site| site.result_used == Some(true))
                    .count() as u32;
                known_sites[source] += known;
                ignored_sites[source] += known - used;
                nodes[source].metrics.consumes_results += used;
                nodes[target].metrics.provides_results += used;
            }
            EdgeKind::Data => {
                data_out[source].insert(target);
                data_in[target].insert(source);
                call_in[target] += edge.sites;
                nodes[source].metrics.constants_used += 1;
            }
        }
    }

    for index in &project {
        let fan_in =
            (in_adj[*index].len() + data_in[*index].difference(&in_adj[*index]).count()) as u32;
        let fan_out =
            (out_adj[*index].len() + data_out[*index].difference(&out_adj[*index]).count()) as u32;
        nodes[*index].metrics.fan_in = fan_in;
        nodes[*index].metrics.fan_out = fan_out;
        nodes[*index].metrics.call_in = call_in[*index];
        nodes[*index].metrics.call_out = call_out[*index];
        nodes[*index].metrics.result_ignored_ratio = if known_sites[*index] > 0 {
            ignored_sites[*index] as f64 / known_sites[*index] as f64
        } else {
            0.0
        };
        nodes[*index].metrics.instability = if fan_in + fan_out > 0 {
            fan_out as f64 / (fan_in + fan_out) as f64
        } else {
            0.0
        };
        if callable[*index] {
            nodes[*index].metrics.ifc =
                nodes[*index].loc as f64 + (fan_in as f64 * fan_out as f64).powi(2);
        }
    }

    let callable_count = project.iter().filter(|index| callable[**index]).count();
    if callable_count <= TRANSITIVE_NODE_LIMIT {
        let bfs = |start: usize, adjacency: &[HashSet<usize>]| -> u32 {
            let mut seen: HashSet<usize> = HashSet::new();
            let mut queue: VecDeque<usize> = VecDeque::new();
            queue.push_back(start);
            while let Some(node) = queue.pop_front() {
                for next in &adjacency[node] {
                    if seen.insert(*next) {
                        queue.push_back(*next);
                    }
                }
            }
            seen.remove(&start);
            seen.len() as u32
        };
        for index in &project {
            if callable[*index] {
                nodes[*index].metrics.transitive_fan_out = bfs(*index, &out_adj);
                nodes[*index].metrics.transitive_fan_in = bfs(*index, &in_adj);
            }
        }
    } else {
        for index in &project {
            nodes[*index].metrics.transitive_fan_out = nodes[*index].metrics.fan_out;
            nodes[*index].metrics.transitive_fan_in = nodes[*index].metrics.fan_in;
        }
    }
    for index in &project {
        if !callable[*index] {
            nodes[*index].metrics.transitive_fan_in = nodes[*index].metrics.fan_in;
            nodes[*index].metrics.transitive_fan_out = 0;
        }
    }

    let out_vec: Vec<Vec<usize>> = out_adj
        .iter()
        .map(|neighbors| neighbors.iter().copied().collect())
        .collect();
    let components = strongly_connected(&project, &out_vec);
    let mut component_of = vec![usize::MAX; n];
    for (component_index, component) in components.iter().enumerate() {
        for node in component {
            component_of[*node] = component_index;
        }
    }
    let component_count = components.len();
    let mut component_edges: Vec<HashSet<usize>> = vec![HashSet::new(); component_count];
    for index in &project {
        let from = component_of[*index];
        if from == usize::MAX {
            continue;
        }
        for target in &out_adj[*index] {
            let to = component_of[*target];
            if to != usize::MAX && to != from {
                component_edges[from].insert(to);
            }
        }
    }
    let order = topological_order(&component_edges);
    let mut component_depth = vec![0u32; component_count];
    for component in &order {
        for next in &component_edges[*component] {
            component_depth[*next] = component_depth[*next].max(component_depth[*component] + 1);
        }
    }
    let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); component_count];
    for (component, targets) in component_edges.iter().enumerate() {
        for target in targets {
            reverse[*target].push(component);
        }
    }
    let mut component_height = vec![0u32; component_count];
    for component in order.iter().rev() {
        let mut height = 0;
        for next in &component_edges[*component] {
            height = height.max(component_height[*next] + 1);
        }
        component_height[*component] = height;
    }
    for index in &project {
        let component = component_of[*index];
        if component == usize::MAX {
            continue;
        }
        nodes[*index].metrics.scc = component as i64;
        nodes[*index].metrics.scc_size = components[component].len() as u32;
        nodes[*index].metrics.in_cycle = components[component].len() > 1;
        if callable[*index] {
            nodes[*index].metrics.depth = component_depth[component];
            nodes[*index].metrics.height = component_height[component];
        }
    }

    // PageRank over calls + constant usage.
    let mut rank_adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for index in &project {
        rank_adj[*index] = out_adj[*index].union(&data_out[*index]).copied().collect();
    }
    let rank = page_rank(&project, &rank_adj);
    for index in &project {
        nodes[*index].metrics.page_rank = rank[*index];
    }

    if callable_count > 1 {
        let callable_edges: usize = project
            .iter()
            .filter(|index| callable[**index])
            .map(|index| {
                out_adj[*index]
                    .iter()
                    .filter(|target| callable[**target])
                    .count()
            })
            .sum();
        if callable_count * callable_edges <= BETWEENNESS_WORK_LIMIT {
            let centrality = betweenness(&project, &callable, &out_adj);
            for index in &project {
                nodes[*index].metrics.betweenness = centrality[*index];
            }
        }
    }

    for index in &project {
        let node = &nodes[*index];
        nodes[*index].metrics.dead_code_candidate = node.metrics.call_in == 0
            && node.metrics.module_refs == 0
            && !node.exported
            && !node.is_entry_like
            && !node.name.starts_with('_')
            && node.kind != NodeKind::Constructor
            && !node.metrics.in_cycle;
    }

    let callables: Vec<usize> = project
        .iter()
        .copied()
        .filter(|index| callable[*index])
        .collect();
    let min_max = |value: &dyn Fn(usize) -> f64| -> (f64, f64) {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for index in &callables {
            let current = value(*index);
            min = min.min(current);
            max = max.max(current);
        }
        if callables.is_empty() {
            (0.0, 0.0)
        } else {
            (min, max)
        }
    };
    let normalize = |value: f64, (min, max): (f64, f64)| -> f64 {
        if max > min {
            (value - min) / (max - min)
        } else {
            0.0
        }
    };
    let blast = min_max(&|index| nodes[index].metrics.transitive_fan_in as f64);
    let fan_in = min_max(&|index| nodes[index].metrics.fan_in as f64);
    let cyclomatic = min_max(&|index| nodes[index].cyclomatic as f64);
    let loc = min_max(&|index| nodes[index].loc as f64);
    let fan_out = min_max(&|index| nodes[index].metrics.fan_out as f64);
    for index in &callables {
        let value = 0.35 * normalize(nodes[*index].metrics.transitive_fan_in as f64, blast)
            + 0.2 * normalize(nodes[*index].metrics.fan_in as f64, fan_in)
            + 0.15 * normalize(nodes[*index].cyclomatic as f64, cyclomatic)
            + 0.15 * normalize(nodes[*index].loc as f64, loc)
            + 0.15 * normalize(nodes[*index].metrics.fan_out as f64, fan_out);
        nodes[*index].metrics.hotspot = value;
    }

    let call_edges: Vec<&GraphEdge> = edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Call && !edge.external)
        .filter(|edge| id_to_idx.contains_key(&edge.source) && id_to_idx.contains_key(&edge.target))
        .collect();
    let data_edges = edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Data && !edge.external)
        .filter(|edge| id_to_idx.contains_key(&edge.source) && id_to_idx.contains_key(&edge.target))
        .count();

    let cycles: Vec<&Vec<usize>> = components
        .iter()
        .filter(|component| component.len() > 1 && component.iter().any(|node| callable[*node]))
        .collect();
    let cross_file = call_edges
        .iter()
        .filter(|edge| nodes[id_to_idx[&edge.source]].file != nodes[id_to_idx[&edge.target]].file)
        .count();
    let global_known: u32 = call_edges
        .iter()
        .flat_map(|edge| edge.call_sites.iter())
        .filter(|site| site.result_used.is_some())
        .count() as u32;
    let global_used: u32 = call_edges
        .iter()
        .flat_map(|edge| edge.call_sites.iter())
        .filter(|site| site.result_used == Some(true))
        .count() as u32;

    let functions = callables.len() as u32;
    let stats = GraphStats {
        nodes: project.len() as u32,
        functions,
        constants: project.len() as u32 - functions,
        edges: call_edges.len() as u32,
        data_edges: data_edges as u32,
        external_edges: callables
            .iter()
            .map(|index| nodes[*index].metrics.external_fan_out)
            .sum(),
        density: if functions > 1 {
            call_edges.len() as f64 / (functions as f64 * (functions as f64 - 1.0))
        } else {
            0.0
        },
        avg_fan_in: if functions > 0 {
            callables
                .iter()
                .map(|index| nodes[*index].metrics.fan_in as f64)
                .sum::<f64>()
                / functions as f64
        } else {
            0.0
        },
        avg_fan_out: if functions > 0 {
            callables
                .iter()
                .map(|index| nodes[*index].metrics.fan_out as f64)
                .sum::<f64>()
                / functions as f64
        } else {
            0.0
        },
        max_fan_in: callables
            .iter()
            .map(|index| nodes[*index].metrics.fan_in)
            .max()
            .unwrap_or(0),
        max_fan_out: callables
            .iter()
            .map(|index| nodes[*index].metrics.fan_out)
            .max()
            .unwrap_or(0),
        cycles: cycles.len() as u32,
        nodes_in_cycles: cycles.iter().map(|component| component.len() as u32).sum(),
        largest_cycle: cycles
            .iter()
            .map(|component| component.len() as u32)
            .max()
            .unwrap_or(0),
        dead_code: project
            .iter()
            .filter(|index| nodes[**index].metrics.dead_code_candidate)
            .count() as u32,
        isolated: callables
            .iter()
            .filter(|index| nodes[**index].metrics.fan_in + nodes[**index].metrics.fan_out == 0)
            .count() as u32,
        max_height: callables
            .iter()
            .map(|index| nodes[*index].metrics.height)
            .max()
            .unwrap_or(0),
        max_depth: callables
            .iter()
            .map(|index| nodes[*index].metrics.depth)
            .max()
            .unwrap_or(0),
        cross_file_edges: cross_file as u32,
        cross_file_ratio: if call_edges.is_empty() {
            0.0
        } else {
            cross_file as f64 / call_edges.len() as f64
        },
        result_ignored_ratio: if global_known > 0 {
            (global_known - global_used) as f64 / global_known as f64
        } else {
            0.0
        },
    };

    let top = GraphTop {
        fan_in: top_by(nodes, &project, 10, |node| node.metrics.fan_in as f64),
        fan_out: top_by(nodes, &project, 10, |node| node.metrics.fan_out as f64),
        page_rank: top_by(nodes, &project, 10, |node| node.metrics.page_rank),
        betweenness: top_by(nodes, &project, 10, |node| node.metrics.betweenness),
        hotspot: top_by(nodes, &project, 10, |node| node.metrics.hotspot),
        complexity: top_by(nodes, &project, 10, |node| {
            if node.kind.is_callable() {
                node.cyclomatic as f64
            } else {
                0.0
            }
        }),
        ifc: top_by(nodes, &project, 10, |node| node.metrics.ifc),
        dead_code: {
            let mut dead: Vec<&GraphNode> = project
                .iter()
                .map(|index| &nodes[*index])
                .filter(|node| node.metrics.dead_code_candidate)
                .collect();
            dead.sort_by_key(|node| std::cmp::Reverse(node.loc));
            dead.into_iter()
                .take(10)
                .map(|node| GraphTopEntry {
                    id: node.id.clone(),
                    name: node.qname.clone(),
                    file: node.file.clone(),
                    value: node.loc as f64,
                })
                .collect()
        },
    };

    (stats, top)
}
