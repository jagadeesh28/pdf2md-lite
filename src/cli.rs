//! Command-line interface parser and handler for pdfmd-lite.

use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::config::AppConfig;
use crate::figure::{ImageFormat, ImageOptions};

pub fn print_usage() {
    eprintln!(
        r#"pdfmd-lite: Autonomous offline PDF to Obsidian Markdown converter

USAGE:
    # Single PDF conversion:
    pdfmd-lite <input.pdf> <output.md> [begin-page] [end-page]
    pdfmd-lite <input.pdf> <output-dir> [begin-page] [end-page]

    # Batch directory conversion:
    pdfmd-lite <input-dir> <output-dir> [begin-page] [end-page]

    # Autonomous directory watch mode:
    pdfmd-lite --watch <input-dir> <output-dir> [--embed] [--resume]
    pdfmd-lite <input-dir> <output-dir> --watch

    # Note formatting & post-processing for existing Markdown notes:
    pdfmd-lite --format-notes <file-or-dir> [--dry-run] [-o <output.md>]
    pdfmd-lite --fix-latex <file-or-dir> [-o <output.md>]

    # In-place PNG to JPEG conversion for Obsidian vaults:
    pdfmd-lite --convert-images-to-jpeg <vault-or-dir> [--image-quality <1-100>] [--dry-run]
    pdfmd-lite --png-to-jpeg <vault-or-dir> [--image-quality <1-100>] [--dry-run]

    # In-process semantic embeddings & Obsidian RAG sidecars:
    pdfmd-lite <input> <output> --embed
    pdfmd-lite --embed <vault-or-dir> [--force]
    pdfmd-lite --download-models

OPTIONS:
    -i, --input <PATH>          Input PDF file or directory
    -o, --output <PATH>         Output Markdown file or destination directory
    --embed, --embeddings       Generate semantic vector sidecars (<slug>.embeddings.jsonl)
                                or retroactively embed an existing Markdown vault
    --download-models           Download official INT8 all-MiniLM-L6-v2 ONNX model (~22MB)
                                and tokenizer into models/embeddings/
    --force                     Force re-embedding of existing Markdown notes
    --convert-images-to-jpeg, --png-to-jpeg <PATH>
                                Convert all PNG images in vault/folder to JPEG in place
                                and rewrite all Obsidian wiki-links (![[...png]] -> ![[...jpg]])
    --image-format <FMT>        Extracted image format: jpeg (default), png, webp
    --image-quality <1-100>     Image compression quality for JPEG/WebP (default: 80)
    --embed-images-base64       Inline extracted figures directly as base64 data URIs
                                inside Markdown without creating an images folder
    --save-table-images         Save companion crop images for tables (default: false)
    --no-table-images           Skip saving companion crop images for tables
    --format-notes, --fix-notes, --clean-notes <PATH>
                                Format Markdown note(s): unwrap spurious codeblocks,
                                purge extraction banners, normalize LaTeX math/tables,
                                convert figure links to Obsidian wiki-links (![[...]]),
                                and synthesize missing YAML frontmatter
    -w, --watch                 Watch directory for new/modified PDFs and automatically convert
    --dry-run                   Preview conversion plans, page counts, memory budgets, and ETA
                                without writing files to disk (works for batch and single PDF)
    -f, --fix-latex <PATH>      Fix LaTeX formatting, delimiters, and syntax in Markdown file(s)
    --fix-math <PATH>           Alias for --fix-latex
    -p, --pages <BEGIN-END>     Page range (1-based, e.g. 1-10)
    -j, --concurrency <N>       Number of concurrent worker threads (default: hardware-auto)
    -d, --max-depth <DEPTH>     Max directory search depth for batch mode (default: 3)
    -r, --resume                Skip already converted documents in batch mode
    -c, --config <PATH>         Path to custom configuration file (default: pdfmd-lite.toml)
    -q, --quiet, --silent       Silent operation (suppress verbose per-page/document logs)
    --english, -en              Pure English/Latin OCR (rec.onnx, default, zero foreign character hallucinations)
    --telugu, -te               Telugu OCR (rec_world.onnx)
    -l, --lang, --language <L>  Explicit language/script: english, telugu, auto
    -h, --help                  Print this help message

FEATURES:
    - 100% Offline execution without external LLM dependencies (no Qwen or Gemini).
    - In-process Indra OCR and YOLOv8n DocLayNet layout engine.
    - Geometric Table Grid extraction with Obsidian callouts.
    - Automatic LaTeX math delimiter normalization ($..$ and $$..$$).
    - Machine resource-aware dynamic concurrency governor.
    - Obsidian page indexing and comments (<!-- Page N -->).
"#
    );
}

