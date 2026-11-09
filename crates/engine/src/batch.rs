//! Batch directory conversion with short-slug asset folders per ADR 0004.

use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backends::indra::IndraScript;
use crate::figure::ImageOptions;
use crate::pdf::get_pdf_page_count;

/// Default directory search depth for PDF batch conversion.
pub const DEFAULT_MAX_SEARCH_DEPTH: usize = 3;

/// Sanitizes and truncates a string into a safe, shortened folder slug (max 32 chars)
/// to avoid Windows MAX_PATH length limits while keeping names recognizable.
pub fn sanitize_short_slug(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "doc".to_string();
    }
    let sanitized: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' => '_',
            _ => c,
        })
        .collect();

    let clean = sanitized.trim_matches('_');
    if clean.is_empty() {
        return "doc".to_string();
    }

    if clean.len() > 32 {
        let boundary = clean.floor_char_boundary(32);
        clean[..boundary].trim_end_matches('_').to_string()
    } else {
        clean.to_string()
    }
}

/// Collects all PDF files (`.pdf`, case-insensitive) in a directory searching up to `max_depth` directory levels.
pub fn collect_pdf_files_with_depth(dir: &Path, max_depth: usize) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Err(anyhow!("Directory '{}' does not exist", dir.display()));
    }
    let mut pdfs = Vec::new();
    collect_pdf_files_recursive(dir, 0, max_depth, &mut pdfs)?;
    pdfs.sort();
    Ok(pdfs)
}

fn collect_pdf_files_recursive(
    current_dir: &Path,
    current_depth: usize,
    max_depth: usize,
    collected: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = match fs::read_dir(current_dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(
                "Could not read directory '{}': {err}",
                current_dir.display()
            );
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_file() {
            if let Some(ext) = path.extension()
                && ext.eq_ignore_ascii_case("pdf")
            {
                collected.push(path);
            }
        } else if file_type.is_dir() && current_depth < max_depth {
            let is_hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with('.'));

            if !is_hidden {
                collect_pdf_files_recursive(&path, current_depth + 1, max_depth, collected)?;
            }
        }
    }

    Ok(())
}

/// Summary report for batch conversion runs.
#[derive(Debug, Default, Clone)]
pub struct BatchConversionResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: Vec<(PathBuf, String)>,
}

/// Converts a directory of PDF files into Obsidian Markdown documents, preserving relative folder
/// structures and dispatching jobs through the hardware governor's global worker pool.
pub fn process_pdf_directory(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
) -> Result<BatchConversionResult> {
    process_pdf_directory_with_options(
        input_dir,
        output_base_dir,
        page_range,
        max_depth,
        explicit_concurrency,
        script,
        false,
    )
}

/// Converts a directory of PDF files with configurable quiet mode.
pub fn process_pdf_directory_with_options(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
    quiet: bool,
) -> Result<BatchConversionResult> {
    process_pdf_directory_full(
        input_dir,
        output_base_dir,
        page_range,
        max_depth,
        explicit_concurrency,
        script,
        quiet,
        false,
    )
}

/// Converts a directory of PDF files with configurable quiet mode and optional resume support.
#[allow(clippy::too_many_arguments)]
pub fn process_pdf_directory_full(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
    quiet: bool,
    resume: bool,
) -> Result<BatchConversionResult> {
    process_pdf_directory_with_image_options(
        input_dir,
        output_base_dir,
        page_range,
        max_depth,
        explicit_concurrency,
        script,
        quiet,
        resume,
        ImageOptions::default(),
    )
}

/// Converts a directory of PDF files with explicit ImageOptions, quiet mode, and resume support.
#[allow(clippy::too_many_arguments)]
pub fn process_pdf_directory_with_image_options(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
    quiet: bool,
    resume: bool,
    image_opts: ImageOptions,
) -> Result<BatchConversionResult> {
    process_pdf_directory_all(
        input_dir,
        output_base_dir,
        page_range,
        max_depth,
        explicit_concurrency,
        script,
        quiet,
        resume,
        image_opts,
        false,
    )
}

