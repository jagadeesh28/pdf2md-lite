use anyhow::{Context, Result, anyhow};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backends::indra::{TextBoundingBox, TextLine};
use crate::figure::caption::FigureRect;
use crate::markdown::{DocumentMetadata, PdfExtractionResult};

/// Controls the strategy for baseline-aware hybrid PDF DOM text extraction (ADR 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DomExtractionMode {
    /// Automatically validates font CMaps and text density when explicitly requested.
    Auto,
    /// Unconditionally forces digital DOM text stream extraction.
    Always,
    /// Bypasses DOM stream extraction and uses pure Indra neural vision OCR exclusively (default).
    #[default]
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
    tx: std::sync::mpsc::SyncSender<(usize, Result<image::DynamicImage>)>,
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
        let render_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let pixmap = hayro::render(
                page,
                &cache,
                &interpreter_settings,
                &render_settings,
                &pixmap_settings,
            );
            pixmap_to_dynamic_image(&pixmap)
        }));
        let img_result = match render_res {
            Ok(dynamic_image) => Ok(dynamic_image),
            Err(_) => Err(anyhow!("Panic during page rasterization")),
        };
        if tx.send((page_num, img_result)).is_err() {
            // Receiver closed channel early (e.g. error or pipeline abort)
            break;
        }
    }

    Ok(())
}

/// Renders a single page of a PDF document directly into an in-memory `DynamicImage` (RGBA8)
/// at the specified scale factor without intermediate disk writes (ADR 0013 & ADR 0015).
pub fn render_single_page_image(
    pdf_path: &Path,
    page_num: usize,
    scale: f32,
) -> Result<image::DynamicImage> {
    let pdf_bytes = fs::read(pdf_path)
        .with_context(|| format!("failed to read PDF for rendering: {}", pdf_path.display()))?;
    let document = Pdf::new(pdf_bytes).map_err(|error| {
        anyhow!(
            "failed to parse PDF for rendering {}: {error:?}",
            pdf_path.display()
        )
    })?;

    let total_pages = document.pages().len();
    if page_num == 0 || page_num > total_pages {
        return Err(anyhow!(
            "page {page_num} is outside document's {total_pages} pages"
        ));
    }

    let page = &document.pages()[page_num - 1];
    let cache = hayro::RenderCache::new();
    let interpreter_settings = InterpreterSettings::default();
    let render_settings = hayro::RenderSettings::default();
    let pixmap_settings = hayro::PixmapSettings {
        x_scale: scale.max(0.5),
        y_scale: scale.max(0.5),
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };

    let pixmap = hayro::render(
        page,
        &cache,
        &interpreter_settings,
        &render_settings,
        &pixmap_settings,
    );
    Ok(pixmap_to_dynamic_image(&pixmap))
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

std::thread_local! {
    static SUPPRESS_PANIC_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static PANIC_HOOK_INITIALIZED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Initializes a process-wide panic hook that honors thread-local suppression.
pub fn init_process_panic_hook() {
    PANIC_HOOK_INITIALIZED.get_or_init(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let suppressed = SUPPRESS_PANIC_HOOK.with(|flag| flag.get());
            if !suppressed {
                default_hook(info);
            }
        }));
    });
}

/// Safely extracts text by pages from PDF memory, catching and suppressing any panics
/// caused by malformed/unsupported font encoding tables inside pdf-extract.
pub fn safe_extract_text_by_pages(bytes: &[u8]) -> Option<Vec<String>> {
    init_process_panic_hook();
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            SUPPRESS_PANIC_HOOK.with(|flag| flag.set(false));
        }
    }
    SUPPRESS_PANIC_HOOK.with(|flag| flag.set(true));
    let _guard = Guard;

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem_by_pages(bytes).ok()
    }));
    res.ok().flatten()
}

/// Determines the total number of pages in a PDF file quickly without rasterizing pages.
pub fn get_pdf_page_count(pdf_path: &Path) -> Result<usize> {
    let pdf_bytes = fs::read(pdf_path)
        .with_context(|| format!("failed to read PDF file {}", pdf_path.display()))?;

    if let Some(pages) = safe_extract_text_by_pages(&pdf_bytes)
        && !pages.is_empty()
    {
        return Ok(pages.len());
    }

    if let Ok(document) = Pdf::new(pdf_bytes) {
        let count = document.pages().len();
        if count > 0 {
            return Ok(count);
        }
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
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|chunk| u16::from_be_bytes(*chunk))
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

                // If subsetted or Type0 font lacks ToUnicode, fallback to Indra OCR in Auto mode
                let subtype = font_dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .unwrap_or(b"");
                let is_subset_or_type0 = base_font_name.contains('+') || subtype == b"Type0";
                if is_subset_or_type0 && font_dict.get(b"ToUnicode").is_err() {
                    return false;
                }
            }

            // Verify content stream exists and is non-empty
            let content_bytes = doc.get_page_content(page_id);
            !content_bytes.is_empty()
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

