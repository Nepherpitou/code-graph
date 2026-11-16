use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::languages::{Adapter, LanguageId};
use crate::lsp::LspRange;
use crate::source::ast::{
    declares_callable, declares_data, declares_type_only, describe_function, signature_at,
    variable_callable,
};
use crate::source::SourceFile;

use super::{
    Builder, NodeRecord, RawSymbol, LSP_CLASS, LSP_CONSTANT, LSP_CONSTRUCTOR, LSP_ENUM,
    LSP_FUNCTION, LSP_INTERFACE, LSP_METHOD, LSP_MODULE, LSP_NAMESPACE, LSP_OBJECT, LSP_PACKAGE,
    LSP_STRUCT, LSP_VARIABLE,
};
use crate::graph::model::{GraphNode, NodeKind, NodeMetrics};

fn parse_range(value: &Value) -> Option<LspRange> {
    serde_json::from_value(value.clone()).ok()
}

fn parse_symbol(value: &Value) -> Option<RawSymbol> {
    let name = value.get("name")?.as_str()?.to_string();
    let kind = value.get("kind")?.as_u64()? as u32;
    let detail = value
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (range, selection) = if let Some(location) = value.get("location") {
        let range = parse_range(location.get("range")?)?;
        (range, range)
    } else {
        let range = parse_range(value.get("range")?)?;
        let selection = value
            .get("selectionRange")
            .and_then(parse_range)
            .unwrap_or(range);
        (range, selection)
    };
    let children = value
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().filter_map(parse_symbol).collect())
        .unwrap_or_default();
    Some(RawSymbol {
        name,
        kind,
        detail,
        range,
        selection,
        children,
    })
}

pub(crate) fn parse_symbols(value: &Value) -> Vec<RawSymbol> {
    value
        .as_array()
        .map(|symbols| symbols.iter().filter_map(parse_symbol).collect())
        .unwrap_or_default()
}

fn node_kind_from_symbol(kind: u32) -> Option<NodeKind> {
    match kind {
        LSP_FUNCTION => Some(NodeKind::Function),
        LSP_METHOD => Some(NodeKind::Method),
        LSP_CONSTRUCTOR => Some(NodeKind::Constructor),
        LSP_CONSTANT => Some(NodeKind::Constant),
        LSP_VARIABLE => Some(NodeKind::Variable),
        _ => None,
    }
}

fn declaration_line(source: Option<&SourceFile>, line: u32) -> String {
    source
        .and_then(|source| source.text.lines().nth(line as usize))
        .map(|text| text.trim().chars().take(200).collect())
        .unwrap_or_default()
}

/// Count non-empty, non-comment lines in a declaration range.
///
/// This is deliberately a naive, language-agnostic heuristic. It is only used
/// for ranking; it is not part of the dead-code predicate.
fn source_sloc(source: Option<&SourceFile>, start: u32, end: u32) -> u32 {
    let Some(source) = source else {
        return 0;
    };
    if end < start {
        return 0;
    }
    source
        .text
        .lines()
        .skip(start as usize)
        .take((end - start + 1) as usize)
        .filter(|line| {
            let line = line.trim();
            !line.is_empty()
                && !line.starts_with("//")
                && !line.starts_with("/*")
                && !line.starts_with('*')
                && !(line.starts_with('#') && !line.starts_with("#["))
        })
        .count() as u32
}

fn is_exported(
    adapter: &Adapter,
    declaration: &str,
    rel: &str,
    name: &str,
    parent_exported: bool,
) -> bool {
    match adapter.id {
        LanguageId::TypeScript => {
            parent_exported || declaration.split_whitespace().any(|word| word == "export")
        }
        LanguageId::Rust => {
            parent_exported
                || declaration
                    .split_whitespace()
                    .any(|word| word == "pub" || word.starts_with("pub("))
        }
        LanguageId::Python => {
            let module_private = rel
                .rsplit('/')
                .next()
                .map(|file| file.starts_with('_'))
                .unwrap_or(false);
            !name.starts_with('_') && !module_private
        }
        LanguageId::Cpp => {
            parent_exported
                || (matches!(
                    Path::new(rel).extension().and_then(|ext| ext.to_str()),
                    Some("h" | "hh" | "hpp" | "hxx")
                ) && !declaration.split_whitespace().any(|word| word == "static"))
        }
    }
}

