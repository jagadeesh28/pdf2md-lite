use anyhow::{Context, Result, anyhow};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::backends::indra::{TextBoundingBox, TextLine};
use crate::figure::caption::FigureRect;
use crate::markdown::{DocumentMetadata, PdfExtractionResult};

/// Controls the strategy for baseline-aware hybrid PDF DOM text extraction (ADR 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DomExtractionMode {
    /// Automatically validates font CMaps and text density; uses hybrid extraction on clean vector PDFs
    /// while falling back to pure vision OCR on scanned or unmapped-font documents (default).
    #[default]
    Auto,
    /// Unconditionally forces digital DOM text stream extraction.
    Always,
    /// Bypasses DOM stream extraction and uses pure Indra neural vision OCR exclusively.
    Never,
}

impl DomExtractionMode {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "always" | "force" | "true" => Some(Self::Always),
            "never" | "false" | "off" => Some(Self::Never),
            _ => None,
        }
    }
}

/// Validates that `begin_page` and `end_page` are 1-based and valid.
pub fn validate_page_range(begin_page: usize, end_page: usize) -> Result<()> {
    if begin_page == 0 || end_page == 0 || begin_page > end_page {
        return Err(anyhow!(
            "invalid page range {}-{}; pages are 1-based and begin-page must not exceed end-page",
            begin_page,
            end_page
        ));
    }

    Ok(())
}

/// Returns true if extracted text is too sparse or empty, requiring OCR fallback.
pub fn should_use_ocr_fallback(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty() || trimmed.len() < 20
}

/// Normalizes line endings in extracted text.
pub fn clean_extracted_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Converts a rendered `hayro` pixmap directly into an in-memory `image::DynamicImage` (RGBA8),
/// avoiding any intermediate PNG encoding or temporary disk writes.
pub fn pixmap_to_dynamic_image(pixmap: &hayro::vello_cpu::Pixmap) -> image::DynamicImage {
    let width = pixmap.width() as u32;
    let height = pixmap.height() as u32;
    let raw_bytes = pixmap.data_as_u8_slice();
    let rgba = image::RgbaImage::from_raw(width, height, raw_bytes.to_vec())
        .unwrap_or_else(|| image::RgbaImage::new(width, height));
    image::DynamicImage::ImageRgba8(rgba)
}

/// Streams rendered `DynamicImage` pages through a channel as they are decoded by `hayro`,
/// completely eliminating intermediate PNG encoding and disk writes.
pub fn stream_rendered_pages(
    pdf_path: &Path,
    begin_page: usize,
    end_page: usize,
    tx: std::sync::mpsc::SyncSender<Result<(usize, image::DynamicImage)>>,
) -> Result<()> {
    validate_page_range(begin_page, end_page)?;
    let pdf_bytes = fs::read(pdf_path)
        .with_context(|| format!("failed to read PDF for rendering: {}", pdf_path.display()))?;
    let document = Pdf::new(pdf_bytes).map_err(|error| {
        anyhow!(
            "failed to parse PDF for rendering {}: {error:?}",
            pdf_path.display()
        )
    })?;
    let cache = hayro::RenderCache::new();
    let interpreter_settings = InterpreterSettings::default();
    let render_settings = hayro::RenderSettings::default();
    let pixmap_settings = hayro::PixmapSettings {
        x_scale: 2.0,
        y_scale: 2.0,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };

    let total_pages = document.pages().len();
    if end_page > total_pages {
        return Err(anyhow!(
            "page range {}-{} is outside the PDF's {} pages",
            begin_page,
            end_page,
            total_pages
        ));
    }

    for (index, page) in document
        .pages()
        .iter()
        .enumerate()
        .skip(begin_page - 1)
        .take(end_page - begin_page + 1)
    {
        let page_num = index + 1;
        let pixmap = hayro::render(
            page,
            &cache,
            &interpreter_settings,
            &render_settings,
            &pixmap_settings,
        );
        let dynamic_image = pixmap_to_dynamic_image(&pixmap);
        if tx.send(Ok((page_num, dynamic_image))).is_err() {
            // Receiver closed channel early (e.g. error or pipeline abort)
            break;
        }
    }

    Ok(())
}

