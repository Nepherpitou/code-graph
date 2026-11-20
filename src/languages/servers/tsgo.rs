use std::time::Instant;

use super::{announce, probe_call_hierarchy, remaining, Readiness, ReadyCtx, Server};

/// tsgo reports no work-done progress at all: it builds what a request needs
/// and answers synchronously. Call hierarchy is therefore the only signal.
pub(super) struct Tsgo;

pub(super) static SERVER: Tsgo = Tsgo;

impl Server for Tsgo {
    fn command(&self) -> &'static str {
        "tsgo"
    }

    fn args(&self) -> &'static [&'static str] {
        &["--lsp", "--stdio"]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, self.command());
        let started = Instant::now();
        if !probe_call_hierarchy(ctx.client, ctx.samples, remaining(&ctx, started)).await {
            return Readiness::TimedOut {
                pending: vec!["call hierarchy".to_string()],
            };
        }
        Readiness::Ready
    }
}
