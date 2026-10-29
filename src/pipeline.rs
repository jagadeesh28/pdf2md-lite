//! Single-document pipeline orchestrator for pdfmd-lite.
//!
//! Orchestrates YOLOv8n three-way layout routing, Indra OCR, geometric table extraction,
//! LaTeX math normalization, and page indexing with ascending concurrency.

use anyhow::{Context, Result};
use image::GenericImageView;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::backends::indra::{IndraEngine, TextLine};
use crate::batch::sanitize_short_slug;
use crate::concurrency::resolve_concurrency;
use crate::figure::{self, mask_page_regions};
use crate::markdown::{attach_figures_to_page_text, format_page_demarcation, normalize_latex_delimiters};
use crate::pdf::{render_pages_to_images, safe_extract_text_by_pages, validate_page_range};

static INDRA_ENGINE: std::sync::OnceLock<IndraEngine> = std::sync::OnceLock::new();

/// Returns a lazily initialized static reference to the pure-Rust Indra OCR engine.
pub fn get_indra_engine() -> Result<&'static IndraEngine> {
    if let Some(engine) = INDRA_ENGINE.get() {
        return Ok(engine);
    }
    let engine = IndraEngine::new(None)?;
    let _ = INDRA_ENGINE.set(engine);
    Ok(INDRA_ENGINE.get().expect("IndraEngine initialized"))
}

/// Converts a single PDF file into an Obsidian Markdown document with companion assets.
pub fn process_pdf(
    input_path: &Path,
    output_path: &Path,
    begin_page: usize,
    end_page: usize,
    explicit_concurrency: Option<usize>,
) -> Result<()> {
    process_pdf_with_source(
        input_path,
        output_path,
        begin_page,
        end_page,
        explicit_concurrency,
        None,
    )
}

