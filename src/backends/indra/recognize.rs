//! Character recognition model (SVTR CTC) for Indra OCR with multi-width bucketing.

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, Luma};
use std::io::Cursor;
use std::sync::Arc;
use tract_onnx::prelude::*;

use super::detect::TextBoundingBox;
use super::vocab::IndraVocab;

const BUCKET_WIDTHS: [usize; 7] = [160, 320, 480, 640, 800, 1024, 1536];

pub struct IndraRecognizer {
    plans: Vec<(usize, Arc<TypedRunnableModel>)>,
    target_height: u32,
    vocab: IndraVocab,
}

impl IndraRecognizer {
    /// Initializes SVTR recognizer with bucketed width plans.
    pub fn new(model_bytes: &[u8], vocab: IndraVocab) -> Result<Self> {
        let mut plans = Vec::with_capacity(BUCKET_WIDTHS.len());

        for &width in &BUCKET_WIDTHS {
            let mut reader = Cursor::new(model_bytes);
            let model = tract_onnx::onnx()
                .model_for_read(&mut reader)
                .context("Failed to parse SVTR ONNX model")?
                .with_input_fact(
                    0,
                    InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 96, width)),
                )?
                .with_input_fact(
                    1,
                    InferenceFact::dt_shape(i64::datum_type(), tvec!(1)),
                )?
                .into_typed()
                .context("Failed to type SVTR model")?
                .into_runnable()
                .context("Failed to optimize SVTR into runnable plan")?;

            plans.push((width, model));
        }

        Ok(Self {
            plans,
            target_height: 96,
            vocab,
        })
    }

    /// Selects the best bucketed width plan for the given width.
    fn select_plan(&self, width: usize) -> (usize, &Arc<TypedRunnableModel>) {
        for (bucket_w, plan) in &self.plans {
            if width <= *bucket_w {
                return (*bucket_w, plan);
            }
        }
        let last = self.plans.last().unwrap();
        (last.0, &last.1)
    }

    /// Recognizes text from a line on a page given its bounding box and the grayscale page.
    pub fn recognize_line(&self, gray: &GrayImage, bbox: &TextBoundingBox) -> Result<String> {
        let (ink_map, line_w, line_h) = crop_line_ink_map(gray, bbox, self.target_height, 3);
        if line_w == 0 || line_h == 0 {
            return Ok(String::new());
        }

        let (bucket_w, plan) = self.select_plan(line_w);

        // NCHW tensor (1, 1, 96, bucket_w) right-padded with 0.0 (paper background)
        let mut tensor_data = vec![0.0f32; self.target_height as usize * bucket_w];
        for y in 0..line_h {
            for x in 0..line_w.min(bucket_w) {
                tensor_data[y * bucket_w + x] = ink_map[y * line_w + x];
            }
        }

        let input_tensor = tract_ndarray::Array4::from_shape_vec(
            (1, 1, self.target_height as usize, bucket_w),
            tensor_data,
        )?;
        let tract_tensor: Tensor = input_tensor.into();

        let actual_w = (line_w.min(bucket_w)) as i64;
        let lengths_tensor = tract_ndarray::Array1::from_vec(vec![actual_w]);
        let tract_lengths: Tensor = lengths_tensor.into();

        // Run SVTR model
        let outputs = plan.run(tvec!(tract_tensor.into(), tract_lengths.into()))?;

        let token_view = outputs[0].to_plain_array_view::<i64>()?;
        let tokens = token_view.as_slice().context("Tokens not contiguous")?;

        // Output 2 contains the valid sequence length
        let valid_len = if outputs.len() > 2 {
            outputs[2]
                .to_plain_array_view::<i64>()
                .map(|v| v[0] as usize)
                .unwrap_or(tokens.len())
        } else {
            tokens.len()
        };

        let active_tokens = &tokens[0..valid_len.min(tokens.len())];
        let decoded = self.vocab.decode_tokens(active_tokens);
        Ok(decoded)
    }

    /// Backwards-compatible line recognizer taking an isolated cropped image.
    pub fn recognize_crop(&self, crop: &DynamicImage) -> Result<String> {
        let gray = crop.to_luma8();
        let bbox = TextBoundingBox {
            x: 0,
            y: 0,
            width: gray.width(),
            height: gray.height(),
            score: 1.0,
        };
        self.recognize_line(&gray, &bbox)
    }
}

