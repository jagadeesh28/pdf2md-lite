//! In-process Axum Web Server Daemon for pdfmd-lite (`--serve`).
//!
//! Provides REST endpoints, WebSocket event streaming, and static SPA serving
//! for browser-based document conversion, dry-runs, and vault management.

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Multipart, Query, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::{SinkExt, StreamExt};
use pdfmd_lite::{
    AppConfig, DomExtractionMode, DryRunReport, ImageFormat, ImageOptions, IndraScript,
    ProgressEmitter,
    batch::{dry_run_pdf_directory, dry_run_single_pdf},
    get_pdf_page_count,
    path_policy::PathPolicy,
    vault::{download_embedding_models, embed_vault_in_path},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tokio::sync::broadcast;
use tower_http::services::{ServeDir, ServeFile};

/// Event sent over WebSocket and Tauri IPC to frontend subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressEvent {
    #[serde(rename = "doc_start")]
    DocStart {
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        #[serde(rename = "docSlug")]
        doc_slug: String,
        #[serde(rename = "totalPages")]
        total_pages: usize,
        #[serde(rename = "etaSec", skip_serializing_if = "Option::is_none")]
        eta_sec: Option<f32>,
    },
    #[serde(rename = "page_complete")]
    PageComplete {
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        #[serde(rename = "docSlug")]
        doc_slug: String,
        page: usize,
        #[serde(rename = "totalPages")]
        total_pages: usize,
        #[serde(rename = "pagesPerSec")]
        pages_per_sec: Option<f32>,
        #[serde(rename = "etaSec")]
        eta_sec: Option<f32>,
        #[serde(rename = "batchCompletedPages")]
        batch_completed_pages: Option<usize>,
        #[serde(rename = "batchTotalPages")]
        batch_total_pages: Option<usize>,
    },
    #[serde(rename = "doc_complete")]
    DocComplete {
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        #[serde(rename = "docSlug")]
        doc_slug: String,
        status: String,
        message: Option<String>,
        #[serde(rename = "elapsedSec", skip_serializing_if = "Option::is_none")]
        elapsed_sec: Option<f32>,
        #[serde(rename = "outputPath", skip_serializing_if = "Option::is_none")]
        output_path: Option<String>,
    },
    #[serde(rename = "log")]
    Log {
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        level: String,
        message: String,
    },
    #[serde(rename = "resync_required")]
    ResyncRequired { skipped: u64 },
    #[serde(rename = "batch_summary")]
    BatchSummary {
        #[serde(rename = "runId", skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        completed: usize,
        errors: usize,
        #[serde(rename = "elapsedSec", skip_serializing_if = "Option::is_none")]
        elapsed_sec: Option<f32>,
        #[serde(rename = "totalPages", skip_serializing_if = "Option::is_none")]
        total_pages: Option<usize>,
        #[serde(rename = "outputDir", skip_serializing_if = "Option::is_none")]
        output_dir: Option<String>,
    },
    #[serde(rename = "vault_progress")]
    VaultProgress {
        task: String,
        step: String,
        completed: usize,
        total: usize,
        #[serde(rename = "itemsPerSec")]
        items_per_sec: f32,
        #[serde(rename = "etaSec")]
        eta_sec: f32,
    },
}

pub struct ServerProgressState {
    pub start_time: Instant,
    pub total_pages: usize,
    pub completed_pages: usize,
    pub rolling_history: VecDeque<(Instant, usize)>,
}

/// An LRU In-Memory Ring Buffer holding rendered source PDF page previews (ADR 0015).
#[derive(Default)]
pub struct PagePreviewCache {
    entries: std::collections::HashMap<(String, usize), Vec<u8>>,
    order: VecDeque<(String, usize)>,
    capacity: usize,
}

impl PagePreviewCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            order: VecDeque::with_capacity(capacity),
            capacity: capacity.max(5),
        }
    }

    pub fn get(&self, key: &(String, usize)) -> Option<Vec<u8>> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, key: (String, usize), data: Vec<u8>) {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.entries.entry(key.clone())
        {
            e.insert(data);
            return;
        }
        if self.entries.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, data);
    }
}

/// A `ProgressEmitter` implementation that broadcasts events over a Tokio channel.
pub struct BroadcastProgressEmitter {
    tx: broadcast::Sender<ProgressEvent>,
    state: Mutex<ServerProgressState>,
    cancel_flag: Arc<AtomicBool>,
    run_id: Option<String>,
}

