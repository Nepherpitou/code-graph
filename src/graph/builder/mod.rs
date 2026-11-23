//! Builder internals:
//! - `symbols`: document symbols, node creation, symbol filtering.
//! - `targets`: resolve call targets and external nodes.
//! - `edges`: call/data edge aggregation and result-usage classification.
//! - `hierarchy`: call hierarchy traversal.
//! - `references`: constant usage and the missed-call fallback.

mod callbacks;
mod edges;
mod hierarchy;
mod references;
mod symbols;
mod targets;

pub(crate) use symbols::parse_symbols;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use url::Url;

use crate::languages::Adapter;
use crate::lsp::{LspPosition, LspRange};
use crate::source::SourceFile;

use super::model::{CallSite, EdgeKind, GraphEdge, GraphNode};

pub(super) struct NodeRecord {
    node: GraphNode,
    uri: String,
    rel: String,
    range: LspRange,
    selection: LspRange,
}

struct EdgeRecord {
    id: String,
    source: String,
    target: String,
    kind: EdgeKind,
    sites: u32,
    external: bool,
    inferred: bool,
    call_sites: Vec<CallSite>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RawSymbol {
    name: String,
    kind: u32,
    detail: String,
    range: LspRange,
    selection: LspRange,
    children: Vec<RawSymbol>,
}

pub(super) struct Resolved {
    id: Option<String>,
    external: bool,
    created_callable: bool,
}

// LSP SymbolKind values used by document symbols. These are protocol constants,
// not language-specific guesses.
pub(super) const LSP_MODULE: u32 = 2;
pub(super) const LSP_NAMESPACE: u32 = 3;
pub(super) const LSP_PACKAGE: u32 = 4;
pub(super) const LSP_CLASS: u32 = 5;
pub(super) const LSP_METHOD: u32 = 6;
pub(super) const LSP_CONSTRUCTOR: u32 = 9;
pub(super) const LSP_ENUM: u32 = 10;
pub(super) const LSP_INTERFACE: u32 = 11;
pub(super) const LSP_FUNCTION: u32 = 12;
pub(super) const LSP_VARIABLE: u32 = 13;
pub(super) const LSP_CONSTANT: u32 = 14;
pub(super) const LSP_OBJECT: u32 = 19;
pub(super) const LSP_STRUCT: u32 = 23;

pub(crate) struct Builder {
    root: PathBuf,
    adapter: &'static Adapter,
    sources: HashMap<String, SourceFile>,
    records: HashMap<String, NodeRecord>,
    order: Vec<String>,
    uri_selection: HashMap<String, String>,
    by_rel: HashMap<String, Vec<String>>,
    class_map: HashMap<String, String>,
    edges: HashMap<String, EdgeRecord>,
    edge_order: Vec<String>,
    external_callees: HashMap<String, HashSet<String>>,
    used_ids: HashSet<String>,
    max_nodes: usize,
    include_external: bool,
    include_constants: bool,
    truncated: bool,
    constants_total: usize,
    constants_crawled: usize,
    warnings: Vec<String>,
}

pub(crate) struct BuilderOutput {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub warnings: Vec<String>,
    pub truncated: bool,
    pub constants_total: usize,
    pub constants_crawled: usize,
}

impl Builder {
    pub(crate) fn new(
        root: PathBuf,
        adapter: &'static Adapter,
        sources: HashMap<String, SourceFile>,
        max_nodes: usize,
        include_external: bool,
        include_constants: bool,
    ) -> Self {
        Self {
            root,
            adapter,
            sources,
            records: HashMap::new(),
            order: Vec::new(),
            uri_selection: HashMap::new(),
            by_rel: HashMap::new(),
            class_map: HashMap::new(),
            edges: HashMap::new(),
            edge_order: Vec::new(),
            external_callees: HashMap::new(),
            used_ids: HashSet::new(),
            max_nodes,
            include_external,
            include_constants,
            truncated: false,
            constants_total: 0,
            constants_crawled: 0,
            warnings: Vec::new(),
        }
    }

    fn uri_for_rel(&self, rel: &str) -> String {
        Url::from_file_path(self.root.join(rel))
            .map(|url| url.to_string())
            .unwrap_or_default()
    }

