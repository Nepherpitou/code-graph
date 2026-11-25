use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Review project complexity and architecture from the terminal.
    Audit(AuditArgs),
    /// Serve persisted graph snapshots over HTTP.
    Serve(ServeArgs),
}

/// Flags that control how the project is analyzed, shared by `analyze`, `up`,
/// and the audit commands that may build a snapshot.
#[derive(Args, Default)]
pub(super) struct AnalysisControlArgs {
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
    #[arg(long, default_value_t = 10_000)]
    pub(super) max_constants: usize,
    /// Extra path to ignore, relative to <root> (repeatable or comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub(super) exclude: Vec<String>,
    /// LSP protocol logging.
    #[arg(short, long)]
    pub(super) verbose: bool,
    /// Suppress analysis progress output.
    #[arg(long)]
    pub(super) quiet: bool,
}

#[derive(Args)]
pub(super) struct AnalysisArgs {
    /// Directory to analyze.
    #[arg(default_value = ".")]
    pub(super) root: PathBuf,
    #[command(flatten)]
    pub(super) control: AnalysisControlArgs,
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

#[derive(Args)]
#[command(after_help = "EXAMPLES:
  code-graph audit .
  code-graph audit top --order-by complexity
  code-graph audit top --root ../project --file src/domain
  code-graph audit focus createOrder
  code-graph audit cycles
")]
pub(super) struct AuditArgs {
    /// Project path for the overview; defaults to the current directory.
    pub(super) path: Option<PathBuf>,
    #[command(flatten)]
    pub(super) common: AuditCommon,
    #[command(subcommand)]
    pub(super) command: Option<AuditCommand>,
}

#[derive(Args)]
pub(super) struct AuditCommon {
    /// Project root to analyze or query.
    #[arg(long, default_value = ".")]
    pub(super) root: PathBuf,
    /// Snapshot database (default: <root>/.code-graph/code-graph.db).
    #[arg(long)]
    pub(super) db: Option<PathBuf>,
    /// Re-analyze the project before printing the report.
    #[arg(long, conflicts_with = "snapshot")]
    pub(super) refresh: bool,
    /// Use an existing snapshot id, or "latest".
    #[arg(long)]
    pub(super) snapshot: Option<String>,
    /// Output format: human-readable text or JSON.
    #[arg(long, value_enum, default_value_t = AuditFormat::Text)]
    pub(super) format: AuditFormat,
    /// How to analyze when the audit builds a snapshot (no snapshot or --refresh).
    #[command(flatten)]
    pub(super) analysis: AnalysisControlArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum AuditFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum AuditOrder {
    Hotspot,
    Priority,
    Complexity,
    BlastRadius,
    TransitiveFanOut,
    FanIn,
    FanOut,
    CallFanIn,
    CallFanOut,
    DataFanIn,
    DataFanOut,
    Ifc,
    Betweenness,
    Loc,
    Sloc,
    Params,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum AuditDiffOrder {
    Priority,
    Complexity,
    Sloc,
    Loc,
    BlastRadius,
    TransitiveFanOut,
    FanIn,
    FanOut,
    CallFanIn,
    CallFanOut,
    DataFanIn,
    DataFanOut,
    Ifc,
    Betweenness,
    Hotspot,
    Params,
    Name,
}

/// One question about the difference between two snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(super) enum AuditDiffView {
    /// Graph shape: symbols, edges, coupling, complexity totals.
    Summary,
    /// Metric changes per symbol (default).
    Symbols,
    /// Existing symbols the changed code starts calling.
    Uses,
    /// New call edges aggregated by module pair.
    Dependencies,
    /// Calls from unchanged files into the changed ones.
    Callers,
    /// Unchanged callables whose execution path reaches the changed symbols.
    Affected,
    /// Symbol and complexity rollout per module.
    Areas,
    /// How many added call edges were dropped and why.
    FilteredEdges,
    /// Cycles added, removed, or unchanged.
    Cycles,
    /// Dead-code candidates that appeared in the target snapshot.
    DeadCode,
    /// How far the changed entry points reach.
    Reach,
    /// Who depends on the changed symbols now.
    FanIn,
    /// Changed symbols whose outgoing calls largely overlap another symbol's.
    Similar,
    /// Changed files that carry no symbol-level rows.
    SilentFiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(super) enum AuditDiffStatus {
    Changed,
    Added,
    Removed,
    Moved,
}


#[derive(Subcommand)]
pub(super) enum AuditCommand {
    /// Rank callables by a metric.
    Top(AuditTopArgs),
    /// Show metrics and relations for one symbol.
    Focus(AuditFocusArgs),
    /// List strongly connected components with more than one callable.
    Cycles(AuditCyclesArgs),
    /// List existing dead-code candidates.
    DeadCode(AuditDeadArgs),
    /// Search symbols by name, qualified name, or file.
    Search(AuditSearchArgs),
    /// Explain the metrics used by the audit.
    Metrics(AuditMetricsArgs),
    /// Find callables whose outgoing calls largely overlap another symbol's.
    Similar(AuditSimilarArgs),
    /// Compare two persisted snapshots.
    Diff(AuditDiffArgs),
}

#[derive(Args)]
#[command(after_help = "EXAMPLES:
  code-graph audit top --order-by complexity --limit 10
  code-graph audit top --order-by blast-radius --file src/domain
  code-graph audit top --order-by fan-in --kind method
")]
pub(super) struct AuditTopArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Metric to sort by.
    #[arg(long, value_enum, default_value_t = AuditOrder::Priority)]
    pub(super) order_by: AuditOrder,
    /// Maximum number of rows.
    #[arg(long, default_value_t = 10)]
    pub(super) limit: usize,
    /// Only show values at or above this number.
    #[arg(long)]
    pub(super) min: Option<f64>,
    /// Only show nodes in a file or directory (substring match).
    #[arg(long)]
    pub(super) file: Option<String>,
    /// Only show a specific kind: function, method, constructor, constant, variable.
    #[arg(long)]
    pub(super) kind: Option<String>,
    /// Sort ascending instead of descending.
    #[arg(long)]
    pub(super) ascending: bool,
}

#[derive(Args)]
#[command(after_help = "EXAMPLES:
  code-graph audit focus createOrder
  code-graph audit focus --qname OrderService.createOrder
  code-graph audit focus --file src/domain --line 42
  code-graph audit focus --id 'src/domain/OrderService.ts#method:...'
")]
pub(super) struct AuditFocusArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Symbol name, qualified name, or stable id.
    pub(super) selector: Option<String>,
    /// Stable id (preferred for agents).
    #[arg(long)]
    pub(super) id: Option<String>,
    /// Qualified name, for example OrderService.createOrder.
    #[arg(long)]
    pub(super) qname: Option<String>,
    /// Filter by file path substring.
    #[arg(long)]
    pub(super) file: Option<String>,
    /// Filter by line within the declaration.
    #[arg(long)]
    pub(super) line: Option<u32>,
    /// Maximum callers/callees to print.
    #[arg(long, default_value_t = 8)]
    pub(super) limit: usize,
}

#[derive(Args)]
#[command(after_help = "EXAMPLES:
  code-graph audit similar
  code-graph audit similar --file src/domain --limit 10
")]
pub(super) struct AuditSimilarArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Maximum number of pairs to print.
    #[arg(long, default_value_t = 20)]
    pub(super) limit: usize,
    /// Only show pairs where either symbol is in this file or directory (substring match).
    #[arg(long)]
    pub(super) file: Option<String>,
}