impl BroadcastProgressEmitter {
    pub fn new(
        tx: broadcast::Sender<ProgressEvent>,
        total_pages: usize,
        cancel_flag: Arc<AtomicBool>,
        run_id: Option<String>,
    ) -> Self {
        Self {
            tx,
            state: Mutex::new(ServerProgressState {
                start_time: Instant::now(),
                total_pages,
                completed_pages: 0,
                rolling_history: VecDeque::with_capacity(64),
            }),
            cancel_flag,
            run_id,
        }
    }
}

impl ProgressEmitter for BroadcastProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let initial_eta = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let remaining_pages = state.total_pages.saturating_sub(state.completed_pages);
            (remaining_pages.max(total_pages).max(1) as f32) * 1.8
        };
        let _ = self.tx.send(ProgressEvent::DocStart {
            run_id: self.run_id.clone(),
            job_id: Some(doc_slug.to_string()),
            doc_slug: doc_slug.to_string(),
            total_pages,
            eta_sec: Some(initial_eta),
        });
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let (pages_per_sec, eta_sec, batch_completed, batch_total) = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.completed_pages += 1;
            let now = Instant::now();
            state.rolling_history.push_back((now, 1));
            while let Some(&(t, _)) = state.rolling_history.front() {
                if now.duration_since(t).as_secs_f32() > 30.0 && state.rolling_history.len() > 5 {
                    state.rolling_history.pop_front();
                } else {
                    break;
                }
            }

            let elapsed_total = state.start_time.elapsed().as_secs_f32();
            let throughput = if state.rolling_history.len() >= 2 {
                if let Some(&(first_t, _)) = state.rolling_history.front() {
                    let dt = now.duration_since(first_t).as_secs_f32();
                    let pages: usize = state.rolling_history.iter().map(|(_, p)| *p).sum();
                    if dt > 0.05 && pages > 1 {
                        (pages - 1) as f32 / dt
                    } else if elapsed_total > 0.05 {
                        state.completed_pages as f32 / elapsed_total
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else if elapsed_total > 0.05 {
                state.completed_pages as f32 / elapsed_total
            } else {
                0.0
            };

            let remaining_pages = state.total_pages.saturating_sub(state.completed_pages);
            let eta = if remaining_pages == 0 {
                Some(0.0)
            } else if throughput > 0.001 {
                Some(remaining_pages as f32 / throughput)
            } else {
                Some((remaining_pages as f32) * 1.8)
            };

            (
                if throughput > 0.0 {
                    Some(throughput)
                } else {
                    None
                },
                eta,
                state.completed_pages,
                state.total_pages,
            )
        };

        let _ = self.tx.send(ProgressEvent::PageComplete {
            run_id: self.run_id.clone(),
            job_id: Some(doc_slug.to_string()),
            doc_slug: doc_slug.to_string(),
            page: page_num,
            total_pages,
            pages_per_sec,
            eta_sec,
            batch_completed_pages: Some(batch_completed),
            batch_total_pages: Some(batch_total),
        });
    }

    fn on_document_complete(&self, doc_slug: &str, status: &str) {
        let elapsed = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.start_time.elapsed().as_secs_f32()
        };
        let _ = self.tx.send(ProgressEvent::DocComplete {
            run_id: self.run_id.clone(),
            job_id: Some(doc_slug.to_string()),
            doc_slug: doc_slug.to_string(),
            status: status.to_string(),
            message: None,
            elapsed_sec: Some(elapsed),
            output_path: None,
        });
    }

    fn on_message(&self, level: &str, message: &str) {
        let _ = self.tx.send(ProgressEvent::Log {
            run_id: self.run_id.clone(),
            job_id: None,
            level: level.to_string(),
            message: message.to_string(),
        });
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }
}

