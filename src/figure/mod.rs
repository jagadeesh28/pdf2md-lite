pub mod caption;
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

/// Analyzes page layout using YOLOv8n DocLayNet and PDF streams, routing elements into
/// pictures, tables, and formulas.
pub fn analyze_page_layout(
    pdf_path: Option<&Path>,
    image_path: &Path,
    image_output_dir: &Path,
    page_num: usize,
) -> Result<PageLayoutResult> {
    fs::create_dir_all(image_output_dir).with_context(|| {
        format!(
            "Failed to create image output directory {:?}",
            image_output_dir
        )
    })?;

    let page_img = image::open(image_path)
        .with_context(|| format!("Failed to open page image {:?}", image_path))?;
    let (page_w, page_h) = page_img.dimensions();

    let mut result = PageLayoutResult::default();

    // 1. Check native PDF DOM streams for digital vector/raster images
    if let Some(pdf_file) = pdf_path {
        if let Ok(doc) = lopdf::Document::load(pdf_file) {
            if let Ok(stream_figs) =
                pdf_stream::extract_page_stream_figures(&doc, page_num, page_w, page_h)
            {
                for (idx, sf) in stream_figs.into_iter().enumerate() {
                    let rect = caption::FigureRect::from_stream_figure(&sf);
                    let filename = format!("page_{page_num}_stream_{}.png", idx + 1);
                    let file_path = image_output_dir.join(&filename);
                    let _ = sf.image.save(&file_path);

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
        }
    }

    // 2. Run YOLO Document Layout Engine (DocLayNet)
    let yolo = get_yolo_detector().context("Failed to load YOLO Document Layout Engine")?;
    let boxes = yolo
        .detect(&page_img, 0.40)
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
                    let filename = format!("page_{page_num}_fig_{figure_idx}.png");
                    let file_path = image_output_dir.join(&filename);

                    // Crop raw unenhanced image directly from page canvas
                    if let Ok(cropped) = crop_rectangle(&page_img, b.x, b.y, b.width, b.height, page_w, page_h) {
                        let _ = cropped.save(&file_path);
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
                let filename = format!("page_{page_num}_table_{table_idx}.png");
                let file_path = image_output_dir.join(&filename);

                // Save companion raw table crop
                if let Ok(cropped) = crop_rectangle(&page_img, b.x, b.y, b.width, b.height, page_w, page_h) {
                    let _ = cropped.save(&file_path);
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