/// Represents a parsed PDF font decoder with `/ToUnicode` CMap and multi-byte support.
#[derive(Debug, Clone, Default)]
pub struct PdfFontDecoder {
    /// Maps character codes (single byte or 2-byte CID) to decoded Unicode text.
    pub cmap: HashMap<u32, String>,
    /// Whether this font uses 2-byte character codes (e.g., Type0 / Identity-H).
    pub is_two_byte: bool,
}

impl PdfFontDecoder {
    /// Decodes raw PDF string bytes into clean Unicode text.
    pub fn decode(&self, bytes: &[u8]) -> String {
        if bytes.starts_with(&[0xfe, 0xff]) {
            let u16_chars: Vec<u16> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect();
            return String::from_utf16_lossy(&u16_chars);
        } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            return String::from_utf8_lossy(&bytes[3..]).to_string();
        }

        if !self.cmap.is_empty() {
            let mut out = String::new();
            if self.is_two_byte {
                let mut i = 0;
                while i + 1 < bytes.len() {
                    let code = ((bytes[i] as u32) << 8) | (bytes[i + 1] as u32);
                    if let Some(s) = self.cmap.get(&code) {
                        out.push_str(s);
                    } else if bytes[i] == 0 && (32..=126).contains(&bytes[i + 1]) {
                        out.push(bytes[i + 1] as char);
                    }
                    i += 2;
                }
                if i < bytes.len() {
                    let code = bytes[i] as u32;
                    if let Some(s) = self.cmap.get(&code) {
                        out.push_str(s);
                    }
                }
            } else {
                for &b in bytes {
                    let code = b as u32;
                    if let Some(s) = self.cmap.get(&code) {
                        out.push_str(s);
                    } else if (32..=126).contains(&b) {
                        out.push(b as char);
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }

        // Standard Latin-1 / ASCII decoding, sanitizing non-printable control characters
        bytes
            .iter()
            .filter_map(|&b| {
                if (32..=126).contains(&b) || b == b'\t' || b == b'\n' || b >= 160 {
                    Some(b as char)
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Parses a PDF `/ToUnicode` CMap stream into a character-code to Unicode mapping.
pub fn parse_cmap_str(cmap: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();

    let parse_hex_code = |s: &str| -> Option<u32> {
        let trimmed = s.trim().trim_start_matches('<').trim_end_matches('>');
        u32::from_str_radix(trimmed, 16).ok()
    };

    let parse_dst_utf16 = |s: &str| -> Option<String> {
        let trimmed = s.trim().trim_start_matches('<').trim_end_matches('>');
        if trimmed.is_empty() {
            return None;
        }
        let mut hex_padded = trimmed.to_string();
        if !hex_padded.len().is_multiple_of(4) {
            while !hex_padded.len().is_multiple_of(4) {
                hex_padded = format!("0{hex_padded}");
            }
        }
        let mut u16_units = Vec::new();
        let bytes = hex_padded.as_bytes();
        for chunk in bytes.as_chunks::<4>().0 {
            let chunk_str = std::str::from_utf8(chunk).ok()?;
            let val = u16::from_str_radix(chunk_str, 16).ok()?;
            u16_units.push(val);
        }
        Some(String::from_utf16_lossy(&u16_units))
    };

    // 1. Process `beginbfchar` ... `endbfchar` blocks
    let mut rest = cmap;
    while let Some(start_pos) = rest.find("beginbfchar") {
        let block_start = start_pos + "beginbfchar".len();
        let after_start = &rest[block_start..];
        let end_pos = after_start.find("endbfchar").unwrap_or(after_start.len());
        let block = &after_start[..end_pos];

        let mut tokens = block.split_whitespace();
        while let Some(src_tok) = tokens.next() {
            if let Some(dst_tok) = tokens.next()
                && let (Some(code), Some(text)) = (parse_hex_code(src_tok), parse_dst_utf16(dst_tok))
            {
                map.insert(code, text);
            }
        }

        rest = &after_start[end_pos..];
    }

    // 2. Process `beginbfrange` ... `endbfrange` blocks
    let mut rest_range = cmap;
    while let Some(start_pos) = rest_range.find("beginbfrange") {
        let block_start = start_pos + "beginbfrange".len();
        let after_start = &rest_range[block_start..];
        let end_pos = after_start.find("endbfrange").unwrap_or(after_start.len());
        let block = &after_start[..end_pos];

        let mut tokens = block.split_whitespace();
        while let Some(start_tok) = tokens.next() {
            let Some(end_tok) = tokens.next() else { break };
            let Some(dst_tok) = tokens.next() else { break };

            let Some(start_code) = parse_hex_code(start_tok) else { continue };
            let Some(end_code) = parse_hex_code(end_tok) else { continue };

            if dst_tok.starts_with('[') {
                let mut current_code = start_code;
                let first_elem = dst_tok.trim_start_matches('[');
                if !first_elem.is_empty() && first_elem != "]"
                    && let Some(text) = parse_dst_utf16(first_elem)
                {
                    map.insert(current_code, text);
                    current_code += 1;
                }
                for elem in tokens.by_ref() {
                    if elem.ends_with(']') {
                        let inner = elem.trim_end_matches(']');
                        if !inner.is_empty() && current_code <= end_code
                            && let Some(text) = parse_dst_utf16(inner)
                        {
                            map.insert(current_code, text);
                        }
                        break;
                    }
                    if current_code <= end_code
                        && let Some(text) = parse_dst_utf16(elem)
                    {
                        map.insert(current_code, text);
                        current_code += 1;
                    }
                }
            } else if let Some(dst_start_code) = parse_hex_code(dst_tok) {
                let count = end_code.saturating_sub(start_code);
                for i in 0..=count {
                    let code = start_code + i;
                    let dst = dst_start_code + i;
                    if let Some(c) = char::from_u32(dst) {
                        map.insert(code, c.to_string());
                    }
                }
            }
        }

        rest_range = &after_start[end_pos..];
    }

    map
}

/// Extracts and parses font encodings and `/ToUnicode` CMaps for a page.
pub fn build_page_font_decoders(
    doc: &lopdf::Document,
    page_id: lopdf::ObjectId,
) -> HashMap<Vec<u8>, PdfFontDecoder> {
    let mut decoders = HashMap::new();
    let Ok(fonts) = doc.get_page_fonts(page_id) else {
        return decoders;
    };

    for (font_name, font_dict) in fonts {
        let mut decoder = PdfFontDecoder::default();

        let subtype = font_dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .unwrap_or(b"");
        let encoding_name = font_dict
            .get(b"Encoding")
            .ok()
            .and_then(|o| o.as_name().ok())
            .unwrap_or(b"");
        if subtype == b"Type0" || encoding_name == b"Identity-H" || encoding_name == b"Identity-V" {
            decoder.is_two_byte = true;
        }

        if let Ok(to_unicode_obj) = font_dict.get(b"ToUnicode") {
            let stream = match to_unicode_obj {
                lopdf::Object::Reference(id) => {
                    doc.get_object(*id).ok().and_then(|o| o.as_stream().ok())
                }
                lopdf::Object::Stream(s) => Some(s),
                _ => None,
            };
            if let Some(stream) = stream {
                let stream_bytes = stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone());
                let cmap_str = String::from_utf8_lossy(&stream_bytes);
                decoder.cmap = parse_cmap_str(&cmap_str);
                if !decoder.cmap.is_empty() {
                    decoder.is_two_byte = decoder.cmap.keys().any(|&k| k > 255) || decoder.is_two_byte;
                }
            }
        }

        decoders.insert(font_name.clone(), decoder);
    }

    decoders
}

fn decode_bytes_to_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let u16_chars: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_be_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&u16_chars)
    } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(&bytes[3..]).to_string()
    } else {
        // Standard Latin-1 / ASCII decoding, sanitizing control characters
        bytes
            .iter()
            .filter_map(|&b| {
                if (32..=126).contains(&b) || b == b'\t' || b == b'\n' || b >= 160 {
                    Some(b as char)
                } else {
                    None
                }
            })
            .collect()
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
    parse_content_text_lines_with_fonts(
        content,
        scale,
        page_h_pts,
        regions_to_exclude,
        &HashMap::new(),
    )
}

/// Parses decoded PDF content operations into spatial `TextLine` structures with font CMap resolution and baseline tracking.
pub fn parse_content_text_lines_with_fonts(
    content: &lopdf::content::Content,
    scale: f32,
    page_h_pts: f32,
    regions_to_exclude: &[FigureRect],
    fonts: &HashMap<Vec<u8>, PdfFontDecoder>,
) -> Vec<TextLine> {
    let mut lines = Vec::new();

    let mut matrix = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut line_matrix = matrix;
    let mut font_size = 12.0f32;
    let mut base_font_size = 12.0f32;
    let mut line_baseline_y = 0.0f32;
    let mut leading = 14.0f32;
    let mut rise = 0.0f32;
    let mut current_font: Vec<u8> = Vec::new();

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
            let sanitized: String = trimmed
                .chars()
                .filter(|&c| !c.is_control() || c == '\t')
                .collect();
            let clean = sanitized.trim();
            if !clean.is_empty() {
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
                        text: clean.to_string(),
                    });
                }
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
                if let Ok(font_name) = op.operands[0].as_name() {
                    current_font = font_name.to_vec();
                }
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
                    let decoded = if let Some(decoder) = fonts.get(&current_font) {
                        decoder.decode(bytes)
                    } else {
                        decode_bytes_to_string(bytes)
                    };
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
                                let decoded = if let Some(decoder) = fonts.get(&current_font) {
                                    decoder.decode(bytes)
                                } else {
                                    decode_bytes_to_string(bytes)
                                };
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

    let font_decoders = build_page_font_decoders(doc, page_id);
    let lines = parse_content_text_lines_with_fonts(
        &content,
        scale,
        page_h_pts,
        regions_to_exclude,
        &font_decoders,
    );
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
        assert_eq!(DomExtractionMode::default(), DomExtractionMode::Never);
    }