/// Shared application state for Axum web handlers.
#[derive(Clone)]
pub struct AppState {
    pub event_tx: broadcast::Sender<ProgressEvent>,
    pub controller: pdfmd_lite::RunController,
    pub preview_cache: Arc<Mutex<PagePreviewCache>>,
    pub auth_token: String,
    pub allow_remote: bool,
    pub path_policy: Arc<PathPolicy>,
    pub upload_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct RunStateResponse {
    pub state: &'static str,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
    #[serde(rename = "isRunning")]
    pub is_running: bool,
    #[serde(rename = "cancelRequested")]
    pub cancel_requested: bool,
}

#[derive(Debug, Deserialize)]
pub struct DryRunRequest {
    #[serde(rename = "inputPath")]
    pub input_path: String,
    #[serde(rename = "outputDir")]
    pub output_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    pub files: Vec<String>,
    #[serde(rename = "outputDir")]
    pub output_dir: String,
    pub config: Option<AppConfig>,
    pub resume: Option<bool>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FormatNotesRequest {
    #[serde(rename = "vaultPath")]
    pub vault_path: String,
    #[serde(rename = "fixLatex")]
    pub fix_latex: bool,
}

#[derive(Debug, Deserialize)]
pub struct ConvertPngRequest {
    #[serde(rename = "vaultPath")]
    pub vault_path: String,
    #[serde(default)]
    pub quality: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct EmbedVaultRequest {
    #[serde(rename = "vaultPath")]
    pub vault_path: String,
    pub force: bool,
}

/// Helper to convert AppConfig into engine ImageOptions.
fn resolve_image_options(config: &AppConfig) -> ImageOptions {
    let dom_mode = match config.dom_extraction.as_deref() {
        Some("always") => DomExtractionMode::Always,
        Some("never") => DomExtractionMode::Never,
        _ => DomExtractionMode::Auto,
    };
    ImageOptions {
        format: match config.image_format.as_deref() {
            Some("png") => ImageFormat::Png,
            Some("webp") => ImageFormat::Webp,
            _ => ImageFormat::Jpeg,
        },
        quality: config.image_quality.unwrap_or(85),
        save_table_images: config.save_table_images.unwrap_or(true),
        embed_base64: config.embed_images_base64.unwrap_or(false),
        dom_mode,
        debug_layout: None,
    }
}

/// Resolves the filesystem path to `ui/dist` for static SPA asset serving.
fn resolve_ui_dist_path() -> PathBuf {
    let candidates = [
        PathBuf::from("ui/dist"),
        PathBuf::from("../../ui/dist"),
        PathBuf::from("../ui/dist"),
    ];
    for c in &candidates {
        if c.exists() && c.join("index.html").exists() {
            return c.clone();
        }
    }
    PathBuf::from("ui/dist")
}

/// Generates a cryptographically random 32-hex-character bearer token.
pub fn generate_bearer_token() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    let mut token = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut token, "{:02x}", b);
    }
    token
}