/// Converts a single PDF file with an optional relative source path for frontmatter metadata.
pub fn process_pdf_with_source(
    input_path: &Path,
    output_path: &Path,
    begin_page: usize,
    end_page: usize,
    explicit_concurrency: Option<usize>,
    source_label: Option<&str>,
) -> Result<()> {
    validate_page_range(begin_page, end_page)?;

    let pdf_stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc");
    let short_slug = sanitize_short_slug(pdf_stem);
    let images_folder_name = format!("{short_slug}_images");

    let output_dir = output_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let image_output_dir = output_dir.join(&images_folder_name);

    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create output directory {}", output_dir.display()))?;

    // Extract PDF document properties for Obsidian frontmatter via DOM-first gating
    let mut metadata = crate::pdf::extract_pdf_metadata(input_path);
    if metadata.title.is_none() {
        metadata.title = Some(pdf_stem.to_string());
    }

    // DOM-First Fallback Gating: If author, publication date, organization, journal, volume, DOI, or keywords are missing, extract from document header slice
    if metadata.author.is_none()
        || metadata.publication_date.is_none()
        || metadata.organization.is_none()
        || metadata.journal.is_none()
        || metadata.volume.is_none()
        || metadata.doi.is_none()
        || metadata.keywords.is_none()
    {
        let header_slice = if let Ok(bytes) = fs::read(input_path) {
            crate::pdf::safe_extract_text_by_pages(&bytes)
                .and_then(|pages| pages.into_iter().next())
                .unwrap_or_default()
        } else {
            String::new()
        };

        if !header_slice.trim().is_empty() {
            #[cfg(feature = "gliner-ort")]
            if let Some(gliner) = crate::metadata::get_gliner_engine() {
                if let Ok(res) = gliner.extract_metadata(&header_slice, &["title", "author", "date", "organization", "journal", "volume", "keywords", "doi"]) {
                    if metadata.author.is_none() && !res.authors.is_empty() {
                        metadata.author = Some(res.authors.join(", "));
                    }
                    if metadata.publication_date.is_none() && res.date.is_some() {
                        metadata.publication_date = res.date;
                    }
                    if metadata.organization.is_none() && res.organization.is_some() {
                        metadata.organization = res.organization;
                    }
                    if metadata.journal.is_none() && res.journal.is_some() {
                        metadata.journal = res.journal;
                    }
                    if metadata.volume.is_none() && res.volume.is_some() {
                        metadata.volume = res.volume;
                    }
                    if metadata.issue.is_none() && res.issue.is_some() {
                        metadata.issue = res.issue;
                    }
                    if metadata.doi.is_none() && res.doi.is_some() {
                        metadata.doi = res.doi;
                    }
                    if metadata.keywords.is_none() && !res.keywords.is_empty() {
                        metadata.keywords = Some(res.keywords.join(", "));
                    }
                }
            }

            // Fallback pattern extractor if neural weights are not present
            if metadata.author.is_none()
                || metadata.publication_date.is_none()
                || metadata.organization.is_none()
                || metadata.journal.is_none()
                || metadata.volume.is_none()
                || metadata.doi.is_none()
                || metadata.keywords.is_none()
            {
                let res = crate::metadata::extract_header_metadata_fallback(&header_slice);
                if metadata.author.is_none() && !res.authors.is_empty() {
                    metadata.author = Some(res.authors.join(", "));
                }
                if metadata.publication_date.is_none() && res.date.is_some() {
                    metadata.publication_date = res.date;
                }
                if metadata.organization.is_none() && res.organization.is_some() {
                    metadata.organization = res.organization;
                }
                if metadata.journal.is_none() && res.journal.is_some() {
                    metadata.journal = res.journal;
                }
                if metadata.volume.is_none() && res.volume.is_some() {
                    metadata.volume = res.volume;
                }
                if metadata.issue.is_none() && res.issue.is_some() {
                    metadata.issue = res.issue;
                }
                if metadata.doi.is_none() && res.doi.is_some() {
                    metadata.doi = res.doi;
                }
                if metadata.keywords.is_none() && !res.keywords.is_empty() {
                    metadata.keywords = Some(res.keywords.join(", "));
                }
            }
        }
    }
    if let Some(src) = source_label {
        metadata.source = Some(src.to_string());
    } else if metadata.source.is_none() {
        metadata.source = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
    }
    metadata.pages_processed = Some(format!("{begin_page}-{end_page}"));
    let mut full_markdown = metadata.to_yaml_frontmatter();

    process_pages_stream(
        input_path,
        &image_output_dir,
        begin_page,
        end_page,
        explicit_concurrency,
        |page_num, page_md| {
            // For the first processed page, if metadata fields are still missing (e.g. scanned PDF where
            // initial header_slice had no digital text), attempt extraction from the OCR text of page 1.
            if page_num == begin_page
                && (metadata.author.is_none()
                    || metadata.publication_date.is_none()
                    || metadata.organization.is_none()
                    || metadata.journal.is_none()
                    || metadata.volume.is_none()
                    || metadata.doi.is_none()
                    || metadata.keywords.is_none())
            {
                let res = crate::metadata::extract_header_metadata_fallback(&page_md);
                let mut updated = false;
                if metadata.author.is_none() && !res.authors.is_empty() {
                    metadata.author = Some(res.authors.join(", "));
                    updated = true;
                }
                if metadata.publication_date.is_none() && res.date.is_some() {
                    metadata.publication_date = res.date;
                    updated = true;
                }
                if metadata.organization.is_none() && res.organization.is_some() {
                    metadata.organization = res.organization;
                    updated = true;
                }
                if metadata.journal.is_none() && res.journal.is_some() {
                    metadata.journal = res.journal;
                    updated = true;
                }
                if metadata.volume.is_none() && res.volume.is_some() {
                    metadata.volume = res.volume;
                    updated = true;
                }
                if metadata.issue.is_none() && res.issue.is_some() {
                    metadata.issue = res.issue;
                    updated = true;
                }
                if metadata.doi.is_none() && res.doi.is_some() {
                    metadata.doi = res.doi;
                    updated = true;
                }
                if metadata.keywords.is_none() && !res.keywords.is_empty() {
                    metadata.keywords = Some(res.keywords.join(", "));
                    updated = true;
                }
                if updated {
                    full_markdown = metadata.to_yaml_frontmatter();
                }
            }

            if !page_md.trim().is_empty() {
                full_markdown.push_str(&page_md);
                if !full_markdown.ends_with("\n\n") {
                    full_markdown.push_str("\n\n");
                }
            }

            // Write progressive updates to output path
            fs::write(output_path, normalize_latex_delimiters(&full_markdown)).with_context(|| {
                format!(
                    "failed to write markdown output to {}",
                    output_path.display()
                )
            })
        },
    )
    .with_context(|| format!("pipeline processing failed for {}", input_path.display()))?;

    println!(
        "Completed conversion: {} -> {}",
        input_path.display(),
        output_path.display()
    );

    Ok(())
}

