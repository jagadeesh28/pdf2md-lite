// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use pdfmd_lite::{
    AppConfig, DomExtractionMode, DryRunReport, ImageFormat, ImageOptions, IndraScript,
    ProgressEmitter, batch::dry_run_pdf_directory, batch::dry_run_single_pdf,
    get_pdf_page_count,
    vault::convert_vault_png_to_jpeg, vault::download_embedding_models, vault::embed_vault_in_path,
    vault::format_notes_in_path,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DesktopProgressEvent {
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

pub struct BatchProgressTrackerState {
    pub start_time: Instant,
    pub total_pages: usize,
    pub completed_pages: usize,
    pub rolling_history: VecDeque<(Instant, usize)>,
}

pub struct TauriProgressEmitter {
    app: tauri::AppHandle,
    state: Mutex<BatchProgressTrackerState>,
}

impl TauriProgressEmitter {
    pub fn new(app: tauri::AppHandle, total_pages: usize) -> Self {
        Self {
            app,
            state: Mutex::new(BatchProgressTrackerState {
                start_time: Instant::now(),
                total_pages,
                completed_pages: 0,
                rolling_history: VecDeque::with_capacity(64),
            }),
        }
    }
}

impl ProgressEmitter for TauriProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let initial_eta = {
            let state = self.state.lock().unwrap();
            let remaining_pages = state.total_pages.saturating_sub(state.completed_pages);
            (remaining_pages.max(total_pages).max(1) as f32) * 1.8
        };
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocStart {
                doc_slug: doc_slug.to_string(),
                total_pages,
                eta_sec: Some(initial_eta),
            },
        );
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let (pages_per_sec, eta_sec, batch_completed, batch_total) = {
            let mut state = self.state.lock().unwrap();
            state.completed_pages += 1;
            let now = Instant::now();
            state.rolling_history.push_back((now, 1));
            // Keep window of last 30 seconds
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
                Some(0.0) // 0s remaining: finishing up!
            } else if throughput > 0.001 {
                Some(remaining_pages as f32 / throughput)
            } else {
                Some((remaining_pages as f32) * 1.8) // Baseline fallback
            };

            (
                if throughput > 0.0 { Some(throughput) } else { None },
                eta,
                state.completed_pages,
                state.total_pages,
            )
        };

        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::PageComplete {
                doc_slug: doc_slug.to_string(),
                page: page_num,
                total_pages,
                pages_per_sec,
                eta_sec,
                batch_completed_pages: Some(batch_completed),
                batch_total_pages: Some(batch_total),
            },
        );
    }

    fn on_document_complete(&self, doc_slug: &str, status: &str) {
        let elapsed = {
            let state = self.state.lock().unwrap();
            state.start_time.elapsed().as_secs_f32()
        };
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocComplete {
                doc_slug: doc_slug.to_string(),
                status: status.to_string(),
                message: None,
                elapsed_sec: Some(elapsed),
                output_path: None,
            },
        );
    }

    fn on_message(&self, level: &str, message: &str) {
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                level: level.to_string(),
                message: message.to_string(),
            },
        );
    }
}

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

#[tauri::command]
async fn pick_files() -> Result<Vec<String>, String> {
    let files = rfd::AsyncFileDialog::new()
        .add_filter("PDF files (*.pdf)", &["pdf"])
        .pick_files()
        .await;

    match files {
        Some(handles) => Ok(handles
            .into_iter()
            .map(|h| h.path().to_string_lossy().to_string())
            .collect()),
        None => Ok(vec![]),
    }
}