fn collect_md_files(dir: &Path, collected: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_md_files(&p, collected);
        } else if p.is_file() && p.extension().is_some_and(|e| e == "md") {
            collected.push(p);
        }
    }
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
) -> anyhow::Result<FormatNotesSummary> {
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
        } else if let Some(out_p) = output
            && !dry_run
        {
            if let Some(parent) = out_p.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(out_p, fixed)?;
        }
    } else if path.is_dir() {
        let mut md_files = Vec::new();
        collect_md_files(path, &mut md_files);
        summary.files_scanned = md_files.len();

        for p in md_files {
            let content = match fs::read_to_string(&p) {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("Warning: could not read {}: {err}", p.display());
                    continue;
                }
            };
            let (fixed, report) = crate::markdown::format_note_document(&content, Some(&p));

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

                if !dry_run && let Err(err) = fs::write(&p, fixed) {
                    eprintln!("Warning: could not write {}: {err}", p.display());
                }
            }
        }
    } else {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    Ok(summary)
}

/// Normalizes and repairs LaTeX formatting, delimiters, syntax, and table math across a single Markdown file or directory.
pub fn fix_latex_in_path(path: &Path, output: Option<&Path>) -> anyhow::Result<usize> {
    let summary = format_notes_in_path(path, output, false)?;
    Ok(summary.files_modified)
}

/// Backwards compatibility alias for `fix_latex_in_path`.
pub fn fix_math_in_path(path: &Path) -> anyhow::Result<usize> {
    fix_latex_in_path(path, None)
}

/// Summary statistics for vault PNG to JPEG conversion operations.
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

fn collect_files_with_ext(dir: &Path, ext: &str, collected: &mut Vec<PathBuf>) {
    if dir.is_file() {
        if dir.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            collected.push(dir.to_path_buf());
        }
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files_with_ext(&p, ext, collected);
        } else if p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            collected.push(p);
        }
    }
}

