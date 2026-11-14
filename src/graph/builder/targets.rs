use crate::lsp::CallHierarchyItem;

use super::{Builder, NodeRecord, RawSymbol, Resolved};
use crate::graph::model::{GraphNode, NodeKind, NodeMetrics};

fn node_kind_from_lsp(kind: u32) -> Option<NodeKind> {
    match kind {
        5 => Some(NodeKind::Constructor),
        6 => Some(NodeKind::Method),
        9 => Some(NodeKind::Constructor),
        12 => Some(NodeKind::Function),
        13 => Some(NodeKind::Variable),
        14 => Some(NodeKind::Constant),
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
        let rel = self.to_relative(&item.uri);
        let Some(rel) = rel else {
            return Resolved {
                id: None,
                external: true,
                created_callable: false,
            };
        };
        let selection_key = format!(
            "{rel}#{}:{}",
            item.selection_range.start.line, item.selection_range.start.character
        );
        if let Some(id) = self.uri_selection.get(&selection_key) {
            return Resolved {
                id: Some(id.clone()),
                external: false,
                created_callable: false,
            };
        }
        let class_key = format!(
            "{rel}#{}:{}",
            item.range.start.line, item.range.start.character
        );
        if let Some(id) = self.class_map.get(&class_key) {
            return Resolved {
                id: Some(id.clone()),
                external: false,
                created_callable: false,
            };
        }
        let class_key_alt = selection_key;
        if let Some(id) = self.class_map.get(&class_key_alt) {
            return Resolved {
                id: Some(id.clone()),
                external: false,
                created_callable: false,
            };
        }

        if let Some(containing_id) = self.caller_in_file(&rel, item.selection_range.start) {
            let kind = node_kind_from_lsp(item.kind);
            if let Some(kind) = kind {
                if kind.is_callable() {
                    if self.records.len() >= self.max_nodes {
                        self.truncated = true;
                        return Resolved {
                            id: None,
                            external: true,
                            created_callable: false,
                        };
                    }
                    let containing_qname = self
                        .records
                        .get(&containing_id)
                        .map(|record| record.node.qname.clone())
                        .unwrap_or_else(|| item.name.clone());
                    let symbol = RawSymbol {
                        name: item.name.clone(),
                        kind: item.kind,
                        detail: item.detail.clone().unwrap_or_default(),
                        range: item.range,
                        selection: item.selection_range,
                        children: Vec::new(),
                    };
                    if let Some(id) = self.create_node(
                        &symbol,
                        &rel,
                        format!("{containing_qname}.{}", item.name),
                        kind,
                        false,
                        false,
                        true,
                        None,
                    ) {
                        return Resolved {
                            id: Some(id),
                            external: false,
                            created_callable: true,
                        };
                    }
                }
            }
            return Resolved {
                id: Some(containing_id),
                external: false,
                created_callable: false,
            };
        }

        let Some(kind) = node_kind_from_lsp(item.kind) else {
            return Resolved {
                id: None,
                external: true,
                created_callable: false,
            };
        };
        if kind == NodeKind::Constructor && item.kind == 5 {
            if self.records.len() >= self.max_nodes {
                self.truncated = true;
                return Resolved {
                    id: None,
                    external: true,
                    created_callable: false,
                };
            }
            let symbol = RawSymbol {
                name: format!("{}.constructor", item.name),
                kind: 9,
                detail: "implicit constructor".to_string(),
                range: item.range,
                selection: item.selection_range,
                children: Vec::new(),
            };
            let id = self.create_node(
                &symbol,
                &rel,
                format!("{}.constructor", item.name),
                NodeKind::Constructor,
                false,
                true,
                false,
                None,
            );
            return Resolved {
                id,
                external: false,
                created_callable: false,
            };
        }
        if self.records.len() >= self.max_nodes {
            self.truncated = true;
            return Resolved {
                id: None,
                external: true,
                created_callable: false,
            };
        }
        let symbol = RawSymbol {
            name: item.name.clone(),
            kind: item.kind,
            detail: item.detail.clone().unwrap_or_default(),
            range: item.range,
            selection: item.selection_range,
            children: Vec::new(),
        };
        let id = self.create_node(
            &symbol,
            &rel,
            item.name.clone(),
            kind,
            false,
            false,
            true,
            None,
        );
        Resolved {
            id,
            external: false,
            created_callable: kind.is_callable(),
        }
    }
}
