//! One module per language server, behind a common trait.
//!
//! The trait answers two questions: how to start the server, and what "ready"
//! means for it. Readiness is the server's own work-done progress first and a
//! probe second: a server that answers call hierarchy for one file is not
//! necessarily done indexing the project, and references answered mid-indexing
//! come back short without saying so.

mod clangd;
mod generic;
mod pyright;
mod rust_analyzer;
mod tsgo;
mod tsserver;

use std::path::Path;
use std::time::{Duration, Instant};

use crate::lsp::{wait_for_progress, LspClient, LspPosition, Waiting};
use crate::progress::Reporter;

pub(crate) struct ReadyCtx<'a> {
    pub(crate) client: &'a LspClient,
    /// One callable per file, for the servers that can only answer by probing.
    pub(crate) samples: &'a [(String, String, LspPosition)],
    pub(crate) reporter: &'a Reporter,
    pub(crate) budget: Duration,
    /// Whether the root carries the language's project marker (`Cargo.toml`,
    /// `tsconfig.json`, …). A detached file has no workspace to load and no
    /// index to wait for.
    pub(crate) workspace: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    Ready,
    /// The server is still working or refused to answer; listed names are what
    /// the caller may report as missing.
    TimedOut {
        pending: Vec<String>,
    },
}

pub(crate) trait Server: Sync {
    fn command(&self) -> &'static str;
    fn args(&self) -> &'static [&'static str];
    async fn wait_ready(&self, ctx: ReadyCtx<'_>) -> Readiness;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerKind {
    RustAnalyzer,
    Tsserver,
    Tsgo,
    Clangd,
    Pyright,
    Generic,
}

impl ServerKind {
    /// Picks the behaviour from the program name of `--server-cmd`, so a custom
    /// command keeps the readiness policy of the server it actually starts.
    pub(crate) fn resolve(command_line: &str) -> Self {
        // The server may be wrapped (`npx -y tsgo --lsp --stdio`, a proxy
        // script), so every word is a candidate and the known server wins.
        for word in command_line.split_whitespace() {
            let name = Path::new(word)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(word);
            match name {
                "rust-analyzer" => return Self::RustAnalyzer,
                "typescript-language-server" => return Self::Tsserver,
                "tsgo" => return Self::Tsgo,
                "clangd" => return Self::Clangd,
                "pyright-langserver" | "pyright" => return Self::Pyright,
                _ => {}
            }
        }
        Self::Generic
    }

    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::RustAnalyzer => rust_analyzer::SERVER.command(),
            Self::Tsserver => tsserver::SERVER.command(),
            Self::Tsgo => tsgo::SERVER.command(),
            Self::Clangd => clangd::SERVER.command(),
            Self::Pyright => pyright::SERVER.command(),
            Self::Generic => generic::SERVER.command(),
        }
    }

    pub(crate) fn args(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => rust_analyzer::SERVER.args(),
            Self::Tsserver => tsserver::SERVER.args(),
            Self::Tsgo => tsgo::SERVER.args(),
            Self::Clangd => clangd::SERVER.args(),
            Self::Pyright => pyright::SERVER.args(),
            Self::Generic => generic::SERVER.args(),
        }
    }

    pub(crate) async fn wait_ready(self, ctx: ReadyCtx<'_>) -> Readiness {
        match self {
            Self::RustAnalyzer => rust_analyzer::SERVER.wait_ready(ctx).await,
            Self::Tsserver => tsserver::SERVER.wait_ready(ctx).await,
            Self::Tsgo => tsgo::SERVER.wait_ready(ctx).await,
            Self::Clangd => clangd::SERVER.wait_ready(ctx).await,
            Self::Pyright => pyright::SERVER.wait_ready(ctx).await,
            Self::Generic => generic::SERVER.wait_ready(ctx).await,
        }
    }
}

pub(super) fn announce(ctx: &ReadyCtx<'_>, server: &str) {
    ctx.reporter
        .emit("ready", format!("checking {server} server readiness"));
}

/// How long a server may stay silent before it counts as one without progress.
/// The first report of a load arrives a few hundred milliseconds after the
/// trigger, so an instant answer would call a working server silent.
const ABSENCE_GRACE: Duration = Duration::from_secs(2);

