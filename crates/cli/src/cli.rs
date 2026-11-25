//! Command-line interface parser and handler for pdfmd-lite.

use clap::{CommandFactory, Parser};
use std::path::{Path, PathBuf};
use std::process;

use pdfmd_lite::ModelLocator;
use pdfmd_lite::config::AppConfig;
use pdfmd_lite::figure::{ImageFormat, ImageOptions};
use pdfmd_lite::vault::{
    convert_vault_png_to_jpeg, download_embedding_models, embed_vault_in_path, format_notes_in_path,
};

#[derive(Parser, Debug, Default)]
#[command(
    name = "pdfmd-lite",
    author = "pdfmd-lite contributors",
    version,
    about = "Autonomous offline PDF to Obsidian Markdown converter with native LaTeX & tables",
    after_help = "FEATURES:\n  - 100% Offline execution without external LLM dependencies.\n  - In-process Indra OCR and YOLOv8n DocLayNet layout engine.\n  - Geometric Table Grid extraction with Obsidian callouts.\n  - Automatic LaTeX math delimiter normalization ($..$ and $$..$$).\n  - Machine resource-aware dynamic concurrency governor.\n  - Obsidian page indexing and comments (<!-- Page N -->)."
)]
pub struct CliArgs {
    /// Input PDF file or directory
    #[arg(short = 'i', long = "input", value_name = "PATH")]
    pub input: Option<PathBuf>,

    /// Output Markdown file or destination directory
    #[arg(short = 'o', long = "output", value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Positional arguments: [input] [output] [begin-page] [end-page]
    #[arg(value_name = "ARG")]
    pub positionals: Vec<String>,

