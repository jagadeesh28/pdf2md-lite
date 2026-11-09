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
    batch::process_pdf_directory_all, get_pdf_page_count, pipeline::process_pdf_full,
    vault::convert_vault_png_to_jpeg, vault::download_embedding_models, vault::embed_vault_in_path,
    vault::format_notes_in_path,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
    },
    #[serde(rename = "doc_complete")]
    DocComplete {
        #[serde(rename = "docSlug")]
        doc_slug: String,
        status: String,
        message: Option<String>,
    },
    #[serde(rename = "log")]
    Log { level: String, message: String },
    #[serde(rename = "batch_summary")]
    BatchSummary { completed: usize, errors: usize },
}

/// A `ProgressEmitter` implementation that broadcasts events over a Tokio channel.
pub struct BroadcastProgressEmitter {
    tx: broadcast::Sender<ProgressEvent>,
}

impl BroadcastProgressEmitter {
    pub fn new(tx: broadcast::Sender<ProgressEvent>) -> Self {
        Self { tx }
    }
}

impl ProgressEmitter for BroadcastProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let _ = self.tx.send(ProgressEvent::DocStart {
            doc_slug: doc_slug.to_string(),
            total_pages,
        });
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let _ = self.tx.send(ProgressEvent::PageComplete {
            doc_slug: doc_slug.to_string(),
            page: page_num,
            total_pages,
            pages_per_sec: None,
            eta_sec: None,
        });
    }

    fn on_document_complete(&self, doc_slug: &str, status: &str) {
        let _ = self.tx.send(ProgressEvent::DocComplete {
            doc_slug: doc_slug.to_string(),
            status: status.to_string(),
            message: None,
        });
    }

    fn on_message(&self, level: &str, message: &str) {
        let _ = self.tx.send(ProgressEvent::Log {
            level: level.to_string(),
            message: message.to_string(),
        });
    }
}

/// Shared application state for Axum web handlers.
#[derive(Clone)]
pub struct AppState {
    pub event_tx: broadcast::Sender<ProgressEvent>,
    pub is_running: Arc<AtomicBool>,
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
    pub quality: u8,
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
    let input = Path::new(&req.input_path);
    let output = Path::new(&req.output_dir);
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
        let input = input.to_path_buf();
        let output = output.to_path_buf();
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

        let _ = event_tx.send(ProgressEvent::Log {
            level: "info".to_string(),
            message: format!("Processing {} document(s)...", req.files.len()),
        });

        let mut completed = 0;
        let mut errors = 0;

        for file in &req.files {
            let path = Path::new(file);
            let slug = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "document".to_string());

            let _ = event_tx.send(ProgressEvent::DocStart {
                doc_slug: slug.clone(),
                total_pages: 1,
            });

            if path.is_file() {
                let total_pages = get_pdf_page_count(path).unwrap_or(1);
                match process_pdf_full(
                    path,
                    out_dir,
                    1,
                    total_pages,
                    config.concurrency,
                    None,
                    script,
                    image_opts,
                    embed,
                ) {
                    Ok(_) => {
                        completed += 1;
                        let _ = event_tx.send(ProgressEvent::DocComplete {
                            doc_slug: slug,
                            status: "completed".to_string(),
                            message: None,
                        });
                    }
                    Err(e) => {
                        errors += 1;
                        let _ = event_tx.send(ProgressEvent::DocComplete {
                            doc_slug: slug,
                            status: "error".to_string(),
                            message: Some(e.to_string()),
                        });
                    }
                }
            } else if path.is_dir() {
                match process_pdf_directory_all(
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
                ) {
                    Ok(res) => {
                        completed += res.succeeded;
                        errors += res.failed.len();
                    }
                    Err(e) => {
                        errors += 1;
                        let _ = event_tx.send(ProgressEvent::Log {
                            level: "error".to_string(),
                            message: format!("Directory batch error: {e}"),
                        });
                    }
                }
            }
        }

        let _ = event_tx.send(ProgressEvent::BatchSummary { completed, errors });
        is_running.store(false, Ordering::SeqCst);
    });

    Ok(StatusCode::ACCEPTED)
}

async fn format_notes_handler(
    Json(req): Json<FormatNotesRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&req.vault_path);
        let summary = format_notes_in_path(path, None, false)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({
            "filesProcessed": summary.files_scanned,
            "replacementsMade": summary.latex_repaired + summary.codeblocks_unwrapped + summary.banners_purged,
        })))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn convert_png_handler(
    Json(req): Json<ConvertPngRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&req.vault_path);
        let summary = convert_vault_png_to_jpeg(path, req.quality, false)
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
        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "upload.pdf".to_string());
        let data = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let dest_path = upload_dir.join(&file_name);
        tokio::fs::write(&dest_path, data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        saved_paths.push(dest_path.to_string_lossy().to_string());
    }

    Ok(Json(serde_json::json!({
        "files": saved_paths
    })))
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
