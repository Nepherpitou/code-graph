//! Work-done progress that language servers report over `$/progress`.
//!
//! Only titles are tracked: rust-analyzer reuses a token for work it starts
//! again and tsserver mints a random UUID per session, so a token is not a
//! stable name for anything. A title is what the server's own UI shows.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

#[derive(Debug, Default)]
struct State {
    /// Token to the lowercased title of the progress it carries.
    active: HashMap<String, String>,
    ended: HashSet<String>,
    seen: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Progress {
    inner: Arc<Mutex<State>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Waiting {
    /// Every title the server reported has ended.
    Done,
    /// The server reported none of the tracked titles: it has no such progress.
    Absent,
    /// The budget ran out; the titles listed are still active.
    TimedOut(Vec<String>),
}

impl Progress {
    pub(crate) fn observe(&self, method: &str, params: Option<&Value>) {
        if method != "$/progress" {
            return;
        }
        let Some(params) = params else { return };
        let token = match params.get("token") {
            Some(Value::String(token)) => token.clone(),
            Some(Value::Number(token)) => token.to_string(),
            _ => return,
        };
        let Some(value) = params.get("value") else {
            return;
        };
        let mut state = self.inner.lock().unwrap();
        match value.get("kind").and_then(Value::as_str) {
            Some("begin" | "report") => {
                if let Some(title) = value.get("title").and_then(Value::as_str) {
                    let title = title.to_lowercase();
                    state.seen.insert(title.clone());
                    state.active.insert(token, title);
                }
            }
            Some("end") => {
                let title = state.active.remove(&token).or_else(|| {
                    value
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_lowercase)
                });
                if let Some(title) = title {
                    state.seen.insert(title.clone());
                    state.ended.insert(title);
                }
            }
            _ => {}
        }
    }

    fn matching<'a>(
        &self,
        needles: &[&str],
        titles: impl Iterator<Item = &'a String>,
    ) -> Vec<String> {
        let mut found: Vec<String> = titles
            .filter(|title| needles.iter().any(|needle| title.contains(needle)))
            .cloned()
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Titles that are in progress and match one of the needles.
    pub(crate) fn busy(&self, needles: &[&str]) -> Vec<String> {
        let state = self.inner.lock().unwrap();
        self.matching(needles, state.active.values())
    }

    /// Whether the server ever reported one of the needles.
    pub(crate) fn seen(&self, needles: &[&str]) -> bool {
        let state = self.inner.lock().unwrap();
        !self.matching(needles, state.seen.iter()).is_empty()
    }
}

/// Waits until the tracked progress goes quiet for a settle window.
///
/// A server that has not reported any of the titles by then is treated as one
/// without progress: waiting out the budget would only slow the analysis down.
pub(crate) async fn wait_for_progress(
    progress: &Progress,
    needles: &[&str],
    budget: Duration,
) -> Waiting {
    const SETTLE: Duration = Duration::from_millis(250);
    const POLL: Duration = Duration::from_millis(50);
    let started = Instant::now();
    let mut idle_since = Instant::now();
    loop {
        if progress.busy(needles).is_empty() {
            if idle_since.elapsed() >= SETTLE {
                return if progress.seen(needles) {
                    Waiting::Done
                } else {
                    Waiting::Absent
                };
            }
        } else {
            idle_since = Instant::now();
        }
        if started.elapsed() >= budget {
            return Waiting::TimedOut(progress.busy(needles));
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn progress_event(kind: &str, token: &str, title: &str) -> Value {
        json!({
            "token": token,
            "value": { "kind": kind, "title": title }
        })
    }

    #[test]
    fn begin_and_end_track_a_title() {
        let progress = Progress::default();
        progress.observe(
            "$/progress",
            Some(&progress_event("begin", "t1", "Indexing")),
        );
        assert_eq!(progress.busy(&["indexing"]), vec!["indexing"]);
        progress.observe("$/progress", Some(&progress_event("end", "t1", "Indexing")));
        assert!(progress.busy(&["indexing"]).is_empty());
        assert!(progress.seen(&["indexing"]));
    }

    #[test]
    fn report_without_a_title_keeps_the_begin_one() {
        let progress = Progress::default();
        progress.observe(
            "$/progress",
            Some(&progress_event("begin", "t1", "Roots Scanned")),
        );
        let report = json!({ "token": "t1", "value": { "kind": "report", "message": "3/15" } });
        progress.observe("$/progress", Some(&report));
        assert_eq!(progress.busy(&["roots scanned"]), vec!["roots scanned"]);
        progress.observe(
            "$/progress",
            Some(&json!({ "token": "t1", "value": { "kind": "end" } })),
        );
        assert!(progress.busy(&["roots scanned"]).is_empty());
    }

    #[test]
    fn numbers_are_valid_tokens() {
        let progress = Progress::default();
        progress.observe(
            "$/progress",
            Some(&progress_event("begin", "7", "Fetching")),
        );
        assert_eq!(progress.busy(&["fetching"]), vec!["fetching"]);
    }

    #[test]
    fn other_methods_are_ignored() {
        let progress = Progress::default();
        progress.observe(
            "window/logMessage",
            Some(&progress_event("begin", "t1", "Indexing")),
        );
        assert!(progress.busy(&["indexing"]).is_empty());
    }

    #[tokio::test]
    async fn waits_until_the_title_ends() {
        let progress = Progress::default();
        let writer = progress.clone();
        tokio::spawn(async move {
            writer.observe(
                "$/progress",
                Some(&progress_event("begin", "t1", "Indexing")),
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
            writer.observe("$/progress", Some(&progress_event("end", "t1", "Indexing")));
        });
        let waiting = wait_for_progress(&progress, &["indexing"], Duration::from_secs(2)).await;
        assert_eq!(waiting, Waiting::Done);
    }

    #[tokio::test]
    async fn a_silent_server_is_absent_not_pending() {
        let progress = Progress::default();
        let waiting = wait_for_progress(&progress, &["indexing"], Duration::from_secs(2)).await;
        assert_eq!(waiting, Waiting::Absent);
    }

    #[tokio::test]
    async fn a_long_progress_times_out_with_the_title() {
        let progress = Progress::default();
        progress.observe(
            "$/progress",
            Some(&progress_event("begin", "t1", "Indexing")),
        );
        let waiting = wait_for_progress(&progress, &["indexing"], Duration::from_millis(120)).await;
        assert_eq!(waiting, Waiting::TimedOut(vec!["indexing".to_string()]));
    }
}