#[derive(Args)]
pub(super) struct AuditCyclesArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Ignore SCCs smaller than this.
    #[arg(long, default_value_t = 2)]
    pub(super) min_size: usize,
    /// Maximum number of cycles to print.
    #[arg(long, default_value_t = 20)]
    pub(super) limit: usize,
}

#[derive(Args)]
pub(super) struct AuditDeadArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Maximum number of candidates to print.
    #[arg(long, default_value_t = 20)]
    pub(super) limit: usize,
    /// Only show candidates in a file or directory (substring match).
    #[arg(long)]
    pub(super) file: Option<String>,
}

#[derive(Args)]
pub(super) struct AuditSearchArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
    /// Text to search for.
    pub(super) query: String,
    /// Maximum number of matches.
    #[arg(long, default_value_t = 10)]
    pub(super) limit: usize,
    /// Optional kind filter.
    #[arg(long)]
    pub(super) kind: Option<String>,
}

#[derive(Args)]
pub(super) struct AuditMetricsArgs {
    #[command(flatten)]
    pub(super) common: AuditCommon,
}

#[derive(Args)]
#[command(after_help = "EXAMPLES:
  code-graph audit diff
  code-graph audit diff --from 1 --to 12
  code-graph audit diff --refresh
  code-graph audit diff --from previous --to latest --changed-files changed.txt