/// Concurrent page processing worker engine that guarantees exact ascending page ordering.
pub fn process_pages_stream<F>(
    input_path: &Path,
    image_output_dir: &Path,
    begin_page: usize,
    end_page: usize,
    explicit_concurrency: Option<usize>,
    mut on_page: F,
) -> Result<()>
where
    F: FnMut(usize, String) -> Result<()>,
{
    validate_page_range(begin_page, end_page)?;

    let temp_dir = tempfile::tempdir().context("failed to create temporary rendering directory")?;
    let page_images = render_pages_to_images(input_path, temp_dir.path(), begin_page, end_page)
        .with_context(|| format!("failed to rasterize PDF {} for layout/OCR", input_path.display()))?;

    fs::create_dir_all(image_output_dir).with_context(|| {
        format!(
            "failed to create image output directory {}",
            image_output_dir.display()
        )
    })?;

    let pdf_bytes = fs::read(input_path).ok();
    let pdf_pages = pdf_bytes
        .as_ref()
        .and_then(|bytes| safe_extract_text_by_pages(bytes));

    let concurrency = resolve_concurrency(explicit_concurrency);
    let total_pages = page_images.len();

    let mut page_items = Vec::with_capacity(total_pages);
    for (page_offset, image_path) in page_images.into_iter().enumerate() {
        let page_num = begin_page + page_offset;
        let raw_page_text = pdf_pages
            .as_ref()
            .and_then(|pages| pages.get(page_num - 1))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        page_items.push((page_num, image_path, raw_page_text));
    }

    if concurrency <= 1 || total_pages <= 1 {
        for (page_num, image_path, raw_page_text) in page_items {
            let page_md = safe_process_single_page(
                Some(input_path),
                &image_path,
                image_output_dir,
                page_num,
                raw_page_text.as_deref(),
            );
            on_page(page_num, page_md)?;
        }
    } else {
        println!(
            "Processing {total_pages} pages concurrently (concurrency={concurrency}, ascending page order)..."
        );

        let (work_tx, work_rx) = mpsc::channel();
        for item in page_items {
            let _ = work_tx.send(item);
        }
        drop(work_tx);

        let (result_tx, result_rx) = mpsc::channel();
        let mut worker_handles = Vec::with_capacity(concurrency);
        let work_rx = Arc::new(Mutex::new(work_rx));
        let input_path_buf = input_path.to_path_buf();

        for _ in 0..concurrency {
            let work_rx_clone = Arc::clone(&work_rx);
            let result_tx_clone = result_tx.clone();
            let img_out_dir = image_output_dir.to_path_buf();
            let input_path_clone = input_path_buf.clone();

            let handle = thread::spawn(move || {
                loop {
                    let item = {
                        let Ok(rx) = work_rx_clone.lock() else { break };
                        rx.recv().ok()
                    };

                    let Some((page_num, image_path, raw_page_text)) = item else {
                        break;
                    };

                    let page_md = safe_process_single_page(
                        Some(&input_path_clone),
                        &image_path,
                        &img_out_dir,
                        page_num,
                        raw_page_text.as_deref(),
                    );

                    if result_tx_clone.send((page_num, page_md)).is_err() {
                        break;
                    }
                }
            });
            worker_handles.push(handle);
        }
        drop(result_tx);

        let mut pending_results: BTreeMap<usize, String> = BTreeMap::new();
        let mut next_expected_page = begin_page;

        while let Ok((page_num, page_md)) = result_rx.recv() {
            pending_results.insert(page_num, page_md);

            while let Some((&page, _)) = pending_results.iter().next() {
                if page == next_expected_page {
                    let (_, md) = pending_results.remove_entry(&page).unwrap();
                    on_page(page, md)?;
                    next_expected_page += 1;
                } else {
                    break;
                }
            }
        }

        for handle in worker_handles {
            let _ = handle.join();
        }

        while let Some((page, md)) = pending_results.pop_first() {
            if page == next_expected_page {
                next_expected_page += 1;
            }
            on_page(page, md)?;
        }
    }

    Ok(())
}

