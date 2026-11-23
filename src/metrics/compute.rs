//! Core metric computation: degrees, reachability, SCC condensation, centrality,
//! dead-code heuristics, and ranked tables.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::conventions::{call_pairs, is_dependency, Conventions};
use crate::graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeMetrics};

use super::centrality::{betweenness, page_rank};
use super::model::{GraphStats, GraphTop, GraphTopEntry};
use super::scc::{strongly_connected, topological_order};
use super::top::top_by;

const TRANSITIVE_NODE_LIMIT: usize = 3000;
const BETWEENNESS_WORK_LIMIT: usize = 6_000_000;

/// Absolute, snapshot-independent refactoring priority.
///
/// Complexity is the dominant local term; transitive fan-in adds change impact
/// with diminishing returns; instability keeps stable shared helpers low.
pub(crate) fn refactor_priority(node: &GraphNode) -> f64 {
    let complexity = node.cyclomatic as f64 + (1.0 + node.sloc as f64).ln();
    let impact = 1.0 + ((1 + node.metrics.transitive_fan_in) as f64).log2();
    let call_edges = node.metrics.call_fan_in + node.metrics.call_fan_out;
    let instability = if call_edges > 0 {
        node.metrics.call_fan_out as f64 / call_edges as f64
    } else {
        0.0
    };
    complexity * impact * (1.0 + instability)
}

struct GraphAnalysis<'a> {
    nodes: &'a mut [GraphNode],
    edges: &'a [GraphEdge],
    conventions: &'a Conventions,
    project: Vec<usize>,
    id_to_idx: HashMap<String, usize>,
    out_adj: Vec<HashSet<usize>>,
    in_adj: Vec<HashSet<usize>>,
    data_out: Vec<HashSet<usize>>,
    data_in: Vec<HashSet<usize>>,
    callable: Vec<bool>,
    call_out: Vec<u32>,
    call_in: Vec<u32>,
    known_sites: Vec<u32>,
    ignored_sites: Vec<u32>,
}

impl<'a> GraphAnalysis<'a> {
    fn new(
        nodes: &'a mut [GraphNode],
        edges: &'a [GraphEdge],
        conventions: &'a Conventions,
    ) -> Self {
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

        Self {
            nodes,
            edges,
            conventions,
            project,
            id_to_idx,
            out_adj: vec![HashSet::new(); n],
            in_adj: vec![HashSet::new(); n],
            data_out: vec![HashSet::new(); n],
            data_in: vec![HashSet::new(); n],
            callable: vec![false; n],
            call_out: vec![0; n],
            call_in: vec![0; n],
            known_sites: vec![0; n],
            ignored_sites: vec![0; n],
        }
    }

    fn collect_edges(&mut self) {
        let pairs = call_pairs(self.edges);
        for edge in self.edges {
            if edge.external {
                continue;
            }
            let (Some(&source), Some(&target)) = (
                self.id_to_idx.get(&edge.source),
                self.id_to_idx.get(&edge.target),
            ) else {
                continue;
            };
            match edge.kind {
                EdgeKind::Call => {
                    // Overrides, DI wiring and tests are call sites, but they are
                    // not dependencies: reach, fan-in/out, cycles and centrality
                    // must not grow because a method overrides a base one.
                    if is_dependency(
                        &self.nodes[source],
                        &self.nodes[target],
                        &pairs,
                        self.conventions,
                    ) {
                        self.out_adj[source].insert(target);
                        self.in_adj[target].insert(source);
                    }
                    self.add_call_site(edge, source, target);
                }
                EdgeKind::Data => {
                    self.data_out[source].insert(target);
                    self.data_in[target].insert(source);
                    self.call_in[target] += edge.sites;
                    self.nodes[source].metrics.constants_used += 1;
                }
            }
        }

        for index in &self.project {
            self.callable[*index] = matches!(
                self.nodes[*index].kind,
                NodeKind::Function | NodeKind::Method | NodeKind::Constructor
            );
        }
    }

