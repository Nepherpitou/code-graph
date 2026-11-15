use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};

use crate::languages::LanguageId;

#[derive(Parser)]
#[command(
    name = "code-graph",
    version,
    about = "LSP-based call graph and metrics"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Commands,
}

#[derive(Subcommand)]
pub(super) enum Commands {
    /// Build a call graph and metrics from a codebase.
    Analyze(AnalyzeArgs),
    /// Start the HTTP server; reuse the latest project snapshot, or analyze when the DB is empty.
    Up(UpArgs),
    /// Print a summary of a graph JSON file.
    Inspect(InspectArgs),
    /// Serve persisted graph snapshots over HTTP.
    Serve(ServeArgs),
}

#[derive(Args)]
pub(super) struct AnalysisArgs {
    /// Directory to analyze.
    #[arg(default_value = ".")]
    pub(super) root: PathBuf,
    /// Language: typescript, rust, cpp, python, or auto.
    #[arg(long, default_value = "auto")]
    pub(super) lang: String,
    /// Override the LSP server command (arguments are split on spaces).
    #[arg(long)]
    pub(super) server_cmd: Option<String>,
    /// Keep calls to libraries/node_modules as nodes.
    #[arg(long)]
    pub(super) include_external: bool,
    /// Skip constant/variable nodes and reference crawling.
    #[arg(long)]
    pub(super) no_constants: bool,
    /// Disable references-based rescue of calls missed by call hierarchy.
    #[arg(long)]
    pub(super) no_ref_fallback: bool,
    /// Maximum number of graph nodes.
    #[arg(long, default_value_t = 20_000)]
    pub(super) max_nodes: usize,
    /// Maximum number of constants whose references are crawled.
    #[arg(long, default_value_t = 500)]
    pub(super) max_constants: usize,
    /// Extra path to ignore, relative to <root> (repeatable or comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(super) exclude: Vec<String>,
    /// LSP protocol logging.
    #[arg(short, long)]
    pub(super) verbose: bool,
    /// Suppress progress output.
    #[arg(long)]
    pub(super) quiet: bool,
}

#[derive(Parser)]
pub(super) struct AnalyzeArgs {
    #[command(flatten)]
    pub(super) analysis: AnalysisArgs,
    /// Output JSON file.
    #[arg(short, long, default_value = "graph.json")]
    pub(super) out: PathBuf,
    /// Also persist the snapshot to this SQLite database.
    #[arg(long)]
    pub(super) db: Option<PathBuf>,
}

#[derive(Parser)]
pub(super) struct UpArgs {
    #[command(flatten)]
    pub(super) analysis: AnalysisArgs,
    /// SQLite database to write snapshots to (default: <root>/.code-graph/code-graph.db).
    #[arg(long)]
    pub(super) db: Option<PathBuf>,
    /// HTTP bind address.
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub(super) bind: String,
    /// Built SPA directory; served at `/` when present.
    #[arg(long, default_value = "web/dist")]
    pub(super) web_dir: PathBuf,
}

#[derive(Parser)]
pub(super) struct InspectArgs {
    pub(super) file: PathBuf,
}

#[derive(Parser)]
pub(super) struct ServeArgs {
    /// SQLite database produced by `up` or `analyze --db`.
    #[arg(long, default_value = ".code-graph/code-graph.db")]
    pub(super) db: PathBuf,
    /// HTTP bind address.
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub(super) bind: String,
    /// Built SPA directory; served at `/` when present.
    #[arg(long, default_value = "web/dist")]
    pub(super) web_dir: PathBuf,
}

pub(super) fn project_db_path(root: &Path) -> PathBuf {
    root.join(".code-graph").join("code-graph.db")
}

pub(super) fn parse_language(value: &str) -> Result<Option<LanguageId>> {
    match value {
        "auto" => Ok(None),
        "typescript" | "ts" => Ok(Some(LanguageId::TypeScript)),
        "rust" | "rs" => Ok(Some(LanguageId::Rust)),
        "cpp" | "c++" | "c" => Ok(Some(LanguageId::Cpp)),
        "python" | "py" => Ok(Some(LanguageId::Python)),
        other => Err(anyhow!("unknown language: {other}")),
    }
}
