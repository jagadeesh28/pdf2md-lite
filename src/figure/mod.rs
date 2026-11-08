pub mod caption;
pub mod latex;
pub mod models;
pub mod pdf_stream;
pub mod slanet;
pub mod table;
pub mod yolo;

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImage, GenericImageView, Rgba};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use caption::FigureRect;
use yolo::{LayoutClass, get_yolo_detector};

/// Supported output image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    #[serde(alias = "jpg")]
    Jpeg,
    Png,
    Webp,
}

impl Default for ImageFormat {
    fn default() -> Self {
        Self::Jpeg
    }
}

impl ImageFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// Image configuration options for figure extraction and saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageOptions {
    pub format: ImageFormat,
    pub quality: u8,
    pub save_table_images: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            format: ImageFormat::Jpeg,
            quality: 80,
            save_table_images: false,
        }
    }
}

impl ImageOptions {
    pub fn new(format: ImageFormat, quality: u8, save_table_images: bool) -> Self {
        Self {
            format,
            quality: quality.clamp(1, 100),
            save_table_images,
        }
    }
}

/// Flattens an image to 24-bit RGB, blending RGBA transparency cleanly over a white background.
pub fn flatten_to_rgb(img: &DynamicImage) -> image::RgbImage {
    match img {
        DynamicImage::ImageRgb8(rgb) => rgb.clone(),
        DynamicImage::ImageLuma8(gray) => DynamicImage::ImageLuma8(gray.clone()).to_rgb8(),
        _ => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let rgba_raw = rgba.into_raw();
            let mut rgb_raw = vec![0u8; (w as usize) * (h as usize) * 3];
            for (dst_chunk, src_chunk) in rgb_raw.chunks_exact_mut(3).zip(rgba_raw.chunks(4)) {
                let alpha = src_chunk[3] as f32 / 255.0;
                dst_chunk[0] = ((src_chunk[0] as f32 * alpha) + (255.0 * (1.0 - alpha))).round() as u8;
                dst_chunk[1] = ((src_chunk[1] as f32 * alpha) + (255.0 * (1.0 - alpha))).round() as u8;
                dst_chunk[2] = ((src_chunk[2] as f32 * alpha) + (255.0 * (1.0 - alpha))).round() as u8;
            }
            image::RgbImage::from_raw(w, h, rgb_raw).expect("RgbImage dimensions match")
        }
    }
}

/// Encodes and saves a DynamicImage according to the specified ImageFormat and quality.
pub fn save_image_with_format(
    img: &DynamicImage,
    path: &Path,
    format: ImageFormat,
    quality: u8,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    match format {
        ImageFormat::Jpeg => {
            let rgb = flatten_to_rgb(img);
            let file = fs::File::create(path)
                .with_context(|| format!("Failed to create JPEG file: {}", path.display()))?;
            let mut writer = std::io::BufWriter::new(file);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
            encoder
                .encode_image(&rgb)
                .with_context(|| format!("Failed to encode JPEG: {}", path.display()))?;
        }
        ImageFormat::Png => {
            img.save_with_format(path, image::ImageFormat::Png)
                .with_context(|| format!("Failed to save PNG: {}", path.display()))?;
        }
        ImageFormat::Webp => {
            img.save_with_format(path, image::ImageFormat::WebP)
                .with_context(|| format!("Failed to save WebP: {}", path.display()))?;
        }
    }

    Ok(())
}

/// A visual figure or table crop extracted from a document page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CroppedFigure {
    pub file_path: PathBuf,
    pub filename: String,
    pub figure_label: String,
    pub caption: Option<String>,
    pub page_num: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub is_table: bool,
}

impl CroppedFigure {
    /// Returns the directory name containing this figure (e.g., "doc_images").
    pub fn images_dir_name(&self) -> String {
        self.file_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("images")
            .to_string()
    }
}

/// Masks out (whites out) rectangular regions on the page image with a small margin padding.
pub fn mask_page_regions(page_img: &DynamicImage, regions: &[FigureRect]) -> DynamicImage {
    if regions.is_empty() {
        return page_img.clone();
    }

    let mut masked_img = page_img.clone();
    let (orig_w, orig_h) = masked_img.dimensions();
    let margin = 4u32;
    let white_pixel = Rgba([255, 255, 255, 255]);

    for r in regions {
        let min_x = r.x.saturating_sub(margin);
        let min_y = r.y.saturating_sub(margin);
        let max_x = (r.x + r.width + margin).min(orig_w);
        let max_y = (r.y + r.height + margin).min(orig_h);

        for y in min_y..max_y {
            for x in min_x..max_x {
                masked_img.put_pixel(x, y, white_pixel);
            }
        }
    }

    masked_img
}

