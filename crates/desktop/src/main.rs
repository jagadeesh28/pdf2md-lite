// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use pdfmd_lite::{
    AppConfig, DomExtractionMode, DryRunReport, ImageFormat, ImageOptions, IndraScript,
    ProgressEmitter, batch::dry_run_pdf_directory, batch::dry_run_single_pdf,
    batch::process_pdf_directory_all, get_pdf_page_count, pipeline::process_pdf_full,
    vault::convert_vault_png_to_jpeg, vault::download_embedding_models, vault::embed_vault_in_path,
    vault::format_notes_in_path,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

pub struct TauriProgressEmitter {
    app: tauri::AppHandle,
}

impl ProgressEmitter for TauriProgressEmitter {
    fn on_document_start(&self, doc_slug: &str, total_pages: usize) {
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocStart {
                doc_slug: doc_slug.to_string(),
                total_pages,
            },
        );
    }

    fn on_page_complete(&self, doc_slug: &str, page_num: usize, total_pages: usize) {
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::PageComplete {
                doc_slug: doc_slug.to_string(),
                page: page_num,
                total_pages,
                pages_per_sec: None,
                eta_sec: None,
            },
        );
    }

    fn on_document_complete(&self, doc_slug: &str, status: &str) {
        let _ = self.app.emit(
            "conversion-progress",
            DesktopProgressEvent::DocComplete {
                doc_slug: doc_slug.to_string(),
                status: status.to_string(),
                message: None,
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

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    Ok(AppConfig::load(None).unwrap_or_default())
}

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    let path = PathBuf::from("pdfmd-lite.toml");
    config.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn dry_run(input_path: String, output_dir: String) -> Result<DryRunReport, String> {
    let input = Path::new(&input_path);
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

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::Log {
                level: "info".to_string(),
                message: format!("Processing {} document(s)...", files.len()),
            },
        );

        let mut completed = 0;
        let mut errors = 0;

        for file in &files {
            let path = Path::new(file);
            let slug = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "document".to_string());

            let _ = app_handle.emit(
                "conversion-progress",
                DesktopProgressEvent::DocStart {
                    doc_slug: slug.clone(),
                    total_pages: 1,
                },
            );

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
                        let _ = app_handle.emit(
                            "conversion-progress",
                            DesktopProgressEvent::DocComplete {
                                doc_slug: slug,
                                status: "completed".to_string(),
                                message: None,
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
                            },
                        );
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
        }

        let _ = app_handle.emit(
            "conversion-progress",
            DesktopProgressEvent::BatchSummary { completed, errors },
        );
    });

    Ok(())
}

#[tauri::command]
fn format_notes(vault_path: String, _fix_latex: bool) -> Result<serde_json::Value, String> {
    let path = Path::new(&vault_path);
    let summary = format_notes_in_path(path, None, false).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "filesProcessed": summary.files_scanned,
        "replacementsMade": summary.latex_repaired + summary.codeblocks_unwrapped + summary.banners_purged,
    }))
}

#[tauri::command]
fn convert_png_to_jpeg(vault_path: String, quality: u8) -> Result<serde_json::Value, String> {
    let path = Path::new(&vault_path);
    let summary = convert_vault_png_to_jpeg(path, quality, false).map_err(|e| e.to_string())?;
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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            dry_run,
            start_conversion,
            format_notes,
            convert_png_to_jpeg,
            embed_vault,
            download_models
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
