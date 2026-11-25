// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use pdfmd_lite::{
    AppConfig, DomExtractionMode, DryRunReport, ImageFormat, ImageOptions, IndraScript,
    ProgressEmitter, batch::dry_run_pdf_directory, batch::dry_run_single_pdf, get_pdf_page_count,
    vault::download_embedding_models, vault::embed_vault_in_path,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tauri::Emitter;

/// Describes a single document produced by a folder (directory) batch conversion
/// so the frontend can surface per-file completion and "Inspect Note" actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedDocument {
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    #[serde(rename = "outputPath")]
    pub output_path: String,
    #[serde(rename = "totalPages")]
    pub total_pages: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DesktopProgressEvent {
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
        #[serde(rename = "documents", skip_serializing_if = "Option::is_none")]
        documents: Option<Vec<PlannedDocument>>,
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
    run_id: Option<String>,
}

impl TauriProgressEmitter {
    pub fn new(app: tauri::AppHandle, total_pages: usize, run_id: Option<String>) -> Self {
        Self {
            app,
            state: Mutex::new(BatchProgressTrackerState {
                start_time: Instant::now(),
                total_pages,
                completed_pages: 0,
                rolling_history: VecDeque::with_capacity(64),
            }),
            run_id,
        }
    }
}

impl ProgressEmitter for TauriProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let initial_eta = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let remaining_pages = state.total_pages.saturating_sub(state.completed_pages);
            (remaining_pages.max(total_pages).max(1) as f32) * 1.8
        };
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocStart {
                run_id: self.run_id.clone(),
                job_id: Some(doc_slug.to_string()),
                doc_slug: doc_slug.to_string(),
                total_pages,
                eta_sec: Some(initial_eta),
            },
        );
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let (pages_per_sec, eta_sec, batch_completed, batch_total) = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
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
                let first_t = state.rolling_history.front().map(|f| f.0).unwrap_or(now);
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

        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::PageComplete {
                run_id: self.run_id.clone(),
                job_id: Some(doc_slug.to_string()),
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
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.start_time.elapsed().as_secs_f32()
        };
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocComplete {
                run_id: self.run_id.clone(),
                job_id: Some(doc_slug.to_string()),
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
                run_id: self.run_id.clone(),
                job_id: None,
                level: level.to_string(),
                message: message.to_string(),
            },
        );
    }

    fn is_cancelled(&self) -> bool {
        RUN_CONTROLLER.is_cancelled()
    }
}

static RUN_CONTROLLER: std::sync::LazyLock<pdfmd_lite::RunController> =
    std::sync::LazyLock::new(pdfmd_lite::RunController::new);

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

/// Computes the per-file conversion plan for a folder (source PDF -> generated `.md`)
/// using the exact same short-slug layout the batch converter writes to, so the UI can
/// expose per-file completion and "Inspect Note" actions for folder conversions.
#[allow(clippy::too_many_arguments)]
fn plan_directory_documents(
    input_dir: &Path,
    output_base_dir: &Path,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
    resume: bool,
    image_opts: ImageOptions,
    embed: bool,
) -> Vec<PlannedDocument> {
    let Ok(report) = dry_run_pdf_directory(
        input_dir,
        output_base_dir,
        None,
        max_depth,
        explicit_concurrency,
        script,
        resume,
        image_opts,
        embed,
    ) else {
        return Vec::new();
    };

    report
        .plans
        .into_iter()
        .map(|plan| {
            let file_name = plan
                .input_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| plan.input_path.to_string_lossy().to_string());
            let status = if plan.target_md_path.is_file() {
                "completed".to_string()
            } else {
                "error".to_string()
            };
            PlannedDocument {
                file_name,
                source_path: plan.input_path.to_string_lossy().to_string(),
                output_path: plan.target_md_path.to_string_lossy().to_string(),
                total_pages: plan.total_pages,
                status,
            }
        })
        .collect()
}

