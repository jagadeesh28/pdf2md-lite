//! Batch directory conversion with short-slug asset folders per ADR 0004.

use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backends::indra::IndraScript;
use crate::pdf::get_pdf_page_count;
use crate::pipeline::process_pdf_with_options;

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
        clean[..32].trim_end_matches('_').to_string()
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
            eprintln!(
                "Warning: could not read directory '{}': {err}",
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
            "\nFound {} PDF files in '{}'. Processing batch (concurrency={}, Language: {})...",
            pdf_files.len(),
            input_dir.display(),
            total_workers,
            script
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
    let mut initial_failed = Vec::new();

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
            eprintln!(
                "Failed to create output directory {}: {err:#}",
                pdf_output_dir.display()
            );
            initial_failed.push((pdf_path, err.to_string()));
            continue;
        }

        let target_md_path = pdf_output_dir.join(format!("{folder_slug}.md"));

        let total_pages = match get_pdf_page_count(&pdf_path) {
            Ok(count) => count,
            Err(err) => {
                eprintln!(
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
                    if !quiet {
                        eprintln!(
                            "Warning: Skipping {} (start page {start} exceeds total pages {total_pages})",
                            pdf_path.display()
                        );
                    }
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

    let total_docs = doc_items.len() + initial_failed.len();
    let result = std::sync::Arc::new(std::sync::Mutex::new(BatchConversionResult {
        total: total_docs,
        succeeded: 0,
        failed: initial_failed,
    }));

    // If total workers <= 1, process sequentially
    if total_workers <= 1 {
        for item in doc_items {
            let t0 = std::time::Instant::now();
            let pages_count = item.end_page.saturating_sub(item.begin_page) + 1;
            match process_pdf_with_options(
                &item.pdf_path,
                &item.target_md_path,
                item.begin_page,
                item.end_page,
                Some(1),
                item.rel_source.as_deref(),
                script,
            ) {
                Ok(_) => {
                    let elapsed = t0.elapsed().as_secs_f32();
                    if !quiet {
                        println!(
                            "[{}/{}] [OK] {} ({} page(s), {:.2}s)",
                            item.index, total_docs, item.folder_slug, pages_count, elapsed
                        );
                    }
                    result.lock().unwrap().succeeded += 1;
                }
                Err(err) => {
                    eprintln!("Failed to convert '{}': {err:#}", item.pdf_path.display());
                    result.lock().unwrap().failed.push((item.pdf_path, err.to_string()));
                }
            }
        }
    } else {
        // Global Multi-Document Work Pool:
        // Allocate up to 4 concurrent documents in flight, each allocated proportional sub-workers,
        // strictly guaranteeing that sum(doc_workers) <= total_workers across the process.
        let concurrent_docs = total_workers.min(4).min(doc_items.len()).max(1);
        let per_doc_workers = (total_workers / concurrent_docs).max(1);

        let queue = std::sync::Arc::new(std::sync::Mutex::new(doc_items.into_iter()));
        let mut handles = Vec::with_capacity(concurrent_docs);

        for _ in 0..concurrent_docs {
            let queue_clone = std::sync::Arc::clone(&queue);
            let result_clone = std::sync::Arc::clone(&result);

            let handle = std::thread::spawn(move || {
                loop {
                    let item = {
                        let Ok(mut q) = queue_clone.lock() else { break };
                        q.next()
                    };

                    let Some(item) = item else { break };
                    let t0 = std::time::Instant::now();
                    let pages_count = item.end_page.saturating_sub(item.begin_page) + 1;

                    let conv_res = process_pdf_with_options(
                        &item.pdf_path,
                        &item.target_md_path,
                        item.begin_page,
                        item.end_page,
                        Some(per_doc_workers),
                        item.rel_source.as_deref(),
                        script,
                    );

                    let mut res_guard = result_clone.lock().unwrap();
                    match conv_res {
                        Ok(_) => {
                            let elapsed = t0.elapsed().as_secs_f32();
                            if !quiet {
                                println!(
                                    "[{}/{}] [OK] {} ({} page(s), {:.2}s)",
                                    item.index, total_docs, item.folder_slug, pages_count, elapsed
                                );
                            }
                            res_guard.succeeded += 1;
                        }
                        Err(err) => {
                            eprintln!("Failed to convert '{}': {err:#}", item.pdf_path.display());
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

    Ok(final_res)
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
}
