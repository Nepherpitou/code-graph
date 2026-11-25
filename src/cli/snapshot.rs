//! Opening and building persisted snapshots for the audit commands.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

use crate::analysis::{analyze, AnalyzeOptions};
use crate::store::{GraphSummary, Store};

pub(super) type Opened = (Store, GraphSummary);

pub(super) fn db_path(root: &Path, db: Option<PathBuf>) -> PathBuf {
    db.unwrap_or_else(|| root.join(".code-graph").join("code-graph.db"))
}

pub(super) async fn open_store(db: &Path, hint: &str) -> Result<Store> {
    if !db.exists() {
        bail!("snapshot database not found: {}\n{hint}", db.display());
    }
    Store::open(db).await
}

pub(super) async fn latest(db: &Path) -> Result<Option<Opened>> {
    let store = Store::open(db).await?;
    Ok(store.latest_graph().await?.map(|summary| (store, summary)))
}

pub(super) async fn resolve(
    store: &Store,
    selector: &str,
    allow_previous: bool,
) -> Result<GraphSummary> {
    if selector.eq_ignore_ascii_case("latest") {
        return store
            .latest_graph()
            .await?
            .ok_or_else(|| anyhow!("no snapshots in database"));
    }
    if allow_previous && selector.eq_ignore_ascii_case("previous") {
        return store.previous_graph().await?.ok_or_else(|| {
            anyhow!(
                "no previous snapshot; database needs at least two snapshots.\nRun `code-graph audit <root> --refresh` to create another one."
            )
        });
    }
    let id: i64 = selector.parse().map_err(|_| {
        if allow_previous {
            anyhow!("snapshot must be a numeric id, \"latest\", or \"previous\", got {selector:?}")
        } else {
            anyhow!("snapshot must be a numeric id or \"latest\", got {selector:?}")
        }
    })?;
    if let Some(summary) = store.graph(id).await? {
        return Ok(summary);
    }
    let available = store
        .graphs()
        .await?
        .iter()
        .map(|graph| graph.id.to_string())
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    if available.is_empty() {
        bail!("snapshot #{id} not found; database has no snapshots")
    }
    bail!("snapshot #{id} not found; available: {available}")
}

pub(super) async fn build(root: &Path, db: &Path, options: AnalyzeOptions) -> Result<Opened> {
    eprintln!("[code-graph] analyzing {} ...", root.display());
    let report = analyze(options).await?;
    let store = Store::open(db).await?;
    let graph_id = store.insert_report(&report).await?;
    let summary = store
        .graph(graph_id)
        .await?
        .ok_or_else(|| anyhow!("snapshot #{graph_id} was stored but cannot be read back"))?;
    eprintln!(
        "[code-graph] stored snapshot #{graph_id} in {} ({} ms)",
        db.display(),
        report.meta.duration_ms
    );
    Ok((store, summary))
}