/// Modifies all available PNG files in an Obsidian vault/folder into JPEG in place,
/// converting RGBA with clean white background blending and rewriting all Obsidian
/// wiki-links (`![[...png]]`) and standard markdown links (`![alt](...png)`).
pub fn convert_vault_png_to_jpeg(
    vault_path: &Path,
    quality: u8,
    dry_run: bool,
) -> anyhow::Result<ConvertImagesSummary> {
    let mut summary = ConvertImagesSummary::default();

    if !vault_path.exists() {
        anyhow::bail!("Path does not exist: {}", vault_path.display());
    }

    // 1. Scan and convert all PNG images
    let mut png_files = Vec::new();
    collect_files_with_ext(vault_path, "png", &mut png_files);
    summary.images_scanned = png_files.len();

    for png_path in &png_files {
        let orig_size = fs::metadata(png_path).map(|m| m.len()).unwrap_or(0);
        summary.orig_bytes += orig_size;

        let jpg_path = png_path.with_extension("jpg");

        if dry_run {
            summary.images_converted += 1;
            summary.new_bytes += orig_size / 4;
            continue;
        }

        let img = match image::open(png_path) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Warning: could not open image {}: {e}", png_path.display());
                summary.new_bytes += orig_size;
                continue;
            }
        };

        match crate::figure::save_image_with_format(
            &img,
            &jpg_path,
            crate::figure::ImageFormat::Jpeg,
            quality,
        ) {
            Ok(()) => {
                let new_size = fs::metadata(&jpg_path).map(|m| m.len()).unwrap_or(0);
                summary.new_bytes += new_size;
                summary.images_converted += 1;

                // Safely remove original PNG now that JPG has been created
                let _ = fs::remove_file(png_path);
            }
            Err(e) => {
                eprintln!("Warning: failed to convert {}: {e}", png_path.display());
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
                eprintln!("Warning: could not read {}: {e}", md_path.display());
                continue;
            }
        };

        let (updated, links_changed) = crate::markdown::replace_png_links_with_jpg(&content);
        if links_changed > 0 {
            summary.notes_updated += 1;
            summary.links_updated += links_changed;

            if !dry_run && let Err(e) = fs::write(md_path, updated) {
                eprintln!("Warning: could not write {}: {e}", md_path.display());
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
pub fn embed_vault_in_path(
    path: &Path,
    force: bool,
    quiet: bool,
) -> anyhow::Result<EmbedVaultSummary> {
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
                eprintln!("Warning: Failed to embed {}: {e}", md_path.display());
            }
        }
    }

    Ok(summary)
}

