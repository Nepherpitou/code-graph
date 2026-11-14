//! Feature slices:
//! - `cli`: argument parsing, command dispatch, human-readable inspect output.
//! - `analysis`: LSP orchestration and report assembly.
//! - `graph`: graph schema and the builder that turns LSP facts into nodes/edges.
//! - `metrics`: graph metrics and ranked tables.
//! - `lsp`, `languages`, `source`: protocol, language capabilities, and parsing.
//! - `report`: the JSON contract consumed by the web UI.

mod analysis;
mod api;
mod cli;
mod graph;
mod languages;
mod lsp;
mod metrics;
mod progress;
mod query;
mod report;
mod source;
mod store;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    cli::run().await
}
