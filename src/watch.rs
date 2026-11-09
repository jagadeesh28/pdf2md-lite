//! Directory watch mode for autonomous PDF conversion.
//!
//! Monitors an input directory for new or modified PDF files using native filesystem notify events,
//! debouncing write operations before initiating background conversions.

use anyhow::{Context, Result};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use crate::backends::indra::IndraScript;
use crate::figure::ImageOptions;

/// Checks whether a filesystem path represents a valid, non-temporary PDF file.
pub fn is_candidate_pdf(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if !ext.eq_ignore_ascii_case("pdf") {
        return false;
    }

    let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    // Filter out temporary files created by editors, downloaders, or OS
    if filename.starts_with('.')
        || filename.starts_with("~$")
        || filename.ends_with(".tmp")
        || filename.ends_with(".crdownload")
    {
        return false;
    }

    true
}

/// Computes the target Markdown file path in the output directory, preserving
/// the relative folder hierarchy from the watched input root directory.
pub fn compute_watch_target_paths(
    pdf_path: &Path,
    watch_root: &Path,
    output_root: &Path,
) -> (PathBuf, PathBuf) {
    let relative_parent = pdf_path
        .strip_prefix(watch_root)
        .ok()
        .and_then(|rel| rel.parent())
        .unwrap_or_else(|| Path::new(""));

    let target_parent_dir = if relative_parent.as_os_str().is_empty() {
        output_root.to_path_buf()
    } else {
        output_root.join(relative_parent)
    };

    let pdf_stem = pdf_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let folder_slug = crate::batch::sanitize_short_slug(pdf_stem);

    let doc_output_dir = target_parent_dir.join(&folder_slug);
    let target_md_path = doc_output_dir.join(format!("{folder_slug}.md"));

    (doc_output_dir, target_md_path)
}

