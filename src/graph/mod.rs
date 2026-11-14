//! Graph feature: the serialized node/edge schema and the builder that
//! resolves LSP symbols, call hierarchy items, and constant references.

mod builder;
pub mod model;

pub(crate) use builder::{parse_symbols, Builder, RawSymbol};
pub use model::*;