/// Converts a directory of PDF files with all configuration options including embedding generation.
#[allow(clippy::too_many_arguments)]
pub fn process_pdf_directory_all(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    script: IndraScript,
    quiet: bool,
    resume: bool,
    image_opts: ImageOptions,
    embed: bool,
) -> Result<BatchConversionResult> {
    let pdf_files = collect_pdf_files_with_depth(input_dir, max_depth)?;
    if pdf_files.is_empty() {
        return Err(anyhow!(
            "No PDF files (.pdf) found in directory '{}'",
            input_dir.display()
        ));
    }

    fs::create_dir_all(output_base_dir).with_context(|| {
        format!(
            "failed to create output base directory {}",
            output_base_dir.display()
        )
    })?;

    let total_workers = crate::concurrency::resolve_concurrency(explicit_concurrency);

    if !quiet {
        println!(
            "\nFound {} PDF files in '{}'. Processing batch (concurrency={}, Language: {}{})...",
            pdf_files.len(),
            input_dir.display(),
            total_workers,
            script,
            if resume { ", Resume: enabled" } else { "" }
        );
    }

    // Structure metadata for each document item
    struct DocItem {
        index: usize,
        pdf_path: PathBuf,
        target_md_path: PathBuf,
        rel_source: Option<String>,
        begin_page: usize,
        end_page: usize,
        folder_slug: String,
    }

    let mut used_slugs = HashSet::new();
    let mut doc_items = Vec::with_capacity(pdf_files.len());
    let mut skipped_items = Vec::new();
    let mut initial_failed = Vec::new();
    let mut initial_succeeded = 0;
    let _total_pdf_count = pdf_files.len();

    for (index, pdf_path) in pdf_files.into_iter().enumerate() {
        let relative_parent = pdf_path
            .strip_prefix(input_dir)
            .ok()
            .and_then(|rel| rel.parent())
            .unwrap_or_else(|| Path::new(""));

        let base_output_target_dir = if relative_parent.as_os_str().is_empty() {
            output_base_dir.to_path_buf()
        } else {
            output_base_dir.join(relative_parent)
        };

        let pdf_stem = pdf_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let base_slug = sanitize_short_slug(pdf_stem);

        let mut folder_slug = base_slug.clone();
        let mut counter = 1;
        while used_slugs.contains(&base_output_target_dir.join(&folder_slug)) {
            folder_slug = format!("{base_slug}_{counter}");
            counter += 1;
        }
        used_slugs.insert(base_output_target_dir.join(&folder_slug));

        let pdf_output_dir = base_output_target_dir.join(&folder_slug);
        if let Err(err) = fs::create_dir_all(&pdf_output_dir) {
            tracing::error!(
                "Failed to create output directory {}: {err:#}",
                pdf_output_dir.display()
            );
            initial_failed.push((pdf_path, err.to_string()));
            continue;
        }

        let target_md_path = pdf_output_dir.join(format!("{folder_slug}.md"));

        // Incremental Batch Resume: If the target markdown file already exists and contains
        // valid converted content (at least 100 bytes), verify content-hash or length before skipping.
        if resume
            && target_md_path.is_file()
            && let Ok(meta) = fs::metadata(&target_md_path)
            && meta.len() > 100
        {
            let mut should_skip = true;
            if let Ok(existing_content) = fs::read_to_string(&target_md_path) {
                let (existing_meta, _) =
                    crate::markdown::parse_existing_frontmatter(&existing_content);
                if let Some(em) = existing_meta
                    && let Some(existing_sha) = em.pdf_sha256
                    && let Ok(source_sha) = crate::markdown::compute_file_sha256(&pdf_path)
                {
                    should_skip = existing_sha == source_sha;
                }
            }

            if should_skip {
                skipped_items.push((index + 1, folder_slug));
                initial_succeeded += 1;
                continue;
            }
        }

        let total_pages = match get_pdf_page_count(&pdf_path) {
            Ok(count) => count,
            Err(err) => {
                tracing::warn!(
                    "Failed to read page count for {}: {err:#}",
                    pdf_path.display()
                );
                initial_failed.push((pdf_path, err.to_string()));
                continue;
            }
        };

        let (begin_page, end_page) = match page_range {
            Some((start, end)) => {
                if start > total_pages {
                    tracing::warn!(
                        "Skipping {} (start page {start} exceeds total pages {total_pages})",
                        pdf_path.display()
                    );
                    initial_failed.push((
                        pdf_path,
                        format!("start page {start} exceeds total pages {total_pages}"),
                    ));
                    continue;
                }
                (start, end.min(total_pages))
            }
            None => (1, total_pages),
        };

        let rel_source = pdf_path
            .strip_prefix(input_dir)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.replace('\\', "/"));

        doc_items.push(DocItem {
            index: index + 1,
            pdf_path,
            target_md_path,
            rel_source,
            begin_page,
            end_page,
            folder_slug,
        });
    }

    let total_docs = doc_items.len() + initial_succeeded + initial_failed.len();
    let total_pages_to_process: usize = doc_items
        .iter()
        .map(|item| item.end_page.saturating_sub(item.begin_page) + 1)
        .sum();

    let tracker = std::sync::Arc::new(crate::progress::BatchProgressTracker::new(
        total_docs,
        total_pages_to_process,
        quiet,
    ));

    for (idx, slug) in skipped_items {
        tracker.on_doc_skipped(idx, &slug);
    }

    let result = std::sync::Arc::new(std::sync::Mutex::new(BatchConversionResult {
        total: total_docs,
        succeeded: initial_succeeded,
        failed: initial_failed,
    }));

    // If total workers <= 1, process sequentially
    if total_workers <= 1 {
        for item in doc_items {
            let t0 = std::time::Instant::now();
            let pages_count = item.end_page.saturating_sub(item.begin_page) + 1;
            tracker.on_doc_start(0, &item.folder_slug, 1);
            let conv_res = crate::pipeline::process_pdf_full(
                &item.pdf_path,
                &item.target_md_path,
                item.begin_page,
                item.end_page,
                Some(1),
                item.rel_source.as_deref(),
                script,
                image_opts,
                embed,
            );
            let elapsed = t0.elapsed().as_secs_f32();
            match conv_res {
                Ok(_) => {
                    tracker.on_doc_complete(
                        0,
                        item.index,
                        &item.folder_slug,
                        pages_count,
                        elapsed,
                        true,
                    );
                    result.lock().unwrap().succeeded += 1;
                }
                Err(err) => {
                    tracker.on_doc_failed(0, item.index, &item.folder_slug, &err.to_string());
                    result
                        .lock()
                        .unwrap()
                        .failed
                        .push((item.pdf_path, err.to_string()));
                }
            }
        }
    } else {
        // Global Multi-Document Work Pool:
        // Allocate up to 4 concurrent documents in flight, each allocated proportional sub-workers,
        // strictly guaranteeing that sum(doc_workers) <= total_workers across the process.
        // Uses batch_page_worker_limit to prevent nested concurrency oversubscription.
        let concurrent_docs = total_workers.min(4).min(doc_items.len()).max(1);
        let per_doc_workers =
            crate::concurrency::batch_page_worker_limit(None, total_workers) / concurrent_docs;
        let per_doc_workers = per_doc_workers.max(1);

        let queue = std::sync::Arc::new(std::sync::Mutex::new(doc_items.into_iter()));
        let mut handles = Vec::with_capacity(concurrent_docs);

        for worker_id in 0..concurrent_docs {
            let queue_clone = std::sync::Arc::clone(&queue);
            let result_clone = std::sync::Arc::clone(&result);
            let tracker_clone = std::sync::Arc::clone(&tracker);

            let handle = std::thread::spawn(move || {
                loop {
                    let item = {
                        let Ok(mut q) = queue_clone.lock() else { break };
                        q.next()
                    };

                    let Some(item) = item else { break };
                    let t0 = std::time::Instant::now();
                    let pages_count = item.end_page.saturating_sub(item.begin_page) + 1;
                    tracker_clone.on_doc_start(worker_id, &item.folder_slug, per_doc_workers);

                    let conv_res = crate::pipeline::process_pdf_full(
                        &item.pdf_path,
                        &item.target_md_path,
                        item.begin_page,
                        item.end_page,
                        Some(per_doc_workers),
                        item.rel_source.as_deref(),
                        script,
                        image_opts,
                        embed,
                    );
                    let elapsed = t0.elapsed().as_secs_f32();

                    let mut res_guard = result_clone.lock().unwrap();
                    match conv_res {
                        Ok(_) => {
                            tracker_clone.on_doc_complete(
                                worker_id,
                                item.index,
                                &item.folder_slug,
                                pages_count,
                                elapsed,
                                true,
                            );
                            res_guard.succeeded += 1;
                        }
                        Err(err) => {
                            tracker_clone.on_doc_failed(
                                worker_id,
                                item.index,
                                &item.folder_slug,
                                &err.to_string(),
                            );
                            res_guard.failed.push((item.pdf_path, err.to_string()));
                        }
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.join();
        }
    }

    let final_res = std::sync::Arc::try_unwrap(result)
        .unwrap()
        .into_inner()
        .unwrap();

    tracker.finish();

    Ok(final_res)
}

/// Information describing a planned conversion for a single document during dry-run.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversionPlan {
    pub input_path: PathBuf,
    pub target_md_path: PathBuf,
    pub images_target: String,
    pub embeddings_target: Option<PathBuf>,
    pub total_pages: usize,
    pub page_range: (usize, usize),
    pub pages_to_process: usize,
    pub status: String,
}

/// Comprehensive dry-run projection report without performing disk writes or rasterization.
#[derive(Debug, Clone)]
pub struct DryRunReport {
    pub plans: Vec<ConversionPlan>,
    pub total_documents: usize,
    pub total_pages_to_process: usize,
    pub resolved_concurrency: usize,
    pub memory_budget_mb: usize,
    pub estimated_time_secs: (f32, f32),
}

impl DryRunReport {
    /// Formats and prints the dry-run report to stdout.
    pub fn print(&self) {
        println!(
            "================================================================================"
        );
        println!(
            "                         pdfmd-lite Conversion Dry-Run                          "
        );
        println!(
            "================================================================================"
        );
        println!(
            "Execution Plan ({} document(s), {} pages to process):",
            self.total_documents, self.total_pages_to_process
        );
        for (i, plan) in self.plans.iter().enumerate() {
            println!(
                " [{}/{}] {}\n        Target:  {}\n        Pages:   {} (range: {}-{})\n        Images:  {}\n        Status:  {}",
                i + 1,
                self.total_documents,
                plan.input_path.display(),
                plan.target_md_path.display(),
                plan.pages_to_process,
                plan.page_range.0,
                plan.page_range.1,
                plan.images_target,
                plan.status
            );
            if let Some(ref emb) = plan.embeddings_target {
                println!("        Sidecar: {}", emb.display());
            }
        }
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!("Resource Budgets & Performance Projections:");
        println!(
            "  - Concurrency workers:        {}",
            self.resolved_concurrency
        );
        println!(
            "  - Estimated memory budget:    {} MB",
            self.memory_budget_mb
        );
        println!(
            "  - Estimated conversion time:  {:.1}s - {:.1}s (~{})",
            self.estimated_time_secs.0,
            self.estimated_time_secs.1,
            crate::progress::format_eta(self.estimated_time_secs.1)
        );
        println!(
            "================================================================================"
        );
    }
}

/// Previews conversion plan, page counts, memory budgets, and projected duration for a directory of PDFs.
#[allow(clippy::too_many_arguments)]
pub fn dry_run_pdf_directory(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
    _script: IndraScript,
    resume: bool,
    image_opts: ImageOptions,
    embed: bool,
) -> Result<DryRunReport> {
    let pdf_files = collect_pdf_files_with_depth(input_dir, max_depth)?;
    let mut plans = Vec::with_capacity(pdf_files.len());
    let mut used_slugs = HashSet::new();

    for pdf_path in pdf_files {
        let relative_parent = pdf_path
            .strip_prefix(input_dir)
            .ok()
            .and_then(|rel| rel.parent())
            .unwrap_or_else(|| Path::new(""));

        let base_output_target_dir = if relative_parent.as_os_str().is_empty() {
            output_base_dir.to_path_buf()
        } else {
            output_base_dir.join(relative_parent)
        };

        let pdf_stem = pdf_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let base_slug = sanitize_short_slug(pdf_stem);

        let mut folder_slug = base_slug.clone();
        let mut counter = 1;
        while used_slugs.contains(&base_output_target_dir.join(&folder_slug)) {
            folder_slug = format!("{base_slug}_{counter}");
            counter += 1;
        }
        used_slugs.insert(base_output_target_dir.join(&folder_slug));

        let pdf_output_dir = base_output_target_dir.join(&folder_slug);
        let target_md_path = pdf_output_dir.join(format!("{folder_slug}.md"));

        let total_pages = get_pdf_page_count(&pdf_path).unwrap_or(1);
        let (begin, end) = match page_range {
            Some((s, e)) => (s, e.min(total_pages)),
            None => (1, total_pages),
        };
        let pages_to_process = end.saturating_sub(begin) + 1;

        let status = if target_md_path.is_file() {
            if resume {
                let mut matches_sha = false;
                if let Ok(content) = fs::read_to_string(&target_md_path) {
                    let (meta, _) = crate::markdown::parse_existing_frontmatter(&content);
                    if let Some(m) = meta
                        && let Some(sha) = m.pdf_sha256
                        && let Ok(source_sha) = crate::markdown::compute_file_sha256(&pdf_path)
                    {
                        matches_sha = sha == source_sha;
                    }
                }
                if matches_sha {
                    "WOULD_SKIP (Already converted, content-hash matches)"
                } else {
                    "WOULD_OVERWRITE (Existing file hash differs or missing)"
                }
            } else {
                "WOULD_OVERWRITE (Target note exists)"
            }
        } else {
            "WOULD_CONVERT (New note)"
        };

        let images_target = if image_opts.embed_base64 {
            "inlined (base64 data URIs, no folder created)".to_string()
        } else {
            format!(
                "{folder_slug}_images/ ({}, quality: {})",
                image_opts.format.extension(),
                image_opts.quality
            )
        };

        let embeddings_target = if embed {
            Some(pdf_output_dir.join(format!("{folder_slug}.embeddings.jsonl")))
        } else {
            None
        };

        plans.push(ConversionPlan {
            input_path: pdf_path,
            target_md_path,
            images_target,
            embeddings_target,
            total_pages,
            page_range: (begin, end),
            pages_to_process,
            status: status.to_string(),
        });
    }

    let total_documents = plans.len();
    let total_pages_to_process: usize = plans
        .iter()
        .filter(|p| !p.status.starts_with("WOULD_SKIP"))
        .map(|p| p.pages_to_process)
        .sum();

    let resolved_concurrency = crate::concurrency::resolve_concurrency(explicit_concurrency);
    let memory_budget_mb = 350 + (resolved_concurrency * 200);

    let min_secs = (total_pages_to_process as f32 * 0.8) / resolved_concurrency as f32;
    let max_secs = (total_pages_to_process as f32 * 1.8) / resolved_concurrency as f32;

    Ok(DryRunReport {
        plans,
        total_documents,
        total_pages_to_process,
        resolved_concurrency,
        memory_budget_mb,
        estimated_time_secs: (min_secs, max_secs),
    })
}

/// Previews conversion plan for a single PDF document.
pub fn dry_run_single_pdf(
    input_path: &Path,
    output_path: &Path,
    begin_page: usize,
    end_page: usize,
    explicit_concurrency: Option<usize>,
    image_opts: ImageOptions,
    embed: bool,
) -> Result<DryRunReport> {
    let pdf_stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc");
    let short_slug = sanitize_short_slug(pdf_stem);

    let total_pages = get_pdf_page_count(input_path).unwrap_or(end_page);
    let begin = begin_page.max(1);
    let end = end_page.min(total_pages);
    let pages_to_process = end.saturating_sub(begin) + 1;

    let target_md = if output_path.is_dir() || output_path.extension().is_none() {
        output_path.join(format!("{short_slug}.md"))
    } else {
        output_path.to_path_buf()
    };

    let status = if target_md.is_file() {
        "WOULD_OVERWRITE (Target note exists)"
    } else {
        "WOULD_CONVERT (New note)"
    };

    let images_target = if image_opts.embed_base64 {
        "inlined (base64 data URIs, no folder created)".to_string()
    } else {
        let parent = target_md.parent().unwrap_or_else(|| Path::new("."));
        format!(
            "{}/ ({}, quality: {})",
            parent.join(format!("{short_slug}_images")).display(),
            image_opts.format.extension(),
            image_opts.quality
        )
    };

    let embeddings_target = if embed {
        let parent = target_md.parent().unwrap_or_else(|| Path::new("."));
        Some(parent.join(format!("{short_slug}.embeddings.jsonl")))
    } else {
        None
    };

    let plan = ConversionPlan {
        input_path: input_path.to_path_buf(),
        target_md_path: target_md,
        images_target,
        embeddings_target,
        total_pages,
        page_range: (begin, end),
        pages_to_process,
        status: status.to_string(),
    };

    let resolved_concurrency = crate::concurrency::resolve_concurrency(explicit_concurrency);
    let memory_budget_mb = 350 + (resolved_concurrency * 200);

    let min_secs = (pages_to_process as f32 * 0.8) / resolved_concurrency as f32;
    let max_secs = (pages_to_process as f32 * 1.8) / resolved_concurrency as f32;

    Ok(DryRunReport {
        plans: vec![plan],
        total_documents: 1,
        total_pages_to_process: pages_to_process,
        resolved_concurrency,
        memory_budget_mb,
        estimated_time_secs: (min_secs, max_secs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_short_slug() {
        assert_eq!(
            sanitize_short_slug("A Very Long Scientific Document Report On Thermodynamics 2026"),
            "A_Very_Long_Scientific_Document"
        );
        assert_eq!(sanitize_short_slug("test:document?1"), "test_document_1");
        assert_eq!(sanitize_short_slug(""), "doc");
    }

    #[test]
    fn test_relative_folder_structure_preservation() {
        let input_dir = Path::new("input");
        let nested_pdf = Path::new("input/aerodynamics/turbines/stage1.pdf");
        let rel_parent = nested_pdf
            .strip_prefix(input_dir)
            .ok()
            .and_then(|r| r.parent())
            .unwrap();

        assert_eq!(rel_parent, Path::new("aerodynamics/turbines"));

        let output_base = Path::new("output");
        let target_dir = output_base.join(rel_parent).join("stage1");
        assert_eq!(target_dir, Path::new("output/aerodynamics/turbines/stage1"));
    }

    #[test]
    fn test_resume_mode_skips_existing_markdown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let input_dir = temp_dir.path().join("in");
        let output_dir = temp_dir.path().join("out");
        fs::create_dir_all(&input_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();

        // Write a minimal valid PDF
        let pdf_path = input_dir.join("sample.pdf");
        let doc = lopdf::Document::with_version("1.4");
        let mut f = fs::File::create(&pdf_path).unwrap();
        let mut doc_mut = doc;
        doc_mut.save_to(&mut f).unwrap();

        // Pre-create the output markdown note with valid length (> 100 bytes)
        let note_dir = output_dir.join("sample");
        fs::create_dir_all(&note_dir).unwrap();
        let note_path = note_dir.join("sample.md");
        fs::write(
            &note_path,
            "---\ntitle: Sample Document\nsource: sample.pdf\ntags:\n  - test\n---\n\nExisting note content that was already processed.\n",
        )
        .unwrap();

        let result = process_pdf_directory_full(
            &input_dir,
            &output_dir,
            None,
            3,
            Some(1),
            IndraScript::English,
            true,
            true, // resume enabled
        )
        .unwrap();

        assert_eq!(result.succeeded, 1);
        assert_eq!(result.total, 1);
        assert!(result.failed.is_empty());
    }

    #[test]
    fn test_dry_run_pdf_directory_plans_without_writing_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let input_dir = temp_dir.path().join("in");
        let output_dir = temp_dir.path().join("out");
        fs::create_dir_all(&input_dir).unwrap();

        // Write two minimal PDFs
        for name in &["doc_a.pdf", "doc_b.pdf"] {
            let pdf_path = input_dir.join(name);
            let doc = lopdf::Document::with_version("1.4");
            let mut f = fs::File::create(&pdf_path).unwrap();
            let mut doc_mut = doc;
            doc_mut.save_to(&mut f).unwrap();
        }

        let report = dry_run_pdf_directory(
            &input_dir,
            &output_dir,
            None,
            3,
            Some(2),
            IndraScript::English,
            true,
            ImageOptions::default(),
            true, // embed
        )
        .unwrap();

        assert_eq!(report.total_documents, 2);
        assert_eq!(report.plans.len(), 2);
        assert_eq!(report.resolved_concurrency, 2);
        assert_eq!(report.memory_budget_mb, 350 + (2 * 200));
        assert!(report.plans.iter().all(|p| p.embeddings_target.is_some()));

        // Crucial verification: output_dir was NOT created and no notes were written!
        assert!(!output_dir.exists());
    }

    #[test]
    fn test_dry_run_single_pdf_projections() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pdf_path = temp_dir.path().join("single.pdf");
        let out_note = temp_dir.path().join("notes").join("single.md");

        let doc = lopdf::Document::with_version("1.4");
        let mut f = fs::File::create(&pdf_path).unwrap();
        let mut doc_mut = doc;
        doc_mut.save_to(&mut f).unwrap();

        let report = dry_run_single_pdf(
            &pdf_path,
            &out_note,
            1,
            5,
            Some(4),
            ImageOptions::default().with_embed_base64(true),
            false,
        )
        .unwrap();

        assert_eq!(report.total_documents, 1);
        assert_eq!(report.resolved_concurrency, 4);
        assert_eq!(report.memory_budget_mb, 350 + (4 * 200));
        assert!(report.plans[0].images_target.contains("inlined (base64"));
        assert!(!out_note.exists());
    }
}