/// Renders PDF pages into PNG images within a `pages` directory under `temp_dir`.
pub fn render_pages_to_images(
    pdf_path: &Path,
    temp_dir: &Path,
    begin_page: usize,
    end_page: usize,
) -> Result<Vec<PathBuf>> {
    let output_dir = temp_dir.join("pages");
    fs::create_dir_all(&output_dir).context("failed to create temporary page image directory")?;

    let pdf_bytes = fs::read(pdf_path)
        .with_context(|| format!("failed to read PDF for rendering: {}", pdf_path.display()))?;
    let document = Pdf::new(pdf_bytes).map_err(|error| {
        anyhow!(
            "failed to parse PDF for rendering {}: {error:?}",
            pdf_path.display()
        )
    })?;
    let cache = hayro::RenderCache::new();
    let interpreter_settings = InterpreterSettings::default();
    let render_settings = hayro::RenderSettings::default();
    let pixmap_settings = hayro::PixmapSettings {
        x_scale: 2.0,
        y_scale: 2.0,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };

    let total_pages = document.pages().len();
    if end_page > total_pages {
        return Err(anyhow!(
            "page range {}-{} is outside the PDF's {} pages",
            begin_page,
            end_page,
            total_pages
        ));
    }

    let mut page_paths = Vec::with_capacity(end_page.saturating_sub(begin_page) + 1);

    for (index, page) in document
        .pages()
        .iter()
        .enumerate()
        .skip(begin_page - 1)
        .take(end_page - begin_page + 1)
    {
        let page_path = output_dir.join(format!("page-{}.png", index + 1));
        let png = hayro::render(
            page,
            &cache,
            &interpreter_settings,
            &render_settings,
            &pixmap_settings,
        )
        .into_png()
        .with_context(|| format!("failed to encode rendered PDF page {}", index + 1))?;
        fs::write(&page_path, png)
            .with_context(|| format!("failed to save rendered page {}", index + 1))?;
        page_paths.push(page_path);
    }

    Ok(page_paths)
}

/// Safely extracts text by pages from PDF memory, catching and suppressing any panics
/// caused by malformed/unsupported font encoding tables inside pdf-extract.
pub fn safe_extract_text_by_pages(bytes: &[u8]) -> Option<Vec<String>> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem_by_pages(bytes).ok()
    }));
    std::panic::set_hook(prev_hook);
    res.ok().flatten()
}

/// Determines the total number of pages in a PDF file quickly without rasterizing pages.
pub fn get_pdf_page_count(pdf_path: &Path) -> Result<usize> {
    let pdf_bytes = fs::read(pdf_path)
        .with_context(|| format!("failed to read PDF file {}", pdf_path.display()))?;

    if let Ok(document) = Pdf::new(pdf_bytes.clone()) {
        let count = document.pages().len();
        if count > 0 {
            return Ok(count);
        }
    }

    if let Some(pages) = safe_extract_text_by_pages(&pdf_bytes)
        && !pages.is_empty()
    {
        return Ok(pages.len());
    }

    Err(anyhow!(
        "failed to determine page count for PDF {}",
        pdf_path.display()
    ))
}

/// Attempts to extract plain text directly from the PDF streams without OCR.
pub fn extract_pdf_text(
    input_path: &Path,
    begin_page: usize,
    end_page: usize,
) -> Result<PdfExtractionResult> {
    validate_page_range(begin_page, end_page)?;

    let bytes = fs::read(input_path)
        .with_context(|| format!("failed to read PDF file {}", input_path.display()))?;

    let pages = safe_extract_text_by_pages(&bytes).ok_or_else(|| {
        anyhow!(
            "failed to parse PDF text streams in {}",
            input_path.display()
        )
    })?;
    if end_page > pages.len() {
        return Err(anyhow!(
            "page range {}-{} is outside the PDF's {} pages",
            begin_page,
            end_page,
            pages.len()
        ));
    }

    let text = pages[begin_page - 1..end_page].join("\n\n");

    let cleaned = clean_extracted_text(&text);
    if should_use_ocr_fallback(&cleaned) {
        return Err(anyhow!(
            "No readable text was found in the PDF; the scanned-PDF OCR fallback is required."
        ));
    }

    Ok(PdfExtractionResult {
        text: cleaned,
        images: Vec::new(),
        figures: Vec::new(),
    })
}