#[tauri::command]
async fn pick_folder() -> Result<Option<String>, String> {
    let folder = rfd::AsyncFileDialog::new().pick_folder().await;
    Ok(folder.map(|h| h.path().to_string_lossy().to_string()))
}

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    let mut config = AppConfig::load(None).unwrap_or_default();
    if config.image_quality.is_none() {
        config.image_quality = Some(85);
    }
    if config.image_format.is_none() {
        config.image_format = Some("jpeg".to_string());
    }
    if config.language.is_none() {
        config.language = Some("en".to_string());
    }
    if config.dom_extraction.is_none() {
        config.dom_extraction = Some("auto".to_string());
    }
    Ok(config)
}

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    let path = PathBuf::from("pdfmd-lite.toml");
    config.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn dry_run(input_path: String, output_dir: String) -> Result<DryRunReport, String> {
    let resolved = resolve_document_path(&input_path)
        .ok_or_else(|| format!("Input path does not exist: {input_path}"))?;
    let input = resolved.as_path();
    let output = Path::new(&output_dir);
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

    if input.is_file() {
        let total_pages = get_pdf_page_count(input).unwrap_or(1);
        dry_run_single_pdf(
            input,
            output,
            1,
            total_pages,
            concurrency,
            image_opts,
            embed,
        )
        .map_err(|e| e.to_string())
    } else if input.is_dir() {
        dry_run_pdf_directory(
            input,
            output,
            None,
            max_depth,
            concurrency,
            script,
            false,
            image_opts,
            embed,
        )
        .map_err(|e| e.to_string())
    } else {
        Err(format!("Input path does not exist: {}", input.display()))
    }
}

#[tauri::command]
async fn start_conversion(
    app: tauri::AppHandle,
    files: Vec<String>,
    output_dir: String,
    config: Option<AppConfig>,
    resume: Option<bool>,
) -> Result<(), String> {
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let resume = resume.unwrap_or(true);
        let out_dir = Path::new(&output_dir);
        let config = config.unwrap_or_default();
        let image_opts = resolve_image_options(&config);
        let embed = config.embed.unwrap_or(false);
        let script = config
            .language
            .as_deref()
            .and_then(|l| l.parse().ok())
            .unwrap_or(IndraScript::English);

        let mut total_batch_pages = 0;
        for file in &files {
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
            total_batch_pages = files.len().max(1);
        }

        let tracker = std::sync::Arc::new(TauriProgressEmitter::new(
            app_handle.clone(),
            total_batch_pages,
        ));

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                level: "info".to_string(),
                message: format!(
                    "Processing {} document(s) (total ~{} pages)...",
                    files.len(),
                    total_batch_pages
                ),
            },
        );

        let mut completed = 0;
        let mut errors = 0;
        let batch_start = Instant::now();

        let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for file in &files {
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
                        tracker.on_document_start(&slug, total_pages);
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
                            Some(tracker.as_ref()),
                        ) {
                            Ok(_) => {
                                completed += 1;
                                let doc_elapsed = doc_start.elapsed().as_secs_f32();
                                let _ = app_handle.emit(
                                    "conversion-progress",
                                    DesktopProgressEvent::DocComplete {
                                        doc_slug: slug.clone(),
                                        status: "completed".to_string(),
                                        message: None,
                                        elapsed_sec: Some(doc_elapsed),
                                        output_path: Some(target_md.to_string_lossy().to_string()),
                                    },
                                );
                            }
                            Err(e) => {
                                errors += 1;
                                let _ = app_handle.emit(
                                    "conversion-progress",
                                    DesktopProgressEvent::DocComplete {
                                        doc_slug: slug,
                                        status: "error".to_string(),
                                        message: Some(e.to_string()),
                                        elapsed_sec: Some(doc_start.elapsed().as_secs_f32()),
                                        output_path: None,
                                    },
                                );
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
                            Some(tracker.clone()),
                        ) {
                            Ok(res) => {
                                completed += res.succeeded;
                                errors += res.failed.len();
                            }
                            Err(e) => {
                                errors += 1;
                                let _ = app_handle.emit(
                                    "conversion-progress",
                                    DesktopProgressEvent::Log {
                                        level: "error".to_string(),
                                        message: format!("Directory batch error: {e}"),
                                    },
                                );
                            }
                        }
                    }
                    _ => {
                        errors += 1;
                        let _ = app_handle.emit(
                            "conversion-progress",
                            DesktopProgressEvent::DocComplete {
                                doc_slug: slug,
                                status: "error".to_string(),
                                message: Some(format!("Document file or directory not found: '{file}'")),
                                elapsed_sec: None,
                                output_path: None,
                            },
                        );
                    }
                }
            }
        }));

        if let Err(panic_err) = run_result {
            let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_err.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic during conversion".to_string()
            };
            let _ = app_handle.emit(
                "conversion-progress",
                DesktopProgressEvent::Log {
                    level: "error".to_string(),
                    message: format!("Conversion halted due to internal error: {msg}"),
                },
            );
            errors += 1;
        }

        let batch_elapsed = batch_start.elapsed().as_secs_f32();
        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                level: if errors == 0 { "info".to_string() } else { "warn".to_string() },
                message: format!(
                    "Conversion finished in {:.2}s: {} succeeded, {} failed (Output: {}).",
                    batch_elapsed, completed, errors, out_dir.display()
                ),
            },
        );

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::BatchSummary {
                completed,
                errors,
                elapsed_sec: Some(batch_elapsed),
                total_pages: Some(total_batch_pages),
                output_dir: Some(out_dir.to_string_lossy().to_string()),
            },
        );
    });

    Ok(())
}

