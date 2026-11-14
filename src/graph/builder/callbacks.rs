//! Function references passed as call arguments.
//!
//! `arr.map(bar)` does not contain a call to `bar`, so LSP call hierarchy
//! reports `map` but not the callback. Attribute the reference to the nearest
//! named callable: a callback passed from `foo` is an edge `foo -> bar`.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::languages::Adapter;
use crate::lsp::LspRange;
use crate::source::ast::{byte_to_lsp, find_callable};

use super::Builder;

fn binding_name(node: Node, text: &str) -> Option<String> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        .or_else(|| node.child_by_field_name("left"))
        .and_then(|name| name.utf8_text(text.as_bytes()).ok())
        .map(str::to_string)
}

fn has_local_binding(callable: Node, text: &str, name: &str) -> bool {
    let mut stack = vec![callable];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "variable_declarator"
            | "assignment"
            | "function_declaration"
            | "function_expression"
            | "catch_clause" => {
                if binding_name(node, text).as_deref() == Some(name) {
                    return true;
                }
            }
            "formal_parameters" | "parameters" | "parameter_list" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if binding_name(child, text).as_deref() == Some(name)
                        || (child.kind() == "identifier"
                            && child.utf8_text(text.as_bytes()).ok() == Some(name))
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.id() == node.id() {
                continue;
            }
            stack.push(child);
        }
    }
    false
}

fn callback_identifier<'a>(mut node: Node<'a>, adapter: &Adapter) -> Option<Node<'a>> {
    loop {
        match node.kind() {
            "identifier" | "property_identifier" => return Some(node),
            "member_expression" => {
                node = node.child_by_field_name("property")?;
            }
            kind if adapter.transparent_nodes.contains(&kind) => {
                node = node
                    .child_by_field_name("expression")
                    .or_else(|| node.child_by_field_name("value"))
                    .or_else(|| node.named_child(0))?;
            }
            _ => return None,
        }
    }
}

impl Builder {
    fn resolve_callback_target(&self, rel: &str, name: &str) -> Option<String> {
        let mut same_file = Vec::new();
        if let Some(ids) = self.by_rel.get(rel) {
            for id in ids {
                let Some(record) = self.records.get(id) else {
                    continue;
                };
                if record.node.kind.is_callable()
                    && (record.node.name == name || record.node.qname == name)
                {
                    same_file.push(id.clone());
                }
            }
        }
        if same_file.len() == 1 {
            return same_file.pop();
        }

        let mut matches = self
            .records
            .iter()
            .filter(|(_, record)| {
                record.node.kind.is_callable()
                    && (record.node.name == name || record.node.qname == name)
            })
            .map(|(id, _)| id.clone());
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    pub(crate) fn add_callback_edges(&mut self) {
        let mut pending = Vec::new();
        let mut seen = HashSet::new();

        for (rel, source) in &self.sources {
            let Some(tree) = source.tree.as_ref() else {
                continue;
            };
            let text = source.text.as_str();
            let mut stack = vec![tree.root_node()];
            while let Some(node) = stack.pop() {
                if self.adapter.call_nodes.contains(&node.kind()) {
                    if let Some(arguments) = node.child_by_field_name("arguments") {
                        let mut cursor = arguments.walk();
                        for argument in arguments.named_children(&mut cursor) {
                            let Some(identifier) = callback_identifier(argument, self.adapter)
                            else {
                                continue;
                            };
                            let Ok(name) = identifier.utf8_text(text.as_bytes()) else {
                                continue;
                            };
                            if argument.kind() != "member_expression" {
                                if let Some(callable) = find_callable(argument, self.adapter) {
                                    if has_local_binding(callable, text, name) {
                                        continue;
                                    }
                                }
                            }
                            let Some(target) = self.resolve_callback_target(rel, name) else {
                                continue;
                            };
                            let position = byte_to_lsp(text, argument.start_byte());
                            let Some(caller) = self.caller_in_file(rel, position) else {
                                continue;
                            };
                            let key = (caller.clone(), target.clone(), argument.start_byte());
                            if !seen.insert(key) {
                                continue;
                            }
                            pending.push((
                                caller,
                                target,
                                rel.clone(),
                                LspRange {
                                    start: position,
                                    end: byte_to_lsp(text, argument.end_byte()),
                                },
                            ));
                        }
                    }
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    stack.push(child);
                }
            }
        }

        for (caller, target, rel, range) in pending {
            let key = format!("call:{caller}->{target}");
            if self.edges.contains_key(&key) {
                continue;
            }
            let site = self.call_site(&rel, range);
            self.add_edge(&caller, &target, site, false, true);
        }
    }
}
