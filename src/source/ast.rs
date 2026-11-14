use tree_sitter::{Node, Tree};

use crate::languages::Adapter;
use crate::lsp::LspPosition;

pub fn byte_to_lsp(text: &str, offset: usize) -> LspPosition {
    let offset = offset.min(text.len());
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let character = prefix[line_start..].encode_utf16().count() as u32;
    LspPosition { line, character }
}

pub fn lsp_to_byte(text: &str, position: LspPosition) -> Option<usize> {
    let mut line_start = 0usize;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index == position.line as usize {
            let mut units = 0usize;
            for (byte, ch) in line.char_indices() {
                if units >= position.character as usize {
                    return Some(line_start + byte);
                }
                units += ch.len_utf16();
            }
            return Some(line_start + line.len());
        }
        line_start += line.len();
    }
    if position.line as usize >= text.split_inclusive('\n').count() {
        Some(text.len())
    } else {
        None
    }
}

pub fn node_at<'a>(tree: &'a Tree, text: &str, position: LspPosition) -> Option<Node<'a>> {
    let offset = lsp_to_byte(text, position)?;
    let offset = offset.min(text.len().saturating_sub(1));
    let end = (offset + 1).min(text.len());
    tree.root_node().descendant_for_byte_range(offset, end)
}

pub fn find_callable<'a>(mut node: Node<'a>, adapter: &Adapter) -> Option<Node<'a>> {
    loop {
        if adapter.callable_nodes.contains(&node.kind()) {
            return Some(node);
        }
        if matches!(
            node.kind(),
            "program" | "source_file" | "module" | "translation_unit"
        ) {
            return None;
        }
        node = node.parent()?;
    }
}

pub fn declares_callable(
    tree: &Tree,
    text: &str,
    adapter: &Adapter,
    position: LspPosition,
) -> bool {
    let Some(node) = node_at(tree, text, position) else {
        return false;
    };
    let Some(callable) = find_callable(node, adapter) else {
        return false;
    };
    let offset = lsp_to_byte(text, position).unwrap_or(0);
    if let Some(name) = callable.child_by_field_name("name") {
        return contains_offset(name, offset);
    }
    callable.start_position().row == position.line as usize
}

pub fn declares_data(tree: &Tree, text: &str, adapter: &Adapter, position: LspPosition) -> bool {
    let Some(mut node) = node_at(tree, text, position) else {
        return false;
    };
    let offset = lsp_to_byte(text, position).unwrap_or(0);
    let declarator = loop {
        if matches!(
            node.kind(),
            "variable_declarator" | "assignment" | "field_definition" | "public_field_definition"
        ) {
            break node;
        }
        if matches!(
            node.kind(),
            "program" | "source_file" | "module" | "translation_unit"
        ) {
            return false;
        }
        match node.parent() {
            Some(parent) => node = parent,
            None => return false,
        }
    };
    let name = declarator
        .child_by_field_name("name")
        .or_else(|| declarator.child_by_field_name("left"));
    if name
        .map(|name| !contains_offset(name, offset))
        .unwrap_or(true)
    {
        return false;
    }
    // Constants/variables inside a function are locals, not graph primitives.
    let mut ancestor = declarator.parent();
    while let Some(current) = ancestor {
        if adapter.callable_nodes.contains(&current.kind()) {
            return false;
        }
        ancestor = current.parent();
    }
    true
}

pub fn describe_function(
    source_tree: &Tree,
    text: &str,
    adapter: &Adapter,
    position: LspPosition,
) -> Option<(u32, u32)> {
    let node = node_at(source_tree, text, position)?;
    let callable = find_callable(node, adapter)?;
    Some((
        count_parameters(callable, adapter),
        cyclomatic(callable, adapter),
    ))
}

fn normalized_text(node: Node<'_>, text: &str) -> String {
    node.utf8_text(text.as_bytes())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn signature_at(
    source_tree: &Tree,
    text: &str,
    adapter: &Adapter,
    position: LspPosition,
) -> Option<String> {
    let node = node_at(source_tree, text, position)?;
    let callable = find_callable(node, adapter)?;
    let mut parts = Vec::new();

    if let Some(type_parameters) = callable.child_by_field_name("type_parameters") {
        let text = normalized_text(type_parameters, text);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    if let Some(parameters) = find_parameters(callable, adapter) {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if parameter.kind() == "comment" {
                continue;
            }
            let value = parameter
                .child_by_field_name("type")
                .map(|node| normalized_text(node, text))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| normalized_text(parameter, text));
            parts.push(value);
        }
    }
    if let Some(return_type) = callable.child_by_field_name("return_type") {
        let text = normalized_text(return_type, text);
        if !text.is_empty() {
            parts.push(format!("-> {text}"));
        }
    }
    Some(parts.join(";"))
}