fn is_entry_like(name: &str, rel: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or(name).to_lowercase();
    let prefixes = [
        "main",
        "init",
        "run",
        "start",
        "setup",
        "teardown",
        "beforeeach",
        "aftereach",
        "describe",
        "it",
        "test",
        "bench",
    ];
    if prefixes.iter().any(|prefix| last.starts_with(prefix)) {
        return true;
    }
    if last.starts_with("test_") || last.ends_with("_test") {
        return true;
    }
    if rel
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__" | "spec"))
    {
        return true;
    }
    if rel.contains(".test.") || rel.contains(".spec.") {
        return true;
    }
    rel.ends_with("tests.rs") || rel.ends_with("test.rs")
}

fn normalize_container_name(name: &str) -> String {
    let mut cleaned = name.trim().to_string();
    if let Some(rest) = cleaned.strip_prefix("impl") {
        cleaned = rest.trim_start().to_string();
    }
    if cleaned.starts_with('<') {
        if let Some(end) = cleaned.find('>') {
            cleaned = cleaned[end + 1..].trim().to_string();
        }
    }
    if let Some((_, rest)) = cleaned.split_once(" for ") {
        cleaned = rest.trim().to_string();
    }
    let cleaned = cleaned.replace(['<', '>'], "");
    cleaned
        .split('\'')
        .next()
        .unwrap_or(&cleaned)
        .trim()
        .to_string()
}

