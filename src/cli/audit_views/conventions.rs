//! Per-language conventions the audit diff views rely on.
//!
//! Test markers, framework suffixes, abstract markers, shared plumbing and the
//! reasons a changed file produces no symbol rows are all knowledge about a
//! stack. They live here and nowhere else: views resolve one `Conventions` from
//! the snapshot language and ask it questions instead of matching path strings.

use crate::graph::GraphNode;

pub(super) struct Conventions {
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
    pub(super) fn of(language: &str) -> &'static Self {
        match language {
            "typescript" | "javascript" => &TYPESCRIPT,
            "rust" => &RUST,
            "python" => &PYTHON,
            "cpp" => &CPP,
            _ => &GENERIC,
        }
    }

    pub(super) fn is_test_file(&self, path: &str) -> bool {
        self.test_suffixes.iter().any(|suffix| path.ends_with(suffix))
            || self
                .test_segments
                .iter()
                .any(|segment| path.contains(segment))
    }

    /// Test functions are entry points for the harness, not code without callers.
    pub(super) fn is_test_node(&self, node: &GraphNode) -> bool {
        self.test_qname_prefixes
            .iter()
            .any(|prefix| node.qname.starts_with(prefix))
            || self.is_test_file(&node.file)
    }

    pub(super) fn is_framework_file(&self, path: &str) -> bool {
        self.framework_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix))
    }

    /// Abstract methods have no body in the declaration head; the marker is the
    /// language's way of saying so.
    pub(super) fn is_abstract(&self, node: &GraphNode) -> bool {
        let head = node.declaration.trim_start();
        self.abstract_markers
            .iter()
            .any(|marker| head.starts_with(marker))
    }

    /// Shared plumbing: decorators, pipes, DTOs. Calls into it are not a
    /// dependency of the feature.
    pub(super) fn is_shared_file(&self, path: &str) -> bool {
        self.shared_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }

    /// Why a changed file produced no symbol rows, when conventions can tell.
    pub(super) fn silent_reason(&self, file: &str) -> &'static str {
        for (needle, label) in self.silent_reasons {
            if file.contains(needle) {
                return label;
            }
        }
        "no-callable-delta"
    }
}
