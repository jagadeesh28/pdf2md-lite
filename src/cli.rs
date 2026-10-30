//! Command-line interface parser and handler for pdfmd-lite.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::batch::process_pdf_directory_with_options;
use crate::config::AppConfig;
use crate::markdown::normalize_latex_delimiters;
use crate::pipeline::process_pdf_with_options;

pub fn print_usage() {
    eprintln!(
        r#"pdfmd-lite: Autonomous offline PDF to Obsidian Markdown converter

USAGE:
    # Single PDF conversion:
    pdfmd-lite <input.pdf> <output.md> [begin-page] [end-page]
    pdfmd-lite <input.pdf> <output-dir> [begin-page] [end-page]

    # Batch directory conversion:
    pdfmd-lite <input-dir> <output-dir> [begin-page] [end-page]

    # Delimiter normalization for existing Obsidian notes:
    pdfmd-lite --fix-math <file-or-dir>

OPTIONS:
    -i, --input <PATH>          Input PDF file or directory
    -o, --output <PATH>         Output Markdown file or destination directory
    -p, --pages <BEGIN-END>     Page range (1-based, e.g. 1-10)
    -j, --concurrency <N>       Number of concurrent worker threads (default: hardware-auto)
    -d, --max-depth <DEPTH>     Max directory search depth for batch mode (default: 3)
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

/// Normalizes LaTeX math delimiters across a single Markdown file or all .md files in a directory.
pub fn fix_math_in_path(path: &Path) -> anyhow::Result<usize> {
    let mut count = 0;
    if path.is_file() {
        let content = fs::read_to_string(path)?;
        let fixed = normalize_latex_delimiters(&content);
        if fixed != content {
            fs::write(path, fixed)?;
            count += 1;
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path)?.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_some_and(|e| e == "md") {
                let content = fs::read_to_string(&p)?;
                let fixed = normalize_latex_delimiters(&content);
                if fixed != content {
                    fs::write(&p, fixed)?;
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        process::exit(if args.is_empty() { 1 } else { 0 });
    }

    if args[0] == "--fix-math" || args[0] == "-f" {
        if args.len() < 2 {
            eprintln!("Error: Flag '--fix-math' requires a file or directory path");
            process::exit(1);
        }
        let target = Path::new(&args[1]);
        match fix_math_in_path(target) {
            Ok(c) => {
                println!("Successfully normalized LaTeX math delimiters in {c} file(s).");
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error normalizing math: {e:#}");
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
    let mut quiet = false;

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
        } else if arg == "-q" || arg == "--quiet" || arg == "--silent" {
            quiet = true;
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

    if input.is_dir() {
        let page_range = match (begin_page, end_page) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        };

        match process_pdf_directory_with_options(
            &input,
            &output,
            page_range,
            final_max_depth,
            final_concurrency,
            final_script,
            quiet,
        ) {
            Ok(summary) => {
                println!(
                    "\nBatch complete: {} processed ({} succeeded, {} failed)",
                    summary.total,
                    summary.succeeded,
                    summary.failed.len()
                );
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
        let end = end_page.unwrap_or_else(|| {
            crate::pdf::get_pdf_page_count(&input).unwrap_or(begin)
        });

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

        println!("Converting single PDF (Language: {final_script})...");
        if let Err(err) = process_pdf_with_options(
            &input,
            &target_md,
            begin,
            end,
            final_concurrency,
            None,
            final_script,
        ) {
            eprintln!("Error converting PDF: {err:#}");
            process::exit(1);
        }
    }
}