    /// Start local embedded Axum web server daemon (default: 127.0.0.1:3000)
    #[arg(
        long = "serve",
        num_args = 0..=1,
        default_missing_value = "127.0.0.1:3000",
        value_name = "PORT_OR_ADDR"
    )]
    pub serve: Option<String>,

    /// Check presence, resolution path, and SHA-256 for all offline neural models
    #[arg(long = "doctor")]
    pub doctor: bool,

    /// Download official INT8 all-MiniLM-L6-v2 ONNX model and tokenizer
    #[arg(long = "download-models", visible_alias = "fetch-models")]
    pub download_models: bool,

    /// Convert all PNG images in vault/folder to JPEG in place and rewrite links
    #[arg(
        long = "convert-images-to-jpeg",
        visible_aliases = ["png-to-jpeg", "convert-images", "convert-to-jpeg"],
        num_args = 0..=1,
        value_name = "PATH"
    )]
    pub convert_images: Option<String>,

    /// Format Markdown note(s): unwrap spurious codeblocks, purge banners, fix LaTeX
    #[arg(
        long = "format-notes",
        visible_aliases = ["fix-notes", "clean-notes", "format"],
        num_args = 0..=1,
        value_name = "PATH"
    )]
    pub format_notes: Option<String>,

    /// Fix LaTeX formatting, delimiters, and syntax in Markdown file(s)
    #[arg(
        short = 'f',
        long = "fix-latex",
        visible_aliases = ["fix-math", "fix-latex-formatting"],
        num_args = 0..=1,
        value_name = "PATH"
    )]
    pub fix_latex: Option<String>,

    /// Watch directory for new/modified PDFs and automatically convert
    #[arg(short = 'w', long = "watch")]
    pub watch: bool,

    /// Preview conversion plans, page counts, memory budgets, and ETA without writing files
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Generate semantic vector sidecars or retroactively embed an existing Markdown vault
    #[arg(short = 'e', long = "embed", visible_alias = "embeddings")]
    pub embed: bool,

    /// Force re-embedding of existing Markdown notes
    #[arg(long = "force")]
    pub force: bool,

    /// Page range (1-based, e.g. 1-10)
    #[arg(short = 'p', long = "pages", value_name = "BEGIN-END")]
    pub pages: Option<String>,

    /// Number of concurrent worker threads (default: hardware-auto)
    #[arg(short = 'j', long = "concurrency", value_name = "N")]
    pub concurrency: Option<usize>,

    /// Max directory search depth for batch mode (default: 3)
    #[arg(short = 'd', long = "max-depth", value_name = "DEPTH")]
    pub max_depth: Option<usize>,

    /// Skip already converted documents in batch mode
    #[arg(short = 'r', long = "resume")]
    pub resume: bool,

    /// Path to custom configuration file (default: pdfmd-lite.toml)
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Extracted image format: jpeg (default), png, webp
    #[arg(
        long = "image-format",
        visible_alias = "img-format",
        value_name = "FMT"
    )]
    pub image_format: Option<String>,

    /// Image compression quality for JPEG/WebP (1-100, default: 80)
    #[arg(
        long = "image-quality",
        visible_alias = "quality",
        value_name = "1-100"
    )]
    pub image_quality: Option<u8>,

    /// Inline extracted figures directly as base64 data URIs inside Markdown
    #[arg(long = "embed-images-base64", visible_alias = "base64-images")]
    pub embed_images_base64: bool,

    /// Save companion crop images for tables
    #[arg(long = "save-table-images")]
    pub save_table_images: bool,

    /// Skip saving companion crop images for tables
    #[arg(long = "no-table-images", visible_alias = "skip-table-images")]
    pub no_table_images: bool,

    /// Explicit language/script: english, telugu, auto
    #[arg(
        short = 'l',
        long = "lang",
        visible_alias = "language",
        value_name = "LANG"
    )]
    pub language: Option<String>,

    /// Pure English/Latin OCR (default)
    #[arg(long = "english", visible_alias = "en")]
    pub english: bool,

    /// Telugu OCR (rec_world.onnx)
    #[arg(long = "telugu", visible_alias = "te")]
    pub telugu: bool,

    /// Auto script selection
    #[arg(long = "auto")]
    pub auto: bool,

    /// DOM extraction strategy: auto (default), always, never
    #[arg(
        long = "dom-extraction",
        visible_alias = "dom-mode",
        value_name = "MODE"
    )]
    pub dom_extraction: Option<String>,

    /// Explicit path to directory containing neural model weights
    #[arg(long = "models-dir", value_name = "PATH")]
    pub models_dir: Option<PathBuf>,

    /// Path to optional structured log file
    #[arg(long = "log-file", value_name = "PATH")]
    pub log_file: Option<PathBuf>,

    /// Log level: trace, debug, info (default), warn, error
    #[arg(long = "log-level", value_name = "LEVEL")]
    pub log_level: Option<String>,

    /// Silent operation (suppress verbose logs)
    #[arg(short = 'q', long = "quiet", visible_alias = "silent")]
    pub quiet: bool,

    /// Allow web server daemon to bind to non-loopback addresses (remote access)
    #[arg(long = "allow-remote")]
    pub allow_remote: bool,

    /// Enable heuristic academic structure heading promotion (numbered sections and initials)
    #[arg(long = "academic-structure")]
    pub academic_structure: bool,

    /// Hardware device for neural model execution: auto (default), cpu, directml
    #[arg(long = "device", value_name = "DEVICE")]
    pub device: Option<String>,

    /// Allow degraded conversion when optional models or requested scripts are missing
    #[arg(long = "allow-degraded")]
    pub allow_degraded: bool,

    /// Install offline neural models from local directory or .zip bundle into AppData
    #[arg(long = "install-models", value_name = "PATH")]
    pub install_models: Option<PathBuf>,

    /// Emit diagnostic layout traces into the specified directory
    #[arg(long = "debug-layout", value_name = "DIR")]
    pub debug_layout: Option<PathBuf>,
}

