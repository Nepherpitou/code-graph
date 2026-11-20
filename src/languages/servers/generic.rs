use super::{announce, probe_workspace_symbol, Readiness, ReadyCtx, Server};

/// An unknown `--server-cmd`: nothing is known about its progress, so the
/// analysis probes workspace symbols and moves on if they stay empty.
pub(super) struct Generic;

pub(super) static SERVER: Generic = Generic;

impl Server for Generic {
    fn command(&self) -> &'static str {
        ""
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness {
        announce(&ctx, "unknown");
        if !probe_workspace_symbol(&ctx, 3).await {
            return Readiness::TimedOut {
                pending: vec!["workspace symbols".to_string()],
            };
        }
        Readiness::Ready
    }
}