/// Axum middleware validating bearer authentication and origin/host headers (ADR 0017).
pub async fn validate_auth_and_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = req.headers();

    // 1. Host header validation: reject unexpected hosts to prevent DNS rebinding
    if let Some(host_val) = headers.get(header::HOST)
        && let Ok(host_str) = host_val.to_str()
    {
        let host_name = host_str.split(':').next().unwrap_or(host_str);
        let is_allowed_host = host_name == "localhost"
            || host_name == "127.0.0.1"
            || host_name == "::1"
            || host_name == "[::1]";
        if !is_allowed_host && !state.allow_remote {
            return Err(StatusCode::MISDIRECTED_REQUEST);
        }
    }

    // 2. Origin header validation: reject unauthorized cross-origin state-modifying requests
    let method = req.method();
    if (method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
        || method == Method::PATCH)
        && let Some(origin_val) = headers.get(header::ORIGIN)
        && let Ok(origin_str) = origin_val.to_str()
    {
        let is_allowed_origin = origin_str.starts_with("http://localhost")
            || origin_str.starts_with("http://127.0.0.1")
            || origin_str.starts_with("http://[::1]")
            || origin_str.starts_with("tauri://")
            || origin_str.starts_with("http://ipc.localhost");
        if !is_allowed_origin && !state.allow_remote {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // 3. Bearer Token Authentication
    let mut authenticated = false;
    if let Some(auth_val) = headers.get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_val.to_str()
        && let Some(token) = auth_str.strip_prefix("Bearer ")
        && token == state.auth_token
    {
        authenticated = true;
    }

    // Fallback to ?token= query parameter (for WebSocket and page-preview)
    if !authenticated && let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=")
                && token == state.auth_token
            {
                authenticated = true;
                break;
            }
        }
    }

    if !authenticated {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

/// Builds the Axum application router.
pub fn create_router(state: AppState) -> Router {
    let dist_dir = resolve_ui_dist_path();
    let index_file = dist_dir.join("index.html");

    let api_routes = Router::new()
        .route("/config", get(get_config).put(update_config))
        .route("/models", get(get_models_handler))
        .route("/dry-run", post(dry_run_handler))
        .route("/convert", post(convert_handler))
        .route("/format-notes", post(format_notes_handler))
        .route("/convert-png-to-jpeg", post(convert_png_handler))
        .route("/embed-vault", post(embed_vault_handler))
        .route("/download-models", post(download_models_handler))
        .route(
            "/upload",
            post(upload_handler).layer(DefaultBodyLimit::max(256 * 1024 * 1024)),
        )
        .route("/page-preview", get(page_preview_handler))
        .route("/note", get(get_note_handler))
        .route("/asset", get(get_asset_handler))
        .route("/capabilities", get(get_capabilities_handler))
        .route("/cancel", post(cancel_handler))
        .route("/run-state", get(run_state_handler))
        .route("/events", get(ws_handler))
        .layer(from_fn_with_state(state.clone(), validate_auth_and_origin))
        .with_state(state);

    let mut router = Router::new().nest("/api", api_routes);

    if dist_dir.exists() && index_file.exists() {
        let serve_dir = ServeDir::new(&dist_dir).fallback(ServeFile::new(&index_file));
        router = router.fallback_service(serve_dir);
    }

    router
}

/// Starts the local web server daemon on the given bind address (e.g. `127.0.0.1:3000`).
pub async fn start_server(bind_addr: &str, allow_remote: bool) -> Result<()> {
    let addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("Invalid socket bind address: '{bind_addr}'"))?;

    // Fail early if non-loopback bind address is requested without --allow-remote
    if !allow_remote && !addr.ip().is_loopback() {
        anyhow::bail!(
            "Non-loopback bind address '{bind_addr}' rejected without --allow-remote. Use 127.0.0.1 or pass --allow-remote."
        );
    }

    let auth_token = generate_bearer_token();
    let data_dir = pdfmd_lite::config::default_data_dir();
    let upload_dir = data_dir.join("uploads");
    if let Err(e) = std::fs::create_dir_all(&upload_dir) {
        tracing::warn!(
            "Failed to create uploads directory at {}: {e}",
            upload_dir.display()
        );
    }

    let output_dir = pdfmd_lite::config::default_output_dir();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path_policy = Arc::new(PathPolicy::new(vec![upload_dir.clone(), output_dir, cwd]));

    let (event_tx, _) = broadcast::channel(1024);
    let state = AppState {
        event_tx,
        controller: pdfmd_lite::RunController::new(),
        preview_cache: Arc::new(Mutex::new(PagePreviewCache::new(30))),
        auth_token: auth_token.clone(),
        allow_remote,
        path_policy,
        upload_dir,
    };

    let router = create_router(state);

    tracing::info!("pdfmd-lite web daemon listening on http://{addr}");
    println!(">> pdfmd-lite web application running at: http://{addr}/?token={auth_token}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route Handlers
// ---------------------------------------------------------------------------

async fn get_config() -> Result<Json<AppConfig>, StatusCode> {
    let config = AppConfig::load(None).unwrap_or_default();
    Ok(Json(config))
}

async fn update_config(Json(incoming): Json<AppConfig>) -> Result<StatusCode, StatusCode> {
    let default_path = pdfmd_lite::config::default_config_path();
    let mut current = AppConfig::load(None).unwrap_or_default();
    current.merge_patch(incoming);
    current
        .save(&default_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct NoteQuery {
    pub path: String,
}

async fn get_note_handler(
    State(state): State<AppState>,
    Query(query): Query<NoteQuery>,
) -> Result<(HeaderMap, String), (StatusCode, String)> {
    let safe_path = state
        .path_policy
        .check_read(Path::new(&query.path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Note path rejected: {e}")))?;

    if !safe_path.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Note not found: {}", safe_path.display()),
        ));
    }

    let content = std::fs::read_to_string(&safe_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read note: {e}"),
        )
    })?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    Ok((headers, content))
}

#[derive(Debug, Deserialize)]
pub struct AssetQuery {
    pub path: String,
}

async fn get_asset_handler(
    State(state): State<AppState>,
    Query(query): Query<AssetQuery>,
) -> Result<(HeaderMap, Vec<u8>), (StatusCode, String)> {
    let safe_path = state
        .path_policy
        .check_read(Path::new(&query.path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Asset path rejected: {e}")))?;

    if !safe_path.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Asset not found: {}", safe_path.display()),
        ));
    }

    let bytes = std::fs::read(&safe_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read asset: {e}"),
        )
    })?;

    let mime = match safe_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    };

    let mut headers = HeaderMap::new();
    if let Ok(val) = HeaderValue::from_str(mime) {
        headers.insert(header::CONTENT_TYPE, val);
    }
    Ok((headers, bytes))
}

async fn get_capabilities_handler() -> Json<pdfmd_lite::CapabilitiesResponse> {
    Json(pdfmd_lite::get_capabilities(false))
}

