//! Command-line boundary: parse flags, invoke analysis, write reports, and
//! start the HTTP server.

mod args;
mod inspect;

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Parser;

use crate::analysis::{analyze, AnalyzeOptions};
use crate::api;
use crate::store::Store;

use args::{AnalysisArgs, Cli, Commands};

fn build_options(args: AnalysisArgs) -> Result<AnalyzeOptions> {
    let lang = args::parse_language(&args.lang)?;
    if !args.root.exists() {
        return Err(anyhow!("path does not exist: {}", args.root.display()));
    }
    Ok(AnalyzeOptions {
        root: args.root,
        lang,
        server_cmd: args.server_cmd,
        include_external: args.include_external,
        include_constants: !args.no_constants,
        ref_fallback: !args.no_ref_fallback,
        max_nodes: args.max_nodes,
        max_constants: args.max_constants,
        exclude: args.exclude.into_iter().map(PathBuf::from).collect(),
        verbose: args.verbose,
        quiet: args.quiet,
    })
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Inspect(args) => inspect::inspect(&args.file),
        Commands::Serve(args) => api::serve(&args.db, &args.bind, &args.web_dir, None).await,
        Commands::Up(args) => {
            let options = build_options(args.analysis)?;
            let db = args
                .db
                .unwrap_or_else(|| args::project_db_path(&options.root));
            api::serve(&db, &args.bind, &args.web_dir, Some(options)).await
        }
        Commands::Analyze(args) => {
            let out = args.out;
            let db = args.db;
            let report = analyze(build_options(args.analysis)?).await?;

            let bytes = serde_json::to_vec(&report)?;
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, bytes)?;
            if let Some(db) = &db {
                let store = Store::open(db).await?;
                let graph_id = store.insert_report(&report).await?;
                eprintln!(
                    "[code-graph] stored snapshot #{graph_id} in {}",
                    db.display()
                );
            }
            eprintln!(
                "[code-graph] {} nodes, {} call edges, {} data edges, {} cycles, {} dead-code candidates in {}ms -> {}",
                report.stats.nodes,
                report.stats.edges,
                report.stats.data_edges,
                report.stats.cycles,
                report.stats.dead_code,
                report.meta.duration_ms,
                out.display()
            );
            for warning in &report.warnings {
                eprintln!("[code-graph] warning: {warning}");
            }
            Ok(())
        }
    }
}
