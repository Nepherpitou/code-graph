//! Per-language conventions shared by the metrics, the builder and the audit views.
//!
//! Test markers, framework suffixes, abstract markers, shared plumbing and the
//! reasons a changed file produces no symbol rows are all knowledge about a
//! stack. They live here and nowhere else: callers resolve one `Conventions`
//! from the snapshot language and ask it questions instead of matching path
//! strings. Call edges that come from overrides, DI wiring or tests are
//! classified here too, so metrics and views cannot disagree about them.

use std::collections::HashSet;

use super::model::{EdgeKind, GraphEdge, GraphNode, NodeKind};

pub(crate) struct Conventions {
    test_suffixes: &'static [&'static str],
    test_segments: &'static [&'static str],
    test_qname_prefixes: &'static [&'static str],
    framework_suffixes: &'static [&'static str],
    abstract_markers: &'static [&'static str],
    shared_prefixes: &'static [&'static str],
    silent_reasons: &'static [(&'static str, &'static str)],
}

const TYPESCRIPT: Conventions = Conventions {
    test_suffixes: &[".spec.ts"],
    test_segments: &["/test/"],
    test_qname_prefixes: &["tests."],
    framework_suffixes: &[
        ".controller.ts",
        ".module.ts",
        ".provider.ts",
        ".guard.ts",
        ".interceptor.ts",
    ],
    abstract_markers: &["abstract"],
    shared_prefixes: &["common/decorators/", "common/pipes/", "common/dto/"],
    silent_reasons: &[
        (".spec.ts", "test"),
        (".dto", "dto-schema"),
        ("schema", "dto-schema"),
        ("/ports/", "port"),
        (".module.ts", "di-module"),
    ],
};

const RUST: Conventions = Conventions {
    test_suffixes: &[],
    test_segments: &["/test/"],
    test_qname_prefixes: &["tests."],
    framework_suffixes: &[],
    // Rust has no `abstract` keyword; trait methods need an analyzer flag.
    abstract_markers: &[],
    shared_prefixes: &[],
    silent_reasons: &[],
};

const PYTHON: Conventions = Conventions {
    test_suffixes: &["_test.py"],
    test_segments: &["/test/"],
    test_qname_prefixes: &["tests.", "test_"],
    framework_suffixes: &[],
    abstract_markers: &[],
    shared_prefixes: &[],
    silent_reasons: &[],
};

const CPP: Conventions = Conventions {
    test_suffixes: &["_test.cpp", "_test.cc"],
    test_segments: &["/test/"],
    test_qname_prefixes: &["tests."],
    framework_suffixes: &[],
    abstract_markers: &[],
    shared_prefixes: &[],
    silent_reasons: &[],
};

const GENERIC: Conventions = Conventions {
    test_suffixes: &[],
    test_segments: &["/test/"],
    test_qname_prefixes: &["tests."],
    framework_suffixes: &[],
    abstract_markers: &[],
    shared_prefixes: &[],
    silent_reasons: &[],
};

impl Conventions {
    pub(crate) fn of(language: &str) -> &'static Self {
        match language {
            "typescript" | "javascript" => &TYPESCRIPT,
            "rust" => &RUST,
            "python" => &PYTHON,
            "cpp" => &CPP,
            _ => &GENERIC,
        }
    }

    pub(crate) fn is_test_file(&self, path: &str) -> bool {
        self.test_suffixes.iter().any(|suffix| path.ends_with(suffix))
            || self
                .test_segments
                .iter()
                .any(|segment| path.contains(segment))
    }

    /// Test functions are entry points for the harness, not code without callers.
    pub(crate) fn is_test_node(&self, node: &GraphNode) -> bool {
        self.test_qname_prefixes
            .iter()
            .any(|prefix| node.qname.starts_with(prefix))
            || self.is_test_file(&node.file)
    }

    pub(crate) fn is_framework_file(&self, path: &str) -> bool {
        self.framework_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix))
    }

    /// Abstract methods have no body in the declaration head; the marker is the
    /// language's way of saying so.
    pub(crate) fn is_abstract(&self, node: &GraphNode) -> bool {
        let head = node.declaration.trim_start();
        self.abstract_markers
            .iter()
            .any(|marker| head.starts_with(marker))
    }

    /// Shared plumbing: decorators, pipes, DTOs. Calls into it are not a
    /// dependency of the feature.
    pub(crate) fn is_shared_file(&self, path: &str) -> bool {
        self.shared_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }

    /// Why a changed file produced no symbol rows, when conventions can tell.
    pub(crate) fn silent_reason(&self, file: &str) -> &'static str {
        for (needle, label) in self.silent_reasons {
            if file.contains(needle) {
                return label;
            }
        }
        "no-callable-delta"
    }
}

/// Why a call edge is not a dependency of the code under review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Bucket {
    Override,
    Test,
    Wiring,
    Duplicate,
    Resignature,
    Unverified,
}

impl Bucket {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Bucket::Override => "override",
            Bucket::Test => "test",
            Bucket::Wiring => "wiring",
            Bucket::Duplicate => "duplicate",
            Bucket::Resignature => "resignature",
            Bucket::Unverified => "unverified",
        }
    }
}

/// Whether a call edge only reflects the language server linking overrides of a
/// base method to every sibling implementation.
pub(crate) fn is_override_link(
    src: &GraphNode,
    dst: &GraphNode,
    pairs: &HashSet<(&str, &str)>,
    conventions: &Conventions,
) -> bool {
    if src.name != dst.name {
        return false;
    }
    if conventions.is_abstract(src) || conventions.is_abstract(dst) {
        return true;
    }
    pairs.contains(&(dst.id.as_str(), src.id.as_str()))
}

/// Call edges of one snapshot as id pairs, for override detection: a pair that
/// exists in both directions is an override link, not a call.
pub(crate) fn call_pairs(edges: &[GraphEdge]) -> HashSet<(&str, &str)> {
    edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Call && !edge.external)
        .map(|edge| (edge.source.as_str(), edge.target.as_str()))
        .collect()
}

/// Whether a call edge carries a dependency: not an override link, not DI
/// wiring, not a test.
pub(crate) fn is_dependency(
    src: &GraphNode,
    dst: &GraphNode,
    pairs: &HashSet<(&str, &str)>,
    conventions: &Conventions,
) -> bool {
    wiring_exclusion(src, dst, pairs, conventions).is_none()
}

/// Why a call edge is not a real dependency, or `None` when it is one.
pub(crate) fn wiring_exclusion(
    src: &GraphNode,
    dst: &GraphNode,
    pairs: &HashSet<(&str, &str)>,
    conventions: &Conventions,
) -> Option<Bucket> {
    if is_override_link(src, dst, pairs, conventions) {
        Some(Bucket::Override)
    } else if conventions.is_test_file(&src.file) || conventions.is_test_file(&dst.file) {
        Some(Bucket::Test)
    } else if matches!(src.kind, NodeKind::Constructor) || matches!(dst.kind, NodeKind::Constructor)
    {
        Some(Bucket::Wiring)
    } else {
        None
    }
}