    #[test]
    fn test_font_cmap_parsing_and_decoding() {
        let cmap_sample = r#"
            /CIDInit /ProcSet findresource begin
            12 dict begin
            begincmap
            1 begincodespacerange
            <0000> <FFFF>
            endcodespacerange
            5 beginbfchar
            <003a> <0057>
            <0044> <0061>
            <0057> <0074>
            <0046> <0063>
            <004b> <0068>
            endbfchar
            endcmap
        "#;

        let map = parse_cmap_str(cmap_sample);
        assert_eq!(map.len(), 5);
        assert_eq!(map.get(&0x003a), Some(&"W".to_string()));
        assert_eq!(map.get(&0x0044), Some(&"a".to_string()));
        assert_eq!(map.get(&0x0057), Some(&"t".to_string()));
        assert_eq!(map.get(&0x0046), Some(&"c".to_string()));
        assert_eq!(map.get(&0x004b), Some(&"h".to_string()));

        let mut decoder = PdfFontDecoder {
            cmap: map,
            is_two_byte: true,
        };

        // Decodes 2-byte CIDs [0x00, 0x3a, 0x00, 0x44, 0x00, 0x57, 0x00, 0x46, 0x00, 0x4b] -> "Watch"
        let raw_cids = [0x00, 0x3a, 0x00, 0x44, 0x00, 0x57, 0x00, 0x46, 0x00, 0x4b];
        assert_eq!(decoder.decode(&raw_cids), "Watch");

        // When single-byte mode
        decoder.is_two_byte = false;
        let single_byte_cids = [0x3a, 0x44, 0x57, 0x46, 0x4b];
        assert_eq!(decoder.decode(&single_byte_cids), "Watch");
    }

    #[test]
    fn test_cmap_decoding_on_subset_font_pdf() {
        let pdf_path = Path::new(r"F:\Jag\AI\cornell\statistics\CORE521OD1_lesson-transcript.pdf");
        if !pdf_path.exists() {
            return;
        }

        let doc = lopdf::Document::load(pdf_path).expect("Failed to load PDF");
        let lines = extract_baseline_aware_dom_text_lines(&doc, 8, 2.0, &[])
            .expect("DOM extraction should succeed");

        let full_text: String = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
        // Ensure that decoded text has "Watch" and "Four Levels of Measurement", NOT ":DWFK"
        assert!(full_text.contains("Watch"), "Text must contain 'Watch', got: {}", &full_text[..full_text.len().min(200)]);
        assert!(full_text.contains("Four Levels of Measurement"), "Text must contain 'Four Levels of Measurement'");
        assert!(!full_text.contains(":DWFK"), "Text must not contain garbled ':DWFK'");
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