#[tauri::command]
fn open_output_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        let _ = std::fs::create_dir_all(p);
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn format_notes(
    app: tauri::AppHandle,
    vault_path: String,
    _fix_latex: bool,
) -> Result<serde_json::Value, String> {
    let path = Path::new(&vault_path);
    let app_handle = app.clone();
    let summary = pdfmd_lite::format_notes_in_path_with_progress(
        path,
        None,
        false,
        move |p| {
            let _ = app_handle.emit(
                "conversion-progress",
                DesktopProgressEvent::VaultProgress {
                    task: p.task,
                    step: p.step,
                    completed: p.completed,
                    total: p.total,
                    items_per_sec: p.items_per_sec,
                    eta_sec: p.eta_sec,
                },
            );
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "filesProcessed": summary.files_scanned,
        "replacementsMade": summary.latex_repaired + summary.codeblocks_unwrapped + summary.banners_purged,
        "frontmatterAdded": summary.frontmatter_added,
        "fieldsBackfilled": summary.frontmatter_fields_backfilled,
    }))
}

#[tauri::command]
fn convert_png_to_jpeg(
    app: tauri::AppHandle,
    vault_path: String,
    quality: Option<u8>,
) -> Result<serde_json::Value, String> {
    let path = Path::new(&vault_path);
    let q = quality.unwrap_or(85);
    let app_handle = app.clone();
    let summary = pdfmd_lite::convert_vault_png_to_jpeg_with_progress(
        path,
        q,
        false,
        move |p| {
            let _ = app_handle.emit(
                "conversion-progress",
                DesktopProgressEvent::VaultProgress {
                    task: p.task,
                    step: p.step,
                    completed: p.completed,
                    total: p.total,
                    items_per_sec: p.items_per_sec,
                    eta_sec: p.eta_sec,
                },
            );
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "imagesConverted": summary.images_converted,
        "bytesSaved": summary.orig_bytes.saturating_sub(summary.new_bytes),
    }))
}

#[tauri::command]
fn embed_vault(vault_path: String, force: bool) -> Result<serde_json::Value, String> {
    let path = Path::new(&vault_path);
    let summary = embed_vault_in_path(path, force, false).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "notesProcessed": summary.notes_scanned,
        "chunksEmbedded": summary.chunks_embedded,
    }))
}

#[tauri::command]
fn download_models() -> Result<serde_json::Value, String> {
    download_embedding_models().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "success": true,
        "message": "All required offline models downloaded and verified.",
    }))
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &**s,
                None => "Box<Any>",
            },
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let crash_text = format!("PANIC at {location}:\n{msg}\n");
        let _ = std::fs::write("crash.log", &crash_text);
        if let Ok(mut dir) = std::env::current_exe() {
            dir.pop();
            let _ = std::fs::write(dir.join("crash.log"), &crash_text);
        }
        eprintln!("{crash_text}");
    }));

    tauri::Builder::default()
        .on_webview_event(|webview, event| {
            if let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let file_paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
                let _ = webview.emit("native-file-drop", file_paths);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            dry_run,
            start_conversion,
            pick_files,
            pick_folder,
            open_output_folder,
            format_notes,
            convert_png_to_jpeg,
            embed_vault,
            download_models
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