    /// Site counters keep counting excluded edges: a method used only by its
    /// tests still has callers, it just has no dependencies.
    fn add_call_site(&mut self, edge: &GraphEdge, source: usize, target: usize) {
        self.call_out[source] += edge.sites;
        self.call_in[target] += edge.sites;
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
        self.known_sites[source] += known;
        self.ignored_sites[source] += known - used;
        self.nodes[source].metrics.consumes_results += used;
        self.nodes[target].metrics.provides_results += used;
    }

    fn compute_degrees(&mut self) {
        for index in &self.project {
            let call_fan_in = self.in_adj[*index].len() as u32;
            let call_fan_out = self.out_adj[*index].len() as u32;
            let data_fan_in = self.data_in[*index]
                .difference(&self.in_adj[*index])
                .count() as u32;
            let data_fan_out = self.data_out[*index]
                .difference(&self.out_adj[*index])
                .count() as u32;
            let fan_in = call_fan_in + data_fan_in;
            let fan_out = call_fan_out + data_fan_out;
            let known_sites = self.known_sites[*index];
            let ignored_sites = self.ignored_sites[*index];
            let metrics = &mut self.nodes[*index].metrics;
            metrics.fan_in = fan_in;
            metrics.fan_out = fan_out;
            metrics.call_fan_in = call_fan_in;
            metrics.call_fan_out = call_fan_out;
            metrics.data_fan_in = data_fan_in;
            metrics.data_fan_out = data_fan_out;
            metrics.call_in = self.call_in[*index];
            metrics.call_out = self.call_out[*index];
            metrics.result_ignored_ratio = if known_sites > 0 {
                ignored_sites as f64 / known_sites as f64
            } else {
                0.0
            };
            metrics.instability = if call_fan_in + call_fan_out > 0 {
                call_fan_out as f64 / (call_fan_in + call_fan_out) as f64
            } else {
                0.0
            };
            if self.callable[*index] {
                metrics.ifc = self.nodes[*index].loc as f64
                    + (call_fan_in as f64 * call_fan_out as f64).powi(2);
            }
        }
    }

    fn compute_reachability(&mut self) {
        let callable_count = self
            .project
            .iter()
            .filter(|index| self.callable[**index])
            .count();
        if callable_count <= TRANSITIVE_NODE_LIMIT {
            for index in &self.project {
                if self.callable[*index] {
                    let fan_out = bfs(*index, &self.out_adj);
                    let fan_in = bfs(*index, &self.in_adj);
                    self.nodes[*index].metrics.transitive_fan_out = fan_out;
                    self.nodes[*index].metrics.transitive_fan_in = fan_in;
                }
            }
        } else {
            for index in &self.project {
                self.nodes[*index].metrics.transitive_fan_out = self.nodes[*index].metrics.fan_out;
                self.nodes[*index].metrics.transitive_fan_in = self.nodes[*index].metrics.fan_in;
            }
        }
        for index in &self.project {
            if !self.callable[*index] {
                self.nodes[*index].metrics.transitive_fan_in = self.nodes[*index].metrics.fan_in;
                self.nodes[*index].metrics.transitive_fan_out = 0;
            }
        }
    }

