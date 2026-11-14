//! HTTP API over persisted snapshots. The SPA requests slices, paths, and search
//! results instead of downloading the whole graph.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::analysis::{self, analyze_with_progress, AnalyzeOptions};
use crate::progress::{ProgressCallback, ProgressEvent, ProgressSink};
use crate::query::{
    build_overview, file_slice, rank_entry_points, EdgeFilter, EntryPoint, GraphSlice, Overview,
    PathResult, QueryService,
};
use crate::store::{GraphSummary, Store};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisStatus {
    state: String,
    phase: String,
    message: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    graph_id: Option<i64>,
    error: Option<String>,
    can_analyze: bool,
}

impl Default for AnalysisStatus {
    fn default() -> Self {
        Self {
            state: "idle".to_string(),
            phase: "idle".to_string(),
            message: "no analysis running".to_string(),
            started_at: None,
            finished_at: None,
            graph_id: None,
            error: None,
            can_analyze: false,
        }
    }
}

struct JobState {
    status: RwLock<AnalysisStatus>,
}

impl JobState {
    fn new(can_analyze: bool) -> Self {
        let status = AnalysisStatus {
            can_analyze,
            ..AnalysisStatus::default()
        };
        Self {
            status: RwLock::new(status),
        }
    }

    fn snapshot(&self) -> AnalysisStatus {
        self.status.read().unwrap().clone()
    }

    fn start(&self) {
        self.try_start();
    }

    fn try_start(&self) -> bool {
        let mut status = self.status.write().unwrap();
        if status.state == "running" {
            return false;
        }
        status.state = "running".to_string();
        status.phase = "start".to_string();
        status.message = "analysis started".to_string();
        status.started_at = Some(analysis::now_iso8601());
        status.finished_at = None;
        status.error = None;
        status.graph_id = None;
        true
    }

    fn progress(&self, event: ProgressEvent) {
        let mut status = self.status.write().unwrap();
        status.state = "running".to_string();
        status.phase = event.phase;
        status.message = event.message;
        if status.started_at.is_none() {
            status.started_at = Some(analysis::now_iso8601());
        }
    }

    fn storing(&self) {
        let mut status = self.status.write().unwrap();
        status.phase = "store".to_string();
        status.message = "storing snapshot".to_string();
    }

    fn ready(&self, graph_id: i64) {
        let mut status = self.status.write().unwrap();
        status.state = "ready".to_string();
        status.phase = "done".to_string();
        status.message = format!("snapshot #{graph_id} ready");
        status.finished_at = Some(analysis::now_iso8601());
        status.graph_id = Some(graph_id);
    }

    fn failed(&self, error: String) {
        let mut status = self.status.write().unwrap();
        status.state = "failed".to_string();
        status.phase = "error".to_string();
        status.message = error.clone();
        status.finished_at = Some(analysis::now_iso8601());
        status.error = Some(error);
    }
}

impl ProgressSink for JobState {
    fn emit(&self, event: ProgressEvent) {
        self.progress(event);
    }
}

#[derive(Clone)]
struct AppState {
    store: Store,
    query: Arc<QueryService>,
    job: Arc<JobState>,
    analyze_options: Option<Arc<AnalyzeOptions>>,
}

enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            ApiError::Internal(error) => {
                tracing::error!("API error: {error:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        ApiError::Internal(error)
    }
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct BootstrapParams {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct OverviewParams {
    group: Option<String>,
}

#[derive(Deserialize)]
struct FileParams {
    path: String,
    group: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct NodesParams {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct EntryParams {
    roots: Option<usize>,
    depth: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubgraphParams {
    roots: Option<String>,
    depth: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathParams {
    from: String,
    to: String,
    edges: Option<String>,
    max_depth: Option<usize>,
}

#[derive(Deserialize)]
struct ImpactParams {
    node: String,
    depth: Option<usize>,
    direction: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SliceResponse {
    nodes: Vec<crate::graph::GraphNode>,
    edges: Vec<crate::graph::GraphEdge>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileResponse {
    nodes: Vec<crate::graph::GraphNode>,
    edges: Vec<crate::graph::GraphEdge>,
    total: usize,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryResponse {
    nodes: Vec<crate::graph::GraphNode>,
    edges: Vec<crate::graph::GraphEdge>,
    roots: Vec<EntryPoint>,
    total_roots: usize,
    depth: usize,
    truncated: bool,
}

impl From<crate::query::FileSlice> for FileResponse {
    fn from(slice: crate::query::FileSlice) -> Self {
        Self {
            nodes: slice.nodes,
            edges: slice.edges,
            total: slice.total,
            truncated: slice.truncated,
        }
    }
}

impl From<GraphSlice> for SliceResponse {
    fn from(slice: GraphSlice) -> Self {
        Self {
            nodes: slice.nodes,
            edges: slice.edges,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PathResponse {
    found: bool,
    path: Vec<String>,
    nodes: Vec<crate::graph::GraphNode>,
    edges: Vec<crate::graph::GraphEdge>,
}

impl From<PathResult> for PathResponse {
    fn from(result: PathResult) -> Self {
        Self {
            found: result.found,
            path: result.path,
            nodes: result.nodes,
            edges: result.edges,
        }
    }
}

#[derive(Serialize)]
struct GraphsResponse {
    graphs: Vec<GraphSummary>,
}

pub async fn serve(
    db: &Path,
    bind: &str,
    web_dir: &Path,
    analyze_options: Option<AnalyzeOptions>,
) -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("code_graph=info,tower_http=info")
            }),
        )
        .try_init();

    let store = Store::open(db).await?;
    let latest = store.latest_graph().await?;
    let analyze_options = analyze_options.map(Arc::new);
    let job = Arc::new(JobState::new(analyze_options.is_some()));
    let analysis_store = store.clone();
    let state = AppState {
        query: Arc::new(QueryService::new(store.clone())),
        store,
        job: job.clone(),
        analyze_options: analyze_options.clone(),
    };
    let api = Router::new()
        .route("/status", get(status))
        .route("/refresh", post(refresh))
        .route("/graphs", get(list_graphs))
        .route("/graphs/{id}", get(get_graph))
        .route("/graphs/{id}/search", get(search))
        .route("/graphs/{id}/bootstrap", get(bootstrap))
        .route("/graphs/{id}/overview", get(overview))
        .route("/graphs/{id}/file", get(file))
        .route("/graphs/{id}/nodes", get(nodes))
        .route("/graphs/{id}/entry-points", get(entry_points))
        .route("/graphs/{id}/subgraph", get(subgraph))
        .route("/graphs/{id}/path", get(path))
        .route("/graphs/{id}/impact", get(impact))
        .with_state(state);

    let mut router = Router::new()
        .nest("/api/v1", api)
        .layer(CorsLayer::permissive());

    if web_dir.exists() {
        let index = web_dir.join("index.html");
        let files = ServeDir::new(web_dir).fallback(ServeFile::new(index));
        router = router.fallback_service(files);
    }

    let addr: SocketAddr = bind
        .parse()
        .map_err(|error| anyhow!("invalid bind address {bind}: {error}"))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("serving on http://{addr}");
    if let Some(options) = analyze_options.as_ref() {
        if let Some(graph) = latest {
            job.ready(graph.id);
            tracing::info!(
                "using existing snapshot #{}; POST /api/v1/refresh to re-analyze",
                graph.id
            );
        } else {
            job.start();
            spawn_analysis(analysis_store, job.clone(), options.as_ref().clone())
                .map_err(|error| anyhow!("cannot spawn analysis thread: {error}"))?;
        }
    }
    axum::serve(listener, router).await?;
    Ok(())
}

fn spawn_analysis(
    store: Store,
    job: Arc<JobState>,
    options: AnalyzeOptions,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("code-graph-analysis".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    job.failed(format!("cannot start analysis runtime: {error}"));
                    return;
                }
            };
            runtime.block_on(run_analysis(store, job, options));
        })
        .map(|_| ())
}

async fn run_analysis(store: Store, job: Arc<JobState>, options: AnalyzeOptions) {
    let callback: ProgressCallback = job.clone();
    match analyze_with_progress(options, callback).await {
        Ok(report) => {
            job.storing();
            match store.insert_report(&report).await {
                Ok(graph_id) => job.ready(graph_id),
                Err(error) => job.failed(format!("{error:#}")),
            }
        }
        Err(error) => job.failed(format!("{error:#}")),
    }
}

async fn status(State(state): State<AppState>) -> Json<AnalysisStatus> {
    Json(state.job.snapshot())
}

async fn refresh(State(state): State<AppState>) -> Result<Json<AnalysisStatus>, ApiError> {
    let Some(options) = state.analyze_options.as_ref() else {
        return Err(ApiError::BadRequest(
            "analysis is not configured; start the server with `code-graph up`".to_string(),
        ));
    };
    if !state.job.try_start() {
        return Err(ApiError::Conflict(
            "analysis is already running".to_string(),
        ));
    }
    if let Err(error) = spawn_analysis(
        state.store.clone(),
        state.job.clone(),
        options.as_ref().clone(),
    ) {
        let message = format!("cannot spawn analysis thread: {error}");
        state.job.failed(message.clone());
        return Err(ApiError::Internal(anyhow!(message)));
    }
    Ok(Json(state.job.snapshot()))
}

async fn list_graphs(State(state): State<AppState>) -> Result<Json<GraphsResponse>, ApiError> {
    Ok(Json(GraphsResponse {
        graphs: state.store.graphs().await?,
    }))
}

async fn get_graph(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<GraphSummary>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    state
        .store
        .graph(graph_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("graph {id} not found")))
}

async fn search(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SliceResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let query = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(50);
    let nodes = state
        .store
        .search_nodes(graph_id, &query, params.kind.as_deref(), limit)
        .await?;
    Ok(Json(SliceResponse {
        nodes,
        edges: Vec::new(),
    }))
}

async fn bootstrap(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<BootstrapParams>,
) -> Result<Json<SliceResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let slice = state
        .query
        .bootstrap(graph_id, params.limit.unwrap_or(200))
        .await?;
    Ok(Json(slice.into()))
}

async fn overview(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<OverviewParams>,
) -> Result<Json<Overview>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let nodes = state.store.all_nodes(graph_id).await?;
    let edges = state.store.all_edges(graph_id).await?;
    Ok(Json(build_overview(
        &nodes,
        &edges,
        params.group.as_deref().unwrap_or("dir"),
    )))
}

async fn file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<FileParams>,
) -> Result<Json<FileResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let nodes = state.store.all_nodes(graph_id).await?;
    let edges = state.store.all_edges(graph_id).await?;
    let slice = file_slice(
        &nodes,
        &edges,
        &params.path,
        params.group.as_deref().unwrap_or("dir"),
        params.limit.unwrap_or(500),
    );
    Ok(Json(slice.into()))
}

