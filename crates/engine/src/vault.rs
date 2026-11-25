//! Vault-wide note maintenance, image conversion, and retroactive embedding operations.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::figure::ImageFormat;

fn collect_files_with_ext(dir: &Path, ext: &str, collected: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files_with_ext(&p, ext, collected);
        } else if p.is_file()
            && p.extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
        {
            collected.push(p);
        }
    }
}

pub fn collect_md_files(dir: &Path, collected: &mut Vec<PathBuf>) {
    collect_files_with_ext(dir, "md", collected);
}

/// Summary statistics for note formatting operations.
#[derive(Debug, Default, Clone)]
pub struct FormatNotesSummary {
    pub files_scanned: usize,
    pub files_modified: usize,
    pub codeblocks_unwrapped: usize,
    pub banners_purged: usize,
    pub latex_repaired: usize,
    pub frontmatter_added: usize,
    pub frontmatter_fields_backfilled: usize,
    pub figure_links_converted: usize,
}

/// Progress notification emitted during retroactive vault maintenance tasks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultTaskProgress {
    pub task: String,
    pub step: String,
    pub completed: usize,
    pub total: usize,
    pub items_per_sec: f32,
    pub eta_sec: f32,
}

/// Formats Markdown note(s) across a single file or directory with real-time progress & ETA.
pub fn format_notes_in_path_with_progress<F>(
    path: &Path,
    output: Option<&Path>,
    dry_run: bool,
    mut on_progress: F,
) -> Result<FormatNotesSummary>
where
    F: FnMut(VaultTaskProgress),
{
    let mut summary = FormatNotesSummary::default();

    if path.is_file() {
        summary.files_scanned = 1;
        on_progress(VaultTaskProgress {
            task: "metadata_formatting".to_string(),
            step: "Formatting note & inserting metadata (1/1)".to_string(),
            completed: 1,
            total: 1,
            items_per_sec: 1.0,
            eta_sec: 0.0,
        });
        let content = fs::read_to_string(path)?;
        if is_pdfmd_note(&content) {
            let (fixed, report) = crate::markdown::format_note_document(&content, Some(path));

            if report.is_changed() {
                if !dry_run {
                    let write_res = if let Some(out_p) = output {
                        atomic_write_file(out_p, &fixed)
                    } else {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let root_dir = path.parent().unwrap_or(path);
                        let _ = backup_file(path, root_dir, ts);
                        atomic_write_file(path, &fixed)
                    };

                    if write_res.is_ok() {
                        summary.files_modified = 1;
                        summary.codeblocks_unwrapped += report.codeblocks_unwrapped;
                        summary.banners_purged += report.banners_purged;
                        if report.latex_repaired {
                            summary.latex_repaired += 1;
                        }
                        if report.frontmatter_added {
                            summary.frontmatter_added += 1;
                        }
                        summary.frontmatter_fields_backfilled +=
                            report.frontmatter_fields_backfilled;
                        summary.figure_links_converted += report.figure_links_converted;
                    }
                } else {
                    summary.files_modified = 1;
                    summary.codeblocks_unwrapped += report.codeblocks_unwrapped;
                    summary.banners_purged += report.banners_purged;
                    if report.latex_repaired {
                        summary.latex_repaired += 1;
                    }
                    if report.frontmatter_added {
                        summary.frontmatter_added += 1;
                    }
                    summary.frontmatter_fields_backfilled += report.frontmatter_fields_backfilled;
                    summary.figure_links_converted += report.figure_links_converted;
                }
            }
        }
    } else if path.is_dir() {
        let mut md_files = Vec::new();
        collect_md_files(path, &mut md_files);
        let total = md_files.len();
        summary.files_scanned = total;
        let start = std::time::Instant::now();

        on_progress(VaultTaskProgress {
            task: "metadata_formatting".to_string(),
            step: if total == 0 {
                "No Markdown notes found in directory".to_string()
            } else {
                format!("Discovered {total} Markdown note(s). Formatting...")
            },
            completed: 0,
            total,
            items_per_sec: 0.0,
            eta_sec: 0.0,
        });

        for (idx, file) in md_files.iter().enumerate() {
            let completed = idx + 1;
            let elapsed = start.elapsed().as_secs_f32();
            let throughput = if elapsed > 0.05 {
                completed as f32 / elapsed
            } else {
                0.0
            };
            let remaining = total.saturating_sub(completed);
            let eta_sec = if remaining == 0 {
                0.0
            } else if throughput > 0.01 {
                remaining as f32 / throughput
            } else {
                (remaining as f32) * 0.05
            };

            on_progress(VaultTaskProgress {
                task: "metadata_formatting".to_string(),
                step: format!("Formatting notes & metadata ({completed}/{total})"),
                completed,
                total,
                items_per_sec: throughput,
                eta_sec,
            });

            let content = match fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Could not read {}: {e}", file.display());
                    continue;
                }
            };
            if !is_pdfmd_note(&content) {
                continue;
            }
            let (fixed, report) = crate::markdown::format_note_document(&content, Some(file));
            if report.is_changed() {
                if !dry_run {
                    let dest = if let Some(out_dir) = output {
                        if let Ok(rel) = file.strip_prefix(path) {
                            out_dir.join(rel)
                        } else {
                            out_dir.join(
                                file.file_name()
                                    .unwrap_or_else(|| std::ffi::OsStr::new("note.md")),
                            )
                        }
                    } else {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let _ = backup_file(file, path, ts);
                        file.clone()
                    };

                    match atomic_write_file(&dest, &fixed) {
                        Ok(_) => {
                            summary.files_modified += 1;
                            summary.codeblocks_unwrapped += report.codeblocks_unwrapped;
                            summary.banners_purged += report.banners_purged;
                            if report.latex_repaired {
                                summary.latex_repaired += 1;
                            }
                            if report.frontmatter_added {
                                summary.frontmatter_added += 1;
                            }
                            summary.frontmatter_fields_backfilled +=
                                report.frontmatter_fields_backfilled;
                            summary.figure_links_converted += report.figure_links_converted;
                        }
                        Err(e) => {
                            tracing::warn!("Could not write {}: {e}", dest.display());
                        }
                    }
                } else {
                    summary.files_modified += 1;
                    summary.codeblocks_unwrapped += report.codeblocks_unwrapped;
                    summary.banners_purged += report.banners_purged;
                    if report.latex_repaired {
                        summary.latex_repaired += 1;
                    }
                    if report.frontmatter_added {
                        summary.frontmatter_added += 1;
                    }
                    summary.frontmatter_fields_backfilled += report.frontmatter_fields_backfilled;
                    summary.figure_links_converted += report.figure_links_converted;
                }
            }
        }
    }

    Ok(summary)
}

