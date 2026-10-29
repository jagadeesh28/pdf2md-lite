//! Single-document pipeline orchestrator for pdfmd-lite.
//!
//! Orchestrates YOLOv8n three-way layout routing, Indra OCR, geometric table extraction,
//! LaTeX math normalization, and page indexing with ascending concurrency.

use anyhow::{Context, Result};
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

    let mut full_markdown = String::new();

    process_pages_stream(
        input_path,
        &image_output_dir,
        begin_page,
        end_page,
        explicit_concurrency,
        |_page_num, page_md| {
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
            let page_md = process_single_page(
                Some(input_path),
                &image_path,
                image_output_dir,
                page_num,
                raw_page_text.as_deref(),
            )?;
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

                    let page_res = process_single_page(
                        Some(&input_path_clone),
                        &image_path,
                        &img_out_dir,
                        page_num,
                        raw_page_text.as_deref(),
                    );

                    if result_tx_clone.send((page_num, page_res)).is_err() {
                        break;
                    }
                }
            });
            worker_handles.push(handle);
        }
        drop(result_tx);

        let mut pending_results: BTreeMap<usize, String> = BTreeMap::new();
        let mut next_expected_page = begin_page;

        while let Ok((page_num, page_res)) = result_rx.recv() {
            let page_md = page_res?;
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

    // Phase 5: Formula Box Wrapping (DocLayNet Class 2)
    // Any OCR line falling inside a detected formula bounding box is wrapped in display math $$..$$
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

        if !inside_lines.is_empty() {
            let mut formula_text = String::new();
            for l in &inside_lines {
                let trimmed = l.text.trim();
                if !trimmed.is_empty() {
                    formula_text.push_str(trimmed);
                    formula_text.push(' ');
                }
            }
            let formula_trimmed = formula_text.trim();
            if !formula_trimmed.is_empty() && !formula_trimmed.starts_with("$$") {
                let display_math = format!("\n\n$$\n{formula_trimmed}\n$$\n\n");
                // Replace first occurrence of the raw text with display math block
                if let Some(first_line) = inside_lines.first() {
                    let needle = first_line.text.trim();
                    if !needle.is_empty() && formatted_text.contains(needle) {
                        formatted_text = formatted_text.replacen(needle, &display_math, 1);
                    }
                }
            }
        }
    }

    // Phase 6: Geometric Table Grid Extraction (DocLayNet Class 8)
    for (table_rect, fig) in &layout.tables {
        if let Some((table_md, raw_lines)) =
            figure::table::extract_table_with_lines(table_rect, &text_lines)
        {
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
