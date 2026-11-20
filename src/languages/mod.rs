//! Language capabilities: server command, file extensions, tree-sitter node
//! classifications, grammar selection, and language detection.

mod adapters;
mod servers;

use std::fs;
use std::path::Path;

use serde::Serialize;
use tree_sitter::Language;

use adapters::{CPP, PYTHON, RUST, TYPESCRIPT};

pub(crate) use servers::{ReadyCtx, Readiness, ServerKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageId {
    TypeScript,
    Rust,
    Cpp,
    Python,
}

impl LanguageId {
    pub fn as_str(self) -> &'static str {
        match self {
            LanguageId::TypeScript => "typescript",
            LanguageId::Rust => "rust",
            LanguageId::Cpp => "cpp",
            LanguageId::Python => "python",
        }
    }
}

pub struct Adapter {
    pub id: LanguageId,
    pub extensions: &'static [&'static str],
    pub server: ServerKind,
    pub root_markers: &'static [&'static str],
    pub call_nodes: &'static [&'static str],
    pub statement_nodes: &'static [&'static str],
    pub transparent_nodes: &'static [&'static str],
    pub callable_nodes: &'static [&'static str],
    pub decision_nodes: &'static [&'static str],
    pub decision_operators: &'static [&'static str],
    pub callee_fields: &'static [&'static str],
}

pub fn adapter(id: LanguageId) -> &'static Adapter {
    match id {
        LanguageId::TypeScript => &TYPESCRIPT,
        LanguageId::Rust => &RUST,
        LanguageId::Cpp => &CPP,
        LanguageId::Python => &PYTHON,
    }
}

pub fn all() -> [&'static Adapter; 4] {
    [&TYPESCRIPT, &RUST, &CPP, &PYTHON]
}

pub fn grammar(id: LanguageId, file: &str) -> Language {
    match id {
        LanguageId::TypeScript => {
            let ext = Path::new(file)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext == "tsx" {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            } else if matches!(ext, "js" | "jsx" | "mjs" | "cjs") {
                tree_sitter_javascript::LANGUAGE.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
        }
        LanguageId::Rust => tree_sitter_rust::LANGUAGE.into(),
        LanguageId::Cpp => {
            let ext = Path::new(file)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if matches!(ext, "c" | "h") {
                tree_sitter_c::LANGUAGE.into()
            } else {
                tree_sitter_cpp::LANGUAGE.into()
            }
        }
        LanguageId::Python => tree_sitter_python::LANGUAGE.into(),
    }
}

pub fn detect_language(root: &Path) -> Option<LanguageId> {
    let mut scores: Vec<(LanguageId, i32)> = all()
        .iter()
        .map(|adapter| {
            let mut score = 0;
            for marker in adapter.root_markers {
                if root.join(marker).exists() {
                    score += 5;
                }
            }
            (adapter.id, score)
        })
        .collect();

    fn walk(dir: &Path, depth: u32, scores: &mut Vec<(LanguageId, i32)>) {
        if depth > 2 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1, scores);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext = format!(".{ext}");
                for (id, score) in scores.iter_mut() {
                    if adapter(*id).extensions.contains(&ext.as_str()) {
                        *score += 1;
                    }
                }
            }
        }
    }
    walk(root, 0, &mut scores);

    scores
        .into_iter()
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(_, score)| *score)
        .map(|(id, _)| id)
}

pub fn file_language_id(file: &str) -> &'static str {
    match Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "rs" => "rust",
        "py" => "python",
        _ => "cpp",
    }
}