    fn compute_components(&mut self) -> Vec<Vec<usize>> {
        let out_vec: Vec<Vec<usize>> = self
            .out_adj
            .iter()
            .map(|neighbors| neighbors.iter().copied().collect())
            .collect();
        let components = strongly_connected(&self.project, &out_vec);
        let mut component_of = vec![usize::MAX; self.nodes.len()];
        for (component_index, component) in components.iter().enumerate() {
            for node in component {
                component_of[*node] = component_index;
            }
        }
        let component_count = components.len();
        let mut component_edges: Vec<HashSet<usize>> = vec![HashSet::new(); component_count];
        for index in &self.project {
            let from = component_of[*index];
            if from == usize::MAX {
                continue;
            }
            for target in &self.out_adj[*index] {
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
                component_depth[*next] =
                    component_depth[*next].max(component_depth[*component] + 1);
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
        for index in &self.project {
            let component = component_of[*index];
            if component == usize::MAX {
                continue;
            }
            let metrics = &mut self.nodes[*index].metrics;
            metrics.scc = component as i64;
            metrics.scc_size = components[component].len() as u32;
            metrics.in_cycle = components[component].len() > 1;
            if self.callable[*index] {
                metrics.depth = component_depth[component];
                metrics.height = component_height[component];
            }
        }
        components
    }

    fn compute_centrality(&mut self) {
        let mut rank_adj: Vec<HashSet<usize>> = vec![HashSet::new(); self.nodes.len()];
        for index in &self.project {
            rank_adj[*index] = self.out_adj[*index]
                .union(&self.data_out[*index])
                .copied()
                .collect();
        }
        let rank = page_rank(&self.project, &rank_adj);
        for index in &self.project {
            self.nodes[*index].metrics.page_rank = rank[*index];
        }

        let callable_count = self
            .project
            .iter()
            .filter(|index| self.callable[**index])
            .count();
        if callable_count > 1 {
            let callable_edges: usize = self
                .project
                .iter()
                .filter(|index| self.callable[**index])
                .map(|index| {
                    self.out_adj[*index]
                        .iter()
                        .filter(|target| self.callable[**target])
                        .count()
                })
                .sum();
            if callable_count * callable_edges <= BETWEENNESS_WORK_LIMIT {
                let centrality = betweenness(&self.project, &self.callable, &self.out_adj);
                for index in &self.project {
                    self.nodes[*index].metrics.betweenness = centrality[*index];
                }
            }
        }
    }

    fn compute_dead_code(&mut self) {
        for index in &self.project {
            let node = &self.nodes[*index];
            self.nodes[*index].metrics.dead_code_candidate = node.metrics.call_in == 0
                && node.metrics.module_refs == 0
                // Controllers, modules and providers are wired by decorators the
                // call graph cannot see; a method without callers there is a route
                // or a hook, not a dead helper.
                && !self.conventions.is_framework_file(&node.file)
                && !self.conventions.is_test_node(node)
                && !node.exported
                && !node.is_entry_like
                && !node.name.starts_with('_')
                && node.kind != NodeKind::Constructor
                && !node.metrics.in_cycle;
        }
    }

    fn compute_scores(&mut self) {
        let callables: Vec<usize> = self
            .project
            .iter()
            .copied()
            .filter(|index| self.callable[*index])
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
        let blast = min_max(&|index| self.nodes[index].metrics.transitive_fan_in as f64);
        let fan_in = min_max(&|index| self.nodes[index].metrics.fan_in as f64);
        let cyclomatic = min_max(&|index| self.nodes[index].cyclomatic as f64);
        let loc = min_max(&|index| self.nodes[index].loc as f64);
        let fan_out = min_max(&|index| self.nodes[index].metrics.fan_out as f64);
        for index in &callables {
            let value = 0.35
                * normalize(self.nodes[*index].metrics.transitive_fan_in as f64, blast)
                + 0.2 * normalize(self.nodes[*index].metrics.fan_in as f64, fan_in)
                + 0.15 * normalize(self.nodes[*index].cyclomatic as f64, cyclomatic)
                + 0.15 * normalize(self.nodes[*index].loc as f64, loc)
                + 0.15 * normalize(self.nodes[*index].metrics.fan_out as f64, fan_out);
            self.nodes[*index].metrics.hotspot = value;
            let priority = refactor_priority(&self.nodes[*index]);
            self.nodes[*index].metrics.refactor_priority = priority;
        }
    }

    fn finish(self, components: &[Vec<usize>]) -> (GraphStats, GraphTop) {
        let callables: Vec<usize> = self
            .project
            .iter()
            .copied()
            .filter(|index| self.callable[*index])
            .collect();
        let call_edges: Vec<&GraphEdge> = self
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Call && !edge.external)
            .filter(|edge| {
                self.id_to_idx.contains_key(&edge.source)
                    && self.id_to_idx.contains_key(&edge.target)
            })
            .collect();
        let data_edges = self
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Data && !edge.external)
            .filter(|edge| {
                self.id_to_idx.contains_key(&edge.source)
                    && self.id_to_idx.contains_key(&edge.target)
            })
            .count();

        let cycles: Vec<&Vec<usize>> = components
            .iter()
            .filter(|component| {
                component.len() > 1 && component.iter().any(|node| self.callable[*node])
            })
            .collect();
        let cross_file = call_edges
            .iter()
            .filter(|edge| {
                self.nodes[self.id_to_idx[&edge.source]].file
                    != self.nodes[self.id_to_idx[&edge.target]].file
            })
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
            nodes: self.project.len() as u32,
            functions,
            constants: self.project.len() as u32 - functions,
            edges: call_edges.len() as u32,
            data_edges: data_edges as u32,
            external_edges: callables
                .iter()
                .map(|index| self.nodes[*index].metrics.external_fan_out)
                .sum(),
            density: if functions > 1 {
                call_edges.len() as f64 / (functions as f64 * (functions as f64 - 1.0))
            } else {
                0.0
            },
            avg_fan_in: if functions > 0 {
                callables
                    .iter()
                    .map(|index| self.nodes[*index].metrics.fan_in as f64)
                    .sum::<f64>()
                    / functions as f64
            } else {
                0.0
            },
            avg_fan_out: if functions > 0 {
                callables
                    .iter()
                    .map(|index| self.nodes[*index].metrics.fan_out as f64)
                    .sum::<f64>()
                    / functions as f64
            } else {
                0.0
            },
            max_fan_in: callables
                .iter()
                .map(|index| self.nodes[*index].metrics.fan_in)
                .max()
                .unwrap_or(0),
            max_fan_out: callables
                .iter()
                .map(|index| self.nodes[*index].metrics.fan_out)
                .max()
                .unwrap_or(0),
            cycles: cycles.len() as u32,
            nodes_in_cycles: cycles.iter().map(|component| component.len() as u32).sum(),
            largest_cycle: cycles
                .iter()
                .map(|component| component.len() as u32)
                .max()
                .unwrap_or(0),
            dead_code: self
                .project
                .iter()
                .filter(|index| self.nodes[**index].metrics.dead_code_candidate)
                .count() as u32,
            isolated: callables
                .iter()
                .filter(|index| {
                    self.nodes[**index].metrics.fan_in + self.nodes[**index].metrics.fan_out == 0
                })
                .count() as u32,
            max_height: callables
                .iter()
                .map(|index| self.nodes[*index].metrics.height)
                .max()
                .unwrap_or(0),
            max_depth: callables
                .iter()
                .map(|index| self.nodes[*index].metrics.depth)
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
            fan_in: top_by(self.nodes, &self.project, 10, |node| {
                node.metrics.fan_in as f64
            }),
            fan_out: top_by(self.nodes, &self.project, 10, |node| {
                node.metrics.fan_out as f64
            }),
            page_rank: top_by(self.nodes, &self.project, 10, |node| node.metrics.page_rank),
            betweenness: top_by(self.nodes, &self.project, 10, |node| {
                node.metrics.betweenness
            }),
            hotspot: top_by(self.nodes, &self.project, 10, |node| node.metrics.hotspot),
            complexity: top_by(self.nodes, &self.project, 10, |node| {
                if node.kind.is_callable() {
                    node.cyclomatic as f64
                } else {
                    0.0
                }
            }),
            ifc: top_by(self.nodes, &self.project, 10, |node| node.metrics.ifc),
            dead_code: {
                let mut dead: Vec<&GraphNode> = self
                    .project
                    .iter()
                    .map(|index| &self.nodes[*index])
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
}

fn bfs(start: usize, adjacency: &[HashSet<usize>]) -> u32 {
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
}

pub fn compute(
    nodes: &mut [GraphNode],
    edges: &[GraphEdge],
    conventions: &Conventions,
) -> (GraphStats, GraphTop) {
    let mut analysis = GraphAnalysis::new(nodes, edges, conventions);
    analysis.collect_edges();
    analysis.compute_degrees();
    analysis.compute_reachability();
    let components = analysis.compute_components();
    analysis.compute_centrality();
    analysis.compute_dead_code();
    analysis.compute_scores();
    analysis.finish(&components)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::NodeKind;

    fn node(
        cyclomatic: u32,
        sloc: u32,
        transitive_fan_in: u32,
        call_fan_in: u32,
        call_fan_out: u32,
    ) -> GraphNode {
        GraphNode {
            id: "test#function:node".to_string(),
            name: "node".to_string(),
            qname: "node".to_string(),
            kind: NodeKind::Function,
            file: "test.rs".to_string(),
            line: 1,
            end_line: 1,
            loc: sloc,
            sloc,
            params: 0,
            cyclomatic,
            exported: false,
            external: false,
            lazy: false,
            synthetic: false,
            is_entry_like: false,
            declaration: String::new(),
            metrics: NodeMetrics {
                transitive_fan_in,
                call_fan_in,
                call_fan_out,
                ..NodeMetrics::default()
            },
        }
    }

    #[test]
    fn priority_rewards_complexity_impact_and_unstable_deps() {
        let base = refactor_priority(&node(5, 10, 10, 0, 0));
        assert!(refactor_priority(&node(10, 10, 10, 0, 0)) > base);
        assert!(refactor_priority(&node(5, 100, 10, 0, 0)) > base);
        assert!(refactor_priority(&node(5, 10, 20, 0, 0)) > base);
        assert!(refactor_priority(&node(5, 10, 10, 2, 8)) > base);
    }

    #[test]
    fn stable_leaf_is_not_a_refactor_target() {
        let leaf = refactor_priority(&node(1, 6, 0, 0, 0));
        let central = refactor_priority(&node(1, 6, 36, 0, 0));
        assert!(leaf < 3.0);
        assert!(central > leaf);
    }

    fn symbol(id: &str, name: &str, kind: NodeKind, file: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            name: name.to_string(),
            qname: name.to_string(),
            kind,
            file: file.to_string(),
            line: 1,
            end_line: 2,
            loc: 1,
            sloc: 1,
            params: 0,
            cyclomatic: 1,
            exported: false,
            external: false,
            lazy: false,
            synthetic: false,
            is_entry_like: false,
            declaration: String::new(),
            metrics: NodeMetrics::default(),
        }
    }

    fn call(source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            id: format!("call:{source}->{target}"),
            source: source.to_string(),
            target: target.to_string(),
            kind: EdgeKind::Call,
            sites: 1,
            result_used: crate::graph::ResultUsage::All,
            external: false,
            inferred: None,
            call_sites: vec![crate::graph::CallSite {
                file: String::new(),
                line: 1,
                character: 0,
                result_used: Some(true),
            }],
        }
    }