fn is_pdfmd_note(content: &str) -> bool {
    let (meta, _) = crate::markdown::parse_existing_frontmatter(content);
    meta.and_then(|m| m.generator).as_deref() == Some("pdfmd-lite")
        || content.contains("generator: pdfmd-lite")
        || content.contains("generator: \"pdfmd-lite\"")
}

fn backup_file(file: &Path, root: &Path, timestamp: u64) -> Result<()> {
    let backup_dir = root.join(".pdfmd-backup").join(timestamp.to_string());
    let rel = file
        .strip_prefix(root)
        .unwrap_or_else(|_| file.file_name().map(Path::new).unwrap_or(file));
    let target = backup_dir.join(rel);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(file, target)?;
    Ok(())
}

fn atomic_write_file(dest: &Path, content: &str) -> Result<()> {
    let partial = dest.with_extension("md.partial");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&partial, content)?;
    fs::rename(&partial, dest)?;
    Ok(())
}

/// Formats Markdown note(s) across a single file or directory:
/// - Unwraps spurious codeblocks
/// - Purges extraction banners
/// - Normalizes LaTeX math equations and table cells
/// - Converts figure links to Obsidian wiki-links (![[...]])
/// - Synthesizes/backfills Obsidian YAML frontmatter
pub fn format_notes_in_path(
    path: &Path,
    output: Option<&Path>,
    dry_run: bool,
) -> Result<FormatNotesSummary> {
    format_notes_in_path_with_progress(path, output, dry_run, |_| {})
}