/// Provisions official INT8 all-MiniLM-L6-v2 ONNX model and tokenizer.json into models/embeddings/.
pub fn download_embedding_models_cli() -> anyhow::Result<()> {
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

    println!("\nEmbedding model provisioning complete!");
    Ok(())
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        process::exit(if args.is_empty() { 1 } else { 0 });
    }

    let format_notes_mode = args.iter().any(|a| {
        a == "--format-notes"
            || a == "--fix-notes"
            || a == "--clean-notes"
            || a == "--format"
            || a == "--fix-latex"
            || a == "--fix-latex-formatting"
            || a == "--fix-math"
            || a == "-f"
    });
    let convert_images_mode = args.iter().any(|a| {
        a == "--convert-images-to-jpeg"
            || a == "--png-to-jpeg"
            || a == "--convert-images"
            || a == "--convert-to-jpeg"
    });
    let dry_run = args.iter().any(|a| a == "--dry-run" || a == "-n");
    let mut watch_mode = args.iter().any(|a| a == "--watch" || a == "-w");

    let download_models_mode = args
        .iter()
        .any(|a| a == "--download-models" || a == "--fetch-models");
    if download_models_mode {
        if let Err(e) = download_embedding_models_cli() {
            eprintln!("Error provisioning models: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    if convert_images_mode {
        let mut target_path: Option<PathBuf> = None;
        let mut quality: u8 = 80;
        let mut idx = 0;

        while idx < args.len() {
            let a = &args[idx];
            if a == "--convert-images-to-jpeg"
                || a == "--png-to-jpeg"
                || a == "--convert-images"
                || a == "--convert-to-jpeg"
            {
                if idx + 1 < args.len() && !args[idx + 1].starts_with('-') && target_path.is_none()
                {
                    target_path = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                    continue;
                }
            } else if a == "--image-quality" || a == "--quality" {
                if idx + 1 < args.len() {
                    if let Ok(q) = args[idx + 1].parse::<u8>() {
                        quality = q;
                    }
                    idx += 2;
                    continue;
                }
            } else if a == "-i" || a == "--input" {
                if idx + 1 < args.len() {
                    target_path = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                    continue;
                }
            } else if !a.starts_with('-') && target_path.is_none() {
                target_path = Some(PathBuf::from(a));
            }
            idx += 1;
        }

        let target = match target_path {
            Some(p) => p,
            None => {
                eprintln!(
                    "Error: Flag '--convert-images-to-jpeg' requires a vault or directory path"
                );
                print_usage();
                process::exit(1);
            }
        };

        match convert_vault_png_to_jpeg(&target, quality, dry_run) {
            Ok(summary) => {
                let action = if dry_run {
                    "Previewed (dry-run)"
                } else {
                    "Successfully converted"
                };
                let pct_saved = if summary.orig_bytes > 0 && summary.new_bytes < summary.orig_bytes
                {
                    format!(
                        "{:.1}% reduction",
                        (summary.orig_bytes - summary.new_bytes) as f64 / summary.orig_bytes as f64
                            * 100.0
                    )
                } else {
                    "0.0% reduction".to_string()
                };
                println!("{action} PNG images to JPEG in {}:", target.display());
                println!("  - PNG images scanned:        {}", summary.images_scanned);
                println!(
                    "  - PNG images converted:      {}",
                    summary.images_converted
                );
                println!("  - Markdown notes scanned:    {}", summary.notes_scanned);
                println!("  - Notes with updated links:  {}", summary.notes_updated);
                println!("  - Total image links changed: {}", summary.links_updated);
                println!(
                    "  - Original image disk size:  {:.2} MB",
                    summary.orig_bytes as f64 / (1024.0 * 1024.0)
                );
                println!(
                    "  - New image disk size:       {:.2} MB ({pct_saved})",
                    summary.new_bytes as f64 / (1024.0 * 1024.0)
                );
                if dry_run {
                    println!("\n(Dry-run mode: no changes were written to disk)");
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error converting images: {e:#}");
                process::exit(1);
            }
        }
    }

    if format_notes_mode {
        let mut target_path: Option<PathBuf> = None;
        let mut out_path: Option<PathBuf> = None;
        let mut idx = 0;

        while idx < args.len() {
            let a = &args[idx];
            if a == "--format-notes"
                || a == "--fix-notes"
                || a == "--clean-notes"
                || a == "--format"
                || a == "--fix-latex"
                || a == "--fix-latex-formatting"
                || a == "--fix-math"
                || a == "-f"
            {
                if idx + 1 < args.len() && !args[idx + 1].starts_with('-') && target_path.is_none()
                {
                    target_path = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                    continue;
                }
            } else if a == "--dry-run" || a == "-n" {
                idx += 1;
                continue;
            } else if a == "-i" || a == "--input" {
                if idx + 1 < args.len() {
                    target_path = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                    continue;
                }
            } else if a == "-o" || a == "--output" {
                if idx + 1 < args.len() {
                    out_path = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                    continue;
                }
            } else if !a.starts_with('-') {
                if target_path.is_none() {
                    target_path = Some(PathBuf::from(a));
                } else if out_path.is_none() {
                    out_path = Some(PathBuf::from(a));
                }
            }
            idx += 1;
        }

        let target = match target_path {
            Some(p) => p,
            None => {
                eprintln!(
                    "Error: Flag '--format-notes' requires a Markdown file or directory path"
                );
                print_usage();
                process::exit(1);
            }
        };

        match format_notes_in_path(&target, out_path.as_deref(), dry_run) {
            Ok(summary) => {
                let action = if dry_run {
                    "Previewed (dry-run)"
                } else {
                    "Successfully formatted"
                };
                println!("{action} note(s) in {}:", target.display());
                println!(
                    "  - Files scanned:                  {}",
                    summary.files_scanned
                );
                println!(
                    "  - Files modified:                 {}",
                    summary.files_modified
                );
                println!(
                    "  - Spurious codeblocks unwrapped:  {}",
                    summary.codeblocks_unwrapped
                );
                println!(
                    "  - Extraction banners purged:      {}",
                    summary.banners_purged
                );
                println!(
                    "  - LaTeX equations / tables fixed: {}",
                    summary.latex_repaired
                );
                println!(
                    "  - Frontmatter blocks synthesized: {}",
                    summary.frontmatter_added
                );
                println!(
                    "  - Metadata properties backfilled: {}",
                    summary.frontmatter_fields_backfilled
                );
                println!(
                    "  - Figure links converted:         {}",
                    summary.figure_links_converted
                );
                if dry_run {
                    println!("\n(Dry-run mode: no changes were written to disk)");
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error formatting notes: {e:#}");
                process::exit(1);
            }
        }
    }

    // Parse options
    let mut input_path: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut begin_page: Option<usize> = None;
    let mut end_page: Option<usize> = None;
    let mut concurrency: Option<usize> = None;
    let mut max_depth: Option<usize> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut language: Option<crate::backends::indra::IndraScript> = None;
    let mut image_format: Option<ImageFormat> = None;
    let mut image_quality: Option<u8> = None;
    let mut save_table_images: Option<bool> = None;
    let mut embed_images_base64 = false;
    let mut quiet = false;
    let mut resume = false;
    let mut embed = false;
    let mut force = false;

    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-i" || arg == "--input" {
            if i + 1 < args.len() {
                input_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg == "-o" || arg == "--output" {
            if i + 1 < args.len() {
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg == "-j" || arg == "--concurrency" {
            if i + 1 < args.len() {
                concurrency = args[i + 1].parse().ok();
                i += 2;
                continue;
            }
        } else if arg == "-d" || arg == "--max-depth" {
            if i + 1 < args.len() {
                max_depth = args[i + 1].parse().ok();
                i += 2;
                continue;
            }
        } else if arg == "-c" || arg == "--config" {
            if i + 1 < args.len() {
                config_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg == "-p" || arg == "--pages" {
            if i + 1 < args.len() {
                let range = &args[i + 1];
                if let Some((s, e)) = range.split_once('-') {
                    begin_page = s.trim().parse().ok();
                    end_page = e.trim().parse().ok();
                }
                i += 2;
                continue;
            }
        } else if arg == "--image-format" || arg == "--img-format" {
            if i + 1 < args.len() {
                image_format = ImageFormat::from_str_loose(&args[i + 1]);
                i += 2;
                continue;
            }
        } else if arg == "--image-quality" || arg == "--quality" {
            if i + 1 < args.len() {
                image_quality = args[i + 1].parse().ok();
                i += 2;
                continue;
            }
        } else if arg == "--save-table-images" {
            save_table_images = Some(true);
        } else if arg == "--no-table-images" || arg == "--skip-table-images" {
            save_table_images = Some(false);
        } else if arg == "--embed-images-base64" || arg == "--base64-images" {
            embed_images_base64 = true;
        } else if arg == "-w" || arg == "--watch" {
            watch_mode = true;
        } else if arg == "-q" || arg == "--quiet" || arg == "--silent" {
            quiet = true;
        } else if arg == "-r" || arg == "--resume" {
            resume = true;
        } else if arg == "--embed" || arg == "--embeddings" {
            embed = true;
        } else if arg == "--force" {
            force = true;
        } else if arg == "--english" || arg == "-en" {
            language = Some(crate::backends::indra::IndraScript::English);
        } else if arg == "--telugu" || arg == "-te" {
            language = Some(crate::backends::indra::IndraScript::Telugu);
        } else if arg == "--auto" {
            language = Some(crate::backends::indra::IndraScript::Auto);
        } else if arg == "-l" || arg == "--lang" || arg == "--language" {
            if i + 1 < args.len() {
                match args[i + 1].parse::<crate::backends::indra::IndraScript>() {
                    Ok(lang) => language = Some(lang),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        process::exit(1);
                    }
                }
                i += 2;
                continue;
            }
        } else if !arg.starts_with('-') {
            positional.push(arg.clone());
        }
        i += 1;
    }

    // Fill in from positionals if not set via flags
    if input_path.is_none() && !positional.is_empty() {
        input_path = Some(PathBuf::from(&positional[0]));
    }
    if output_path.is_none() && positional.len() > 1 {
        output_path = Some(PathBuf::from(&positional[1]));
    }
    if begin_page.is_none() && positional.len() > 2 {
        begin_page = positional[2].parse().ok();
    }
    if end_page.is_none() && positional.len() > 3 {
        end_page = positional[3].parse().ok();
    }

    // Standalone vault embedding mode: `pdfmd-lite --embed <vault-dir-or-file> [--force]`
    if embed && output_path.is_none() && input_path.is_some() {
        let target = input_path.clone().unwrap();
        if target.is_file() || target.is_dir() {
            println!(
                "Retroactively embedding vault notes in '{}'...",
                target.display()
            );
            match embed_vault_in_path(&target, force, quiet) {
                Ok(summary) => {
                    println!("\nVault embedding complete:");
                    println!("  - Markdown notes scanned:     {}", summary.notes_scanned);
                    println!("  - Markdown notes embedded:    {}", summary.notes_embedded);
                    println!("  - Notes skipped (up to date): {}", summary.notes_skipped);
                    println!(
                        "  - Total chunks indexed:       {}",
                        summary.chunks_embedded
                    );
                    process::exit(0);
                }
                Err(e) => {
                    eprintln!("Error embedding vault: {e:#}");
                    process::exit(1);
                }
            }
        }
    }

    let input = match input_path {
        Some(p) => p,
        None => {
            eprintln!("Error: Missing required input file or directory.");
            print_usage();
            process::exit(1);
        }
    };

    let output = match output_path {
        Some(p) => p,
        None => {
            eprintln!("Error: Missing required output path or directory.");
            print_usage();
            process::exit(1);
        }
    };

    // Load config file if present
    let config = AppConfig::load(config_path.as_deref()).unwrap_or_default();
    let final_concurrency = concurrency.or(config.concurrency);
    let final_max_depth = max_depth.or(config.max_depth).unwrap_or(3);
    let final_script = language
        .or_else(|| {
            config
                .language
                .as_deref()
                .and_then(|s| s.parse::<crate::backends::indra::IndraScript>().ok())
        })
        .unwrap_or(crate::backends::indra::IndraScript::English);

    let final_image_format = image_format
        .or_else(|| {
            config
                .image_format
                .as_deref()
                .and_then(ImageFormat::from_str_loose)
        })
        .unwrap_or(ImageFormat::Jpeg);

    let final_image_quality = image_quality.or(config.image_quality).unwrap_or(80);

    let final_save_table_images = save_table_images
        .or(config.save_table_images)
        .unwrap_or(false);

    let final_embed_base64 = embed_images_base64 || config.embed_images_base64.unwrap_or(false);

    let image_opts = ImageOptions::new(
        final_image_format,
        final_image_quality,
        final_save_table_images,
    )
    .with_embed_base64(final_embed_base64);

    if watch_mode {
        let watch_opts = crate::watch::WatchOptions {
            debounce_delay: std::time::Duration::from_millis(800),
            poll_interval: std::time::Duration::from_millis(250),
            concurrency: final_concurrency,
            script: final_script,
            image_opts,
            embed,
            quiet,
        };
        if let Err(e) = crate::watch::watch_directory(&input, &output, watch_opts) {
            eprintln!("Watch mode error: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    if dry_run {
        if input.is_dir() {
            let page_range = match (begin_page, end_page) {
                (Some(s), Some(e)) => Some((s, e)),
                _ => None,
            };
            match crate::batch::dry_run_pdf_directory(
                &input,
                &output,
                page_range,
                final_max_depth,
                final_concurrency,
                final_script,
                resume,
                image_opts,
                embed,
            ) {
                Ok(report) => {
                    report.print();
                    process::exit(0);
                }
                Err(e) => {
                    eprintln!("Dry-run error: {e:#}");
                    process::exit(1);
                }
            }
        } else {
            let begin = begin_page.unwrap_or(1);
            let end =
                end_page.unwrap_or_else(|| crate::pdf::get_pdf_page_count(&input).unwrap_or(begin));
            match crate::batch::dry_run_single_pdf(
                &input,
                &output,
                begin,
                end,
                final_concurrency,
                image_opts,
                embed,
            ) {
                Ok(report) => {
                    report.print();
                    process::exit(0);
                }
                Err(e) => {
                    eprintln!("Dry-run error: {e:#}");
                    process::exit(1);
                }
            }
        }
    }

    if input.is_dir() {
        let page_range = match (begin_page, end_page) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };

        match crate::batch::process_pdf_directory_all(
            &input,
            &output,
            page_range,
            final_max_depth,
            final_concurrency,
            final_script,
            quiet,
            resume,
            image_opts,
            embed,
        ) {
            Ok(summary) => {
                if !quiet {
                    println!(
                        "\nBatch complete: {} processed ({} succeeded, {} failed)",
                        summary.total,
                        summary.succeeded,
                        summary.failed.len()
                    );
                }
                if !summary.failed.is_empty() {
                    for (f, err) in summary.failed {
                        eprintln!(" - {}: {err}", f.display());
                    }
                    process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("Batch processing error: {e:#}");
                process::exit(1);
            }
        }
    } else {
        let begin = begin_page.unwrap_or(1);
        let end =
            end_page.unwrap_or_else(|| crate::pdf::get_pdf_page_count(&input).unwrap_or(begin));

        // Determine target markdown output path
        let target_md = if output.is_dir() || output.extension().is_none() {
            let stem = input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("document");
            output.join(format!("{stem}.md"))
        } else {
            output
        };

        println!(
            "Converting single PDF (Language: {final_script}, Images: {}, Quality: {})...",
            final_image_format.extension(),
            final_image_quality
        );
        if let Err(err) = crate::pipeline::process_pdf_full(
            &input,
            &target_md,
            begin,
            end,
            final_concurrency,
            None,
            final_script,
            image_opts,
            embed,
        ) {
            eprintln!("Error converting PDF: {err:#}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};

    #[test]
    fn test_convert_vault_png_to_jpeg_in_place() {
        let temp_dir = tempfile::tempdir().unwrap();
        let vault = temp_dir.path();

        // Create dummy PNG image in vault/images/
        let img_dir = vault.join("notes_images");
        fs::create_dir_all(&img_dir).unwrap();
        let png_path = img_dir.join("figure1.png");

        let dummy = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
        dummy
            .save_with_format(&png_path, image::ImageFormat::Png)
            .unwrap();
        assert!(png_path.exists());

        // Create markdown file referencing the PNG
        let md_path = vault.join("Note.md");
        let initial_md =
            "# Title\n\n![Figure 1](notes_images/figure1.png)\n\n![[notes_images/figure1.png]]\n";
        fs::write(&md_path, initial_md).unwrap();

        // Run conversion
        let summary = convert_vault_png_to_jpeg(vault, 80, false).unwrap();
        assert_eq!(summary.images_scanned, 1);
        assert_eq!(summary.images_converted, 1);
        assert_eq!(summary.notes_updated, 1);
        assert_eq!(summary.links_updated, 2);

        // Verify PNG is gone and JPG exists
        assert!(!png_path.exists());
        let jpg_path = img_dir.join("figure1.jpg");
        assert!(jpg_path.exists());
        assert!(fs::metadata(&jpg_path).unwrap().len() > 0);

        // Verify Markdown file links were updated
        let updated_md = fs::read_to_string(&md_path).unwrap();
        assert!(updated_md.contains("![Figure 1](notes_images/figure1.jpg)"));
        assert!(updated_md.contains("![[notes_images/figure1.jpg]]"));
        assert!(!updated_md.contains("figure1.png"));
    }
}
