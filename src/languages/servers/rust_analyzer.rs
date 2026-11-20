use std::time::Instant;

use crate::lsp::Waiting;

use super::{
    announce, probe_call_hierarchy, remaining, wait_for_late_title, wait_until_quiet, Readiness,
    ReadyCtx, Server,
};

/// Titles rust-analyzer reports while the crate graph is being built. `cargo
/// check` (`rust-analyzer/flycheck/*`) is left out on purpose: it feeds
/// diagnostics, not references, and on a cold CI machine it takes minutes.
const GRAPH_TITLES: &[&str] = &[
    "fetching",
    "building crategraph",
    "loading proc-macros",
    "roots scanned",
];

/// The reference index. It runs only in a loaded workspace, after compile-time
/// dependencies are built — a phase that reports nothing at all.
const INDEX_TITLE: &str = "indexing";

pub(super) struct RustAnalyzer;

pub(super) static SERVER: RustAnalyzer = RustAnalyzer;

impl Server for RustAnalyzer {
    fn command(&self) -> &'static str {
        "rust-analyzer"
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, self.command());
        let started = Instant::now();
        if ctx.workspace {
            // The crate graph comes first. Its reports are certain in a
            // workspace, so a slow start is waited out rather than guessed at.
            if let Waiting::TimedOut(pending) = wait_until_quiet(&ctx, GRAPH_TITLES).await {
                return Readiness::TimedOut { pending };
            }
            // Compile-time dependency builds report nothing, so the tracker
            // looks idle before the index starts. Quiet is not finished work.
            if let Waiting::TimedOut(pending) =
                wait_for_late_title(&ctx, INDEX_TITLE, remaining(&ctx, started)).await
            {
                return Readiness::TimedOut { pending };
            }
        }
        // A detached file has no workspace, no index, and nothing to wait for;
        // the probe is the only signal a server is up at all.
        if !probe_call_hierarchy(ctx.client, ctx.samples, remaining(&ctx, started)).await {
            return Readiness::TimedOut {
                pending: vec!["call hierarchy".to_string()],
            };
        }
        Readiness::Ready
    }
}
