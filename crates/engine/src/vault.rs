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
        } else if p.is_file() && p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext)) {
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
    let mut summary = FormatNotesSummary::default();

    if path.is_file() {
        summary.files_scanned = 1;
        let content = fs::read_to_string(path)?;
        let (fixed, report) = crate::markdown::format_note_document(&content, Some(path));

        if report.is_changed() {
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

            if !dry_run {
                if let Some(out_p) = output {
                    if let Some(parent) = out_p.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(out_p, fixed)?;
                } else {
                    fs::write(path, fixed)?;
                }
            }
        }
    } else if path.is_dir() {
        let mut md_files = Vec::new();
        collect_md_files(path, &mut md_files);
        summary.files_scanned = md_files.len();

        for file in md_files {
            let content = match fs::read_to_string(&file) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Could not read {}: {e}", file.display());
                    continue;
                }
            };
            let (fixed, report) = crate::markdown::format_note_document(&content, Some(&file));
            if report.is_changed() {
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

                if !dry_run {
                    let dest = if let Some(out_dir) = output {
                        if let Ok(rel) = file.strip_prefix(path) {
                            out_dir.join(rel)
                        } else {
                            out_dir.join(file.file_name().unwrap())
                        }
                    } else {
                        file.clone()
                    };

                    if let Some(parent) = dest.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Err(e) = fs::write(&dest, fixed) {
                        tracing::warn!("Could not write {}: {e}", dest.display());
                    }
                }
            }
        }
    }

    Ok(summary)
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

/// Converts existing PNG figures in an Obsidian vault to compressed JPEG images in-place.
pub fn convert_vault_png_to_jpeg(
    vault_path: &Path,
    quality: u8,
    dry_run: bool,
) -> Result<ConvertImagesSummary> {
    let mut summary = ConvertImagesSummary::default();

    // 1. Scan and convert all PNG image files
    let mut png_files = Vec::new();
    collect_files_with_ext(vault_path, "png", &mut png_files);
    summary.images_scanned = png_files.len();

    for png_path in &png_files {
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
    summary.notes_scanned = md_files.len();

    for md_path in &md_files {
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
                    println!(
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
pub fn download_embedding_models() -> Result<()> {
    let target_dir = Path::new("models/embeddings");
    fs::create_dir_all(target_dir)?;

    let model_target = target_dir.join("model.onnx");
    let tokenizer_target = target_dir.join("tokenizer.json");

    println!(
        "Checking embedding model assets in '{}'...",
        target_dir.display()
    );

    let model_url =
        "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model_quantized.onnx";
    let tokenizer_url =
        "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/raw/main/tokenizer.json";

    if !model_target.exists() {
        println!("Downloading quantized all-MiniLM-L6-v2 ONNX model (~22 MB)...");
        let status = std::process::Command::new("curl.exe")
            .args(["-L", "-o", model_target.to_str().unwrap(), model_url])
            .status();

        match status {
            Ok(s) if s.success() => println!("Downloaded {}", model_target.display()),
            _ => {
                anyhow::bail!(
                    "Failed to download model via curl.\nPlease manually download:\n  {}\nand save to:\n  {}",
                    model_url,
                    model_target.display()
                );
            }
        }
    } else {
        println!("Model already exists: {}", model_target.display());
    }

    if !tokenizer_target.exists() {
        println!("Downloading tokenizer.json...");
        let status = std::process::Command::new("curl.exe")
            .args([
                "-L",
                "-o",
                tokenizer_target.to_str().unwrap(),
                tokenizer_url,
            ])
            .status();

        match status {
            Ok(s) if s.success() => println!("Downloaded {}", tokenizer_target.display()),
            _ => {
                anyhow::bail!(
                    "Failed to download tokenizer via curl.\nPlease manually download:\n  {}\nand save to:\n  {}",
                    tokenizer_url,
                    tokenizer_target.display()
                );
            }
        }
    } else {
        println!("Tokenizer already exists: {}", tokenizer_target.display());
    }

    let metadata_dir = Path::new("models/metadata");
    fs::create_dir_all(metadata_dir)?;
    let gliner_target = metadata_dir.join("gliner_small.onnx");
    let gliner_url =
        "https://huggingface.co/onnx-community/gliner_small-v2.1/resolve/main/onnx/model_quantized.onnx";

    if !gliner_target.exists() {
        println!("Downloading GLiNER small metadata extractor ONNX model (~183 MB)...");
        let status = std::process::Command::new("curl.exe")
            .args(["-L", "-o", gliner_target.to_str().unwrap(), gliner_url])
            .status();

        match status {
            Ok(s) if s.success() => println!("Downloaded {}", gliner_target.display()),
            _ => {
                tracing::warn!(
                    "Failed to download GLiNER model via curl. Manual URL: {gliner_url}"
                );
            }
        }
    } else {
        println!("GLiNER model already exists: {}", gliner_target.display());
    }

    println!("\nModel provisioning complete!");
    Ok(())
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
}