/// Determines whether a text line consists of light text on a dark background (inverted).
fn is_dark_line(gray: &GrayImage, bbox: &TextBoundingBox) -> bool {
    let x0 = bbox.x.min(gray.width());
    let y0 = bbox.y.min(gray.height());
    let x1 = (bbox.x + bbox.width).min(gray.width());
    let y1 = (bbox.y + bbox.height).min(gray.height());

    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 {
        return false;
    }

    let mut hist = [0u32; 256];
    let mut total = 0u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = gray.get_pixel(x, y)[0] as usize;
            hist[p] += 1;
            total += 1;
        }
    }
    if total == 0 {
        return false;
    }

    let find_quantile = |q: f32| -> f32 {
        let target = (q * (total - 1) as f32).round() as u32;
        let mut cum = 0u32;
        for (i, &count) in hist.iter().enumerate() {
            cum += count;
            if cum >= target {
                return i as f32;
            }
        }
        255.0
    };

    let lo = find_quantile(0.02);
    let med = find_quantile(0.50);
    let hi = find_quantile(0.98);

    (hi - med) > (2.0 * (med - lo) + 8.0)
}

/// Crops a line with upstream SVTR context padding (window=3) and returns float32 ink map.
fn crop_line_ink_map(
    gray: &GrayImage,
    bbox: &TextBoundingBox,
    target_height: u32,
    window: u32,
) -> (Vec<f32>, usize, usize) {
    let h = (bbox.height as f32).max(1.0);
    // Upstream LineNet: pad_y = (window * (1 + 2 * 0.2) - 1) / 2 = 1.6 for window=3
    let pad_y = (window as f32 * (1.0 + 2.0 * 0.2) - 1.0) / 2.0;
    let pad_x = 0.3f32;

    let x0 = bbox.x as f32;
    let y0 = bbox.y as f32;
    let x1 = (bbox.x + bbox.width) as f32;
    let y1 = (bbox.y + bbox.height) as f32;

    let big_x0 = x0 - pad_x * h;
    let big_x1 = x1 + pad_x * h;
    let big_y0 = y0 - pad_y * h;
    let big_y1 = y1 + pad_y * h;

    let page_w = gray.width() as i32;
    let page_h = gray.height() as i32;

    let ix0 = big_x0.floor() as i32;
    let iy0 = big_y0.floor() as i32;
    let ix1 = big_x1.ceil() as i32;
    let iy1 = big_y1.ceil() as i32;

    let cw = (ix1 - ix0).max(1) as u32;
    let ch = (iy1 - iy0).max(1) as u32;

    let dark = is_dark_line(gray, bbox);
    let fill_val = if dark { 0u8 } else { 255u8 };
    let mut crop = GrayImage::from_pixel(cw, ch, Luma([fill_val]));

    let sx0 = ix0.clamp(0, page_w);
    let sy0 = iy0.clamp(0, page_h);
    let sx1 = ix1.clamp(0, page_w);
    let sy1 = iy1.clamp(0, page_h);

    if sx1 > sx0 && sy1 > sy0 {
        for y in sy0..sy1 {
            let dst_y = (y - iy0) as u32;
            for x in sx0..sx1 {
                let dst_x = (x - ix0) as u32;
                crop.put_pixel(dst_x, dst_y, *gray.get_pixel(x as u32, y as u32));
            }
        }
    }

    let out_w = ((cw as f32 * target_height as f32 / ch as f32).round() as u32).clamp(8, 4096);
    let filter = if ch > target_height {
        image::imageops::FilterType::Triangle
    } else {
        image::imageops::FilterType::Triangle
    };
    let resized = image::imageops::resize(&crop, out_w, target_height, filter);

    let mut ink_map = Vec::with_capacity((out_w * target_height) as usize);
    for y in 0..target_height {
        for x in 0..out_w {
            let p = resized.get_pixel(x, y)[0] as f32 / 255.0;
            // 0.0 = white paper background, 1.0 = dark ink
            let val = if dark { p } else { 1.0 - p };
            ink_map.push(val);
        }
    }

    (ink_map, out_w as usize, target_height as usize)
}
