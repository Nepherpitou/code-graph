use crate::lsp::Waiting;

use super::{announce, probe_workspace_symbol, wait_titles, Readiness, ReadyCtx, Server};

/// clangd's background index progress is named "backgroundIndexProgress" and
/// only appears for projects with a compilation database.
const TITLES: &[&str] = &["backgroundindex", "background index"];

pub(super) struct Clangd;

pub(super) static SERVER: Clangd = Clangd;

impl Server for Clangd {
    fn command(&self) -> &'static str {
        "clangd"
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, self.command());
        if let Waiting::TimedOut(pending) = wait_titles(&ctx, TITLES).await {
            return Readiness::TimedOut { pending };
        }
        if !probe_workspace_symbol(&ctx, 3).await {
            return Readiness::TimedOut {
                pending: vec!["workspace symbols".to_string()],
            };
        }
        Readiness::Ready
    }
}
