use crate::lsp::CallHierarchyItem;

use super::{
    Builder, NodeRecord, NodeSpec, RawSymbol, Resolved, LSP_CLASS, LSP_CONSTANT, LSP_CONSTRUCTOR,
    LSP_FUNCTION, LSP_METHOD, LSP_VARIABLE,
};
use crate::graph::model::{GraphNode, NodeKind, NodeMetrics};

fn raw_symbol(
    item: &CallHierarchyItem,
    name: String,
    kind: u32,
    detail: String,
) -> RawSymbol {
    RawSymbol {
        name,
        kind,
        detail,
        range: item.range,
        selection: item.selection_range,
        children: Vec::new(),
    }
}

fn node_kind_from_lsp(kind: u32) -> Option<NodeKind> {
    match kind {
        LSP_CLASS => Some(NodeKind::Constructor),
        LSP_METHOD => Some(NodeKind::Method),
        LSP_CONSTRUCTOR => Some(NodeKind::Constructor),
        LSP_FUNCTION => Some(NodeKind::Function),
        LSP_VARIABLE => Some(NodeKind::Variable),
        LSP_CONSTANT => Some(NodeKind::Constant),
        _ => None,
    }
}

impl Builder {
    pub(super) fn ensure_external_node(
        &mut self,
        item: &CallHierarchyItem,
        rel: Option<&str>,
    ) -> String {
        let id = format!("ext:{}#{}", item.uri, item.name);
        if self.records.contains_key(&id) {
            return id;
        }
        let kind = node_kind_from_lsp(item.kind).unwrap_or(NodeKind::Other);
        let node = GraphNode {
            id: id.clone(),
            name: item.name.clone(),
            qname: item.detail.clone().unwrap_or_else(|| item.name.clone()),
            kind,
            file: rel.unwrap_or(&item.uri).to_string(),
            line: item.selection_range.start.line + 1,
            end_line: item.range.end.line + 1,
            loc: item.range.end.line.saturating_sub(item.range.start.line) + 1,
            sloc: 0,
            params: 0,
            cyclomatic: 0,
            exported: true,
            external: true,
            lazy: false,
            synthetic: false,
            is_entry_like: false,
            declaration: item.detail.clone().unwrap_or_default(),
            metrics: NodeMetrics::default(),
        };
        self.order.push(id.clone());
        self.records.insert(
            id.clone(),
            NodeRecord {
                node,
                uri: item.uri.clone(),
                rel: rel.unwrap_or(&item.uri).to_string(),
                range: item.range,
                selection: item.selection_range,
            },
        );
        id
    }

    pub(super) fn resolve_target(&mut self, item: &CallHierarchyItem) -> Resolved {
        let Some(rel) = self.to_relative(&item.uri) else {
            return Resolved::external();
        };
        let selection_key = format!(
            "{rel}#{}:{}",
            item.selection_range.start.line, item.selection_range.start.character
        );
        if let Some(id) = self.uri_selection.get(&selection_key) {
            return Resolved::found(id.clone(), false);
        }
        let class_key = format!(
            "{rel}#{}:{}",
            item.range.start.line, item.range.start.character
        );
        if let Some(id) = self
            .class_map
            .get(&class_key)
            .or_else(|| self.class_map.get(&selection_key))
        {
            return Resolved::found(id.clone(), false);
        }
        if let Some(containing_id) = self.caller_in_file(&rel, item.selection_range.start) {
            return self.resolve_containing(item, &rel, containing_id);
        }
        self.resolve_new(item, &rel)
    }

    fn resolve_containing(
        &mut self,
        item: &CallHierarchyItem,
        rel: &str,
        containing_id: String,
    ) -> Resolved {
        let Some(kind) = node_kind_from_lsp(item.kind) else {
            return Resolved::found(containing_id, false);
        };
        if !kind.is_callable() {
            return Resolved::found(containing_id, false);
        }
        if self.at_capacity() {
            return Resolved::external();
        }
        let containing_qname = self
            .records
            .get(&containing_id)
            .map(|record| record.node.qname.clone())
            .unwrap_or_else(|| item.name.clone());
        let symbol = raw_symbol(
            item,
            item.name.clone(),
            item.kind,
            item.detail.clone().unwrap_or_default(),
        );
        let qname = format!("{containing_qname}.{}", item.name);
        match self.create_node(NodeSpec {
            symbol: &symbol,
            rel,
            qname,
            kind,
            parent_exported: false,
            synthetic: false,
            lazy: true,
        }) {
            Some(id) => Resolved::found(id, true),
            None => Resolved::found(containing_id, false),
        }
    }

    fn resolve_new(&mut self, item: &CallHierarchyItem, rel: &str) -> Resolved {
        let Some(kind) = node_kind_from_lsp(item.kind) else {
            return Resolved::external();
        };
        if kind == NodeKind::Constructor && item.kind == LSP_CLASS {
            if self.at_capacity() {
                return Resolved::external();
            }
            let name = format!("{}.constructor", item.name);
            let symbol = raw_symbol(
                item,
                name.clone(),
                LSP_CONSTRUCTOR,
                "implicit constructor".to_string(),
            );
            let id = self.create_node(NodeSpec {
                symbol: &symbol,
                rel,
                qname: name,
                kind: NodeKind::Constructor,
                parent_exported: false,
                synthetic: true,
                lazy: false,
            });
            return Resolved {
                id,
                external: false,
                created_callable: false,
            };
        }
        if self.at_capacity() {
            return Resolved::external();
        }
        let symbol = raw_symbol(
            item,
            item.name.clone(),
            item.kind,
            item.detail.clone().unwrap_or_default(),
        );
        let id = self.create_node(NodeSpec {
            symbol: &symbol,
            rel,
            qname: item.name.clone(),
            kind,
            parent_exported: false,
            synthetic: false,
            lazy: true,
        });
        Resolved {
            id,
            external: false,
            created_callable: kind.is_callable(),
        }
    }

    fn at_capacity(&mut self) -> bool {
        if self.records.len() >= self.max_nodes {
            self.truncated = true;
            true
        } else {
            false
        }
    }
}
