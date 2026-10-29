//! Pure-Rust YOLO Document Layout Engine for Figure, Table, and Formula Extraction.
//!
//! Executes an in-process YOLOv8n DocLayNet ONNX model via `tract-onnx` without external
//! runtimes, Python, or cloud APIs.
//! Implements aspect-ratio preserving Letterbox Padding (to 640x640), affine coordinate
//! inversion, greedy NMS, and hierarchical containment merging per ADR 0002 and ADR 0009.

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::io::Cursor;
use std::sync::Arc;
use tract_onnx::prelude::*;

/// Target layout classes for document visual and structural element extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
    Picture,
    Table,
    Formula,
}

impl LayoutClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            LayoutClass::Picture => "figure",
            LayoutClass::Table => "table",
            LayoutClass::Formula => "formula",
        }
    }
}

/// A detected document layout bounding box in native document pixel coordinates.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub class: LayoutClass,
    pub score: f32,
}

impl LayoutBox {
    pub fn xmin(&self) -> f32 {
        self.x as f32
    }
    pub fn ymin(&self) -> f32 {
        self.y as f32
    }
    pub fn xmax(&self) -> f32 {
        (self.x + self.width) as f32
    }
    pub fn ymax(&self) -> f32 {
        (self.y + self.height) as f32
    }
    pub fn area(&self) -> f32 {
        (self.width * self.height) as f32
    }

    /// Computes Intersection over Union (IoU) with another box.
    pub fn iou(&self, other: &LayoutBox) -> f32 {
        let x1 = self.xmin().max(other.xmin());
        let y1 = self.ymin().max(other.ymin());
        let x2 = self.xmax().min(other.xmax());
        let y2 = self.ymax().min(other.ymax());

        let inter_w = (x2 - x1).max(0.0);
        let inter_h = (y2 - y1).max(0.0);
        let inter_area = inter_w * inter_h;

        if inter_area <= 0.0 {
            return 0.0;
        }

        let union_area = self.area() + other.area() - inter_area;
        if union_area <= 0.0 {
            0.0
        } else {
            inter_area / union_area
        }
    }

    /// Computes what fraction of `self` is contained inside `other`.
    pub fn containment_ratio(&self, other: &LayoutBox) -> f32 {
        let x1 = self.xmin().max(other.xmin());
        let y1 = self.ymin().max(other.ymin());
        let x2 = self.xmax().min(other.xmax());
        let y2 = self.ymax().min(other.ymax());

        let inter_w = (x2 - x1).max(0.0);
        let inter_h = (y2 - y1).max(0.0);
        let inter_area = inter_w * inter_h;

        let self_area = self.area();
        if self_area <= 0.0 {
            0.0
        } else {
            inter_area / self_area
        }
    }

    /// Merges another bounding box into this one by expanding boundaries.
    pub fn merge(&mut self, other: &LayoutBox) {
        let x1 = self.xmin().min(other.xmin()) as u32;
        let y1 = self.ymin().min(other.ymin()) as u32;
        let x2 = self.xmax().max(other.xmax()) as u32;
        let y2 = self.ymax().max(other.ymax()) as u32;

        self.x = x1;
        self.y = y1;
        self.width = x2.saturating_sub(x1);
        self.height = y2.saturating_sub(y1);
        self.score = self.score.max(other.score);
    }
}

/// Metadata recorded during Letterbox Padding for accurate coordinate unmapping.
#[derive(Debug, Clone)]
pub struct LetterboxInfo {
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub orig_w: u32,
    pub orig_h: u32,
    pub target_side: u32,
}

impl LetterboxInfo {
    /// Inverts letterbox-padded bounding box coordinates back to original native page pixel coordinates.
    pub fn unmap_box(&self, cx: f32, cy: f32, w: f32, h: f32) -> (u32, u32, u32, u32) {
        let x1 = (cx - w / 2.0 - self.pad_x) / self.scale;
        let y1 = (cy - h / 2.0 - self.pad_y) / self.scale;
        let x2 = (cx + w / 2.0 - self.pad_x) / self.scale;
        let y2 = (cy + h / 2.0 - self.pad_y) / self.scale;

        let clamped_x1 = x1.clamp(0.0, self.orig_w as f32);
        let clamped_y1 = y1.clamp(0.0, self.orig_h as f32);
        let clamped_x2 = x2.clamp(0.0, self.orig_w as f32);
        let clamped_y2 = y2.clamp(0.0, self.orig_h as f32);

        let x = clamped_x1.round() as u32;
        let y = clamped_y1.round() as u32;
        let width = (clamped_x2 - clamped_x1).max(0.0).round() as u32;
        let height = (clamped_y2 - clamped_y1).max(0.0).round() as u32;

        (x, y, width, height)
    }
}