/// Three-way layout segmentation result for a single page.
#[derive(Default, Debug)]
pub struct PageLayoutResult {
    /// Picture figures (to be masked from OCR canvas, cropped as raw images, linked in Markdown).
    pub pictures: Vec<CroppedFigure>,
    /// Bounding boxes of pictures for canvas masking.
    pub picture_masks: Vec<FigureRect>,
    /// Table bounding boxes (for geometric grid markdown extraction).
    pub tables: Vec<(FigureRect, CroppedFigure)>,
    /// Formula bounding boxes (for display math $$..$$ formatting).
    pub formulas: Vec<FigureRect>,
}

/// Analyzes page layout using YOLOv8n DocLayNet and PDF streams directly from an in-memory DynamicImage.
pub fn analyze_page_layout_image(
    pdf_path: Option<&Path>,
    page_img: &DynamicImage,
    image_output_dir: &Path,
    page_num: usize,
) -> Result<PageLayoutResult> {
    analyze_page_layout_image_with_doc(None, pdf_path, page_img, image_output_dir, page_num)
}

/// Analyzes page layout using YOLOv8n DocLayNet and PDF streams, accepting an optional pre-parsed `lopdf::Document`
/// to completely eliminate redundant per-page PDF parsing from disk.
pub fn analyze_page_layout_image_with_doc(
    preloaded_doc: Option<&lopdf::Document>,
    pdf_path: Option<&Path>,
    page_img: &DynamicImage,
    image_output_dir: &Path,
    page_num: usize,
) -> Result<PageLayoutResult> {
    analyze_page_layout_image_with_doc_and_options(
        preloaded_doc,
        pdf_path,
        page_img,
        image_output_dir,
        page_num,
        ImageOptions::default(),
    )
}

/// Analyzes page layout using YOLOv8n DocLayNet and PDF streams with pre-parsed lopdf Document and custom ImageOptions.
pub fn analyze_page_layout_image_with_doc_and_options(
    preloaded_doc: Option<&lopdf::Document>,
    pdf_path: Option<&Path>,
    page_img: &DynamicImage,
    image_output_dir: &Path,
    page_num: usize,
    image_opts: ImageOptions,
) -> Result<PageLayoutResult> {
    fs::create_dir_all(image_output_dir).with_context(|| {
        format!(
            "Failed to create image output directory {:?}",
            image_output_dir
        )
    })?;

    let (page_w, page_h) = page_img.dimensions();
    let mut result = PageLayoutResult::default();
    let ext = image_opts.format.extension();

    // 1. Check native PDF DOM streams for digital vector/raster images using pre-parsed or lazily loaded doc
    let stream_figs_opt = if let Some(doc) = preloaded_doc {
        pdf_stream::extract_page_stream_figures(doc, page_num, page_w, page_h).ok()
    } else if let Some(pdf_file) = pdf_path {
        lopdf::Document::load(pdf_file)
            .ok()
            .and_then(|doc| pdf_stream::extract_page_stream_figures(&doc, page_num, page_w, page_h).ok())
    } else {
        None
    };

    if let Some(stream_figs) = stream_figs_opt {
        for (idx, sf) in stream_figs.into_iter().enumerate() {
            let rect = caption::FigureRect::from_stream_figure(&sf);
            let filename = format!("page_{page_num}_stream_{}.{ext}", idx + 1);
            let file_path = image_output_dir.join(&filename);
            let _ = save_image_with_format(&sf.image, &file_path, image_opts.format, image_opts.quality);

            result.picture_masks.push(rect);
            result.pictures.push(CroppedFigure {
                file_path,
                filename,
                figure_label: format!("Figure {page_num}-{}", idx + 1),
                caption: None,
                page_num,
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                is_table: false,
            });
        }
    }

    // 2. Run YOLO Document Layout Engine (DocLayNet)
    let yolo = get_yolo_detector().context("Failed to load YOLO Document Layout Engine")?;
    let boxes = yolo
        .detect(page_img, 0.40)
        .context("YOLO layout detection failed")?;

    let mut figure_idx = result.pictures.len();
    let mut table_idx = 0;

    for b in boxes {
        let rect = FigureRect {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        };

        match b.class {
            LayoutClass::Picture => {
                // If this region doesn't overlap heavily with an already extracted stream figure
                let overlaps = result.picture_masks.iter().any(|m| {
                    let inter_w = (rect.x + rect.width).min(m.x + m.width).saturating_sub(rect.x.max(m.x));
                    let inter_h = (rect.y + rect.height).min(m.y + m.height).saturating_sub(rect.y.max(m.y));
                    (inter_w * inter_h) > 0 && (inter_w * inter_h) as f32 / (rect.width * rect.height) as f32 > 0.5
                });

                if !overlaps {
                    figure_idx += 1;
                    let filename = format!("page_{page_num}_fig_{figure_idx}.{ext}");
                    let file_path = image_output_dir.join(&filename);

                    // Crop raw unenhanced image directly from page canvas
                    if let Ok(cropped) = crop_rectangle(page_img, b.x, b.y, b.width, b.height, page_w, page_h) {
                        let _ = save_image_with_format(&cropped, &file_path, image_opts.format, image_opts.quality);
                    }

                    result.picture_masks.push(rect.clone());
                    result.pictures.push(CroppedFigure {
                        file_path,
                        filename,
                        figure_label: format!("Figure {page_num}-{figure_idx}"),
                        caption: None,
                        page_num,
                        x: b.x,
                        y: b.y,
                        width: b.width,
                        height: b.height,
                        is_table: false,
                    });
                }
            }
            LayoutClass::Table => {
                table_idx += 1;
                let filename = format!("page_{page_num}_table_{table_idx}.{ext}");
                let file_path = image_output_dir.join(&filename);

                // Save companion raw table crop only if configured
                if image_opts.save_table_images {
                    if let Ok(cropped) = crop_rectangle(page_img, b.x, b.y, b.width, b.height, page_w, page_h) {
                        let _ = save_image_with_format(&cropped, &file_path, image_opts.format, image_opts.quality);
                    }
                }

                let fig = CroppedFigure {
                    file_path,
                    filename,
                    figure_label: format!("Table {page_num}-{table_idx}"),
                    caption: None,
                    page_num,
                    x: b.x,
                    y: b.y,
                    width: b.width,
                    height: b.height,
                    is_table: true,
                };
                result.tables.push((rect, fig));
            }
            LayoutClass::Formula => {
                result.formulas.push(rect);
            }
        }
    }

    Ok(result)
}

