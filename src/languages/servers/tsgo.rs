use super::{announce, Readiness, ReadyCtx, Server};

/// tsgo reports no work-done progress and has no background index: it builds
/// what a request needs and answers synchronously. Document symbols have
/// already been collected by the time readiness runs, so there is nothing to
/// wait for and nothing worth probing — a probe here only cost CI minutes.
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
        Readiness::Ready
    }
}