/// Summary statistics for vault image format conversions.
#[derive(Debug, Default, Clone)]
pub struct ConvertImagesSummary {
    pub images_scanned: usize,
    pub images_converted: usize,
    pub notes_scanned: usize,
    pub notes_updated: usize,
    pub links_updated: usize,
    pub orig_bytes: u64,
    pub new_bytes: u64,
}

/// Converts existing PNG figures in an Obsidian vault to compressed JPEG images in-place with progress & ETA.
pub fn convert_vault_png_to_jpeg_with_progress<F>(
    vault_path: &Path,
    quality: u8,
    dry_run: bool,
    mut on_progress: F,
) -> Result<ConvertImagesSummary>
where
    F: FnMut(VaultTaskProgress),
{
    let mut summary = ConvertImagesSummary::default();

    // 1. Scan and convert all PNG image files
    let mut png_files = Vec::new();
    collect_files_with_ext(vault_path, "png", &mut png_files);
    let total_png = png_files.len();
    summary.images_scanned = total_png;
    let png_start = std::time::Instant::now();

    on_progress(VaultTaskProgress {
        task: "image_migration".to_string(),
        step: if total_png == 0 {
            "No PNG images found to convert".to_string()
        } else {
            format!("Discovered {total_png} PNG image(s). Starting conversion...")
        },
        completed: 0,
        total: total_png,
        items_per_sec: 0.0,
        eta_sec: 0.0,
    });

    for (idx, png_path) in png_files.iter().enumerate() {
        let completed = idx + 1;
        let elapsed = png_start.elapsed().as_secs_f32();
        let throughput = if elapsed > 0.05 {
            completed as f32 / elapsed
        } else {
            0.0
        };
        let remaining = total_png.saturating_sub(completed);
        let eta_sec = if remaining == 0 {
            0.0
        } else if throughput > 0.01 {
            remaining as f32 / throughput
        } else {
            (remaining as f32) * 0.12
        };

        on_progress(VaultTaskProgress {
            task: "image_migration".to_string(),
            step: format!("Converting PNG images ({completed}/{total_png})"),
            completed,
            total: total_png,
            items_per_sec: throughput,
            eta_sec,
        });

        let orig_size = match fs::metadata(png_path) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        summary.orig_bytes += orig_size;

        let jpg_path = png_path.with_extension("jpg");

        if dry_run {
            summary.new_bytes += (orig_size as f64 * 0.25) as u64;
            summary.images_converted += 1;
            continue;
        }

        let dyn_img = match image::open(png_path) {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!("Failed to open image {}: {e}", png_path.display());
                summary.new_bytes += orig_size;
                continue;
            }
        };

        match crate::figure::save_image_with_format(&dyn_img, &jpg_path, ImageFormat::Jpeg, quality)
        {
            Ok(()) => {
                let new_size = fs::metadata(&jpg_path).map(|m| m.len()).unwrap_or(0);
                summary.new_bytes += new_size;
                summary.images_converted += 1;
                let _ = fs::remove_file(png_path);
            }
            Err(e) => {
                tracing::warn!("Failed to convert {}: {e}", png_path.display());
                summary.new_bytes += orig_size;
            }
        }
    }

    // 2. Scan and update all Markdown notes
    let mut md_files = Vec::new();
    collect_files_with_ext(vault_path, "md", &mut md_files);
    let total_md = md_files.len();
    summary.notes_scanned = total_md;
    let md_start = std::time::Instant::now();

    if total_md > 0 {
        on_progress(VaultTaskProgress {
            task: "image_migration".to_string(),
            step: format!("Updating note references across {total_md} note(s)..."),
            completed: 0,
            total: total_md,
            items_per_sec: 0.0,
            eta_sec: 0.0,
        });
    }

    for (idx, md_path) in md_files.iter().enumerate() {
        let completed = idx + 1;
        let elapsed = md_start.elapsed().as_secs_f32();
        let throughput = if elapsed > 0.05 {
            completed as f32 / elapsed
        } else {
            0.0
        };
        let remaining = total_md.saturating_sub(completed);
        let eta_sec = if remaining == 0 {
            0.0
        } else if throughput > 0.01 {
            remaining as f32 / throughput
        } else {
            (remaining as f32) * 0.02
        };

        on_progress(VaultTaskProgress {
            task: "image_migration".to_string(),
            step: format!("Updating note references ({completed}/{total_md})"),
            completed,
            total: total_md,
            items_per_sec: throughput,
            eta_sec,
        });

        let content = match fs::read_to_string(md_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Could not read {}: {e}", md_path.display());
                continue;
            }
        };

        let (updated, links_changed) = crate::markdown::replace_png_links_with_jpg(&content);
        if links_changed > 0 {
            summary.notes_updated += 1;
            summary.links_updated += links_changed;

            if !dry_run && let Err(e) = fs::write(md_path, updated) {
                tracing::warn!("Could not write {}: {e}", md_path.display());
            }
        }
    }

    Ok(summary)
}