/// Analyzes page layout using YOLOv8n DocLayNet and PDF streams, loading from disk.
pub fn analyze_page_layout(
    pdf_path: Option<&Path>,
    image_path: &Path,
    image_output_dir: &Path,
    page_num: usize,
) -> Result<PageLayoutResult> {
    let page_img = image::open(image_path)
        .with_context(|| format!("Failed to open page image {:?}", image_path))?;
    analyze_page_layout_image(pdf_path, &page_img, image_output_dir, page_num)
}

/// Helper to safely crop a rectangle with margin from a DynamicImage.
fn crop_rectangle(
    img: &DynamicImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    page_w: u32,
    page_h: u32,
) -> Result<DynamicImage> {
    let margin = 4u32;
    let min_x = x.saturating_sub(margin);
    let min_y = y.saturating_sub(margin);
    let crop_w = (width + 2 * margin).min(page_w.saturating_sub(min_x)).max(1);
    let crop_h = (height + 2 * margin).min(page_h.saturating_sub(min_y)).max(1);

    Ok(img.crop_imm(min_x, min_y, crop_w, crop_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_format_extension_and_parsing() {
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Webp.extension(), "webp");

        assert_eq!(ImageFormat::from_str_loose("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_str_loose("JPEG"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_str_loose("png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_str_loose("webp"), Some(ImageFormat::Webp));
        assert_eq!(ImageFormat::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_flatten_rgba_to_rgb() {
        let mut rgba = image::RgbaImage::new(2, 2);
        // Fully transparent black pixel should become white
        rgba.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        // Fully opaque red pixel should remain red
        rgba.put_pixel(1, 1, image::Rgba([255, 0, 0, 255]));

        let dynamic = DynamicImage::ImageRgba8(rgba);
        let rgb = flatten_to_rgb(&dynamic);

        assert_eq!(rgb.get_pixel(0, 0), &image::Rgb([255, 255, 255]));
        assert_eq!(rgb.get_pixel(1, 1), &image::Rgb([255, 0, 0]));
    }

    #[test]
    fn test_save_image_as_jpeg() {
        let dummy = DynamicImage::new_rgba8(20, 20);
        let temp_dir = tempfile::tempdir().unwrap();
        let target = temp_dir.path().join("sample.jpg");

        save_image_with_format(&dummy, &target, ImageFormat::Jpeg, 80).unwrap();
        assert!(target.exists());
        assert!(fs::metadata(&target).unwrap().len() > 0);
    }
}