/// Resolves an existing document by path after verifying path policy syntax.
fn resolve_document_path(file: &str) -> Option<PathBuf> {
    let path = Path::new(file);
    if pdfmd_lite::PathPolicy::check_syntax(path).is_err() {
        return None;
    }
    if path.exists() {
        return Some(path.to_path_buf());
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
fn save_config(incoming: AppConfig) -> Result<(), String> {
    let path = pdfmd_lite::config::default_config_path();
    let mut current = AppConfig::load(None).unwrap_or_default();
    current.merge_patch(incoming);
    current.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_capabilities() -> Result<pdfmd_lite::CapabilitiesResponse, String> {
    Ok(pdfmd_lite::get_capabilities(true))
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
    run_id: Option<String>,
) -> Result<(), String> {
    let run_guard = RUN_CONTROLLER
        .start_run(run_id.clone())
        .map_err(|e| e.to_string())?;
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = run_guard;
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
            run_id.clone(),
        ));

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                run_id: run_id.clone(),
                job_id: None,
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
        let mut completed_documents: Vec<PlannedDocument> = Vec::new();
        let batch_start = Instant::now();

        for file in &files {
            if RUN_CONTROLLER.is_cancelled() {
                let _ = app_handle.emit(
                    "conversion-progress",
                    DesktopProgressEvent::Log {
                        run_id: run_id.clone(),
                        job_id: None,
                        level: "warn".to_string(),
                        message: "Batch conversion halted due to cancellation request.".to_string(),
                    },
                );
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
                    tracker.on_document_start(&slug, total_pages);
                    let target_md = if out_dir.is_dir() || out_dir.extension().is_none() {
                        out_dir.join(format!("{slug}.md"))
                    } else {
                        out_dir.to_path_buf()
                    };
                    let doc_start = Instant::now();
                    let script_copy = script;
                    let image_opts_copy = image_opts.clone();
                    let tracker_copy = tracker.clone();
                    let target_md_copy = target_md.clone();
                    let path_copy = path.clone();
                    let concurrency = config.concurrency;

                    let doc_res = pdfmd_lite::execute_isolated_document(move || {
                        pdfmd_lite::process_pdf_full_with_emitter(
                            &path_copy,
                            &target_md_copy,
                            1,
                            total_pages,
                            concurrency,
                            None,
                            script_copy,
                            image_opts_copy,
                            embed,
                            Some(tracker_copy.as_ref()),
                        )
                    });

                    match doc_res {
                        Ok(_) => {
                            completed += 1;
                            let doc_elapsed = doc_start.elapsed().as_secs_f32();
                            let _ = app_handle.emit(
                                "conversion-progress",
                                DesktopProgressEvent::DocComplete {
                                    run_id: run_id.clone(),
                                    job_id: Some(slug.clone()),
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
                                    run_id: run_id.clone(),
                                    job_id: Some(slug.clone()),
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
                        image_opts.clone(),
                        embed,
                        Some(tracker.clone()),
                    ) {
                        Ok(res) => {
                            completed += res.succeeded;
                            errors += res.failed.len();
                            completed_documents.extend(plan_directory_documents(
                                path,
                                out_dir,
                                config.max_depth.unwrap_or(3),
                                config.concurrency,
                                script,
                                resume,
                                image_opts.clone(),
                                embed,
                            ));
                        }
                        Err(e) => {
                            errors += 1;
                            let _ = app_handle.emit(
                                "conversion-progress",
                                DesktopProgressEvent::Log {
                                    run_id: run_id.clone(),
                                    job_id: None,
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
                            run_id: run_id.clone(),
                            job_id: Some(slug.clone()),
                            doc_slug: slug,
                            status: "error".to_string(),
                            message: Some(format!(
                                "Document file or directory not found: '{file}'"
                            )),
                            elapsed_sec: None,
                            output_path: None,
                        },
                    );
                }
            }
        }

        let batch_elapsed = batch_start.elapsed().as_secs_f32();
        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                run_id: run_id.clone(),
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
            },
        );

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::BatchSummary {
                run_id: run_id.clone(),
                completed,
                errors,
                elapsed_sec: Some(batch_elapsed),
                total_pages: Some(total_batch_pages),
                output_dir: Some(out_dir.to_string_lossy().to_string()),
                documents: if completed_documents.is_empty() {
                    None
                } else {
                    Some(completed_documents)
                },
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
async fn format_notes(
    app: tauri::AppHandle,
    vault_path: String,
    _fix_latex: bool,
) -> Result<serde_json::Value, String> {
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&vault_path);
        let emitter_handle = app_handle.clone();
        let summary = pdfmd_lite::format_notes_in_path_with_progress(
            path,
            None,
            false,
            move |p| {
                let _ = emitter_handle.emit(
                    "conversion-progress",
                    DesktopProgressEvent::VaultProgress {
                        task: p.task.clone(),
                        step: p.step.clone(),
                        completed: p.completed,
                        total: p.total,
                        items_per_sec: p.items_per_sec,
                        eta_sec: p.eta_sec,
                    },
                );

                if p.completed == 1 || p.completed % 25 == 0 || p.completed == p.total {
                    let pct = if p.total > 0 {
                        (p.completed as f32 / p.total as f32) * 100.0
                    } else {
                        100.0
                    };
                    let _ = emitter_handle.emit(
                        "conversion-progress",
                        DesktopProgressEvent::Log {
                            run_id: None,
                            job_id: None,
                            level: "info".to_string(),
                            message: format!(
                                "Formatting notes: {}/{} ({:.1}%) - {:.1} notes/s, ETA: ~{:.0}s",
                                p.completed, p.total, pct, p.items_per_sec, p.eta_sec
                            ),
                        },
                    );
                }
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "filesProcessed": summary.files_scanned,
            "replacementsMade": summary.latex_repaired + summary.codeblocks_unwrapped + summary.banners_purged,
            "frontmatterAdded": summary.frontmatter_added,
            "fieldsBackfilled": summary.frontmatter_fields_backfilled,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn convert_png_to_jpeg(
    app: tauri::AppHandle,
    vault_path: String,
    quality: Option<u8>,
) -> Result<serde_json::Value, String> {
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&vault_path);
        let q = quality.unwrap_or(85);
        let emitter_handle = app_handle.clone();
        let summary =
            pdfmd_lite::convert_vault_png_to_jpeg_with_progress(path, q, false, move |p| {
                let _ = emitter_handle.emit(
                    "conversion-progress",
                    DesktopProgressEvent::VaultProgress {
                        task: p.task.clone(),
                        step: p.step.clone(),
                        completed: p.completed,
                        total: p.total,
                        items_per_sec: p.items_per_sec,
                        eta_sec: p.eta_sec,
                    },
                );

                if p.completed == 1 || p.completed % 25 == 0 || p.completed == p.total {
                    let pct = if p.total > 0 {
                        (p.completed as f32 / p.total as f32) * 100.0
                    } else {
                        100.0
                    };
                    let _ = emitter_handle.emit(
                        "conversion-progress",
                        DesktopProgressEvent::Log {
                            run_id: None,
                            job_id: None,
                            level: "info".to_string(),
                            message: format!(
                                "PNG migration: {}/{} images ({:.1}%) - {:.1} img/s, ETA: ~{:.0}s",
                                p.completed, p.total, pct, p.items_per_sec, p.eta_sec
                            ),
                        },
                    );
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "imagesConverted": summary.images_converted,
            "bytesSaved": summary.orig_bytes.saturating_sub(summary.new_bytes),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn embed_vault(vault_path: String, force: bool) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&vault_path);
        let summary = embed_vault_in_path(path, force, false).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "notesProcessed": summary.notes_scanned,
            "chunksEmbedded": summary.chunks_embedded,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_model_status() -> Result<Vec<pdfmd_lite::ModelStatus>, String> {
    tokio::task::spawn_blocking(pdfmd_lite::get_model_status)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn download_models() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        download_embedding_models().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "success": true,
            "message": "All required offline models downloaded and verified.",
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_conversion() -> Result<(), String> {
    RUN_CONTROLLER.request_cancel();
    Ok(())
}

#[tauri::command]
fn get_run_state() -> Result<pdfmd_lite::RunState, String> {
    Ok(RUN_CONTROLLER.current_state().0)
}

#[tauri::command]
async fn get_page_preview(file_path: String, page: usize) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(&file_path);
        let resolved = resolve_document_path(&file_path).unwrap_or(path);
        let dynamic_img = pdfmd_lite::pdf::render_single_page_image(&resolved, page.max(1), 1.5)
            .map_err(|e| format!("Failed to render page: {e:#}"))?;
        pdfmd_lite::encode_image_to_base64_data_uri(&dynamic_img, ImageFormat::Jpeg, 85)
            .map_err(|e| format!("Failed to encode data URI: {e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reads a generated Markdown note from disk so the split-pane inspector can render
/// the real converted content instead of a placeholder.
#[tauri::command]
async fn read_markdown(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let candidate = PathBuf::from(&path);
        let resolved = if candidate.is_file() {
            candidate
        } else {
            resolve_document_path(&path)
                .ok_or_else(|| format!("Markdown output not found: {path}"))?
        };
        std::fs::read_to_string(&resolved)
            .map_err(|e| format!("Failed to read '{}': {e}", resolved.display()))
    })
    .await
    .map_err(|e| e.to_string())?
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
                let file_paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                let _ = webview.emit("native-file-drop", file_paths);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            dry_run,
            start_conversion,
            cancel_conversion,
            get_run_state,
            get_page_preview,
            read_markdown,
            pick_files,
            pick_folder,
            open_output_folder,
            format_notes,
            convert_png_to_jpeg,
            embed_vault,
            download_models,
            get_model_status,
            get_capabilities
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