async fn dry_run_handler(
    State(state): State<AppState>,
    Json(req): Json<DryRunRequest>,
) -> Result<Json<DryRunReport>, (StatusCode, String)> {
    let input = state
        .path_policy
        .check_read(Path::new(&req.input_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Input path rejected: {e}")))?;
    let output = state
        .path_policy
        .check_write(Path::new(&req.output_dir))
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Output directory rejected: {e}"),
            )
        })?;

    let config = AppConfig::load(None).unwrap_or_default();
    let image_opts = resolve_image_options(&config);
    let embed = config.embed.unwrap_or(false);
    let script = config
        .language
        .as_deref()
        .and_then(|l| l.parse().ok())
        .unwrap_or(IndraScript::English);
    let max_depth = config.max_depth.unwrap_or(3);
    let concurrency = config.concurrency;

    tokio::task::spawn_blocking({
        let input = input.clone();
        let output = output.clone();
        move || {
            if input.is_file() {
                let total_pages = get_pdf_page_count(&input).unwrap_or(1);
                dry_run_single_pdf(
                    &input,
                    &output,
                    1,
                    total_pages,
                    concurrency,
                    image_opts,
                    embed,
                )
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
            } else if input.is_dir() {
                dry_run_pdf_directory(
                    &input,
                    &output,
                    None,
                    max_depth,
                    concurrency,
                    script,
                    false,
                    image_opts,
                    embed,
                )
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    format!("Path does not exist: {}", input.display()),
                ))
            }
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
}

async fn run_state_handler(State(state): State<AppState>) -> Json<RunStateResponse> {
    let (run_state, active_id) = state.controller.current_state();
    Json(RunStateResponse {
        state: match run_state {
            pdfmd_lite::RunState::Idle => "idle",
            pdfmd_lite::RunState::Running => "running",
            pdfmd_lite::RunState::Cancelling => "cancelling",
        },
        run_id: active_id,
        is_running: run_state != pdfmd_lite::RunState::Idle,
        cancel_requested: state.controller.is_cancelled(),
    })
}