/// Extracts embedded metadata properties (title, author, subject, dates) from a PDF.
pub fn extract_pdf_metadata(pdf_path: &Path) -> DocumentMetadata {
    let mut meta = DocumentMetadata::default();

    if let Ok(doc) = lopdf::Document::load(pdf_path) {
        let info_obj = doc.trailer.get(b"Info").ok();
        let info_dict = match info_obj {
            Some(lopdf::Object::Reference(id)) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_dict().ok().cloned()),
            Some(lopdf::Object::Dictionary(dict)) => Some(dict.clone()),
            _ => None,
        };

        if let Some(dict) = info_dict {
            if let Some(val) = dict.get(b"Title").ok().and_then(decode_pdf_string)
                && !val.trim().is_empty()
            {
                meta.title = Some(val);
            }
            if let Some(val) = dict.get(b"Author").ok().and_then(decode_pdf_string)
                && !val.trim().is_empty()
            {
                meta.author = Some(val);
            }
            if let Some(val) = dict.get(b"Subject").ok().and_then(decode_pdf_string)
                && !val.trim().is_empty()
            {
                meta.subject = Some(val);
            }
            if let Some(val) = dict.get(b"Keywords").ok().and_then(decode_pdf_string)
                && !val.trim().is_empty()
            {
                meta.keywords = Some(val);
            }
            if let Some(val) = dict.get(b"CreationDate").ok().and_then(decode_pdf_string)
                && !val.trim().is_empty()
            {
                meta.creation_date = Some(clean_pdf_date(&val));
            }
        }
    }

    if let Ok(count) = get_pdf_page_count(pdf_path) {
        meta.total_pages = Some(count);
    }

    meta
}

fn decode_pdf_string(obj: &lopdf::Object) -> Option<String> {
    match obj {
        lopdf::Object::String(bytes, _) => {
            if bytes.starts_with(&[0xfe, 0xff]) {
                let u16_chars: Vec<u16> = bytes[2..]
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect();
                Some(String::from_utf16_lossy(&u16_chars).trim().to_string())
            } else {
                Some(String::from_utf8_lossy(bytes).trim().to_string())
            }
        }
        lopdf::Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        _ => None,
    }
}

fn clean_pdf_date(raw: &str) -> String {
    let s = raw.strip_prefix("D:").unwrap_or(raw);
    if s.len() >= 8 {
        let year = &s[0..4];
        let month = &s[4..6];
        let day = &s[6..8];
        if year.chars().all(|c| c.is_ascii_digit())
            && month.chars().all(|c| c.is_ascii_digit())
            && day.chars().all(|c| c.is_ascii_digit())
        {
            return format!("{year}-{month}-{day}");
        }
    }
    s.to_string()
}

