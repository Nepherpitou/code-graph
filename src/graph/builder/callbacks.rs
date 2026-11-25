//! Function references passed as call arguments.
//!
//! `arr.map(bar)` does not contain a call to `bar`, so LSP call hierarchy
//! reports `map` but not the callback. Attribute the reference to the nearest
//! named callable: a callback passed from `foo` is an edge `foo -> bar`.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::languages::Adapter;
use crate::lsp::LspRange;
use crate::source::ast::{byte_to_lsp, find_callable};
use crate::source::SourceFile;

use super::Builder;

fn binding_name(node: Node, text: &str) -> Option<String> {
    let raw = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        .or_else(|| node.child_by_field_name("left"))
        .and_then(|name| name.utf8_text(text.as_bytes()).ok())?;
    let normalized = raw
        .trim()
        .strip_prefix("mut ")
        .or_else(|| raw.trim().strip_prefix("ref "))
        .unwrap_or(raw.trim());
    // A pattern can contain punctuation or a type; only keep a bare binding.
    let normalized = normalized
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

/// Collect identifiers bound by a destructuring/let/for/match pattern. This is
/// deliberately conservative: a false local-binding match suppresses one
/// inferred callback edge, while a miss creates a false call edge.
fn collect_pattern_names(pattern: Node, text: &str, names: &mut HashSet<String>) {
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" => {
                if let Ok(name) = node.utf8_text(text.as_bytes()) {
                    let name = name
                        .trim()
                        .strip_prefix("mut ")
                        .or_else(|| name.trim().strip_prefix("ref "))
                        .unwrap_or(name.trim());
                    if !name.is_empty() {
                        names.insert(name.to_string());
                    }
                }
            }
            // Stop at type/initializer fields; patterns themselves are enough.
            "type_identifier" | "primitive_type" | "type_arguments" | "parameters" => {}
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    stack.push(child);
                }
            }
        }
    }
}

fn local_binding_names(callable: Node, text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut stack = vec![callable];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "variable_declarator"
            | "assignment"
            | "function_declaration"
            | "function_expression"
            | "catch_clause" => {
                if let Some(bound) = binding_name(node, text) {
                    names.insert(bound);
                }
            }
            // Rust binding forms. `binding_name` handles the simple
            // `let mut x` / parameter cases; collect_pattern_names handles
            // destructuring so a shadowing local cannot become a callback.
            "let_declaration" | "let_condition" | "for_expression" | "parameter"
            | "closure_parameter" | "match_arm" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    collect_pattern_names(pattern, text, &mut names);
                }
                if let Some(bound) = binding_name(node, text) {
                    names.insert(bound);
                }
            }
            "formal_parameters" | "parameters" | "parameter_list" | "closure_parameters" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(pattern) = child.child_by_field_name("pattern") {
                        collect_pattern_names(pattern, text, &mut names);
                    }
                    if let Some(bound) = binding_name(child, text) {
                        names.insert(bound);
                    } else if child.kind() == "identifier" {
                        if let Ok(bound) = child.utf8_text(text.as_bytes()) {
                            names.insert(bound.to_string());
                        }
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
    names
}

#[cfg(test)]
fn has_local_binding(callable: Node, text: &str, name: &str) -> bool {
    local_binding_names(callable, text).contains(name)
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

struct PendingCallback {
    caller: String,
    target: String,
    rel: String,
    range: LspRange,
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

    fn callback_pending(
        &self,
        rel: &str,
        argument: Node<'_>,
        text: &str,
        bindings: &mut HashMap<usize, HashSet<String>>,
    ) -> Option<PendingCallback> {
        let identifier = callback_identifier(argument, self.adapter)?;
        let name = identifier.utf8_text(text.as_bytes()).ok()?;
        if argument.kind() != "member_expression" {
            if let Some(callable) = find_callable(argument, self.adapter) {
                let names = bindings
                    .entry(callable.id())
                    .or_insert_with(|| local_binding_names(callable, text));
                if names.contains(name) {
                    return None;
                }
            }
        }
        let target = self.resolve_callback_target(rel, name)?;
        let position = byte_to_lsp(text, argument.start_byte());
        let caller = self.caller_in_file(rel, position)?;
        Some(PendingCallback {
            caller,
            target,
            rel: rel.to_string(),
            range: LspRange {
                start: position,
                end: byte_to_lsp(text, argument.end_byte()),
            },
        })
    }

    fn scan_callbacks(
        &self,
        rel: &str,
        source: &SourceFile,
        seen: &mut HashSet<(String, String, usize)>,
        pending: &mut Vec<PendingCallback>,
    ) {
        let Some(tree) = source.tree.as_ref() else {
            return;
        };
        let text = source.text.as_str();
        let mut bindings = HashMap::new();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if self.adapter.call_nodes.contains(&node.kind()) {
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    let mut cursor = arguments.walk();
                    for argument in arguments.named_children(&mut cursor) {
                        let Some(callback) =
                            self.callback_pending(rel, argument, text, &mut bindings)
                        else {
                            continue;
                        };
                        let key = (
                            callback.caller.clone(),
                            callback.target.clone(),
                            argument.start_byte(),
                        );
                        if seen.insert(key) {
                            pending.push(callback);
                        }
                    }
                }
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                stack.push(child);
            }
        }
    }

    fn emit_callback_edges(&mut self, pending: Vec<PendingCallback>) {
        for callback in pending {
            let key = format!("call:{}->{}", callback.caller, callback.target);
            if self.edges.contains_key(&key) {
                continue;
            }
            let site = self.call_site(&callback.rel, callback.range);
            self.add_edge(&callback.caller, &callback.target, site, false, true);
        }
    }

    pub(crate) fn add_callback_edges(&mut self) {
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        for (rel, source) in &self.sources {
            self.scan_callbacks(rel, source, &mut seen, &mut pending);
        }
        self.emit_callback_edges(pending);
    }
}

#[cfg(test)]
mod tests {
    use super::has_local_binding;
    use tree_sitter::{Node, Parser, Tree};

    fn parse(source: &str) -> (String, Tree) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("rust grammar");
        let tree = parser.parse(source, None).expect("parse source");
        (source.to_string(), tree)
    }

    fn function<'a>(node: Node<'a>) -> Option<Node<'a>> {
        if node.kind() == "function_item" {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if let Some(found) = function(child) {
                return Some(found);
            }
        }
        None
    }

    fn callable<'a>(tree: &'a Tree) -> Node<'a> {
        function(tree.root_node()).expect("function item")
    }

    #[test]
    fn rust_let_and_for_bindings_shadow_function_names() {
        let (text, tree) =
            parse("fn demo() { let mut file = 1; for nodes in 0..2 { file += nodes; } }");
        let callable = callable(&tree);
        assert!(has_local_binding(callable, &text, "file"));
        assert!(has_local_binding(callable, &text, "nodes"));
    }

    #[test]
    fn rust_parameters_and_destructuring_bind_names() {
        let (text, tree) = parse("fn demo(index: usize) { let Some(file) = None else { return; }; let _ = (index, file); }");
        let callable = callable(&tree);
        assert!(has_local_binding(callable, &text, "index"));
        assert!(has_local_binding(callable, &text, "file"));
    }

    #[test]
    fn unrelated_names_are_not_bindings() {
        let (text, tree) = parse("fn demo() { let local = 1; let _ = local; }");
        let callable = callable(&tree);
        assert!(!has_local_binding(callable, &text, "file"));
        assert!(!has_local_binding(callable, &text, "nodes"));
    }
}