/// Waits for the server's own progress titles; an empty list means the server
/// reports none, and waiting for it would only burn the budget.
pub(super) async fn wait_titles(ctx: &ReadyCtx<'_>, titles: &[&str]) -> Waiting {
    if titles.is_empty() {
        return Waiting::Absent;
    }
    let started = Instant::now();
    while !ctx.client.progress().seen(titles) && started.elapsed() < ABSENCE_GRACE {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    wait_for_progress(ctx.client.progress(), titles, remaining(ctx, started)).await
}

/// Waits for a workspace server's progress without treating a slow first
/// report as "no progress": in a real workspace the reports are certain, only
/// their timing is unknown.
pub(super) async fn wait_until_quiet(ctx: &ReadyCtx<'_>, titles: &[&str]) -> Waiting {
    let started = Instant::now();
    while !ctx.client.progress().seen(titles) {
        if started.elapsed() >= ctx.budget {
            return Waiting::TimedOut(vec![titles.join(", ")]);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    match wait_for_progress(ctx.client.progress(), titles, remaining(ctx, started)).await {
        Waiting::Absent => Waiting::Done,
        other => other,
    }
}

/// Rotates over the sample callables asking for call hierarchy until one answer
/// is non-empty. Rotating matters: the first discovered file can belong to a
/// nested crate that the root workspace never loads.
pub(super) async fn probe_call_hierarchy(
    client: &LspClient,
    samples: &[(String, String, LspPosition)],
    budget: Duration,
) -> bool {
    if samples.is_empty() {
        return true;
    }
    let deadline = Instant::now() + budget;
    let mut attempt = 0usize;
    loop {
        let (_, uri, position) = &samples[attempt % samples.len()];
        if client
            .prepare_call_hierarchy_quick(uri, *position)
            .await
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        {
            return true;
        }
        attempt += 1;
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Some servers answer an arbitrary workspace/symbol query with an empty list
/// even when the project is loaded, so a few attempts are worth making before
/// giving up.
pub(super) async fn probe_workspace_symbol(
    client: &LspClient,
    samples: &[(String, String, LspPosition)],
    attempts: usize,
) -> bool {
    let Some((name, _, _)) = samples.first() else {
        return true;
    };
    for _ in 0..attempts {
        if client
            .workspace_symbol_count(name)
            .await
            .map(|count| count > 0)
            .unwrap_or(false)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

pub(super) fn remaining(ctx: &ReadyCtx<'_>, started: Instant) -> Duration {
    ctx.budget.saturating_sub(started.elapsed())
}

/// Waits for a title the server starts only after some silent work — rust-analyzer
/// builds compile-time dependencies between the crate graph and the index without
/// reporting progress, so the tracker looks idle for a while before `Indexing`
/// begins. A quiet tracker is therefore not the same as finished work.
pub(super) async fn wait_for_late_title(
    ctx: &ReadyCtx<'_>,
    needle: &str,
    grace: Duration,
) -> Waiting {
    let started = Instant::now();
    loop {
        if !ctx.client.progress().busy(&[needle]).is_empty() {
            return wait_for_progress(ctx.client.progress(), &[needle], remaining(ctx, started))
                .await;
        }
        if ctx.client.progress().seen(&[needle]) {
            return Waiting::Done;
        }
        if started.elapsed() >= grace {
            return Waiting::Absent;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_commands_keep_the_server_behaviour() {
        assert_eq!(
            ServerKind::resolve("npx -y tsgo --lsp --stdio"),
            ServerKind::Tsgo
        );
        assert_eq!(
            ServerKind::resolve("python3 /tmp/proxy.py rust-analyzer"),
            ServerKind::RustAnalyzer
        );
        assert_eq!(
            ServerKind::resolve("/usr/local/bin/typescript-language-server --stdio"),
            ServerKind::Tsserver
        );
        assert_eq!(ServerKind::resolve("/usr/bin/clangd"), ServerKind::Clangd);
        assert_eq!(
            ServerKind::resolve("pyright-langserver --stdio"),
            ServerKind::Pyright
        );
        assert_eq!(
            ServerKind::resolve("my-server --stdio"),
            ServerKind::Generic
        );
    }

    #[test]
    fn known_servers_carry_their_command_line() {
        assert_eq!(ServerKind::RustAnalyzer.command(), "rust-analyzer");
        assert_eq!(ServerKind::RustAnalyzer.args(), &[] as &[&str]);
        assert_eq!(ServerKind::Tsgo.args(), &["--lsp", "--stdio"]);
        assert_eq!(ServerKind::Tsserver.args(), &["--stdio"] as &[&str]);
    }
}
