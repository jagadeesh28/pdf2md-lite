//! In-process Axum Web Server Daemon for pdfmd-lite (`--serve`).
//!
//! Provides REST endpoints, WebSocket event streaming, and static SPA serving
//! for browser-based document conversion, dry-runs, and vault management.

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{
        Multipart, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use futures::{SinkExt, StreamExt};
use pdfmd_lite::{
    AppConfig, DomExtractionMode, DryRunReport, ImageFormat, ImageOptions, IndraScript,
    ProgressEmitter, batch::dry_run_pdf_directory, batch::dry_run_single_pdf,
    get_pdf_page_count,
    vault::download_embedding_models, vault::embed_vault_in_path,
};
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
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Event sent over WebSocket and Tauri IPC to frontend subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressEvent {
    #[serde(rename = "doc_start")]
    DocStart {
        #[serde(rename = "docSlug")]
        doc_slug: String,
        #[serde(rename = "totalPages")]
        total_pages: usize,
        #[serde(rename = "etaSec", skip_serializing_if = "Option::is_none")]
        eta_sec: Option<f32>,
    },
    #[serde(rename = "page_complete")]
    PageComplete {
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
    Log { level: String, message: String },
    #[serde(rename = "batch_summary")]
    BatchSummary {
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
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.entries.entry(key.clone()) {
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
}

impl BroadcastProgressEmitter {
    pub fn new(
        tx: broadcast::Sender<ProgressEvent>,
        total_pages: usize,
        cancel_flag: Arc<AtomicBool>,
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
        }
    }
}

impl ProgressEmitter for BroadcastProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let initial_eta = {
            let state = self.state.lock().unwrap();
            let remaining_pages = state.total_pages.saturating_sub(state.completed_pages);
            (remaining_pages.max(total_pages).max(1) as f32) * 1.8
        };
        let _ = self.tx.send(ProgressEvent::DocStart {
            doc_slug: doc_slug.to_string(),
            total_pages,
            eta_sec: Some(initial_eta),
        });
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let (pages_per_sec, eta_sec, batch_completed, batch_total) = {
            let mut state = self.state.lock().unwrap();
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
                let first_t = state.rolling_history.front().unwrap().0;
                let dt = now.duration_since(first_t).as_secs_f32();
                let pages: usize = state.rolling_history.iter().map(|(_, p)| *p).sum();
                if dt > 0.05 && pages > 1 {
                    (pages - 1) as f32 / dt
                } else if elapsed_total > 0.05 {
                    state.completed_pages as f32 / elapsed_total
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
                if throughput > 0.0 { Some(throughput) } else { None },
                eta,
                state.completed_pages,
                state.total_pages,
            )
        };

        let _ = self.tx.send(ProgressEvent::PageComplete {
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
            let state = self.state.lock().unwrap();
            state.start_time.elapsed().as_secs_f32()
        };
        let _ = self.tx.send(ProgressEvent::DocComplete {
            doc_slug: doc_slug.to_string(),
            status: status.to_string(),
            message: None,
            elapsed_sec: Some(elapsed),
            output_path: None,
        });
    }

    fn on_message(&self, level: &str, message: &str) {
        let _ = self.tx.send(ProgressEvent::Log {
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
    pub is_running: Arc<AtomicBool>,
    pub cancel_requested: Arc<AtomicBool>,
    pub preview_cache: Arc<Mutex<PagePreviewCache>>,
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

/// Attempts to locate a document by path, falling back to looking under `input/`
/// or searching recursively under `input/` if only a file name was provided.
fn resolve_document_path(file: &str) -> Option<PathBuf> {
    let path = Path::new(file);
    if path.exists() {
        return Some(path.to_path_buf());
    }

    let candidate = Path::new("input").join(file);
    if candidate.exists() {
        return Some(candidate);
    }

    if let Some(target_name) = path.file_name()
        && Path::new("input").exists()
    {
        for entry in walkdir::WalkDir::new("input")
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == target_name {
                return Some(entry.path().to_path_buf());
            }
        }
    }

    None
}

/// Builds the Axum application router.
pub fn create_router(state: AppState) -> Router {
    let dist_dir = resolve_ui_dist_path();
    let index_file = dist_dir.join("index.html");

    let api_routes = Router::new()
        .route("/config", get(get_config).put(update_config))
        .route("/dry-run", post(dry_run_handler))
        .route("/convert", post(convert_handler))
        .route("/format-notes", post(format_notes_handler))
        .route("/convert-png-to-jpeg", post(convert_png_handler))
        .route("/embed-vault", post(embed_vault_handler))
        .route("/download-models", post(download_models_handler))
        .route("/upload", post(upload_handler))
        .route("/page-preview", get(page_preview_handler))
        .route("/cancel", post(cancel_handler))
        .route("/events", get(ws_handler))
        .with_state(state);

    let mut router = Router::new()
        .nest("/api", api_routes)
        .layer(CorsLayer::permissive());

    if dist_dir.exists() && index_file.exists() {
        let serve_dir = ServeDir::new(&dist_dir).fallback(ServeFile::new(&index_file));
        router = router.fallback_service(serve_dir);
    }

    router
}

/// Starts the local web server daemon on the given bind address (e.g. `127.0.0.1:3000`).
pub async fn start_server(bind_addr: &str) -> Result<()> {
    let (event_tx, _) = broadcast::channel(1024);
    let state = AppState {
        event_tx,
        is_running: Arc::new(AtomicBool::new(false)),
        cancel_requested: Arc::new(AtomicBool::new(false)),
        preview_cache: Arc::new(Mutex::new(PagePreviewCache::new(30))),
    };

    let router = create_router(state);
    let addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("Invalid socket bind address: '{bind_addr}'"))?;

    tracing::info!("pdfmd-lite web daemon listening on http://{addr}");
    println!(">> pdfmd-lite web application running at: http://{addr}");

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

async fn update_config(Json(config): Json<AppConfig>) -> Result<StatusCode, StatusCode> {
    let default_path = Path::new("pdfmd-lite.toml");
    config
        .save(default_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn dry_run_handler(
    Json(req): Json<DryRunRequest>,
) -> Result<Json<DryRunReport>, (StatusCode, String)> {
    let resolved = resolve_document_path(&req.input_path).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Path does not exist: {}", req.input_path),
        )
    })?;
    let input = resolved;
    let output = PathBuf::from(&req.output_dir);
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

async fn convert_handler(
    State(state): State<AppState>,
    Json(req): Json<ConvertRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if state.is_running.swap(true, Ordering::SeqCst) {
        return Err((
            StatusCode::CONFLICT,
            "A conversion task is already in progress".to_string(),
        ));
    }

    let is_running = state.is_running.clone();
    let cancel_requested = state.cancel_requested.clone();
    cancel_requested.store(false, Ordering::SeqCst);
    let event_tx = state.event_tx.clone();

    tokio::task::spawn_blocking(move || {
        let resume = req.resume.unwrap_or(true);
        let out_dir = Path::new(&req.output_dir);
        let config = req.config.unwrap_or_default();
        let image_opts = resolve_image_options(&config);
        let embed = config.embed.unwrap_or(false);
        let script = config
            .language
            .as_deref()
            .and_then(|l| l.parse().ok())
            .unwrap_or(IndraScript::English);

        let mut total_batch_pages = 0;
        for file in &req.files {
            if let Some(ref path) = resolve_document_path(file) {
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
        }
        if total_batch_pages == 0 {
            total_batch_pages = req.files.len().max(1);
        }

        let emitter = std::sync::Arc::new(BroadcastProgressEmitter::new(
            event_tx.clone(),
            total_batch_pages,
            cancel_requested.clone(),
        ));

        let _ = event_tx.send(ProgressEvent::Log {
            level: "info".to_string(),
            message: format!(
                "Processing {} document(s) (total ~{} pages)...",
                req.files.len(),
                total_batch_pages
            ),
        });

        let mut completed = 0;
        let mut errors = 0;
        let batch_start = Instant::now();

        for file in &req.files {
            if cancel_requested.load(Ordering::SeqCst) {
                let _ = event_tx.send(ProgressEvent::Log {
                    level: "warn".to_string(),
                    message: "Batch conversion halted due to cancellation request.".to_string(),
                });
                break;
            }
            let path_opt = resolve_document_path(file);
            let path_ref = path_opt.as_deref().unwrap_or_else(|| Path::new(file));
            let raw_slug = path_ref
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "document".to_string());
            let slug = pdfmd_lite::batch::sanitize_short_slug(&raw_slug);

            match path_opt {
                Some(ref path) if path.is_file() => {
                    let total_pages = get_pdf_page_count(path).unwrap_or(1);
                    emitter.on_document_start(&slug, total_pages);
                    let target_md = if out_dir.is_dir() || out_dir.extension().is_none() {
                        out_dir.join(format!("{slug}.md"))
                    } else {
                        out_dir.to_path_buf()
                    };
                    let doc_start = Instant::now();
                    match pdfmd_lite::process_pdf_full_with_emitter(
                        path,
                        &target_md,
                        1,
                        total_pages,
                        config.concurrency,
                        None,
                        script,
                        image_opts,
                        embed,
                        Some(emitter.as_ref()),
                    ) {
                        Ok(_) => {
                            completed += 1;
                            let doc_elapsed = doc_start.elapsed().as_secs_f32();
                            let _ = event_tx.send(ProgressEvent::DocComplete {
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
                                doc_slug: slug,
                                status: "error".to_string(),
                                message: Some(e.to_string()),
                                elapsed_sec: Some(doc_start.elapsed().as_secs_f32()),
                                output_path: None,
                            });
                        }
                    }
                }
                Some(ref path) if path.is_dir() => {
                    match pdfmd_lite::process_pdf_directory_all_with_emitter(
                        path,
                        out_dir,
                        None,
                        config.max_depth.unwrap_or(3),
                        config.concurrency,
                        script,
                        true,
                        resume,
                        image_opts,
                        embed,
                        Some(emitter.clone()),
                    ) {
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
                _ => {
                    errors += 1;
                    let _ = event_tx.send(ProgressEvent::DocComplete {
                        doc_slug: slug,
                        status: "error".to_string(),
                        message: Some(format!("Document file or directory not found: '{file}'")),
                        elapsed_sec: None,
                        output_path: None,
                    });
                }
            }
        }

        let batch_elapsed = batch_start.elapsed().as_secs_f32();
        let _ = event_tx.send(ProgressEvent::Log {
            level: if errors == 0 { "info".to_string() } else { "warn".to_string() },
            message: format!(
                "Conversion finished in {:.2}s: {} succeeded, {} failed (Output: {}).",
                batch_elapsed, completed, errors, out_dir.display()
            ),
        });

        let _ = event_tx.send(ProgressEvent::BatchSummary {
            completed,
            errors,
            elapsed_sec: Some(batch_elapsed),
            total_pages: Some(total_batch_pages),
            output_dir: Some(out_dir.to_string_lossy().to_string()),
        });
        is_running.store(false, Ordering::SeqCst);
        cancel_requested.store(false, Ordering::SeqCst);
    });

    Ok(StatusCode::ACCEPTED)
}

async fn format_notes_handler(
    State(state): State<AppState>,
    Json(req): Json<FormatNotesRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tx = state.event_tx.clone();
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&req.vault_path);
        let summary = pdfmd_lite::format_notes_in_path_with_progress(
            path,
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
    let tx = state.event_tx.clone();
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&req.vault_path);
        let q = req.quality.unwrap_or(85);
        let summary = pdfmd_lite::convert_vault_png_to_jpeg_with_progress(
            path,
            q,
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
                        level: "info".to_string(),
                        message: format!(
                            "PNG migration: {}/{} images ({:.1}%) - {:.1} img/s, ETA: ~{:.0}s",
                            p.completed, p.total, pct, p.items_per_sec, p.eta_sec
                        ),
                    });
                }
            },
        )
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
    Json(req): Json<EmbedVaultRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&req.vault_path);
        let summary = embed_vault_in_path(path, req.force, false)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "notesProcessed": summary.notes_scanned,
            "chunksEmbedded": summary.chunks_embedded,
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let upload_dir = PathBuf::from("temp/uploads");
    tokio::fs::create_dir_all(&upload_dir)
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
    axum::extract::Query(query): axum::extract::Query<PagePreviewQuery>,
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
        let cache = state.preview_cache.lock().unwrap();
        if let Some(bytes) = cache.get(&cache_key) {
            return Ok((
                [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
                bytes,
            ));
        }
    }

    // 2. Resolve document filesystem path
    let doc_path = resolve_document_path(&doc_identifier).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Document '{doc_identifier}' not found"),
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
        let mut cache = state.preview_cache.lock().unwrap();
        cache.insert(cache_key, jpeg_bytes.clone());
    }

    Ok((
        [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
        jpeg_bytes,
    ))
}

async fn cancel_handler(State(state): State<AppState>) -> StatusCode {
    if state.is_running.load(Ordering::SeqCst) {
        state.cancel_requested.store(true, Ordering::SeqCst);
        let _ = state.event_tx.send(ProgressEvent::Log {
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
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event)
                && sender.send(Message::Text(json)).await.is_err()
            {
                break;
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
