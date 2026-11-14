use crate::lsp::LspRange;
use crate::source::ast::result_used_at;

use super::{Builder, EdgeRecord};
use crate::graph::model::{CallSite, EdgeKind, GraphEdge, ResultUsage};

impl Builder {
    pub(super) fn call_site(&self, rel: &str, range: LspRange) -> CallSite {
        let result_used = self.sources.get(rel).and_then(|source| {
            source
                .tree
                .as_ref()
                .and_then(|tree| result_used_at(tree, &source.text, self.adapter, range.start))
        });
        CallSite {
            file: rel.to_string(),
            line: range.start.line + 1,
            character: range.start.character,
            result_used,
        }
    }

    pub(super) fn add_edge(
        &mut self,
        source: &str,
        target: &str,
        site: CallSite,
        external: bool,
        inferred: bool,
    ) {
        let key = format!("call:{source}->{target}");
        if let Some(edge) = self.edges.get_mut(&key) {
            edge.sites += 1;
            edge.inferred = edge.inferred || inferred;
            edge.call_sites.push(site);
            return;
        }
        self.edges.insert(
            key.clone(),
            EdgeRecord {
                id: key.clone(),
                source: source.to_string(),
                target: target.to_string(),
                kind: EdgeKind::Call,
                sites: 1,
                external,
                inferred,
                call_sites: vec![site],
            },
        );
        self.edge_order.push(key);
    }

    pub(super) fn add_data_edge(&mut self, source: &str, target: &str, site: CallSite) {
        let key = format!("data:{source}->{target}");
        if let Some(edge) = self.edges.get_mut(&key) {
            edge.sites += 1;
            edge.call_sites.push(site);
            return;
        }
        self.edges.insert(
            key.clone(),
            EdgeRecord {
                id: key.clone(),
                source: source.to_string(),
                target: target.to_string(),
                kind: EdgeKind::Data,
                sites: 1,
                external: false,
                inferred: false,
                call_sites: vec![site],
            },
        );
        self.edge_order.push(key);
    }

    pub(super) fn finish_edges(&self) -> Vec<GraphEdge> {
        self.edge_order
            .iter()
            .filter_map(|key| self.edges.get(key))
            .map(|edge| {
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
                let result_used = if edge.kind == EdgeKind::Data || known == 0 {
                    ResultUsage::Unknown
                } else if used == known {
                    ResultUsage::All
                } else if used == 0 {
                    ResultUsage::Ignored
                } else {
                    ResultUsage::Some
                };
                GraphEdge {
                    id: edge.id.clone(),
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                    kind: edge.kind,
                    sites: edge.sites,
                    result_used,
                    external: edge.external,
                    inferred: if edge.inferred { Some(true) } else { None },
                    call_sites: edge.call_sites.clone(),
                }
            })
            .collect()
    }
}
