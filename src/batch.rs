//! Batch directory conversion with short-slug asset folders per ADR 0004.

use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pdf::get_pdf_page_count;
use crate::pipeline::process_pdf;

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

/// Converts all PDF files in `input_dir` one by one, organizing each into a short-slug subfolder.
pub fn process_pdf_directory(
    input_dir: &Path,
    output_base_dir: &Path,
    page_range: Option<(usize, usize)>,
    max_depth: usize,
    explicit_concurrency: Option<usize>,
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

    println!(
        "\nFound {} PDF files in '{}'. Processing batch...",
        pdf_files.len(),
        input_dir.display()
    );

    let mut result = BatchConversionResult {
        total: pdf_files.len(),
        succeeded: 0,
        failed: Vec::new(),
    };

    let mut used_slugs = HashSet::new();

    for (index, pdf_path) in pdf_files.iter().enumerate() {
        let pdf_stem = pdf_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let base_slug = sanitize_short_slug(pdf_stem);

        let mut folder_slug = base_slug.clone();
        let mut counter = 1;
        while used_slugs.contains(&folder_slug) {
            folder_slug = format!("{base_slug}_{counter}");
            counter += 1;
        }
        used_slugs.insert(folder_slug.clone());

        let pdf_output_dir = output_base_dir.join(&folder_slug);
        if let Err(err) = fs::create_dir_all(&pdf_output_dir) {
            eprintln!(
                "Failed to create output directory {}: {err:#}",
                pdf_output_dir.display()
            );
            result.failed.push((pdf_path.clone(), err.to_string()));
            continue;
        }

        let target_md_path = pdf_output_dir.join(format!("{folder_slug}.md"));

        let total_pages = match get_pdf_page_count(pdf_path) {
            Ok(count) => count,
            Err(err) => {
                eprintln!(
                    "Failed to read page count for {}: {err:#}",
                    pdf_path.display()
                );
                result.failed.push((pdf_path.clone(), err.to_string()));
                continue;
            }
        };

        let (begin_page, end_page) = match page_range {
            Some((start, end)) => {
                if start > total_pages {
                    eprintln!(
                        "Warning: Skipping {} (start page {start} exceeds total pages {total_pages})",
                        pdf_path.display()
                    );
                    result.failed.push((
                        pdf_path.clone(),
                        format!("start page {start} exceeds total pages {total_pages}"),
                    ));
                    continue;
                }
                (start, end.min(total_pages))
            }
            None => (1, total_pages),
        };

        println!(
            "\n=======================================================\n\
             [{}/{}] Converting '{}'\n\
             Pages: {}..{} (total: {})\n\
             Output directory: {}\n\
             Markdown: {}\n\
             Asset slug: {}\n\
             =======================================================",
            index + 1,
            pdf_files.len(),
            pdf_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown"),
            begin_page,
            end_page,
            total_pages,
            pdf_output_dir.display(),
            target_md_path.display(),
            folder_slug
        );

        match process_pdf(
            pdf_path,
            &target_md_path,
            begin_page,
            end_page,
            explicit_concurrency,
        ) {
            Ok(_) => {
                println!("Successfully converted '{}'", pdf_path.display());
                result.succeeded += 1;
            }
            Err(err) => {
                eprintln!("Failed to convert '{}': {err:#}", pdf_path.display());
                result.failed.push((pdf_path.clone(), err.to_string()));
            }
        }
    }

    Ok(result)
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
}