/// Executes single-page processing protected by panic and error boundaries, emitting an
/// Obsidian `> [!WARNING]` callout whenever an unrecoverable error or panic occurs.
pub fn safe_process_single_page(
    pdf_path: Option<&Path>,
    image_path: &Path,
    image_output_dir: &Path,
    page_num: usize,
    raw_page_text: Option<&str>,
) -> String {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_single_page(
            pdf_path,
            image_path,
            image_output_dir,
            page_num,
            raw_page_text,
        )
    }));

    match result {
        Ok(Ok(page_md)) => page_md,
        Ok(Err(err)) => {
            eprintln!("Warning: Page {page_num} processing failed ({err:#}). Emitting warning callout.");
            let page_demarcation = format_page_demarcation(page_num);
            let mut warning_block = format!(
                "{page_demarcation}> [!WARNING] Page {page_num} Processing Incomplete\n\
                 > An error occurred during page transcription: {err:#}\n\n"
            );
            if let Some(txt) = raw_page_text {
                if !txt.trim().is_empty() {
                    warning_block.push_str("### Recovered Stream Text\n\n");
                    warning_block.push_str(txt.trim());
                    warning_block.push_str("\n\n");
                }
            }
            warning_block
        }
        Err(_) => {
            eprintln!("Warning: Page {page_num} processing panicked. Emitting warning callout.");
            let page_demarcation = format_page_demarcation(page_num);
            let mut warning_block = format!(
                "{page_demarcation}> [!WARNING] Page {page_num} Processing Incomplete\n\
                 > An unexpected panic occurred while decoding or processing page {page_num}.\n\n"
            );
            if let Some(txt) = raw_page_text {
                if !txt.trim().is_empty() {
                    warning_block.push_str("### Recovered Stream Text\n\n");
                    warning_block.push_str(txt.trim());
                    warning_block.push_str("\n\n");
                }
            }
            warning_block
        }
    }
}