/// Checks if a file is ready to read (not currently locked or being written to by another process).
pub fn is_file_ready(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    match fs::OpenOptions::new().read(true).open(path) {
        Ok(file) => {
            // Check that the file size is non-zero
            match file.metadata() {
                Ok(meta) => meta.len() > 0,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Configuration options for the directory watch loop.
#[derive(Debug, Clone, Copy)]
pub struct WatchOptions {
    pub debounce_delay: Duration,
    pub poll_interval: Duration,
    pub concurrency: Option<usize>,
    pub script: IndraScript,
    pub image_opts: ImageOptions,
    pub embed: bool,
    pub quiet: bool,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce_delay: Duration::from_millis(800),
            poll_interval: Duration::from_millis(250),
            concurrency: None,
            script: IndraScript::English,
            image_opts: ImageOptions::default(),
            embed: false,
            quiet: false,
        }
    }
}

/// Starts an autonomous directory watch loop, converting existing and newly added PDFs.
pub fn watch_directory(input_dir: &Path, output_dir: &Path, options: WatchOptions) -> Result<()> {
    if !input_dir.is_dir() {
        anyhow::bail!("Input path '{}' is not a directory", input_dir.display());
    }

    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create output root directory {}",
            output_dir.display()
        )
    })?;

    if !options.quiet {
        println!("pdfmd-lite watch mode active:");
        println!("  - Watching: {}", input_dir.display());
        println!("  - Output:   {}", output_dir.display());
        println!("  - Language: {}", options.script);
        println!(
            "  - Embeddings: {}",
            if options.embed { "enabled" } else { "disabled" }
        );
        println!(
            "  - Base64 images: {}",
            if options.image_opts.embed_base64 {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!("Initial batch pass scanning for un-converted PDFs...");
    }

    // Step 1: Initial batch pass with resume=true to convert un-indexed documents
    let _ = crate::batch::process_pdf_directory_all(
        input_dir,
        output_dir,
        None,
        6,
        options.concurrency,
        options.script,
        options.quiet,
        true, // resume = true
        options.image_opts,
        options.embed,
    );

    if !options.quiet {
        println!("\nWatching for new or modified PDFs (Press Ctrl+C to stop)...");
    }

    // Step 2: Establish notify filesystem watcher
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .context("Failed to initialize filesystem watcher")?;

    watcher
        .watch(input_dir, RecursiveMode::Recursive)
        .with_context(|| format!("Failed to watch directory {}", input_dir.display()))?;

    // Debounce tracker: PathBuf -> (last_event_time, is_pending)
    let mut pending_files: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        // Collect filesystem events with a timeout to allow debounce polling
        match rx.recv_timeout(options.poll_interval) {
            Ok(Ok(event)) => {
                let matches_event = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any
                );

                if matches_event {
                    for path in event.paths {
                        if is_candidate_pdf(&path) {
                            pending_files.insert(path, Instant::now());
                        }
                    }
                }
            }
            Ok(Err(err)) => {
                if !options.quiet {
                    eprintln!("Watcher warning: {err:#}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Timeout elapsed, evaluate debounced pending files
            }
            Err(RecvTimeoutError::Disconnected) => {
                anyhow::bail!("Filesystem watcher event channel disconnected");
            }
        }

        // Process files whose debounce delay has passed
        let now = Instant::now();
        let ready_paths: Vec<PathBuf> = pending_files
            .iter()
            .filter(|(path, timestamp)| {
                now.duration_since(**timestamp) >= options.debounce_delay && is_file_ready(path)
            })
            .map(|(path, _)| path.clone())
            .collect();

        for pdf_path in ready_paths {
            pending_files.remove(&pdf_path);

            let (doc_out_dir, target_md) =
                compute_watch_target_paths(&pdf_path, input_dir, output_dir);

            // Verify if content has actually changed before re-converting
            if target_md.is_file()
                && let Ok(existing_content) = fs::read_to_string(&target_md)
            {
                let (meta, _) = crate::markdown::parse_existing_frontmatter(&existing_content);
                if let Some(m) = meta
                    && let Some(existing_sha) = m.pdf_sha256
                    && let Ok(current_sha) = crate::markdown::compute_file_sha256(&pdf_path)
                    && existing_sha == current_sha
                {
                    continue;
                }
            }

            if !options.quiet {
                println!(
                    "\n[WATCH] Change detected: '{}' -> converting...",
                    pdf_path.display()
                );
            }

            if let Err(e) = fs::create_dir_all(&doc_out_dir) {
                eprintln!(
                    "Failed to create directory {}: {e:#}",
                    doc_out_dir.display()
                );
                continue;
            }

            let page_count = crate::pdf::get_pdf_page_count(&pdf_path).unwrap_or(1);
            let t0 = Instant::now();

            let conv_res = crate::pipeline::process_pdf_full(
                &pdf_path,
                &target_md,
                1,
                page_count,
                options.concurrency,
                None,
                options.script,
                options.image_opts,
                options.embed,
            );

            match conv_res {
                Ok(_) => {
                    let elapsed = t0.elapsed().as_secs_f32();
                    if !options.quiet {
                        println!(
                            "[WATCH] [DONE] Converted '{}' ({page_count} pages, {elapsed:.2}s)",
                            pdf_path.file_name().unwrap_or_default().to_string_lossy()
                        );
                    }
                }
                Err(err) => {
                    eprintln!(
                        "[WATCH] [ERROR] Failed to convert '{}': {err:#}",
                        pdf_path.display()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_candidate_pdf() {
        assert!(is_candidate_pdf(Path::new("doc.pdf")));
        assert!(is_candidate_pdf(Path::new("research/paper.PDF")));
        assert!(!is_candidate_pdf(Path::new("doc.txt")));
        assert!(!is_candidate_pdf(Path::new(".hidden.pdf")));
        assert!(!is_candidate_pdf(Path::new("~$temp.pdf")));
        assert!(!is_candidate_pdf(Path::new("download.pdf.crdownload")));
        assert!(!is_candidate_pdf(Path::new("file.pdf.tmp")));
    }

    #[test]
    fn test_compute_watch_target_paths() {
        let watch_root = Path::new("inbox");
        let output_root = Path::new("vault/literature");
        let pdf = Path::new("inbox/physics/quantum_optics.pdf");

        let (doc_dir, md_path) = compute_watch_target_paths(pdf, watch_root, output_root);
        assert_eq!(
            doc_dir,
            PathBuf::from("vault/literature/physics/quantum_optics")
        );
        assert_eq!(
            md_path,
            PathBuf::from("vault/literature/physics/quantum_optics/quantum_optics.md")
        );
    }
}