pub fn variable_callable(
    source_tree: &Tree,
    text: &str,
    adapter: &Adapter,
    position: LspPosition,
) -> Option<(u32, u32)> {
    let mut node = node_at(source_tree, text, position)?;
    loop {
        if matches!(node.kind(), "variable_declarator" | "assignment") {
            let value = node
                .child_by_field_name("value")
                .or_else(|| node.child_by_field_name("right"));
            if let Some(value) = value {
                if adapter.callable_nodes.contains(&value.kind()) {
                    return Some((count_parameters(value, adapter), cyclomatic(value, adapter)));
                }
            }
            return None;
        }
        if matches!(
            node.kind(),
            "program" | "source_file" | "module" | "translation_unit"
        ) {
            return None;
        }
        node = node.parent()?;
    }
}

fn find_parameters<'a>(callable: Node<'a>, adapter: &Adapter) -> Option<Node<'a>> {
    let wanted = ["formal_parameters", "parameters", "parameter_list"];
    let mut queue: Vec<(Node<'a>, u32)> = vec![(callable, 0)];
    while let Some((node, depth)) = queue.pop() {
        if node.id() != callable.id() && adapter.callable_nodes.contains(&node.kind()) {
            continue;
        }
        if depth > 0 && wanted.contains(&node.kind()) {
            return Some(node);
        }
        if depth >= 4 {
            continue;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            queue.push((child, depth + 1));
        }
    }
    None
}

fn count_parameters(callable: Node<'_>, adapter: &Adapter) -> u32 {
    if let Some(parameters) = find_parameters(callable, adapter) {
        let mut cursor = parameters.walk();
        let count = parameters
            .named_children(&mut cursor)
            .filter(|child| !matches!(child.kind(), "comment" | "self_parameter"))
            .count();
        return count as u32;
    }
    if callable.child_by_field_name("parameter").is_some() {
        return 1;
    }
    0
}

fn cyclomatic(callable: Node<'_>, adapter: &Adapter) -> u32 {
    fn walk(node: Node<'_>, root_id: usize, adapter: &Adapter, complexity: &mut u32) {
        if node.id() != root_id && adapter.callable_nodes.contains(&node.kind()) {
            return;
        }
        let kind = node.kind();
        if adapter.decision_nodes.contains(&kind) {
            *complexity += 1;
        }
        if kind == "binary_expression" {
            if let Some(operator) = node.child_by_field_name("operator") {
                if adapter.decision_operators.contains(&operator.kind()) {
                    *complexity += 1;
                }
            }
        }
        if matches!(kind, "switch_case" | "case_statement") {
            // counted above
        }
        if kind == "match_expression" {
            let mut cursor = node.walk();
            let arms = node
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "match_arm")
                .count();
            if arms > 0 {
                *complexity += arms.saturating_sub(1) as u32;
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, root_id, adapter, complexity);
        }
    }

    let mut complexity = 1u32;
    walk(callable, callable.id(), adapter, &mut complexity);
    complexity
}

fn contains_offset(node: Node<'_>, offset: usize) -> bool {
    node.start_byte() <= offset && offset <= node.end_byte()
}

pub fn result_used_at(
    tree: &Tree,
    text: &str,
    adapter: &Adapter,
    position: LspPosition,
) -> Option<bool> {
    let offset = lsp_to_byte(text, position)?;
    let mut node = node_at(tree, text, position)?;
    let call = loop {
        if adapter.call_nodes.contains(&node.kind()) {
            break Some(node);
        }
        if adapter.callable_nodes.contains(&node.kind())
            || matches!(
                node.kind(),
                "program" | "source_file" | "module" | "translation_unit"
            )
        {
            break None;
        }
        match node.parent() {
            Some(parent) => node = parent,
            None => break None,
        }
    };
    let call = call?;

    let callee = adapter
        .callee_fields
        .iter()
        .filter_map(|field| call.child_by_field_name(field))
        .next();
    if let Some(callee) = callee {
        if !contains_offset(callee, offset) {
            return None;
        }
    }

    let mut current = call;
    let mut parent = current.parent();
    while let Some(p) = parent {
        if adapter.transparent_nodes.contains(&p.kind()) {
            current = p;
            parent = current.parent();
        } else {
            break;
        }
    }
    let parent = parent?;
    if adapter.statement_nodes.contains(&parent.kind()) {
        return Some(false);
    }
    if adapter.id == crate::languages::LanguageId::TypeScript && parent.kind() == "unary_expression"
    {
        if let Some(operator) = parent.child_by_field_name("operator") {
            if operator.kind() == "void" || operator.utf8_text(text.as_bytes()).ok() == Some("void")
            {
                return Some(false);
            }
        }
    }
    Some(true)
}
