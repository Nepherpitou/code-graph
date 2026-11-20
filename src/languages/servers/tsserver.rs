use crate::lsp::Waiting;

use super::{announce, probe_workspace_symbol, wait_titles, Readiness, ReadyCtx, Server};

/// typescript-language-server reports project loading under this title, with a
/// random token per session.
const TITLES: &[&str] = &["initializing js/ts language features"];

pub(super) struct Tsserver;

pub(super) static SERVER: Tsserver = Tsserver;

impl Server for Tsserver {
    fn command(&self) -> &'static str {
        "typescript-language-server"
    }

    fn args(&self) -> &'static [&'static str] {
        &["--stdio"]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, self.command());
        if let Waiting::TimedOut(pending) = wait_titles(&ctx, TITLES).await {
            return Readiness::TimedOut { pending };
        }
        if !probe_workspace_symbol(ctx.client, ctx.samples, 3).await {
            return Readiness::TimedOut {
                pending: vec!["workspace symbols".to_string()],
            };
        }
        Readiness::Ready
    }
}