async fn convert_handler(
    State(state): State<AppState>,
    Json(req): Json<ConvertRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let guard = state
        .controller
        .start_run(req.run_id.clone())
        .map_err(|e| {
            (
                StatusCode::CONFLICT,
                format!("A conversion task is already in progress: {e}"),
            )
        })?;

    let out_dir = match state.path_policy.check_write(Path::new(&req.output_dir)) {
        Ok(p) => p,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Output directory rejected: {e}"),
            ));
        }
    };

    let mut validated_files = Vec::new();
    for file in &req.files {
        match state.path_policy.check_read(Path::new(file)) {
            Ok(p) => validated_files.push(p),
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("File '{file}' rejected: {e}"),
                ));
            }
        }
    }

    let cancel_token = guard.controller().cancel_token();
    let run_id = guard.run_id().to_string();
    let event_tx = state.event_tx.clone();

    tokio::task::spawn_blocking(move || {
        let _guard = guard; // RAII guard resets RunController to Idle on task completion/panic
        let resume = req.resume.unwrap_or(true);
        let config = req.config.unwrap_or_default();
        let image_opts = resolve_image_options(&config);
        let embed = config.embed.unwrap_or(false);
        let script = config
            .language
            .as_deref()
            .and_then(|l| l.parse().ok())
            .unwrap_or(IndraScript::English);

        let mut total_batch_pages = 0;
        for path in &validated_files {
            if path.is_file() {
                total_batch_pages += get_pdf_page_count(path).unwrap_or(1);
            } else if path.is_dir()
                && let Ok(pdfs) = pdfmd_lite::batch::collect_pdf_files_with_depth(
                    path,
                    config.max_depth.unwrap_or(3),
                )
            {
                total_batch_pages += pdfs
                    .iter()
                    .map(|p| get_pdf_page_count(p).unwrap_or(1))
                    .sum::<usize>();
            }
        }
        if total_batch_pages == 0 {
            total_batch_pages = validated_files.len().max(1);
        }

        let emitter = std::sync::Arc::new(BroadcastProgressEmitter::new(
            event_tx.clone(),
            total_batch_pages,
            cancel_token.clone(),
            Some(run_id.clone()),
        ));

        let _ = event_tx.send(ProgressEvent::Log {
            run_id: Some(run_id.clone()),
            job_id: None,
            level: "info".to_string(),
            message: format!(
                "Processing {} document(s) (total ~{} pages)...",
                validated_files.len(),
                total_batch_pages
            ),
        });

        let mut completed = 0;
        let mut errors = 0;
        let batch_start = Instant::now();

        for path in &validated_files {
            if cancel_token.load(Ordering::SeqCst) {
                let _ = event_tx.send(ProgressEvent::Log {
                    run_id: Some(run_id.clone()),
                    job_id: None,
                    level: "warn".to_string(),
                    message: "Batch conversion halted due to cancellation request.".to_string(),
                });
                break;
            }
            let raw_slug = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "document".to_string());
            let slug = pdfmd_lite::batch::sanitize_short_slug(&raw_slug);

            if path.is_file() {
                let total_pages = get_pdf_page_count(path).unwrap_or(1);
                emitter.on_document_start(&slug, total_pages);
                let target_md = if out_dir.is_dir() || out_dir.extension().is_none() {
                    out_dir.join(format!("{slug}.md"))
                } else {
                    out_dir.clone()
                };
                let doc_start = Instant::now();
                let doc_res = pdfmd_lite::execute_isolated_document(|| {
                    pdfmd_lite::process_pdf_full_with_emitter(
                        path,
                        &target_md,
                        1,
                        total_pages,
                        config.concurrency,
                        None,
                        script,
                        image_opts.clone(),
                        embed,
                        Some(emitter.as_ref()),
                    )
                });
                match doc_res {
                    Ok(_) => {
                        completed += 1;
                        let doc_elapsed = doc_start.elapsed().as_secs_f32();
                        let _ = event_tx.send(ProgressEvent::DocComplete {
                            run_id: Some(run_id.clone()),
                            job_id: Some(slug.clone()),
                            doc_slug: slug.clone(),
                            status: "completed".to_string(),
                            message: None,
                            elapsed_sec: Some(doc_elapsed),
                            output_path: Some(target_md.to_string_lossy().to_string()),
                        });
                    }
                    Err(e) => {
                        errors += 1;
                        let _ = event_tx.send(ProgressEvent::DocComplete {
                            run_id: Some(run_id.clone()),
                            job_id: Some(slug.clone()),
                            doc_slug: slug,
                            status: "error".to_string(),
                            message: Some(e.to_string()),
                            elapsed_sec: Some(doc_start.elapsed().as_secs_f32()),
                            output_path: None,
                        });
                    }
                }
            } else if path.is_dir() {
                let doc_res = pdfmd_lite::execute_isolated_document(|| {
                    pdfmd_lite::process_pdf_directory_all_with_emitter(
                        path,
                        &out_dir,
                        None,
                        config.max_depth.unwrap_or(3),
                        config.concurrency,
                        script,
                        true,
                        resume,
                        image_opts.clone(),
                        embed,
                        Some(emitter.clone()),
                    )
                });
                match doc_res {
                    Ok(res) => {
                        completed += res.succeeded;
                        errors += res.failed.len();
                    }
                    Err(e) => {
                        errors += 1;
                        emitter.on_message("error", &format!("Directory batch error: {e}"));
                    }
                }
            }
        }

        let batch_elapsed = batch_start.elapsed().as_secs_f32();
        let _ = event_tx.send(ProgressEvent::Log {
            run_id: Some(run_id.clone()),
            job_id: None,
            level: if errors == 0 {
                "info".to_string()
            } else {
                "warn".to_string()
            },
            message: format!(
                "Conversion finished in {:.2}s: {} succeeded, {} failed (Output: {}).",
                batch_elapsed,
                completed,
                errors,
                out_dir.display()
            ),
        });

        let _ = event_tx.send(ProgressEvent::BatchSummary {
            run_id: Some(run_id),
            completed,
            errors,
            elapsed_sec: Some(batch_elapsed),
            total_pages: Some(total_batch_pages),
            output_dir: Some(out_dir.to_string_lossy().to_string()),
        });
    });

    Ok(StatusCode::ACCEPTED)
}