    #[test]
    fn overrides_are_not_dependencies_or_cycles() {
        let mut nodes = vec![
            symbol("a#run", "run", NodeKind::Method, "a.ts"),
            symbol("b#run", "run", NodeKind::Method, "b.ts"),
        ];
        let edges = vec![call("a#run", "b#run"), call("b#run", "a#run")];
        let (stats, _) = compute(&mut nodes, &edges, Conventions::of("typescript"));
        assert_eq!(stats.cycles, 0);
        assert_eq!(nodes[0].metrics.call_fan_in, 0);
        assert_eq!(nodes[0].metrics.call_fan_out, 0);
        assert!(!nodes[0].metrics.in_cycle);
        assert_eq!(nodes[0].metrics.call_in, 1);
    }

    #[test]
    fn framework_files_are_not_dead_code_but_private_helpers_are() {
        let mut nodes = vec![
            symbol("p#handle", "handle", NodeKind::Method, "src/users/users.provider.ts"),
            symbol("h#helper", "helper", NodeKind::Function, "src/users/helpers.ts"),
            symbol("t#case", "tests.case", NodeKind::Function, "src/lib.rs"),
        ];
        compute(&mut nodes, &[], Conventions::of("typescript"));
        assert!(!nodes[0].metrics.dead_code_candidate);
        assert!(nodes[1].metrics.dead_code_candidate);
        assert!(!nodes[2].metrics.dead_code_candidate);
    }
}
