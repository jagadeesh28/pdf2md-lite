//! Command-line interface parser and handler for pdfmd-lite.

use std::path::PathBuf;
use std::process;

use pdfmd_lite::config::AppConfig;
use pdfmd_lite::figure::{ImageFormat, ImageOptions};
use pdfmd_lite::vault::{
    convert_vault_png_to_jpeg, download_embedding_models, embed_vault_in_path, format_notes_in_path,
};

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
    --serve [PORT]              Start local embedded Axum web server daemon (default: 127.0.0.1:3000)
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
    --log-file <PATH>           Path to optional structured log file
    --log-level <LEVEL>         Log level: trace, debug, info (default), warn, error
    -q, --quiet, --silent       Silent operation (suppress verbose per-page/document logs)
    --english, -en              Pure English/Latin OCR (rec.onnx, default, zero foreign character hallucinations)
    --telugu, -te               Telugu OCR (rec_world.onnx)
    -l, --lang, --language <L>  Explicit language/script: english, telugu, auto
    --dom-extraction <MODE>     DOM extraction strategy: auto (default), always, never
    --dom-mode <MODE>           Alias for --dom-extraction
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

    let serve_pos = args.iter().position(|a| a == "--serve" || a == "serve");
    if let Some(pos) = serve_pos {
        let port_or_addr = args
            .get(pos + 1)
            .filter(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_else(|| "127.0.0.1:3000".to_string());
        let bind_addr = if port_or_addr.contains(':') {
            port_or_addr
        } else {
            format!("127.0.0.1:{port_or_addr}")
        };
        let rt = tokio::runtime::Runtime::new().expect("Failed to initialize Tokio runtime");
        if let Err(e) = rt.block_on(pdfmd_lite_server::start_server(&bind_addr)) {
            eprintln!("Error starting web daemon: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    let download_models_mode = args
        .iter()
        .any(|a| a == "--download-models" || a == "--fetch-models");
    if download_models_mode {
        if let Err(e) = download_embedding_models() {
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
    let mut language: Option<pdfmd_lite::backends::indra::IndraScript> = None;
    let mut image_format: Option<ImageFormat> = None;
    let mut image_quality: Option<u8> = None;
    let mut save_table_images: Option<bool> = None;
    let mut embed_images_base64 = false;
    let mut quiet = false;
    let mut resume = false;
    let mut embed = false;
    let mut force = false;
    let mut log_file: Option<PathBuf> = None;
    let mut log_level: Option<String> = None;
    let mut dom_extraction: Option<pdfmd_lite::pdf::DomExtractionMode> = None;

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
        } else if arg == "--log-file" {
            if i + 1 < args.len() {
                log_file = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg == "--log-level" {
            if i + 1 < args.len() {
                log_level = Some(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg == "-r" || arg == "--resume" {
            resume = true;
        } else if arg == "--embed" || arg == "--embeddings" {
            embed = true;
        } else if arg == "--force" {
            force = true;
        } else if arg == "--english" || arg == "-en" {
            language = Some(pdfmd_lite::backends::indra::IndraScript::English);
        } else if arg == "--telugu" || arg == "-te" {
            language = Some(pdfmd_lite::backends::indra::IndraScript::Telugu);
        } else if arg == "--auto" {
            language = Some(pdfmd_lite::backends::indra::IndraScript::Auto);
        } else if arg == "-l" || arg == "--lang" || arg == "--language" {
            if i + 1 < args.len() {
                match args[i + 1].parse::<pdfmd_lite::backends::indra::IndraScript>() {
                    Ok(lang) => language = Some(lang),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        process::exit(1);
                    }
                }
                i += 2;
                continue;
            }
        } else if arg == "--dom-extraction" || arg == "--dom-mode" {
            if i + 1 < args.len() {
                dom_extraction = pdfmd_lite::pdf::DomExtractionMode::from_str_loose(&args[i + 1]);
                if dom_extraction.is_none() {
                    eprintln!(
                        "Error: Invalid --dom-extraction mode '{}'. Expected 'auto', 'always', or 'never'.",
                        args[i + 1]
                    );
                    process::exit(1);
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

    // Load config file if present
    let config = AppConfig::load(config_path.as_deref()).unwrap_or_default();
    let final_log_file = log_file.or(config.log_file.clone());
    let final_log_level = log_level.or(config.log_level.clone());

    if let Err(e) = pdfmd_lite::telemetry::init_telemetry(
        final_log_level.as_deref(),
        final_log_file.as_deref(),
        quiet,
    ) {
        eprintln!("Warning: Failed to initialize structured logging: {e:#}");
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

    let final_concurrency = concurrency.or(config.concurrency);
    let final_max_depth = max_depth.or(config.max_depth).unwrap_or(3);
    let final_script = language
        .or_else(|| {
            config
                .language
                .as_deref()
                .and_then(|s| s.parse::<pdfmd_lite::backends::indra::IndraScript>().ok())
        })
        .unwrap_or(pdfmd_lite::backends::indra::IndraScript::English);

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

    let final_dom_mode = dom_extraction
        .or_else(|| {
            config
                .dom_extraction
                .as_deref()
                .and_then(pdfmd_lite::pdf::DomExtractionMode::from_str_loose)
        })
        .unwrap_or(pdfmd_lite::pdf::DomExtractionMode::Auto);

    let image_opts = ImageOptions::new(
        final_image_format,
        final_image_quality,
        final_save_table_images,
    )
    .with_embed_base64(final_embed_base64)
    .with_dom_mode(final_dom_mode);

    if watch_mode {
        let watch_opts = pdfmd_lite::watch::WatchOptions {
            debounce_delay: std::time::Duration::from_millis(800),
            poll_interval: std::time::Duration::from_millis(250),
            concurrency: final_concurrency,
            script: final_script,
            image_opts,
            embed,
            quiet,
        };
        if let Err(e) = pdfmd_lite::watch::watch_directory(&input, &output, watch_opts) {
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
            match pdfmd_lite::batch::dry_run_pdf_directory(
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
            let end = end_page
                .unwrap_or_else(|| pdfmd_lite::pdf::get_pdf_page_count(&input).unwrap_or(begin));
            match pdfmd_lite::batch::dry_run_single_pdf(
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

        match pdfmd_lite::batch::process_pdf_directory_all(
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
        let end = end_page
            .unwrap_or_else(|| pdfmd_lite::pdf::get_pdf_page_count(&input).unwrap_or(begin));

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
        if let Err(err) = pdfmd_lite::pipeline::process_pdf_full(
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
