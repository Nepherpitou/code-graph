use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Parser, Tree};
use walkdir::{DirEntry, WalkDir};

use crate::languages::{grammar, Adapter};

const IGNORE_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".next",
    ".nuxt",
    "vendor",
    "third_party",
    "coverage",
    ".cache",
    ".turbo",
    ".gradle",
    ".idea",
    ".vscode",
];

const IGNORE_SUFFIXES: &[&str] = &[".d.ts", ".min.js", ".map"];

pub struct SourceFile {
    pub abs: PathBuf,
    pub rel: String,
    pub text: String,
    pub tree: Option<Tree>,
}

pub struct DiscoverOptions {
    pub excludes: Vec<PathBuf>,
    pub max_file_size: u64,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            excludes: Vec::new(),
            max_file_size: 2 * 1024 * 1024,
        }
    }
}

fn ignored(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy().to_string();
    if name.starts_with('.') {
        return false;
    }
    if entry.file_type().is_dir() && IGNORE_DIRS.contains(&name.as_str()) {
        return false;
    }
    true
}

pub fn discover(root: &Path, adapter: &Adapter, options: &DiscoverOptions) -> Vec<SourceFile> {
    let mut files = Vec::new();
    let excludes: Vec<PathBuf> = options
        .excludes
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(ignored)
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if excludes.iter().any(|ex| path == ex || path.starts_with(ex)) {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if IGNORE_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
            continue;
        }
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
        else {
            continue;
        };
        if !adapter.extensions.contains(&ext.as_str()) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > options.max_file_size {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let mut parser = Parser::new();
        let tree = parser
            .set_language(&grammar(adapter.id, &rel))
            .ok()
            .and_then(|_| parser.parse(&text, None));
        files.push(SourceFile {
            abs: path.to_path_buf(),
            rel,
            text,
            tree,
        });
    }
    files
}
