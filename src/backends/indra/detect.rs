//! Text detection model (DBNet) for Indra OCR.

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::io::Cursor;
use std::sync::Arc;
use tract_onnx::prelude::*;

/// A detected text bounding box in original image pixel coordinates.
#[derive(Debug, Clone, Copy)]
pub struct TextBoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub score: f32,
}

pub struct IndraDetector {
    plan: Arc<TypedRunnableModel>,
    target_side: usize,
}

impl IndraDetector {
    /// Initializes DBNet detector from ONNX model bytes with fixed 1024x1024 input fact.
    pub fn new(model_bytes: &[u8], target_long_side: u32) -> Result<Self> {
        let side = target_long_side as usize;
        let mut reader = Cursor::new(model_bytes);
        let model = tract_onnx::onnx()
            .model_for_read(&mut reader)
            .context("Failed to parse DBNet ONNX model")?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, side, side)),
            )?
            .into_typed()
            .context("Failed to type DBNet model")?
            .into_runnable()
            .context("Failed to optimize DBNet into runnable plan")?;

        Ok(Self {
            plan: model,
            target_side: side,
        })
    }

    /// Detects text line bounding boxes from an input page image.
    pub fn detect(&self, img: &DynamicImage) -> Result<Vec<TextBoundingBox>> {
        let (orig_w, orig_h) = img.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(Vec::new());
        }

        // 1. Calculate scaling to fit within target_side (1024)
        let scale = (self.target_side as f32) / (orig_w.max(orig_h) as f32);
        let scaled_w = ((orig_w as f32 * scale).round() as u32).clamp(1, self.target_side as u32);
        let scaled_h = ((orig_h as f32 * scale).round() as u32).clamp(1, self.target_side as u32);

        let gray = img.to_luma8();
        let resized = image::imageops::resize(
            &gray,
            scaled_w,
            scaled_h,
            image::imageops::FilterType::Triangle,
        );

        // 2. Prepare NCHW tensor (1, 1, target_side, target_side) with 0.0-padding (paper background)
        let mut tensor_data = vec![0.0f32; self.target_side * self.target_side];
        for y in 0..scaled_h {
            for x in 0..scaled_w {
                let p = resized.get_pixel(x, y)[0] as f32 / 255.0;
                // Upstream LineNet / DBNet expects ink map: 0.0 = white paper background, 1.0 = dark ink
                tensor_data[y as usize * self.target_side + x as usize] = 1.0 - p;
            }
        }

        let input_tensor = tract_ndarray::Array4::from_shape_vec(
            (1, 1, self.target_side, self.target_side),
            tensor_data,
        )?;
        let tract_tensor: Tensor = input_tensor.into();

        // 3. Run DBNet inference
        let outputs = self.plan.run(tvec!(tract_tensor.into()))?;
        let output_tensor = outputs[0].to_plain_array_view::<f32>()?;
        let out_shape = outputs[0].shape();
        let (out_h, out_w) = (out_shape[out_shape.len() - 2], out_shape[out_shape.len() - 1]);

        let raw_slice = output_tensor.as_slice().context("Tensor not contiguous")?;
        let channel_size = out_h * out_w;
        let core_logits = &raw_slice[0..channel_size];
        let top_dists = &raw_slice[channel_size..2 * channel_size];
        let bot_dists = &raw_slice[2 * channel_size..3 * channel_size];

        // 4. Decode text line bounding boxes using distance regression (upstream det_post.py)
        let valid_h = ((scaled_h + 1) / 2) as usize;
        let valid_w = ((scaled_w + 1) / 2) as usize;
        let s = 2.0f32; // OUT_STRIDE

        let mut visited = vec![false; valid_h * valid_w];
        let mut raw_boxes = Vec::new();

        for y in 0..valid_h {
            for x in 0..valid_w {
                let v_idx = y * valid_w + x;
                if visited[v_idx] {
                    continue;
                }

                let idx = y * out_w + x;
                let logit = core_logits[idx];
                let prob = 1.0 / (1.0 + (-logit).exp());

                if prob <= 0.5 {
                    continue;
                }

                // Connected component BFS
                let mut min_x = x;
                let mut max_x = x;
                let mut count = 0usize;
                let mut sum_top = 0.0f32;
                let mut sum_bot = 0.0f32;
                let mut sum_sc = 0.0f32;

                let mut queue = std::collections::VecDeque::new();
                queue.push_back((x, y));
                visited[v_idx] = true;

                while let Some((cx, cy)) = queue.pop_front() {
                    count += 1;
                    min_x = min_x.min(cx);
                    max_x = max_x.max(cx);

                    let c_idx = cy * out_w + cx;
                    let ys = (cy as f32 + 0.5) * s;
                    let top = ys - top_dists[c_idx];
                    let bot = ys + bot_dists[c_idx];
                    let sc = 1.0 / (1.0 + (-core_logits[c_idx]).exp());

                    sum_top += top;
                    sum_bot += bot;
                    sum_sc += sc;

                    let neighbors = [
                        (cx.wrapping_sub(1), cy),
                        (cx + 1, cy),
                        (cx, cy.wrapping_sub(1)),
                        (cx, cy + 1),
                    ];

                    for (nx, ny) in neighbors {
                        if nx < valid_w && ny < valid_h {
                            let nv_idx = ny * valid_w + nx;
                            if !visited[nv_idx] {
                                let n_idx = ny * out_w + nx;
                                let n_prob = 1.0 / (1.0 + (-core_logits[n_idx]).exp());
                                if n_prob > 0.5 {
                                    visited[nv_idx] = true;
                                    queue.push_back((nx, ny));
                                }
                            }
                        }
                    }
                }

                if count >= 2 {
                    let mean_top = sum_top / count as f32;
                    let mean_bot = sum_bot / count as f32;
                    let score = sum_sc / count as f32;

                    let lh = (mean_bot - mean_top).max(1.0);
                    let ex = (0.15 * lh).min(0.5 * lh);

                    let x0 = (min_x as f32) * s - ex;
                    let x1 = ((max_x + 1) as f32) * s + ex;
                    let y0 = mean_top;
                    let y1 = mean_bot;

                    let orig_x0 = (x0 / scale).floor().max(0.0) as u32;
                    let orig_y0 = (y0 / scale).floor().max(0.0) as u32;
                    let orig_x1 = (x1 / scale).ceil().min(orig_w as f32) as u32;
                    let orig_y1 = (y1 / scale).ceil().min(orig_h as f32) as u32;

                    if orig_x1 > orig_x0 && orig_y1 > orig_y0 {
                        let bw = orig_x1 - orig_x0;
                        let bh = orig_y1 - orig_y0;
                        if bw >= 2 && bh >= 2 {
                            raw_boxes.push(TextBoundingBox {
                                x: orig_x0,
                                y: orig_y0,
                                width: bw,
                                height: bh,
                                score,
                            });
                        }
                    }
                }
            }
        }

        Ok(raw_boxes)
    }
}