")]
pub(super) struct AuditDiffArgs {
    /// Project root to analyze or query.
    #[arg(long, default_value = ".")]
    pub(super) root: PathBuf,
    /// Snapshot database (default: <root>/.code-graph/code-graph.db).
    #[arg(long)]
    pub(super) db: Option<PathBuf>,
    /// Baseline snapshot id, "latest", or "previous" (default: previous).
    #[arg(long)]
    pub(super) from: Option<String>,
    /// Target snapshot id, "latest", or "previous" (default: latest).
    #[arg(long, conflicts_with = "refresh")]
    pub(super) to: Option<String>,
    /// Analyze the current root and use the new snapshot as the target.
    #[arg(long)]
    pub(super) refresh: bool,
    /// How to analyze when --refresh builds a snapshot.
    #[command(flatten)]
    pub(super) analysis: AnalysisControlArgs,
    /// Output format: human-readable text or JSON.
    #[arg(long, value_enum, default_value_t = AuditFormat::Text)]
    pub(super) format: AuditFormat,
    /// Which view of the diff to print.
    #[arg(long, value_enum, default_value_t = AuditDiffView::Symbols)]
    pub(super) view: AuditDiffView,
    /// Sort changed rows by a metric delta, or by name.
    #[arg(long, value_enum, default_value_t = AuditDiffOrder::Priority)]
    pub(super) order_by: AuditDiffOrder,
    /// Maximum number of rows to print.
    #[arg(long, default_value_t = 50)]
    pub(super) limit: usize,
    /// Suppress changed rows whose selected delta is below this absolute value.
    #[arg(long)]
    pub(super) min_delta: Option<f64>,
    /// Only show nodes in a file or directory (substring match).
    #[arg(long)]
    pub(super) file: Option<String>,
    /// Only show a specific kind: function, method, constructor, constant, variable.
    #[arg(long)]
    pub(super) kind: Option<String>,
    /// Only show changed, added, removed, or moved rows.
    #[arg(long, value_enum)]
    pub(super) status: Vec<AuditDiffStatus>,
    /// Newline-separated paths, relative to the analysis root.
    #[arg(long)]
    pub(super) changed_files: Option<PathBuf>,
    /// Include rows whose only changes are graph-wide metrics.
    #[arg(long)]
    pub(super) all_metrics: bool,
    /// Include added and removed call/data edges.
    #[arg(long)]
    pub(super) edges: bool,
    /// Skip the source-text check that drops edges resolved to a same-named symbol the line never calls.
    #[arg(long)]
    pub(super) no_verify_calls: bool,
    /// Sort ascending instead of descending.
    #[arg(long)]
    pub(super) ascending: bool,
    /// Exit with code 1 when any rows are reported.
    #[arg(long)]
    pub(super) exit_code: bool,
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