/// Determines if a specific PDF page can utilize fast hybrid DOM text extraction (ADR 0012).
///
/// In `Auto` mode:
/// - Verifies that the page contains digital font dictionary entries.
/// - Checks if symbolic/math fonts (CMMI, CMSY, MSBM) have `/ToUnicode` CMaps. If symbolic fonts lack CMaps,
///   falls back to Indra OCR to prevent corrupted mathematical glyphs.
/// - Verifies that the page content stream exists and is non-empty.
pub fn can_use_hybrid_dom(doc: &lopdf::Document, page_num: usize, mode: DomExtractionMode) -> bool {
    match mode {
        DomExtractionMode::Never => false,
        DomExtractionMode::Always => true,
        DomExtractionMode::Auto => {
            let pages = doc.get_pages();
            let page_id = match pages.get(&(page_num as u32)) {
                Some(id) => *id,
                None => return false,
            };

            // Inspect page fonts
            let fonts = match doc.get_page_fonts(page_id) {
                Ok(f) => f,
                Err(_) => return false,
            };

            if fonts.is_empty() {
                // Scanned PDF page without digital fonts -> use vision OCR
                return false;
            }

            for font_dict in fonts.values() {
                let base_font_name = font_dict
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).to_uppercase())
                    .unwrap_or_default();

                let is_math_or_symbol = base_font_name.contains("CMMI")
                    || base_font_name.contains("CMSY")
                    || base_font_name.contains("MSBM")
                    || base_font_name.contains("MATH")
                    || base_font_name.contains("SYMBOL");

                if is_math_or_symbol && font_dict.get(b"ToUnicode").is_err() {
                    // Specialized math font lacking Unicode CMap -> safe fallback to Indra OCR
                    return false;
                }
            }

            // Verify content stream exists and is non-empty
            let content_bytes = doc.get_page_content(page_id);
            if content_bytes.is_empty() {
                return false;
            }

            true
        }
    }
}

fn obj_to_f32(obj: &lopdf::Object) -> Option<f32> {
    match obj {
        lopdf::Object::Real(f) => Some(*f),
        lopdf::Object::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

/// Helper to extract page bounding box from MediaBox or CropBox.
fn get_page_bbox(doc: &lopdf::Document, page_dict: &lopdf::Dictionary) -> (f32, f32, f32, f32) {
    let box_obj = page_dict
        .get(b"CropBox")
        .or_else(|_| page_dict.get(b"MediaBox"))
        .ok();

    if let Some(obj) = box_obj {
        let arr = match obj {
            lopdf::Object::Array(a) => a.as_slice(),
            lopdf::Object::Reference(id) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| a.as_slice())
                .unwrap_or(&[]),
            _ => &[],
        };
        if arr.len() >= 4 {
            let x0 = obj_to_f32(&arr[0]).unwrap_or(0.0);
            let y0 = obj_to_f32(&arr[1]).unwrap_or(0.0);
            let x1 = obj_to_f32(&arr[2]).unwrap_or(595.0);
            let y1 = obj_to_f32(&arr[3]).unwrap_or(842.0);
            return (x0, y0, x1, y1);
        }
    }

    (0.0, 0.0, 595.0, 842.0)
}

fn is_box_overlapping(a: &TextBoundingBox, b: &FigureRect) -> bool {
    let a_x2 = a.x + a.width;
    let a_y2 = a.y + a.height;
    let b_x2 = b.x + b.width;
    let b_y2 = b.y + b.height;

    a.x < b_x2 && a_x2 > b.x && a.y < b_y2 && a_y2 > b.y
}

fn decode_bytes_to_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let u16_chars: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&u16_chars)
    } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(&bytes[3..]).to_string()
    } else {
        // Standard Latin-1 / ASCII decoding
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Formats a token as subscript (`_c` or `_{...}`) or superscript (`^c` or `^{...}`).
fn format_sub_super_script(text: &str, is_superscript: bool) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let marker = if is_superscript { '^' } else { '_' };
    if trimmed.chars().count() == 1 {
        format!("{marker}{trimmed}")
    } else {
        format!("{marker}{{{trimmed}}}")
    }
}

