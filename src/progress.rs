//! Progress events shared by the CLI and the HTTP analysis job.

use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: String,
    pub message: String,
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

pub type ProgressCallback = Arc<dyn ProgressSink>;

struct SilentSink;

impl ProgressSink for SilentSink {
    fn emit(&self, _event: ProgressEvent) {}
}

pub fn silent_callback() -> ProgressCallback {
    Arc::new(SilentSink)
}

#[derive(Clone)]
pub struct Reporter {
    quiet: bool,
    callback: ProgressCallback,
}

impl Reporter {
    pub fn new(quiet: bool, callback: ProgressCallback) -> Self {
        Self { quiet, callback }
    }

    pub fn emit(&self, phase: &str, message: impl Into<String>) {
        let message = message.into();
        if !self.quiet {
            eprintln!("[code-graph] {message}");
        }
        self.callback.emit(ProgressEvent {
            phase: phase.to_string(),
            message,
        });
    }
}