pub struct YoloLayoutDetector {
    plan: Arc<TypedRunnableModel>,
    target_side: usize,
    formula_class_idx: usize,
    picture_class_idx: usize,
    table_class_idx: usize,
}

impl YoloLayoutDetector {
    /// Initializes YOLOv8n detector from ONNX model bytes with fixed 640x640 input shape.
    pub fn new(model_bytes: &[u8]) -> Result<Self> {
        let target_side = 640usize;
        let mut reader = Cursor::new(model_bytes);
        let model = tract_onnx::onnx()
            .model_for_read(&mut reader)
            .context("Failed to parse YOLOv8n DocLayNet ONNX model")?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, target_side, target_side)),
            )?
            .into_typed()
            .context("Failed to type YOLOv8n model")?
            .into_runnable()
            .context("Failed to optimize YOLOv8n into runnable plan")?;

        Ok(Self {
            plan: model,
            target_side,
            formula_class_idx: 2, // Standard DocLayNet Formula class index
            picture_class_idx: 6, // Standard DocLayNet Picture class index
            table_class_idx: 8,   // Standard DocLayNet Table class index
        })
    }

    /// Preprocesses image into a normalized 1x3x640x640 CHW tensor with Letterbox Padding.
    pub fn letterbox_preprocess(&self, img: &DynamicImage) -> (Vec<f32>, LetterboxInfo) {
        let (orig_w, orig_h) = img.dimensions();
        let target = self.target_side as f32;

        let scale = (target / (orig_w as f32)).min(target / (orig_h as f32));
        let scaled_w = ((orig_w as f32 * scale).round() as u32).clamp(1, self.target_side as u32);
        let scaled_h = ((orig_h as f32 * scale).round() as u32).clamp(1, self.target_side as u32);

        let pad_x = (target - scaled_w as f32) / 2.0;
        let pad_y = (target - scaled_h as f32) / 2.0;

        let rgb = img.to_rgb8();
        let resized = image::imageops::resize(
            &rgb,
            scaled_w,
            scaled_h,
            image::imageops::FilterType::Triangle,
        );

        // Neutral grey padding: 114 / 255.0 = 0.44705883
        let grey_val = 114.0f32 / 255.0f32;
        let channel_stride = self.target_side * self.target_side;
        let mut tensor_data = vec![grey_val; 3 * channel_stride];

        let offset_x = pad_x.round() as usize;
        let offset_y = pad_y.round() as usize;

        for y in 0..scaled_h as usize {
            for x in 0..scaled_w as usize {
                let pixel = resized.get_pixel(x as u32, y as u32);
                let target_idx = (offset_y + y) * self.target_side + (offset_x + x);

                tensor_data[target_idx] = pixel[0] as f32 / 255.0;
                tensor_data[channel_stride + target_idx] = pixel[1] as f32 / 255.0;
                tensor_data[2 * channel_stride + target_idx] = pixel[2] as f32 / 255.0;
            }
        }

        let info = LetterboxInfo {
            scale,
            pad_x,
            pad_y,
            orig_w,
            orig_h,
            target_side: self.target_side as u32,
        };

        (tensor_data, info)
    }

    /// Detects figure, table, and formula bounding boxes on an input document image.
    pub fn detect(&self, img: &DynamicImage, score_threshold: f32) -> Result<Vec<LayoutBox>> {
        let (orig_w, orig_h) = img.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(Vec::new());
        }

        let (tensor_data, info) = self.letterbox_preprocess(img);
        let input_tensor =
            Tensor::from_shape(&[1, 3, self.target_side, self.target_side], &tensor_data)?;

        let outputs = self
            .plan
            .run(tvec!(input_tensor.into()))
            .context("YOLOv8 inference run failed")?;

        let output_tensor = outputs[0]
            .to_plain_array_view::<f32>()
            .context("Failed to read YOLOv8 output tensor")?;

        let shape = outputs[0].shape();
        // Shape is [1, 4 + num_classes, num_anchors] e.g. [1, 15, 8400]
        if shape.len() != 3 {
            anyhow::bail!("Unexpected YOLOv8 output tensor rank: {:?}", shape);
        }

        let channels = shape[1];
        let anchors = shape[2];
        let raw_slice = output_tensor.as_slice().context("Tensor not contiguous")?;

        let mut candidates = Vec::new();

        for i in 0..anchors {
            let cx = raw_slice[0 * anchors + i];
            let cy = raw_slice[1 * anchors + i];
            let w = raw_slice[2 * anchors + i];
            let h = raw_slice[3 * anchors + i];

            // Evaluate Formula class (DocLayNet class 2)
            if self.formula_class_idx + 4 < channels {
                let fml_score = raw_slice[(4 + self.formula_class_idx) * anchors + i];
                if fml_score >= score_threshold {
                    let (x, y, width, height) = info.unmap_box(cx, cy, w, h);
                    if width > 15 && height > 10 {
                        candidates.push(LayoutBox {
                            x,
                            y,
                            width,
                            height,
                            class: LayoutClass::Formula,
                            score: fml_score,
                        });
                    }
                }
            }

            // Evaluate Picture class (DocLayNet class 6)
            if self.picture_class_idx + 4 < channels {
                let pic_score = raw_slice[(4 + self.picture_class_idx) * anchors + i];
                if pic_score >= score_threshold {
                    let (x, y, width, height) = info.unmap_box(cx, cy, w, h);
                    if width > 15 && height > 15 {
                        candidates.push(LayoutBox {
                            x,
                            y,
                            width,
                            height,
                            class: LayoutClass::Picture,
                            score: pic_score,
                        });
                    }
                }
            }

            // Evaluate Table class (DocLayNet class 8)
            if self.table_class_idx + 4 < channels {
                let tbl_score = raw_slice[(4 + self.table_class_idx) * anchors + i];
                if tbl_score >= score_threshold {
                    let (x, y, width, height) = info.unmap_box(cx, cy, w, h);
                    if width > 15 && height > 15 {
                        candidates.push(LayoutBox {
                            x,
                            y,
                            width,
                            height,
                            class: LayoutClass::Table,
                            score: tbl_score,
                        });
                    }
                }
            }
        }

        // Apply pure-Rust greedy NMS and hierarchical containment merging
        let nms_boxes = Self::apply_nms(candidates, 0.45);
        let final_boxes = Self::merge_hierarchical_containment(nms_boxes, 0.85);

        Ok(final_boxes)
    }

    /// Class-wise greedy Non-Maximum Suppression.
    pub fn apply_nms(mut boxes: Vec<LayoutBox>, iou_threshold: f32) -> Vec<LayoutBox> {
        boxes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let mut selected: Vec<LayoutBox> = Vec::new();

        for b in boxes {
            let mut suppress = false;
            for s in &selected {
                if s.class == b.class && s.iou(&b) > iou_threshold {
                    suppress = true;
                    break;
                }
            }
            if !suppress {
                selected.push(b);
            }
        }

        selected
    }

    /// Merges bounding boxes that are heavily contained within larger boxes of the same class.
    pub fn merge_hierarchical_containment(
        boxes: Vec<LayoutBox>,
        containment_threshold: f32,
    ) -> Vec<LayoutBox> {
        let mut result: Vec<LayoutBox> = Vec::new();

        for b in boxes {
            let mut merged = false;
            for parent in result.iter_mut() {
                if parent.class == b.class {
                    if b.containment_ratio(parent) > containment_threshold {
                        parent.merge(&b);
                        merged = true;
                        break;
                    } else if parent.containment_ratio(&b) > containment_threshold {
                        let mut new_parent = b.clone();
                        new_parent.merge(parent);
                        *parent = new_parent;
                        merged = true;
                        break;
                    }
                }
            }

            if !merged {
                result.push(b);
            }
        }

        result
    }
}

static YOLO_DETECTOR: std::sync::OnceLock<Arc<YoloLayoutDetector>> = std::sync::OnceLock::new();

/// Returns a shared, lazily initialized singleton instance of the YOLO Layout Detector.
pub fn get_yolo_detector() -> Result<Arc<YoloLayoutDetector>> {
    if let Some(detector) = YOLO_DETECTOR.get() {
        return Ok(Arc::clone(detector));
    }

    let source = crate::figure::models::LayoutModelSource::new(None);
    let bytes = source.load_layout_model()?;
    let detector = Arc::new(YoloLayoutDetector::new(&bytes)?);

    let _ = YOLO_DETECTOR.set(Arc::clone(&detector));
    Ok(detector)
}