    pub(super) fn to_relative(&self, uri: &str) -> Option<String> {
        let url = Url::parse(uri).ok()?;
        let path = url.to_file_path().ok()?;
        let rel = path.strip_prefix(&self.root).ok()?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        if self.sources.contains_key(&rel) {
            Some(rel)
        } else {
            None
        }
    }

    pub(super) fn record(&mut self, record: NodeRecord) {
        let id = record.node.id.clone();
        let rel = record.rel.clone();
        let selection = record.selection.start;
        self.uri_selection.insert(
            format!("{rel}#{}:{}", selection.line, selection.character),
            id.clone(),
        );
        self.by_rel.entry(rel).or_default().push(id.clone());
        self.order.push(id.clone());
        self.records.insert(id, record);
    }

    pub(super) fn caller_in_file(&self, rel: &str, position: LspPosition) -> Option<String> {
        let mut best: Option<&NodeRecord> = None;
        for id in self.by_rel.get(rel)? {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            if !record.node.kind.is_callable() {
                continue;
            }
            let start = record.range.start;
            let end = record.range.end;
            if start.line > position.line || end.line < position.line {
                continue;
            }
            if position.line == start.line && position.character < start.character {
                continue;
            }
            if position.line == end.line && position.character > end.character {
                continue;
            }
            let size = (
                record
                    .range
                    .end
                    .line
                    .saturating_sub(record.range.start.line),
                record
                    .range
                    .end
                    .character
                    .saturating_sub(record.range.start.character),
            );
            match best {
                None => best = Some(record),
                Some(current) => {
                    let current_size = (
                        current
                            .range
                            .end
                            .line
                            .saturating_sub(current.range.start.line),
                        current
                            .range
                            .end
                            .character
                            .saturating_sub(current.range.start.character),
                    );
                    if size < current_size {
                        best = Some(record);
                    }
                }
            }
        }
        best.map(|record| record.node.id.clone())
    }

    pub(crate) fn node_count(&self) -> usize {
        self.order.len()
    }

    /// One callable per file, for the Rust readiness probe.
    ///
    /// The first file is not always part of the LSP workspace: nested crates
    /// (fixtures, examples) can be discovered but never loaded, and their
    /// `prepareCallHierarchy` result stays empty forever. Rotating over several
    /// files lets the probe find a loaded one instead of sleeping for 60s.
    pub(crate) fn readiness_samples(&self) -> Vec<(String, String, LspPosition)> {
        const MAX_SAMPLES: usize = 24;
        let mut seen_files = HashSet::new();
        let mut samples = Vec::new();
        for id in &self.order {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            if !record.node.kind.is_callable()
                || record.node.synthetic
                || record.node.external
                || !seen_files.insert(record.rel.as_str())
            {
                continue;
            }
            samples.push((
                record
                    .node
                    .qname
                    .rsplit('.')
                    .next()
                    .unwrap_or("main")
                    .to_string(),
                record.uri.clone(),
                record.selection.start,
            ));
        }
        if samples.len() > MAX_SAMPLES {
            let total = samples.len();
            samples = (0..MAX_SAMPLES)
                .map(|index| samples[index * total / MAX_SAMPLES].clone())
                .collect();
        }
        samples
    }

    pub(crate) fn finish(mut self) -> BuilderOutput {
        for (id, callees) in &self.external_callees {
            if let Some(record) = self.records.get_mut(id) {
                record.node.metrics.external_fan_out = callees.len() as u32;
            }
        }
        let nodes: Vec<GraphNode> = self
            .order
            .iter()
            .filter_map(|id| self.records.get(id))
            .map(|record| record.node.clone())
            .collect();
        let edges = self.finish_edges();
        if self.constants_total == 0 && self.constants_crawled == 0 {
            self.constants_total = nodes
                .iter()
                .filter(|node| node.kind.is_data() && !node.external)
                .count();
        }
        BuilderOutput {
            nodes,
            edges,
            warnings: self.warnings,
            truncated: self.truncated,
            constants_total: self.constants_total,
            constants_crawled: self.constants_crawled,
        }
    }
}