async fn format_notes_handler(
    State(state): State<AppState>,
    Json(req): Json<FormatNotesRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vault_path = state
        .path_policy
        .check_write(Path::new(&req.vault_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Vault path rejected: {e}")))?;

    let tx = state.event_tx.clone();
    tokio::task::spawn_blocking(move || {
        let summary = pdfmd_lite::format_notes_in_path_with_progress(
            &vault_path,
            None,
            false,
            move |p| {
                let _ = tx.send(ProgressEvent::VaultProgress {
                    task: p.task.clone(),
                    step: p.step.clone(),
                    completed: p.completed,
                    total: p.total,
                    items_per_sec: p.items_per_sec,
                    eta_sec: p.eta_sec,
                });
                if p.completed == 1 || p.completed % 25 == 0 || p.completed == p.total {
                    let pct = if p.total > 0 {
                        (p.completed as f32 / p.total as f32) * 100.0
                    } else {
                        100.0
                    };
                    let _ = tx.send(ProgressEvent::Log {
                        run_id: None,
                        job_id: None,
                        level: "info".to_string(),
                        message: format!(
                            "Note formatting: {}/{} notes ({:.1}%) - {:.1} notes/s, ETA: ~{:.0}s",
                            p.completed, p.total, pct, p.items_per_sec, p.eta_sec
                        ),
                    });
                }
            },
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "filesProcessed": summary.files_scanned,
            "replacementsMade": summary.latex_repaired + summary.codeblocks_unwrapped + summary.banners_purged,
            "frontmatterAdded": summary.frontmatter_added,
            "fieldsBackfilled": summary.frontmatter_fields_backfilled,
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn convert_png_handler(
    State(state): State<AppState>,
    Json(req): Json<ConvertPngRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vault_path = state
        .path_policy
        .check_write(Path::new(&req.vault_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Vault path rejected: {e}")))?;

    let tx = state.event_tx.clone();
    tokio::task::spawn_blocking(move || {
        let q = req.quality.unwrap_or(85);
        let summary =
            pdfmd_lite::convert_vault_png_to_jpeg_with_progress(&vault_path, q, false, move |p| {
                let _ = tx.send(ProgressEvent::VaultProgress {
                    task: p.task.clone(),
                    step: p.step.clone(),
                    completed: p.completed,
                    total: p.total,
                    items_per_sec: p.items_per_sec,
                    eta_sec: p.eta_sec,
                });
                if p.completed == 1 || p.completed % 25 == 0 || p.completed == p.total {
                    let pct = if p.total > 0 {
                        (p.completed as f32 / p.total as f32) * 100.0
                    } else {
                        100.0
                    };
                    let _ = tx.send(ProgressEvent::Log {
                        run_id: None,
                        job_id: None,
                        level: "info".to_string(),
                        message: format!(
                            "PNG migration: {}/{} images ({:.1}%) - {:.1} img/s, ETA: ~{:.0}s",
                            p.completed, p.total, pct, p.items_per_sec, p.eta_sec
                        ),
                    });
                }
            })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "imagesConverted": summary.images_converted,
            "bytesSaved": summary.orig_bytes.saturating_sub(summary.new_bytes),
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn embed_vault_handler(
    State(state): State<AppState>,
    Json(req): Json<EmbedVaultRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vault_path = state
        .path_policy
        .check_write(Path::new(&req.vault_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Vault path rejected: {e}")))?;

    tokio::task::spawn_blocking(move || {
        let summary = embed_vault_in_path(&vault_path, req.force, false)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "notesProcessed": summary.notes_scanned,
            "chunksEmbedded": summary.chunks_embedded,
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn get_models_handler() -> Json<Vec<pdfmd_lite::ModelStatus>> {
    Json(pdfmd_lite::get_model_status())
}

async fn download_models_handler() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        download_embedding_models()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "success": true,
            "message": "All required offline models and tokenizers downloaded and verified successfully.",
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let upload_dir = &state.upload_dir;
    tokio::fs::create_dir_all(upload_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut saved_paths = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let raw_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "upload.pdf".to_string());

        // Sandboxed Upload Guard: Sanitize filename to eliminate path-traversal risk
        let file_stem = Path::new(&raw_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document.pdf");
        let sanitized_name: String = file_stem
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
            .collect();
        let sanitized_name = if sanitized_name.is_empty() {
            "document.pdf".to_string()
        } else {
            sanitized_name
        };

        let data = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Sandboxed Upload Guard: Validate %PDF magic bytes
        if !data.starts_with(b"%PDF-") {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("File '{sanitized_name}' rejected: missing %PDF- magic bytes header"),
            ));
        }

        let dest_path = upload_dir.join(&sanitized_name);
        tokio::fs::write(&dest_path, data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        saved_paths.push(dest_path.to_string_lossy().to_string());
    }

    Ok(Json(serde_json::json!({
        "files": saved_paths
    })))
}

#[derive(Debug, Deserialize)]
pub struct PagePreviewQuery {
    pub slug: Option<String>,
    pub file: Option<String>,
    pub page: usize,
}

async fn page_preview_handler(
    State(state): State<AppState>,
    Query(query): Query<PagePreviewQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let doc_identifier = query.slug.or(query.file).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Missing 'slug' or 'file' query parameter".to_string(),
        )
    })?;
    let page_num = query.page.max(1);

    // 1. Check in-memory LRU ring buffer
    let cache_key = (doc_identifier.clone(), page_num);
    {
        let cache = state
            .preview_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(bytes) = cache.get(&cache_key) {
            return Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes));
        }
    }

    // 2. Validate document filesystem path against PathPolicy
    let doc_path = state
        .path_policy
        .check_read(Path::new(&doc_identifier))
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Document path rejected: {e}"),
            )
        })?;

    // 3. Render page in memory via hayro (scale 1.5 for crisp preview without excessive RAM)
    let dynamic_img = tokio::task::spawn_blocking(move || {
        pdfmd_lite::pdf::render_single_page_image(&doc_path, page_num, 1.5)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to render page preview: {e:#}"),
        )
    })?;

    // 4. Encode to JPEG in memory
    let mut jpeg_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
    dynamic_img
        .to_rgb8()
        .write_to(&mut cursor, image::ImageFormat::Jpeg)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to encode JPEG preview: {e:#}"),
            )
        })?;

    // 5. Store in LRU cache
    {
        let mut cache = state
            .preview_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cache.insert(cache_key, jpeg_bytes.clone());
    }

    Ok(([(header::CONTENT_TYPE, "image/jpeg")], jpeg_bytes))
}

async fn cancel_handler(State(state): State<AppState>) -> StatusCode {
    if state.controller.request_cancel() {
        let _ = state.event_tx.send(ProgressEvent::Log {
            run_id: None,
            job_id: None,
            level: "warn".to_string(),
            message: "Cancellation signal received. Halting batch execution...".to_string(),
        });
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event)
                        && sender.send(Message::Text(json)).await.is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let resync = ProgressEvent::ResyncRequired { skipped };
                    if let Ok(json) = serde_json::to_string(&resync)
                        && sender.send(Message::Text(json)).await.is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn create_test_state() -> (AppState, String) {
        let (event_tx, _) = broadcast::channel(16);
        let token = generate_bearer_token();
        let temp_dir = std::env::temp_dir().join("pdfmd_test_server");
        let _ = std::fs::create_dir_all(&temp_dir);
        let path_policy = Arc::new(PathPolicy::new(vec![temp_dir.clone()]));
        let state = AppState {
            event_tx,
            controller: pdfmd_lite::RunController::new(),
            preview_cache: Arc::new(Mutex::new(PagePreviewCache::new(5))),
            auth_token: token.clone(),
            allow_remote: false,
            path_policy,
            upload_dir: temp_dir,
        };
        (state, token)
    }

    #[test]
    fn test_generate_bearer_token() {
        let token = generate_bearer_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_auth_valid_bearer_token() {
        let (state, token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/config")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_valid_query_token() {
        let (state, token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .uri(format!("/api/config?token={token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_missing_token_rejected() {
        let (state, _token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/config")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_evil_origin_rejected_on_post() {
        let (state, token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/cancel")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .header(header::ORIGIN, "http://evil.com")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_auth_attacker_host_rejected() {
        let (state, token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/config")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "attacker.com")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::MISDIRECTED_REQUEST);
    }

    #[tokio::test]
    async fn test_get_note_allowed_and_forbidden() {
        let (state, token) = create_test_state();
        let test_file = state.upload_dir.join("sample_note.md");
        std::fs::write(&test_file, "# Sample Note\nTest content for note.").unwrap();

        let app = create_router(state);

        // Allowed path
        let path_str = test_file.to_string_lossy().to_string();
        let req = Request::builder()
            .uri(format!("/api/note?path={}", path_str.replace('\\', "/")))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/markdown; charset=utf-8"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&bytes);
        assert!(body_str.contains("# Sample Note"));

        // Disallowed / outside allowed paths
        let outside_path = std::env::temp_dir().join("disallowed_secret.md");
        let outside_str = outside_path.to_string_lossy().to_string();
        let req_outside = Request::builder()
            .uri(format!("/api/note?path={}", outside_str.replace('\\', "/")))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp_outside = app.oneshot(req_outside).await.unwrap();
        assert_eq!(resp_outside.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_asset_mime_and_forbidden() {
        let (state, token) = create_test_state();
        let test_img = state.upload_dir.join("figure1.png");
        std::fs::write(&test_img, b"\x89PNG\r\n\x1a\nfake_bytes").unwrap();

        let app = create_router(state);

        let path_str = test_img.to_string_lossy().to_string();
        let req = Request::builder()
            .uri(format!("/api/asset?path={}", path_str.replace('\\', "/")))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");

        // Disallowed asset
        let outside_img = std::env::temp_dir().join("disallowed_image.png");
        let outside_str = outside_img.to_string_lossy().to_string();
        let req_outside = Request::builder()
            .uri(format!(
                "/api/asset?path={}",
                outside_str.replace('\\', "/")
            ))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp_outside = app.oneshot(req_outside).await.unwrap();
        assert_eq!(resp_outside.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_capabilities() {
        let (state, token) = create_test_state();
        let app = create_router(state);

        let req = Request::builder()
            .uri("/api/capabilities")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["is_tauri"], false);
        let languages = json["languages"].as_array().unwrap();
        assert!(languages.iter().any(|l| l["code"] == "te"));
        assert!(languages.iter().any(|l| l["code"] == "en"));
        assert_eq!(json["active_device"], "auto");
        let modes = json["dom_extraction_modes"].as_array().unwrap();
        assert!(modes.iter().any(|m| m == "auto"));
    }
}