fn process_single_page(
    pdf_path: Option<&Path>,
    image_path: &Path,
    image_output_dir: &Path,
    page_num: usize,
    raw_page_text: Option<&str>,
) -> Result<String> {
    // Phase 1: YOLO Three-Way Layout Analysis
    let mut layout = figure::analyze_page_layout(
        pdf_path,
        image_path,
        image_output_dir,
        page_num,
    )?;

    let page_img = image::open(image_path)
        .with_context(|| format!("Failed to open image for OCR: {:?}", image_path))?;

    // Phase 2: Canvas Masking for Pictures (eliminates hallucinated text from drawings)
    let ocr_canvas = if !layout.picture_masks.is_empty() {
        mask_page_regions(&page_img, &layout.picture_masks)
    } else {
        page_img.clone()
    };

    // Phase 3: Text OCR on the clean canvas
    let engine = get_indra_engine()?;
    let (ocr_text, text_lines) = engine
        .transcribe_image_with_lines(&ocr_canvas)
        .with_context(|| format!("Indra OCR failed for page {page_num}"))?;

    // Phase 3.1: Proximity Caption Matching for Figures
    for fig in &mut layout.pictures {
        if fig.caption.is_none() {
            let rect = figure::caption::FigureRect {
                x: fig.x,
                y: fig.y,
                width: fig.width,
                height: fig.height,
            };
            let matched = figure::caption::match_proximity_caption(
                &rect,
                &text_lines,
                &figure::caption::CaptionMatchConfig::default(),
                (page_num, 1),
            );
            if matched.description.is_some() {
                fig.caption = matched.description;
            }
            if !matched.label.is_empty() {
                fig.figure_label = matched.label;
            }
        }
    }

    // Phase 4: Choose best body text representation
    let mut formatted_text = if ocr_text.trim().is_empty() {
        if let Some(txt) = raw_page_text {
            txt.to_string()
        } else {
            String::new()
        }
    } else {
        ocr_text
    };

    // Phase 5: Formula Box Recognition & Wrapping (DocLayNet Class 2)
    // Uses RapidLaTeXOCR neural equation recognition with seamless fallback to OCR text
    for formula_rect in &layout.formulas {
        let inside_lines: Vec<&TextLine> = text_lines
            .iter()
            .filter(|l| {
                let cx = l.bbox.x + l.bbox.width / 2;
                let cy = l.bbox.y + l.bbox.height / 2;
                cx >= formula_rect.x
                    && cx <= formula_rect.x + formula_rect.width
                    && cy >= formula_rect.y
                    && cy <= formula_rect.y + formula_rect.height
            })
            .collect();

        let mut recognized_latex: Option<String> = None;

        #[cfg(feature = "latex-ort")]
        if let Some(latex_engine) = figure::latex::get_latex_engine() {
            let (pw, ph) = page_img.dimensions();
            let crop_w = formula_rect.width.min(pw.saturating_sub(formula_rect.x));
            let crop_h = formula_rect.height.min(ph.saturating_sub(formula_rect.y));
            if crop_w > 0 && crop_h > 0 {
                let crop = page_img.crop_imm(formula_rect.x, formula_rect.y, crop_w, crop_h);
                if let Ok(pred) = latex_engine.predict_latex(&crop) {
                    if !pred.trim().is_empty() {
                        recognized_latex = Some(pred);
                    }
                }
            }
        }

        // Fall back to concatenating inside OCR lines if RapidLaTeXOCR is not available or yielded empty
        let formula_trimmed = if let Some(latex) = recognized_latex {
            latex
        } else if !inside_lines.is_empty() {
            let mut formula_text = String::new();
            for l in &inside_lines {
                let trimmed = l.text.trim();
                if !trimmed.is_empty() {
                    formula_text.push_str(trimmed);
                    formula_text.push(' ');
                }
            }
            formula_text.trim().to_string()
        } else {
            String::new()
        };

        if !formula_trimmed.is_empty() && !formula_trimmed.starts_with("$$") {
            let display_math = format!("\n\n$$\n{formula_trimmed}\n$$\n\n");
            // Replace first occurrence of the raw text with display math block
            if let Some(first_line) = inside_lines.first() {
                let needle = first_line.text.trim();
                if !needle.is_empty() && formatted_text.contains(needle) {
                    formatted_text = formatted_text.replacen(needle, &display_math, 1);
                } else {
                    formatted_text.push_str(&display_math);
                }
            } else {
                formatted_text.push_str(&display_math);
            }
        }
    }

    // Phase 6: Table Extraction (DocLayNet Class 8)
    // Uses neural SLANet table structure recognition with seamless geometric fallback
    for (table_rect, fig) in &layout.tables {
        let mut table_md_opt = None;
        let mut raw_lines: Vec<String> = Vec::new();

        #[cfg(feature = "slanet-ort")]
        if let Some(slanet) = figure::slanet::get_slanet_engine() {
            let crop = page_img.crop_imm(table_rect.x, table_rect.y, table_rect.width, table_rect.height);
            if let Ok(md) = slanet.predict_table_markdown(&crop, &text_lines, table_rect.x, table_rect.y) {
                if !md.trim().is_empty() {
                    table_md_opt = Some(md);
                    raw_lines = text_lines
                        .iter()
                        .filter(|l| {
                            let mid_x = l.bbox.x + l.bbox.width / 2;
                            let mid_y = l.bbox.y + l.bbox.height / 2;
                            mid_x >= table_rect.x
                                && mid_x <= table_rect.x + table_rect.width
                                && mid_y >= table_rect.y
                                && mid_y <= table_rect.y + table_rect.height
                        })
                        .map(|l| l.text.clone())
                        .collect();
                }
            }
        }

        // Fall back to pure geometric table grid extraction if SLANet not used or yields empty output
        if table_md_opt.is_none() {
            if let Some((geom_md, geom_lines)) =
                figure::table::extract_table_with_lines(table_rect, &text_lines)
            {
                table_md_opt = Some(geom_md);
                raw_lines = geom_lines;
            }
        }

        if let Some(table_md) = table_md_opt {
            let dir_name = fig.images_dir_name();
            let callout = format!(
                "> [!table]- Original Crop: {}\n> ![{}]({dir_name}/{})\n> *{}*",
                fig.figure_label,
                fig.figure_label,
                fig.filename,
                fig.caption.as_deref().unwrap_or(&fig.figure_label)
            );

            formatted_text = figure::table::integrate_table_into_markdown(
                &formatted_text,
                &table_md,
                &raw_lines,
                &fig.figure_label,
                Some(&callout),
            );
        }
    }

    // Phase 7: Attach visual figures (pictures) to body text
    let with_figures = attach_figures_to_page_text(&formatted_text, &layout.pictures);

    // Phase 8: Page Demarcation (<!-- Page N -->)
    let page_demarcation = format_page_demarcation(page_num);
    let final_page_text = format!("{page_demarcation}{with_figures}");

    Ok(final_page_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_process_single_page_emits_warning_on_failure() {
        let fake_image = Path::new("non_existent_page_9999.png");
        let fake_out = Path::new("fake_out");
        let raw_text = Some("Fallback extracted stream text on page 7.");

        let md = safe_process_single_page(None, fake_image, fake_out, 7, raw_text);

        assert!(md.contains("<!-- Page 7 -->"));
        assert!(md.contains("> [!WARNING] Page 7 Processing Incomplete"));
        assert!(md.contains("### Recovered Stream Text"));
        assert!(md.contains("Fallback extracted stream text on page 7."));
    }
}