/// Converts existing PNG figures in an Obsidian vault to compressed JPEG images in-place.
pub fn convert_vault_png_to_jpeg(
    vault_path: &Path,
    quality: u8,
    dry_run: bool,
) -> Result<ConvertImagesSummary> {
    convert_vault_png_to_jpeg_with_progress(vault_path, quality, dry_run, |_| {})
}

/// Summary statistics for retroactive vault embedding operations.
#[derive(Debug, Default, Clone)]
pub struct EmbedVaultSummary {
    pub notes_scanned: usize,
    pub notes_embedded: usize,
    pub notes_skipped: usize,
    pub chunks_embedded: usize,
}

/// Retroactively generates `.embeddings.jsonl` sidecars across an existing Markdown vault or folder.
pub fn embed_vault_in_path(path: &Path, force: bool, quiet: bool) -> Result<EmbedVaultSummary> {
    let mut collected = Vec::new();
    if path.is_file() {
        collected.push(path.to_path_buf());
    } else if path.is_dir() {
        collect_md_files(path, &mut collected);
    } else {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    let engine = crate::embeddings::get_embedding_engine(None)
        .context("Embedding model not loaded. Run 'pdfmd-lite --download-models' to download official weights.")?;

    let mut summary = EmbedVaultSummary {
        notes_scanned: collected.len(),
        ..Default::default()
    };

    for md_path in &collected {
        let sidecar_path = crate::embeddings::sidecar_path_for_markdown(md_path);
        if !force && !crate::embeddings::should_update_embeddings(md_path, &sidecar_path) {
            summary.notes_skipped += 1;
            continue;
        }

        let content = fs::read_to_string(md_path)?;
        let stem = md_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("note");
        let slug = crate::batch::sanitize_short_slug(stem);

        match crate::embeddings::generate_and_save_sidecar(&content, &slug, md_path, &engine) {
            Ok(count) => {
                summary.notes_embedded += 1;
                summary.chunks_embedded += count;
                if !quiet {
                    tracing::info!(
                        "[{}/{}] Embedded {} ({} chunks) -> {}",
                        summary.notes_embedded + summary.notes_skipped,
                        summary.notes_scanned,
                        md_path.display(),
                        count,
                        sidecar_path.display()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to embed {}: {e}", md_path.display());
            }
        }
    }

    Ok(summary)
}

/// Provisions official INT8 all-MiniLM-L6-v2 ONNX model and tokenizer.json into models/embeddings/.
#[cfg(feature = "online-provisioning")]
pub fn download_embedding_models() -> Result<()> {
    use sha2::Digest;

    let target_dir = crate::config::default_data_dir()
        .join("models")
        .join("embeddings");
    fs::create_dir_all(&target_dir)?;

    let model_target = target_dir.join("model.onnx");
    let tokenizer_target = target_dir.join("tokenizer.json");

    tracing::info!(
        "Checking embedding model assets in '{}'...",
        target_dir.display()
    );

    let manifest = crate::model_locator::ModelManifest::embedded().ok();
    let expected_model_hash = manifest
        .as_ref()
        .and_then(|m| m.find_by_subpath("models/embeddings/model.onnx"))
        .map(|e| e.sha256.as_str())
        .unwrap_or("afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1");
    let expected_tok_hash = manifest
        .as_ref()
        .and_then(|m| m.find_by_subpath("models/embeddings/tokenizer.json"))
        .map(|e| e.sha256.as_str())
        .unwrap_or("be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037");

    let model_url =
        "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model_quantized.onnx";
    let tokenizer_url =
        "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/raw/main/tokenizer.json";

    let curl_cmd = if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    };

    if !model_target.exists() {
        tracing::info!("Downloading quantized all-MiniLM-L6-v2 ONNX model (~22 MB)...");
        let partial_model = target_dir.join("model.onnx.partial");
        let status = std::process::Command::new(curl_cmd)
            .args([
                "-L",
                "-s",
                "-o",
                partial_model.to_string_lossy().as_ref(),
                model_url,
            ])
            .status();

        match status {
            Ok(s) if s.success() => {
                let data = fs::read(&partial_model)?;
                let hash = format!("{:x}", sha2::Sha256::digest(&data));
                if !hash.eq_ignore_ascii_case(expected_model_hash) {
                    let _ = fs::remove_file(&partial_model);
                    anyhow::bail!(
                        "Downloaded model failed SHA-256 verification: expected {expected_model_hash}, got {hash}"
                    );
                }
                fs::rename(&partial_model, &model_target)?;
                tracing::info!("Downloaded and verified {}", model_target.display());
            }
            _ => {
                let _ = fs::remove_file(&partial_model);
                anyhow::bail!(
                    "Failed to download model via curl.\nPlease manually download:\n  {}\nand save to:\n  {}",
                    model_url,
                    model_target.display()
                );
            }
        }
    } else {
        tracing::info!("Model already exists: {}", model_target.display());
    }

    if !tokenizer_target.exists() {
        tracing::info!("Downloading tokenizer.json...");
        let partial_tok = target_dir.join("tokenizer.json.partial");
        let status = std::process::Command::new(curl_cmd)
            .args([
                "-L",
                "-s",
                "-o",
                partial_tok.to_string_lossy().as_ref(),
                tokenizer_url,
            ])
            .status();

        match status {
            Ok(s) if s.success() => {
                let data = fs::read(&partial_tok)?;
                let hash = format!("{:x}", sha2::Sha256::digest(&data));
                if !hash.eq_ignore_ascii_case(expected_tok_hash) {
                    let _ = fs::remove_file(&partial_tok);
                    anyhow::bail!(
                        "Downloaded tokenizer failed SHA-256 verification: expected {expected_tok_hash}, got {hash}"
                    );
                }
                fs::rename(&partial_tok, &tokenizer_target)?;
                tracing::info!("Downloaded and verified {}", tokenizer_target.display());
            }
            _ => {
                let _ = fs::remove_file(&partial_tok);
                anyhow::bail!(
                    "Failed to download tokenizer via curl.\nPlease manually download:\n  {}\nand save to:\n  {}",
                    tokenizer_url,
                    tokenizer_target.display()
                );
            }
        }
    } else {
        tracing::info!("Tokenizer already exists: {}", tokenizer_target.display());
    }

    crate::model_locator::invalidate_model_caches();
    tracing::info!("Model provisioning complete!");
    Ok(())
}

/// Installs offline neural models from a local directory or .zip bundle into AppData models directory.
/// Verifies all extracted files against embedded `MANIFEST.toml` (SHA-256 and byte sizes)
/// and invalidates in-memory model caches per Phase 10.
pub fn install_offline_models(
    source_path: &Path,
) -> Result<Vec<crate::model_locator::ModelStatus>> {
    if !source_path.exists() {
        anyhow::bail!("Source path '{}' does not exist", source_path.display());
    }

    let target_models_dir = crate::config::default_data_dir().join("models");
    fs::create_dir_all(&target_models_dir).with_context(|| {
        format!(
            "Failed to create models directory at {}",
            target_models_dir.display()
        )
    })?;

    if source_path.is_file() {
        let file = fs::File::open(source_path).with_context(|| {
            format!(
                "Failed to open model bundle archive at {}",
                source_path.display()
            )
        })?;
        let mut archive = zip::ZipArchive::new(file)
            .with_context(|| format!("Failed to parse zip archive: {}", source_path.display()))?;

        for i in 0..archive.len() {
            let mut item = archive.by_index(i)?;
            let raw_name = item.name().to_string();

            // Prevent zip slip: reject directory traversal and absolute paths
            if raw_name.contains("..") || raw_name.starts_with('/') || raw_name.starts_with('\\') {
                continue;
            }

            let rel_path = crate::model_locator::ModelLocator::strip_models_prefix(&raw_name);
            let out_path = target_models_dir.join(rel_path);

            if item.is_dir() {
                fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out_file = fs::File::create(&out_path)?;
                std::io::copy(&mut item, &mut out_file)?;
            }
        }
    } else if source_path.is_dir() {
        copy_model_dir_recursive(source_path, &target_models_dir)?;
    } else {
        anyhow::bail!(
            "Source path '{}' is neither a file nor a directory",
            source_path.display()
        );
    }

    // Invalidate in-memory caches so new models are immediately recognized
    crate::model_locator::invalidate_model_caches();

    let locator = crate::model_locator::ModelLocator::new(Some(target_models_dir));
    Ok(locator.doctor_report())
}

fn copy_model_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let target = dst.join(&name);

        if path.is_dir() {
            fs::create_dir_all(&target)?;
            copy_model_dir_recursive(&path, &target)?;
        } else if path.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(not(feature = "online-provisioning"))]
pub fn download_embedding_models() -> Result<()> {
    anyhow::bail!(
        "Online model downloading is disabled in default offline builds (ADR 0001, ADR 0017).\n\
         Please use 'pdfmd-lite --install-models <dir|zip>' to import models offline, \n\
         or compile with '--features online-provisioning'."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};

    #[test]
    fn test_convert_vault_png_to_jpeg_in_place() {
        let temp_dir = tempfile::tempdir().unwrap();
        let vault = temp_dir.path();

        let img_dir = vault.join("notes_images");
        fs::create_dir_all(&img_dir).unwrap();
        let png_path = img_dir.join("figure1.png");

        let dummy = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
        dummy
            .save_with_format(&png_path, image::ImageFormat::Png)
            .unwrap();
        assert!(png_path.exists());

        let md_path = vault.join("Note.md");
        let initial_md =
            "# Title\n\n![Figure 1](notes_images/figure1.png)\n\n![[notes_images/figure1.png]]\n";
        fs::write(&md_path, initial_md).unwrap();

        let summary = convert_vault_png_to_jpeg(vault, 80, false).unwrap();
        assert_eq!(summary.images_scanned, 1);
        assert_eq!(summary.images_converted, 1);
        assert_eq!(summary.notes_updated, 1);
        assert_eq!(summary.links_updated, 2);

        assert!(!png_path.exists());
        let jpg_path = img_dir.join("figure1.jpg");
        assert!(jpg_path.exists());
        assert!(fs::metadata(&jpg_path).unwrap().len() > 0);

        let updated_md = fs::read_to_string(&md_path).unwrap();
        assert!(updated_md.contains("![Figure 1](notes_images/figure1.jpg)"));
        assert!(updated_md.contains("![[notes_images/figure1.jpg]]"));
        assert!(!updated_md.contains("figure1.png"));
    }

    #[test]
    fn test_format_notes_scoping_and_backup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let vault = temp_dir.path();

        // 1. Foreign note without generator marker: should be untouched
        let foreign_md = vault.join("Foreign.md");
        let foreign_content = "# My Custom Notes\n\n```markdown\n# Nested block\n```\n";
        fs::write(&foreign_md, foreign_content).unwrap();

        // 2. pdfmd-lite note with generator marker: should be formatted and backed up
        let pdfmd_note = vault.join("PdfmdNote.md");
        let pdfmd_content = "---\ngenerator: \"pdfmd-lite\"\npdfmd_schema: 2\n---\n\n```markdown\n# Spurious block\n```\n";
        fs::write(&pdfmd_note, pdfmd_content).unwrap();

        let summary = format_notes_in_path(vault, None, false).unwrap();
        assert_eq!(summary.files_scanned, 2);
        assert_eq!(summary.files_modified, 1);

        // Foreign note unchanged
        assert_eq!(fs::read_to_string(&foreign_md).unwrap(), foreign_content);

        // pdfmd note modified
        let modified = fs::read_to_string(&pdfmd_note).unwrap();
        assert!(!modified.contains("```markdown"));

        // Backup directory created
        let backup_dir = vault.join(".pdfmd-backup");
        assert!(backup_dir.exists(), "Backup directory should exist");
    }
}
