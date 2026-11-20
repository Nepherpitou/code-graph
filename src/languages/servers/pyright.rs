use super::{announce, probe_workspace_symbol, Readiness, ReadyCtx, Server};

/// pyright-langserver reports no work-done progress; workspace symbols are the
/// only signal that the project behind a file has been picked up.
pub(super) struct Pyright;

pub(super) static SERVER: Pyright = Pyright;

impl Server for Pyright {
    fn command(&self) -> &'static str {
        "pyright-langserver"
    }

    fn args(&self) -> &'static [&'static str] {
        &["--stdio"]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, self.command());
        if !probe_workspace_symbol(&ctx, 3).await {
            return Readiness::TimedOut {
                pending: vec!["workspace symbols".to_string()],
            };
        }
        Readiness::Ready
    }
}