async fn nodes(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<NodesParams>,
) -> Result<Json<Vec<crate::graph::GraphNode>>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let mut nodes = state.store.all_nodes(graph_id).await?;
    nodes.truncate(params.limit.unwrap_or(20_000).clamp(1, 20_000));
    Ok(Json(nodes))
}

async fn entry_points(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<EntryParams>,
) -> Result<Json<EntryResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let nodes = state.store.all_nodes(graph_id).await?;
    let ranking = rank_entry_points(&nodes, params.roots.unwrap_or(8));
    let root_ids: Vec<String> = ranking
        .points
        .iter()
        .map(|point| point.id.clone())
        .collect();
    let depth = params.depth.unwrap_or(2).clamp(1, 8);
    let slice = state
        .query
        .entry_slice(graph_id, &root_ids, depth, params.limit.unwrap_or(180))
        .await?;
    Ok(Json(EntryResponse {
        nodes: slice.nodes,
        edges: slice.edges,
        roots: ranking.points,
        total_roots: ranking.total,
        depth,
        truncated: slice.truncated,
    }))
}

async fn subgraph(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<SubgraphParams>,
) -> Result<Json<SliceResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let roots: Vec<String> = params
        .roots
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(str::to_string)
        .collect();
    if roots.is_empty() {
        return Err(ApiError::BadRequest("roots is required".to_string()));
    }
    let slice = state
        .query
        .subgraph(
            graph_id,
            &roots,
            params.depth.unwrap_or(1),
            params.limit.unwrap_or(500),
        )
        .await?;
    Ok(Json(slice.into()))
}

async fn path(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<PathParams>,
) -> Result<Json<PathResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let filter = EdgeFilter::parse(params.edges.as_deref());
    let result = state
        .query
        .path(
            graph_id,
            &params.from,
            &params.to,
            filter,
            params.max_depth.unwrap_or(12),
        )
        .await?;
    Ok(Json(result.into()))
}

async fn impact(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(params): Query<ImpactParams>,
) -> Result<Json<SliceResponse>, ApiError> {
    let graph_id = resolve_graph(&state.store, &id).await?;
    let reverse = params.direction.as_deref() != Some("out");
    let slice = state
        .query
        .impact(graph_id, &params.node, params.depth.unwrap_or(2), reverse)
        .await?;
    Ok(Json(slice.into()))
}

async fn resolve_graph(store: &Store, id: &str) -> Result<i64, ApiError> {
    if id == "latest" {
        return store
            .latest_graph()
            .await?
            .map(|graph| graph.id)
            .ok_or_else(|| ApiError::NotFound("no graphs in database".to_string()));
    }
    id.parse::<i64>()
        .map_err(|_| ApiError::BadRequest(format!("invalid graph id: {id}")))
}