fn signature_hash(signature: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in signature.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn ensure_unique(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}#{index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

struct NodeShape {
    declaration: String,
    params: u32,
    cyclomatic: u32,
    resolved_kind: NodeKind,
    sloc: u32,
}

impl Builder {
    fn reserve_node(&mut self) -> bool {
        if self.records.len() >= self.max_nodes {
            self.truncated = true;
            false
        } else {
            true
        }
    }

    fn node_shape(
        &self,
        symbol: &RawSymbol,
        rel: &str,
        kind: NodeKind,
        synthetic: bool,
    ) -> Option<NodeShape> {
        let source = self.sources.get(rel);
        let mut declaration = declaration_line(source, symbol.range.start.line);
        if declaration.is_empty() {
            declaration = symbol.detail.clone();
        }
        let mut shape = NodeShape {
            declaration,
            params: 0,
            cyclomatic: 0,
            resolved_kind: kind,
            sloc: if synthetic {
                0
            } else {
                source_sloc(source, symbol.range.start.line, symbol.range.end.line)
            },
        };
        let Some(source) = source else {
            return Some(shape);
        };
        let Some(tree) = &source.tree else {
            return Some(shape);
        };
        if !synthetic && kind.is_callable() {
            if let Some((params, complexity)) =
                describe_function(tree, &source.text, self.adapter, symbol.selection.start)
            {
                if !declares_callable(tree, &source.text, self.adapter, symbol.selection.start) {
                    return None; // e.g. JSX branches TypeScript reports as methods
                }
                shape.params = params;
                shape.cyclomatic = complexity;
            } else {
                let (params, complexity) =
                    variable_callable(tree, &source.text, self.adapter, symbol.selection.start)?;
                shape.params = params;
                shape.cyclomatic = complexity;
            }
        } else if kind.is_data() {
            if self.adapter.id == LanguageId::TypeScript
                && !declares_data(tree, &source.text, self.adapter, symbol.selection.start)
            {
                return None;
            }
            if let Some((params, complexity)) =
                variable_callable(tree, &source.text, self.adapter, symbol.selection.start)
            {
                shape.resolved_kind = NodeKind::Function;
                shape.params = params;
                shape.cyclomatic = complexity;
            }
        }
        Some(shape)
    }

    fn node_signature(
        &self,
        symbol: &RawSymbol,
        rel: &str,
        resolved_kind: NodeKind,
        synthetic: bool,
    ) -> Option<String> {
        if !resolved_kind.is_callable() || synthetic {
            return None;
        }
        self.sources
            .get(rel)
            .and_then(|source| {
                source.tree.as_ref().and_then(|tree| {
                    signature_at(tree, &source.text, self.adapter, symbol.selection.start)
                })
            })
            .or_else(|| {
                let detail = symbol.detail.trim();
                (!detail.is_empty()).then(|| detail.to_string())
            })
    }

    fn node_id(
        &mut self,
        rel: &str,
        qname: &str,
        resolved_kind: NodeKind,
        signature: Option<&str>,
    ) -> String {
        let base = match signature.filter(|value| !value.is_empty()) {
            Some(signature) => format!(
                "{rel}#{}:{qname}@{:016x}",
                resolved_kind.as_str(),
                signature_hash(signature)
            ),
            None => format!("{rel}#{}:{qname}", resolved_kind.as_str()),
        };
        ensure_unique(base, &mut self.used_ids)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_node(
        &mut self,
        symbol: &RawSymbol,
        rel: &str,
        qname: String,
        kind: NodeKind,
        parent_exported: bool,
        synthetic: bool,
        lazy: bool,
        _class_name: Option<String>,
    ) -> Option<String> {
        if !self.reserve_node() {
            return None;
        }
        let shape = self.node_shape(symbol, rel, kind, synthetic)?;
        let name = symbol.name.clone();
        let exported = is_exported(
            self.adapter,
            &shape.declaration,
            rel,
            &name,
            parent_exported,
        );
        let signature = self.node_signature(symbol, rel, shape.resolved_kind, synthetic);
        let id = self.node_id(rel, &qname, shape.resolved_kind, signature.as_deref());
        let line = symbol.selection.start.line;
        let node = GraphNode {
            id: id.clone(),
            name,
            qname: qname.clone(),
            kind: shape.resolved_kind,
            file: rel.to_string(),
            line: line + 1,
            end_line: symbol.range.end.line + 1,
            loc: symbol
                .range
                .end
                .line
                .saturating_sub(symbol.range.start.line)
                + 1,
            sloc: shape.sloc,
            params: shape.params,
            cyclomatic: shape.cyclomatic,
            exported,
            external: false,
            lazy,
            synthetic,
            is_entry_like: shape.resolved_kind.is_callable() && is_entry_like(&qname, rel),
            declaration: shape.declaration,
            metrics: NodeMetrics::default(),
        };
        let uri = self.uri_for_rel(rel);
        self.record(NodeRecord {
            node,
            uri,
            rel: rel.to_string(),
            range: symbol.range,
            selection: symbol.selection,
        });
        Some(id)
    }

    fn is_type_only_symbol(&self, symbol: &RawSymbol, rel: &str) -> bool {
        self.sources
            .get(rel)
            .and_then(|source| {
                source.tree.as_ref().map(|tree| {
                    declares_type_only(tree, &source.text, symbol.selection.start)
                })
            })
            .unwrap_or(false)
    }

    fn add_class_symbol(
        &mut self,
        symbol: &RawSymbol,
        rel: &str,
        local: &str,
        qname: &str,
        parent_exported: bool,
    ) -> bool {
        let class_key = format!(
            "{rel}#{}:{}",
            symbol.range.start.line, symbol.range.start.character
        );
        let class_key_alt = format!(
            "{rel}#{}:{}",
            symbol.selection.start.line, symbol.selection.start.character
        );
        let declaration = declaration_line(self.sources.get(rel), symbol.range.start.line);
        let class_exported = is_exported(self.adapter, &declaration, rel, local, parent_exported);
        let constructor_id = if let Some(constructor) = symbol
            .children
            .iter()
            .find(|child| child.kind == LSP_CONSTRUCTOR)
        {
            self.create_node(
                constructor,
                rel,
                format!("{qname}.constructor"),
                NodeKind::Constructor,
                class_exported,
                false,
                false,
                Some(qname.to_string()),
            )
        } else {
            let synthetic = RawSymbol {
                name: format!("{local}.constructor"),
                kind: LSP_CONSTRUCTOR,
                detail: "implicit constructor".to_string(),
                range: symbol.range,
                selection: symbol.selection,
                children: Vec::new(),
            };
            self.create_node(
                &synthetic,
                rel,
                format!("{qname}.constructor"),
                NodeKind::Constructor,
                class_exported,
                true,
                false,
                Some(qname.to_string()),
            )
        };
        if let Some(id) = constructor_id {
            self.class_map.insert(class_key, id.clone());
            self.class_map.insert(class_key_alt, id);
        }
        class_exported
    }

    fn add_leaf_symbol(
        &mut self,
        symbol: &RawSymbol,
        rel: &str,
        qname: String,
        parent_exported: bool,
        container: Option<&str>,
    ) {
        let Some(kind) = node_kind_from_symbol(symbol.kind) else {
            return;
        };
        if kind.is_data() {
            let declaration = declaration_line(self.sources.get(rel), symbol.range.start.line);
            let is_type_alias = ["type ", "export type ", "declare type "]
                .iter()
                .any(|prefix| declaration.starts_with(prefix));
            if is_type_alias
                || !self.include_constants
                || (kind == NodeKind::Variable && container.is_some())
            {
                return;
            }
        }
        self.create_node(
            symbol,
            rel,
            qname,
            kind,
            parent_exported,
            false,
            false,
            container.map(str::to_string),
        );
    }

    pub(crate) fn add_symbol(
        &mut self,
        symbol: &RawSymbol,
        rel: &str,
        parent_qname: &str,
        parent_exported: bool,
        container: Option<&str>,
    ) {
        if symbol.name.ends_with(") callback") || symbol.name.starts_with('<') {
            return;
        }
        let local = symbol.name.clone();
        let qname = if !parent_qname.is_empty() {
            format!("{parent_qname}.{local}")
        } else if let Some(container) = container {
            format!("{container}.{local}")
        } else {
            local.clone()
        };
        match symbol.kind {
            LSP_CLASS => {
                // Type aliases and interfaces are not instantiable. Do not
                // invent a synthetic constructor (and do not walk their
                // members) when the LSP reports them as LSP_CLASS.
                if self.is_type_only_symbol(symbol, rel) {
                    return;
                }
                let class_exported =
                    self.add_class_symbol(symbol, rel, &local, &qname, parent_exported);
                for child in &symbol.children {
                    if child.kind == LSP_CONSTRUCTOR {
                        continue;
                    }
                    if child.kind == LSP_METHOD || child.kind == LSP_FUNCTION {
                        self.add_symbol(child, rel, &qname, class_exported, Some(&qname));
                    }
                }
            }
            LSP_STRUCT => {} // Rust struct: a type, not a graph primitive
            LSP_OBJECT => {
                let container = normalize_container_name(&local);
                let trait_impl = local.contains(" for ");
                for child in &symbol.children {
                    self.add_symbol(
                        child,
                        rel,
                        "",
                        parent_exported || trait_impl,
                        Some(&container),
                    );
                }
            }
            LSP_INTERFACE | LSP_ENUM | LSP_MODULE | LSP_NAMESPACE | LSP_PACKAGE => {
                for child in &symbol.children {
                    self.add_symbol(child, rel, &qname, parent_exported, Some(&qname));
                }
            }
            _ => self.add_leaf_symbol(symbol, rel, qname, parent_exported, container),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn source(text: &str) -> SourceFile {
        SourceFile {
            abs: PathBuf::new(),
            rel: "test.rs".to_string(),
            text: text.to_string(),
            tree: None,
        }
    }

    #[test]
    fn sloc_skips_blank_and_comment_lines() {
        let source = source("fn f() {\n    // comment\n\n    let x = 1;\n}\n");
        assert_eq!(source_sloc(Some(&source), 0, 3), 2);
        assert_eq!(source_sloc(None, 0, 4), 0);
    }

    #[test]
    fn container_name_drops_rust_lifetimes() {
        assert_eq!(normalize_container_name("GraphAnalysis'a"), "GraphAnalysis");
        assert_eq!(
            normalize_container_name("impl<'a> GraphAnalysis<'a>"),
            "GraphAnalysis"
        );
    }
}