/// Parses decoded PDF content operations into spatial `TextLine` structures with baseline tracking.
pub fn parse_content_text_lines(
    content: &lopdf::content::Content,
    scale: f32,
    page_h_pts: f32,
    regions_to_exclude: &[FigureRect],
) -> Vec<TextLine> {
    let mut lines = Vec::new();

    let mut matrix = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut line_matrix = matrix;
    let mut font_size = 12.0f32;
    let mut base_font_size = 12.0f32;
    let mut line_baseline_y = 0.0f32;
    let mut leading = 14.0f32;
    let mut rise = 0.0f32;

    let mut current_line_text = String::new();
    let mut line_start_x = 0.0f32;
    let mut line_start_y = 0.0f32;
    let mut line_char_count = 0usize;
    let mut line_active = false;

    let flush_line = |lines: &mut Vec<TextLine>,
                      text: &mut String,
                      x_pts: f32,
                      y_pts: f32,
                      char_count: usize,
                      base_fs: f32,
                      active: &mut bool| {
        let trimmed = text.trim();
        if !trimmed.is_empty() && *active {
            let w_pts = (char_count as f32 * base_fs * 0.52).max(10.0);
            let h_pts = base_fs * 1.2;

            let x_px = (x_pts * scale).max(0.0) as u32;
            let y_px = (((page_h_pts - y_pts - h_pts) * scale).max(0.0)) as u32;
            let w_px = (w_pts * scale) as u32;
            let h_px = (h_pts * scale) as u32;

            let bbox = TextBoundingBox {
                x: x_px,
                y: y_px,
                width: w_px,
                height: h_px,
                score: 0.99,
            };

            // Only retain line if it does not fall inside an excluded layout region (formula/table/picture)
            if !regions_to_exclude
                .iter()
                .any(|r| is_box_overlapping(&bbox, r))
            {
                lines.push(TextLine {
                    bbox,
                    text: trimmed.to_string(),
                });
            }
        }
        text.clear();
        *active = false;
    };

    for op in &content.operations {
        match op.operator.as_str() {
            "BT" => {
                matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                line_matrix = matrix;
                rise = 0.0;
            }
            "ET" => {
                flush_line(
                    &mut lines,
                    &mut current_line_text,
                    line_start_x,
                    line_start_y,
                    line_char_count,
                    base_font_size,
                    &mut line_active,
                );
            }
            "Tf" if op.operands.len() >= 2 => {
                if let Some(fs) = obj_to_f32(&op.operands[1]) {
                    font_size = fs.abs().max(1.0);
                }
            }
            "Ts" => {
                if let Some(first) = op.operands.first() {
                    rise = obj_to_f32(first).unwrap_or(0.0);
                }
            }
            "TL" => {
                if let Some(first) = op.operands.first() {
                    leading = obj_to_f32(first).unwrap_or(14.0).abs().max(1.0);
                }
            }
            "Tm" if op.operands.len() >= 6 => {
                let a = obj_to_f32(&op.operands[0]).unwrap_or(1.0);
                let b = obj_to_f32(&op.operands[1]).unwrap_or(0.0);
                let c = obj_to_f32(&op.operands[2]).unwrap_or(0.0);
                let d = obj_to_f32(&op.operands[3]).unwrap_or(1.0);
                let e = obj_to_f32(&op.operands[4]).unwrap_or(0.0);
                let f = obj_to_f32(&op.operands[5]).unwrap_or(0.0);

                let is_new_line = !line_active
                    || (f - line_baseline_y).abs() > (base_font_size * 0.7)
                    || e < line_start_x;

                if is_new_line {
                    flush_line(
                        &mut lines,
                        &mut current_line_text,
                        line_start_x,
                        line_start_y,
                        line_char_count,
                        base_font_size,
                        &mut line_active,
                    );
                    line_start_x = e;
                    line_start_y = f;
                    line_baseline_y = f;
                    base_font_size = font_size;
                    line_char_count = 0;
                    line_active = true;
                }

                matrix = [a, b, c, d, e, f];
                line_matrix = matrix;
            }
            "Td" if op.operands.len() >= 2 => {
                let tx = obj_to_f32(&op.operands[0]).unwrap_or(0.0);
                let ty = obj_to_f32(&op.operands[1]).unwrap_or(0.0);

                if ty.abs() > 2.0 {
                    flush_line(
                        &mut lines,
                        &mut current_line_text,
                        line_start_x,
                        line_start_y,
                        line_char_count,
                        base_font_size,
                        &mut line_active,
                    );
                }

                line_matrix[4] += tx * line_matrix[0] + ty * line_matrix[2];
                line_matrix[5] += tx * line_matrix[1] + ty * line_matrix[3];
                matrix = line_matrix;

                if ty.abs() > 2.0 || !line_active {
                    line_start_x = matrix[4];
                    line_start_y = matrix[5];
                    line_baseline_y = matrix[5];
                    base_font_size = font_size;
                    line_char_count = 0;
                    line_active = true;
                }
            }
            "TD" if op.operands.len() >= 2 => {
                let tx = obj_to_f32(&op.operands[0]).unwrap_or(0.0);
                let ty = obj_to_f32(&op.operands[1]).unwrap_or(0.0);
                leading = -ty;

                flush_line(
                    &mut lines,
                    &mut current_line_text,
                    line_start_x,
                    line_start_y,
                    line_char_count,
                    base_font_size,
                    &mut line_active,
                );

                line_matrix[4] += tx * line_matrix[0] + ty * line_matrix[2];
                line_matrix[5] += tx * line_matrix[1] + ty * line_matrix[3];
                matrix = line_matrix;

                line_start_x = matrix[4];
                line_start_y = matrix[5];
                line_baseline_y = matrix[5];
                base_font_size = font_size;
                line_char_count = 0;
                line_active = true;
            }
            "T*" => {
                flush_line(
                    &mut lines,
                    &mut current_line_text,
                    line_start_x,
                    line_start_y,
                    line_char_count,
                    base_font_size,
                    &mut line_active,
                );
                line_matrix[5] -= leading;
                matrix = line_matrix;
                line_start_x = matrix[4];
                line_start_y = matrix[5];
                line_baseline_y = matrix[5];
                base_font_size = font_size;
                line_char_count = 0;
                line_active = true;
            }
            "Tj" => {
                if let Some(lopdf::Object::String(bytes, _)) = op.operands.first() {
                    let decoded = decode_bytes_to_string(bytes);
                    if !decoded.is_empty() {
                        let delta_y = (matrix[5] - line_baseline_y) + rise;
                        let is_reduced_size = font_size < 0.88 * base_font_size;

                        if is_reduced_size && delta_y > 1.5 {
                            // Superscript
                            current_line_text.push_str(&format_sub_super_script(&decoded, true));
                        } else if is_reduced_size && delta_y < -1.5 {
                            // Subscript
                            current_line_text.push_str(&format_sub_super_script(&decoded, false));
                        } else {
                            current_line_text.push_str(&decoded);
                            if !line_active {
                                line_baseline_y = matrix[5];
                                base_font_size = font_size;
                            }
                        }
                        line_char_count += decoded.chars().count();
                        line_active = true;
                    }
                }
            }
            "TJ" => {
                if let Some(lopdf::Object::Array(items)) = op.operands.first() {
                    for item in items {
                        match item {
                            lopdf::Object::String(bytes, _) => {
                                let decoded = decode_bytes_to_string(bytes);
                                if !decoded.is_empty() {
                                    let delta_y = (matrix[5] - line_baseline_y) + rise;
                                    let is_reduced_size = font_size < 0.88 * base_font_size;

                                    if is_reduced_size && delta_y > 1.5 {
                                        current_line_text
                                            .push_str(&format_sub_super_script(&decoded, true));
                                    } else if is_reduced_size && delta_y < -1.5 {
                                        current_line_text
                                            .push_str(&format_sub_super_script(&decoded, false));
                                    } else {
                                        current_line_text.push_str(&decoded);
                                    }
                                    line_char_count += decoded.chars().count();
                                    line_active = true;
                                }
                            }
                            lopdf::Object::Integer(k)
                                if *k < -180 && !current_line_text.ends_with(' ') =>
                            {
                                current_line_text.push(' ');
                            }
                            lopdf::Object::Real(k)
                                if *k < -180.0 && !current_line_text.ends_with(' ') =>
                            {
                                current_line_text.push(' ');
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Flush any trailing line at the end of the content stream
    flush_line(
        &mut lines,
        &mut current_line_text,
        line_start_x,
        line_start_y,
        line_char_count,
        base_font_size,
        &mut line_active,
    );

    lines
}

/// Extracts text lines directly from PDF content streams with baseline vertical offset tracking (ADR 0012).
///
/// Tracks relative $\Delta y$ coordinate shifts and font size changes to synthesize Markdown subscripts
/// (`_i` or `_{sub}`) and superscripts (`^2` or `^{sup}`), while filtering out text falling inside
/// neural layout regions (formulas, tables, pictures) to ensure clean separation of concerns.
pub fn extract_baseline_aware_dom_text_lines(
    doc: &lopdf::Document,
    page_num: usize,
    scale: f32,
    regions_to_exclude: &[FigureRect],
) -> Result<Vec<TextLine>> {
    let pages = doc.get_pages();
    let page_id = pages
        .get(&(page_num as u32))
        .copied()
        .ok_or_else(|| anyhow!("Page {page_num} not found in PDF document"))?;

    let content_bytes = doc.get_page_content(page_id);
    if content_bytes.is_empty() {
        return Ok(Vec::new());
    }
    let content = lopdf::content::Content::decode(&content_bytes)?;

    let (_page_w_pts, page_h_pts) = {
        if let Ok(page_dict) = doc.get_dictionary(page_id) {
            let (x0, y0, x1, y1) = get_page_bbox(doc, page_dict);
            ((x1 - x0).abs(), (y1 - y0).abs())
        } else {
            (595.0, 842.0)
        }
    };

    let lines = parse_content_text_lines(&content, scale, page_h_pts, regions_to_exclude);
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dom_extraction_mode_parsing() {
        assert_eq!(
            DomExtractionMode::from_str_loose("auto"),
            Some(DomExtractionMode::Auto)
        );
        assert_eq!(
            DomExtractionMode::from_str_loose("ALWAYS"),
            Some(DomExtractionMode::Always)
        );
        assert_eq!(
            DomExtractionMode::from_str_loose("force"),
            Some(DomExtractionMode::Always)
        );
        assert_eq!(
            DomExtractionMode::from_str_loose("never"),
            Some(DomExtractionMode::Never)
        );
        assert_eq!(
            DomExtractionMode::from_str_loose("off"),
            Some(DomExtractionMode::Never)
        );
        assert_eq!(DomExtractionMode::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_baseline_aware_subscript_superscript_detection() {
        // Construct PDF stream: "T" followed by subscript "e" (reduced font size, negative delta y)
        // and "x" followed by superscript "2" (reduced font size, positive delta y)
        let stream = b"
            BT
            /F1 12 Tf
            1 0 0 1 72 700 Tm
            (T) Tj
            /F1 8 Tf
            1 0 0 1 80 697 Tm
            (e) Tj
            /F1 12 Tf
            1 0 0 1 95 700 Tm
            ( and x) Tj
            /F1 8 Tf
            1 0 0 1 125 704 Tm
            (2) Tj
            ET
        ";

        let content = lopdf::content::Content::decode(stream).unwrap();
        let lines = parse_content_text_lines(&content, 2.0, 842.0, &[]);

        assert!(!lines.is_empty());
        let full_text: String = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(full_text.contains("T_e"));
        assert!(full_text.contains("x^2"));
    }

    #[test]
    fn test_dom_exclusion_of_layout_regions() {
        let stream = b"
            BT
            /F1 12 Tf
            1 0 0 1 72 700 Tm
            (Prose text before formula) Tj
            1 0 0 1 72 650 Tm
            (E = mc^2 formula text) Tj
            1 0 0 1 72 600 Tm
            (Prose text after formula) Tj
            ET
        ";

        let content = lopdf::content::Content::decode(stream).unwrap();

        // Formula bounding box in pixel space (page_h = 842 pt * 2.0 = 1684 px)
        // At y_pts = 650, y_px ≈ (842 - 650 - 14.4) * 2 ≈ 355 px
        let formula_region = FigureRect {
            x: 100,
            y: 340,
            width: 300,
            height: 50,
        };

        let lines = parse_content_text_lines(&content, 2.0, 842.0, &[formula_region]);

        // The excluded formula text should NOT be present in lines
        assert!(!lines.iter().any(|l| l.text.contains("E = mc^2")));
        // Surrounding prose should be retained
        assert!(
            lines
                .iter()
                .any(|l| l.text.contains("Prose text before formula"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.text.contains("Prose text after formula"))
        );
    }
}
