//! SQLite persistence for graph snapshots and node search.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::graph::{EdgeKind, GraphEdge, GraphNode};
use crate::metrics::{GraphStats, GraphTop};
use crate::report::{GraphMeta, GraphReport};

#[derive(Debug, Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSummary {
    pub id: i64,
    pub created_at: String,
    pub meta: GraphMeta,
    pub stats: GraphStats,
    pub top: GraphTop,
    pub warnings: Vec<String>,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .with_context(|| format!("cannot open SQLite database {}", path.display()))?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("cannot migrate SQLite database")?;
        Ok(Self { pool })
    }

    pub async fn insert_report(&self, report: &GraphReport) -> Result<i64> {
        let meta_json = serde_json::to_string(&report.meta)?;
        let stats_json = serde_json::to_string(&report.stats)?;
        let top_json = serde_json::to_string(&report.top)?;
        let warnings_json = serde_json::to_string(&report.warnings)?;

        let mut tx = self.pool.begin().await?;
        let graph_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO graphs
             (root, language, generated_at, meta_json, stats_json, top_json, warnings_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING id",
        )
        .bind(&report.meta.root)
        .bind(&report.meta.language)
        .bind(&report.meta.generated_at)
        .bind(&meta_json)
        .bind(&stats_json)
        .bind(&top_json)
        .bind(&warnings_json)
        .fetch_one(&mut *tx)
        .await?;

        for file in &report.files {
            sqlx::query("INSERT INTO files (graph_id, path, payload_json) VALUES (?, ?, ?)")
                .bind(graph_id)
                .bind(&file.file)
                .bind(serde_json::to_string(file)?)
                .execute(&mut *tx)
                .await?;
        }

        let mut node_ids: HashMap<String, i64> = HashMap::with_capacity(report.nodes.len());
        for node in &report.nodes {
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO nodes
                 (graph_id, stable_key, name, qname, kind, file, external,
                  fan_in, fan_out, call_in, call_out, page_rank, hotspot, dead_code, payload_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 RETURNING id",
            )
            .bind(graph_id)
            .bind(&node.id)
            .bind(&node.name)
            .bind(&node.qname)
            .bind(node.kind.as_str())
            .bind(&node.file)
            .bind(i64::from(node.external))
            .bind(node.metrics.fan_in as i64)
            .bind(node.metrics.fan_out as i64)
            .bind(node.metrics.call_in as i64)
            .bind(node.metrics.call_out as i64)
            .bind(node.metrics.page_rank)
            .bind(node.metrics.hotspot)
            .bind(i64::from(node.metrics.dead_code_candidate))
            .bind(serde_json::to_string(node)?)
            .fetch_one(&mut *tx)
            .await?;
            node_ids.insert(node.id.clone(), id);
        }

        for edge in &report.edges {
            let (Some(&source_id), Some(&target_id)) =
                (node_ids.get(&edge.source), node_ids.get(&edge.target))
            else {
                continue;
            };
            sqlx::query(
                "INSERT INTO edges
                 (graph_id, source_id, target_id, source_key, target_key, kind, sites,
                  external, payload_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(graph_id)
            .bind(source_id)
            .bind(target_id)
            .bind(&edge.source)
            .bind(&edge.target)
            .bind(edge_kind(edge.kind))
            .bind(edge.sites as i64)
            .bind(i64::from(edge.external))
            .bind(serde_json::to_string(edge)?)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(graph_id)
    }

    pub async fn graphs(&self) -> Result<Vec<GraphSummary>> {
        let rows = sqlx::query(
            "SELECT id, created_at, meta_json, stats_json, top_json, warnings_json
             FROM graphs ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(summary_from_row).collect()
    }

    pub async fn graph(&self, id: i64) -> Result<Option<GraphSummary>> {
        let row = sqlx::query(
            "SELECT id, created_at, meta_json, stats_json, top_json, warnings_json
             FROM graphs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(summary_from_row).transpose()
    }

    pub async fn latest_graph(&self) -> Result<Option<GraphSummary>> {
        let row = sqlx::query(
            "SELECT id, created_at, meta_json, stats_json, top_json, warnings_json
             FROM graphs ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(summary_from_row).transpose()
    }

    pub async fn previous_graph(&self) -> Result<Option<GraphSummary>> {
        let row = sqlx::query(
            "SELECT id, created_at, meta_json, stats_json, top_json, warnings_json
             FROM graphs ORDER BY id DESC LIMIT 1 OFFSET 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(summary_from_row).transpose()
    }

    pub async fn search_nodes(
        &self,
        graph_id: i64,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphNode>> {
        let query = query.trim();
        let limit = limit.clamp(1, 500) as i64;
        if query.is_empty() {
            let rows = sqlx::query(
                "SELECT payload_json FROM nodes
                 WHERE graph_id = ? AND external = 0
                 ORDER BY (fan_in + fan_out) DESC, page_rank DESC LIMIT ?",
            )
            .bind(graph_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            return decode_nodes(rows);
        }

        if query.chars().count() < 3 {
            return self.search_like(graph_id, query, kind, limit).await;
        }

        let phrase = format!("\"{}\"", query.replace('"', "\"\""));
        let rows = if let Some(kind) = kind {
            sqlx::query(
                "SELECT n.payload_json
                 FROM nodes_fts JOIN nodes n ON n.id = nodes_fts.rowid
                 WHERE nodes_fts MATCH ? AND n.graph_id = ? AND n.kind = ?
                 ORDER BY bm25(nodes_fts), n.fan_in DESC LIMIT ?",
            )
            .bind(&phrase)
            .bind(graph_id)
            .bind(kind)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT n.payload_json
                 FROM nodes_fts JOIN nodes n ON n.id = nodes_fts.rowid
                 WHERE nodes_fts MATCH ? AND n.graph_id = ?
                 ORDER BY bm25(nodes_fts), n.fan_in DESC LIMIT ?",
            )
            .bind(&phrase)
            .bind(graph_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        };

        match rows {
            Ok(rows) => decode_nodes(rows),
            Err(_) => self.search_like(graph_id, query, kind, limit).await,
        }
    }

    async fn search_like(
        &self,
        graph_id: i64,
        query: &str,
        kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<GraphNode>> {
        let pattern = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows = if let Some(kind) = kind {
            sqlx::query(
                "SELECT payload_json FROM nodes
                 WHERE graph_id = ? AND kind = ?
                   AND (name LIKE ? ESCAPE '\\' OR qname LIKE ? ESCAPE '\\' OR file LIKE ? ESCAPE '\\')
                 ORDER BY fan_in DESC LIMIT ?",
            )
            .bind(graph_id)
            .bind(kind)
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT payload_json FROM nodes
                 WHERE graph_id = ?
                   AND (name LIKE ? ESCAPE '\\' OR qname LIKE ? ESCAPE '\\' OR file LIKE ? ESCAPE '\\')
                 ORDER BY fan_in DESC LIMIT ?",
            )
            .bind(graph_id)
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        decode_nodes(rows)
    }

    pub async fn node_count(&self, graph_id: i64) -> Result<usize> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM nodes WHERE graph_id = ?")
            .bind(graph_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count as usize)
    }

    pub async fn top_nodes(&self, graph_id: i64, limit: usize) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT stable_key FROM nodes
             WHERE graph_id = ? AND external = 0
             ORDER BY (fan_in + fan_out) DESC, page_rank DESC LIMIT ?",
        )
        .bind(graph_id)
        .bind(limit.clamp(1, 5000) as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Node row id plus decoded payload, ordered so adjacency indexes and node
    /// positions line up.
    pub async fn nodes_with_ids(&self, graph_id: i64) -> Result<Vec<(i64, GraphNode)>> {
        let rows = sqlx::query("SELECT id, payload_json FROM nodes WHERE graph_id = ? ORDER BY id")
            .bind(graph_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let payload: String = row.try_get("payload_json")?;
                Ok((id, serde_json::from_str(&payload)?))
            })
            .collect()
    }

    pub async fn edge_rows(&self, graph_id: i64) -> Result<Vec<EdgeRow>> {
        let rows = sqlx::query(
            "SELECT id, source_id, target_id, kind FROM edges
             WHERE graph_id = ? ORDER BY id",
        )
        .bind(graph_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(EdgeRow {
                    id: row.try_get("id")?,
                    source_id: row.try_get("source_id")?,
                    target_id: row.try_get("target_id")?,
                    kind: row.try_get("kind")?,
                })
            })
            .collect()
    }

    pub async fn all_nodes(&self, graph_id: i64) -> Result<Vec<GraphNode>> {
        let rows = sqlx::query("SELECT payload_json FROM nodes WHERE graph_id = ? ORDER BY id")
            .bind(graph_id)
            .fetch_all(&self.pool)
            .await?;
        decode_nodes(rows)
    }

    pub async fn all_edges(&self, graph_id: i64) -> Result<Vec<GraphEdge>> {
        let rows = sqlx::query("SELECT payload_json FROM edges WHERE graph_id = ? ORDER BY id")
            .bind(graph_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                serde_json::from_str(&payload).map_err(Into::into)
            })
            .collect()
    }

    pub async fn nodes_by_keys(&self, graph_id: i64, keys: &[String]) -> Result<Vec<GraphNode>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder =
            QueryBuilder::<Sqlite>::new("SELECT payload_json FROM nodes WHERE graph_id = ");
        builder.push_bind(graph_id);
        builder.push(" AND stable_key IN (");
        let mut separated = builder.separated(", ");
        for key in keys {
            separated.push_bind(key);
        }
        separated.push_unseparated(")");
        let rows = builder.build().fetch_all(&self.pool).await?;
        decode_nodes(rows)
    }

    pub async fn edges_by_ids(&self, graph_id: i64, ids: &[i64]) -> Result<Vec<GraphEdge>> {
        self.edges_by_ids_ordered(graph_id, ids).await
    }

    /// Returns edges in the same order as `ids`, which is useful for path output.
    pub async fn edges_by_ids_ordered(&self, graph_id: i64, ids: &[i64]) -> Result<Vec<GraphEdge>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder =
            QueryBuilder::<Sqlite>::new("SELECT id, payload_json FROM edges WHERE graph_id = ");
        builder.push_bind(graph_id);
        builder.push(" AND id IN (");
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        let rows = builder.build().fetch_all(&self.pool).await?;
        let mut payloads: HashMap<i64, GraphEdge> = HashMap::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let payload: String = row.try_get("payload_json")?;
            payloads.insert(id, serde_json::from_str(&payload)?);
        }
        Ok(ids.iter().filter_map(|id| payloads.remove(id)).collect())
    }
}

fn edge_kind(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Call => "call",
        EdgeKind::Data => "data",
    }
}

fn summary_from_row(row: sqlx::sqlite::SqliteRow) -> Result<GraphSummary> {
    Ok(GraphSummary {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        meta: serde_json::from_str(&row.try_get::<String, _>("meta_json")?)?,
        stats: serde_json::from_str(&row.try_get::<String, _>("stats_json")?)?,
        top: serde_json::from_str(&row.try_get::<String, _>("top_json")?)?,
        warnings: serde_json::from_str(&row.try_get::<String, _>("warnings_json")?)?,
    })
}

fn decode_nodes(rows: Vec<sqlx::sqlite::SqliteRow>) -> Result<Vec<GraphNode>> {
    rows.into_iter()
        .map(|row| {
            let payload: String = row.try_get("payload_json")?;
            serde_json::from_str(&payload).map_err(Into::into)
        })
        .collect()
}