pub fn run() {
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() <= 1 {
        let mut cmd = CliArgs::command();
        let _ = cmd.print_help();
        println!();
        process::exit(1);
    }

    // Normalize legacy subcommand aliases if present (e.g. `pdfmd-lite serve` -> `pdfmd-lite --serve`)
    let mut normalized_args = Vec::with_capacity(raw_args.len());
    for (idx, arg) in raw_args.iter().enumerate() {
        if idx == 1 && (arg == "serve" || arg == "doctor" || arg == "check") {
            normalized_args.push(format!("--{arg}"));
        } else {
            normalized_args.push(arg.clone());
        }
    }

    let args = match CliArgs::try_parse_from(&normalized_args) {
        Ok(a) => a,
        Err(e) => {
            e.exit();
        }
    };

    // 1. Web server daemon mode
    if let Some(ref port_or_addr) = args.serve {
        let bind_addr = if port_or_addr.contains(':') {
            port_or_addr.clone()
        } else {
            format!("127.0.0.1:{port_or_addr}")
        };
        let rt = tokio::runtime::Runtime::new().expect("Failed to initialize Tokio runtime");
        if let Err(e) = rt.block_on(pdfmd_lite_server::start_server(
            &bind_addr,
            args.allow_remote,
        )) {
            eprintln!("Error starting web daemon: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    // 2. Download models mode
    if args.download_models {
        if let Err(e) = download_embedding_models() {
            eprintln!("Error provisioning models: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    // 3. Install offline models mode
    if let Some(ref install_path) = args.install_models {
        println!(
            "Installing offline neural models from '{}'...",
            install_path.display()
        );
        match pdfmd_lite::vault::install_offline_models(install_path) {
            Ok(_) => {
                println!("Successfully imported model bundle into AppData models directory.");
                let ok = run_doctor(None);
                process::exit(if ok { 0 } else { 1 });
            }
            Err(e) => {
                eprintln!("Error installing offline models: {e:#}");
                process::exit(1);
            }
        }
    }

    // 4. Doctor mode
    if args.doctor {
        let ok = run_doctor(args.models_dir.as_deref());
        process::exit(if ok { 0 } else { 1 });
    }

    // 4. In-place PNG to JPEG conversion mode
    if let Some(ref target_str) = args.convert_images {
        let target = if target_str.is_empty() {
            args.input
                .clone()
                .or_else(|| args.positionals.first().map(PathBuf::from))
        } else {
            Some(PathBuf::from(target_str))
        };

        let target = match target {
            Some(p) => p,
            None => {
                eprintln!(
                    "Error: Flag '--convert-images-to-jpeg' requires a vault or directory path"
                );
                process::exit(1);
            }
        };

        let quality = args.image_quality.unwrap_or(80);
        match convert_vault_png_to_jpeg(&target, quality, args.dry_run) {
            Ok(summary) => {
                let action = if args.dry_run {
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
                if args.dry_run {
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

    // 5. Note formatting mode
    let format_notes_opt = args.format_notes.as_ref().or(args.fix_latex.as_ref());
    if let Some(target_str) = format_notes_opt {
        let target = if target_str.is_empty() {
            args.input
                .clone()
                .or_else(|| args.positionals.first().map(PathBuf::from))
        } else {
            Some(PathBuf::from(target_str))
        };

        let target = match target {
            Some(p) => p,
            None => {
                eprintln!(
                    "Error: Flag '--format-notes' requires a Markdown file or directory path"
                );
                process::exit(1);
            }
        };

        let out_target = args
            .output
            .clone()
            .or_else(|| args.positionals.get(1).map(PathBuf::from));

        match format_notes_in_path(&target, out_target.as_deref(), args.dry_run) {
            Ok(summary) => {
                let action = if args.dry_run {
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
                if args.dry_run {
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

    // Resolve input and output from explicit flags or positional arguments
    let mut input_path = args.input;
    let mut output_path = args.output;
    let mut begin_page = None;
    let mut end_page = None;

    if input_path.is_none() && !args.positionals.is_empty() {
        input_path = Some(PathBuf::from(&args.positionals[0]));
    }
    if output_path.is_none() && args.positionals.len() > 1 {
        output_path = Some(PathBuf::from(&args.positionals[1]));
    }
    if args.positionals.len() > 2 {
        begin_page = args.positionals[2].parse().ok();
    }
    if args.positionals.len() > 3 {
        end_page = args.positionals[3].parse().ok();
    }

    if let Some((s, e)) = args.pages.as_deref().and_then(|r| r.split_once('-')) {
        begin_page = s.trim().parse().ok();
        end_page = e.trim().parse().ok();
    }

    // Load configuration file if present
    let config = match AppConfig::load(args.config.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Configuration Error: {e:#}");
            process::exit(1);
        }
    };

    let final_models_dir = args.models_dir.or(config.models_dir.clone());
    if let Some(ref dir) = final_models_dir {
        pdfmd_lite::set_global_models_dir(dir.clone());
    }
    if let Some(ref layout_path) = config.layout_model {
        pdfmd_lite::figure::models::set_custom_layout_path(layout_path.clone());
    }

    let final_log_file = args.log_file.or(config.log_file.clone());
    let final_log_level = args.log_level.or(config.log_level.clone());

    if let Err(e) = pdfmd_lite::telemetry::init_telemetry(
        final_log_level.as_deref(),
        final_log_file.as_deref(),
        args.quiet,
    ) {
        eprintln!("Warning: Failed to initialize structured logging: {e:#}");
    }

    // Standalone vault embedding mode: `pdfmd-lite --embed <vault-dir-or-file> [--force]`
    if args.embed && output_path.is_none() && input_path.is_some() {
        let target = input_path.clone().unwrap();
        if target.is_file() || target.is_dir() {
            println!(
                "Retroactively embedding vault notes in '{}'...",
                target.display()
            );
            match embed_vault_in_path(&target, args.force, args.quiet) {
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
            let mut cmd = CliArgs::command();
            let _ = cmd.print_help();
            println!();
            process::exit(1);
        }
    };

    let output = match output_path {
        Some(p) => p,
        None => {
            eprintln!("Error: Missing required output path or directory.");
            let mut cmd = CliArgs::command();
            let _ = cmd.print_help();
            println!();
            process::exit(1);
        }
    };

    let final_concurrency = args.concurrency.or(config.concurrency);
    let final_max_depth = args.max_depth.or(config.max_depth).unwrap_or(3);

    // Configure active hardware device (auto, cpu, directml)
    let device_str = args
        .device
        .as_deref()
        .or(config.device.as_deref())
        .unwrap_or("auto");
    let device_target = match device_str.parse::<pdfmd_lite::DeviceTarget>() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };
    pdfmd_lite::figure::models::set_active_device(device_target);

    let allow_degraded = args.allow_degraded || config.allow_degraded.unwrap_or(false);

    let final_script = if args.english {
        pdfmd_lite::backends::indra::IndraScript::English
    } else if args.telugu {
        pdfmd_lite::backends::indra::IndraScript::Telugu
    } else if args.auto {
        pdfmd_lite::backends::indra::IndraScript::Auto
    } else if let Some(ref lang_str) = args.language.as_ref().or(config.language.as_ref()) {
        match lang_str.parse::<pdfmd_lite::backends::indra::IndraScript>() {
            Ok(s) => s,
            Err(_) => {
                if allow_degraded {
                    tracing::warn!(
                        "Requested language '{}' is unsupported. Degrading to English OCR.",
                        lang_str
                    );
                    pdfmd_lite::backends::indra::IndraScript::English
                } else {
                    eprintln!(
                        "Error: Unsupported language '{lang_str}'. Valid options are: english, telugu, world, auto.\nPass --allow-degraded to allow fallback to English."
                    );
                    process::exit(1);
                }
            }
        }
    } else {
        pdfmd_lite::backends::indra::IndraScript::English
    };

    // Capability check: if Telugu or World is requested, verify world model presence
    if matches!(
        final_script,
        pdfmd_lite::backends::indra::IndraScript::Telugu
            | pdfmd_lite::backends::indra::IndraScript::World
    ) {
        let locator = ModelLocator::new(final_models_dir.clone());
        let has_world = locator.locate("models/indra/rec_world.onnx").is_some();
        if !has_world {
            if allow_degraded {
                tracing::warn!(
                    "Telugu/World script requested but 'models/indra/rec_world.onnx' was not found. Degrading to Latin OCR."
                );
            } else {
                eprintln!(
                    "Error: Telugu/World script requested, but 'models/indra/rec_world.onnx' is not available.\nRun 'pdfmd-lite --install-models <dir|zip>' or pass --allow-degraded to fall back to Latin OCR."
                );
                process::exit(1);
            }
        }
    }

    let final_image_format = args
        .image_format
        .as_deref()
        .and_then(ImageFormat::from_str_loose)
        .or_else(|| {
            config
                .image_format
                .as_deref()
                .and_then(ImageFormat::from_str_loose)
        })
        .unwrap_or(ImageFormat::Jpeg);

    let final_image_quality = args.image_quality.or(config.image_quality).unwrap_or(80);

    let final_save_table_images = if args.save_table_images {
        true
    } else if args.no_table_images {
        false
    } else {
        config.save_table_images.unwrap_or(false)
    };

    let final_embed_base64 =
        args.embed_images_base64 || config.embed_images_base64.unwrap_or(false);

    let final_dom_mode = args
        .dom_extraction
        .as_deref()
        .and_then(pdfmd_lite::pdf::DomExtractionMode::from_str_loose)
        .or_else(|| {
            config
                .dom_extraction
                .as_deref()
                .and_then(pdfmd_lite::pdf::DomExtractionMode::from_str_loose)
        })
        .unwrap_or(pdfmd_lite::pdf::DomExtractionMode::Auto);

    let final_debug_layout = args.debug_layout.or(config.debug_layout);

    let image_opts = ImageOptions::new(
        final_image_format,
        final_image_quality,
        final_save_table_images,
    )
    .with_embed_base64(final_embed_base64)
    .with_dom_mode(final_dom_mode)
    .with_debug_layout(final_debug_layout);

    if args.watch {
        let watch_opts = pdfmd_lite::watch::WatchOptions {
            debounce_delay: std::time::Duration::from_millis(800),
            poll_interval: std::time::Duration::from_millis(250),
            concurrency: final_concurrency,
            script: final_script,
            image_opts,
            embed: args.embed,
            quiet: args.quiet,
        };
        if let Err(e) = pdfmd_lite::watch::watch_directory(&input, &output, watch_opts) {
            eprintln!("Watch mode fatal error: {e:#}");
            process::exit(1);
        }
        process::exit(0);
    }

    if args.dry_run {
        if input.is_file() {
            let total_pages = pdfmd_lite::get_pdf_page_count(&input).unwrap_or(1);
            let b_page = begin_page.unwrap_or(1);
            let e_page = end_page.unwrap_or(total_pages);
            match pdfmd_lite::batch::dry_run_single_pdf(
                &input,
                &output,
                b_page,
                e_page,
                final_concurrency,
                image_opts,
                args.embed,
            ) {
                Ok(report) => {
                    report.print();
                    process::exit(0);
                }
                Err(err) => {
                    eprintln!("Dry-run error: {err:#}");
                    process::exit(1);
                }
            }
        } else if input.is_dir() {
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
                args.resume,
                image_opts,
                args.embed,
            ) {
                Ok(report) => {
                    report.print();
                    process::exit(0);
                }
                Err(err) => {
                    eprintln!("Dry-run error: {err:#}");
                    process::exit(1);
                }
            }
        } else {
            eprintln!("Error: Input path '{}' does not exist.", input.display());
            process::exit(1);
        }
    }

    // Standard Conversion Execution
    if input.is_file() {
        let total_pages = pdfmd_lite::get_pdf_page_count(&input).unwrap_or(1);
        let b_page = begin_page.unwrap_or(1);
        let e_page = end_page.unwrap_or(total_pages);

        println!(
            "Converting '{}' -> '{}' (Pages: {b_page} to {e_page}, Concurrency: {})...",
            input.display(),
            output.display(),
            final_concurrency.map_or("auto".to_string(), |c| c.to_string())
        );

        let t0 = std::time::Instant::now();
        match pdfmd_lite::process_pdf_full(
            &input,
            &output,
            b_page,
            e_page,
            final_concurrency,
            None,
            final_script,
            image_opts,
            args.embed,
        ) {
            Ok(_) => {
                println!(
                    "Successfully converted '{}' in {:.2}s -> '{}'",
                    input.display(),
                    t0.elapsed().as_secs_f32(),
                    output.display()
                );
            }
            Err(e) => {
                eprintln!("Conversion failed: {e:#}");
                process::exit(1);
            }
        }
    } else if input.is_dir() {
        let t0 = std::time::Instant::now();
        println!(
            "Starting batch directory conversion: '{}' -> '{}'...",
            input.display(),
            output.display()
        );

        let page_range = match (begin_page, end_page) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };

        match pdfmd_lite::process_pdf_directory_all(
            &input,
            &output,
            page_range,
            final_max_depth,
            final_concurrency,
            final_script,
            args.quiet,
            args.resume,
            image_opts,
            args.embed,
        ) {
            Ok(result) => {
                let elapsed = t0.elapsed().as_secs_f32();
                let skipped = result
                    .total
                    .saturating_sub(result.succeeded + result.failed.len());
                println!(
                    "\nBatch Conversion Finished in {elapsed:.2}s:\n  - Total:     {}\n  - Succeeded: {}\n  - Failed:    {}\n  - Skipped:   {}",
                    result.total,
                    result.succeeded,
                    result.failed.len(),
                    skipped
                );
                if !result.failed.is_empty() {
                    eprintln!("\nFailed documents:");
                    for (fail_path, fail_err) in &result.failed {
                        eprintln!("  - {}: {fail_err}", fail_path.display());
                    }
                    process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("Batch directory conversion failed: {e:#}");
                process::exit(1);
            }
        }
    } else {
        eprintln!("Error: Input path '{}' does not exist.", input.display());
        process::exit(1);
    }
}

fn run_doctor(custom_dir: Option<&Path>) -> bool {
    let locator = ModelLocator::new(custom_dir.map(PathBuf::from));
    let report = locator.doctor_report();

    println!("\npdfmd-lite Offline Neural Model Diagnostics");
    println!(
        "=========================================================================================="
    );
    println!(
        "{:<35} {:<10} {:<10} {:<10} {:<10} Status / Location",
        "Model Name", "Type", "Size", "SHA256", "Valid"
    );
    println!(
        "------------------------------------------------------------------------------------------"
    );

    let mut missing_required = 0;
    let mut missing_optional = 0;
    let mut invalid_models = 0;

    for status in &report {
        let presence = if status.is_embedded {
            "[EMBEDDED STATIC SLICE]".to_string()
        } else if let Some(ref path) = status.resolved_path {
            format!("[FOUND] {}", path.display())
        } else {
            "[MISSING]".to_string()
        };

        let req_str = if status.required {
            "REQUIRED"
        } else {
            "OPTIONAL"
        };
        let size_str = status
            .size_bytes
            .map(|s| format!("{:.2} MB", s as f64 / 1_048_576.0))
            .unwrap_or_else(|| "-".to_string());
        let hash_str = status
            .sha256
            .as_deref()
            .and_then(|h| h.get(..8))
            .unwrap_or("-");
        let valid_str = if !status.exists {
            if status.required { "NO" } else { "-" }
        } else if status.is_valid {
            "YES"
        } else {
            "MISMATCH"
        };

        println!(
            "{:<35} {:<10} {:<10} {:<10} {:<10} {}",
            status.name, req_str, size_str, hash_str, valid_str, presence
        );

        if !status.exists {
            if status.required {
                missing_required += 1;
            } else {
                missing_optional += 1;
            }
        } else if !status.is_valid {
            invalid_models += 1;
            if let Some(ref err) = status.error {
                eprintln!("  ↳ Integrity Error: {err}");
            }
        }
    }

    println!(
        "=========================================================================================="
    );
    if missing_required > 0 {
        eprintln!("Error: {missing_required} required model(s) are missing. Conversion will fail.");
    } else if invalid_models > 0 {
        eprintln!("Error: {invalid_models} model weight(s) failed SHA-256 integrity verification.");
    } else {
        println!("All required offline neural models are provisioned and verified.");
    }

    if missing_optional > 0 {
        println!(
            "Note: {missing_optional} optional model(s) are not provisioned (formulas, tables, or embeddings will degrade to fallbacks).\nUse 'pdfmd-lite --install-models <dir|zip>' to import optional weights offline."
        );
    }

    missing_required == 0 && invalid_models == 0
}
