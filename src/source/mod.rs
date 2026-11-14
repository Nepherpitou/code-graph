//! Source feature: file discovery + tree-sitter parsing (`ast`).

pub mod ast;
mod discovery;

pub use discovery::{discover, DiscoverOptions, SourceFile};
