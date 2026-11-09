//! Character recognition model (SVTR CTC) for Indra OCR with multi-width bucketing.

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, Luma};
use std::io::Cursor;
use std::sync::Arc;
use tract_onnx::prelude::*;

use super::detect::TextBoundingBox;
use super::vocab::IndraVocab;

const BUCKET_WIDTHS: [usize; 7] = [160, 320, 480, 640, 800, 1024, 1536];
// Batch sizes for which we pre-compile batched SVTR plans.
// Batching multiple line ink-maps into a single inference amortizes tensor setup
// and ONNX dispatch overhead across lines of the same bucket width.
const BATCH_SIZES: [usize; 3] = [1, 4, 8];

pub struct IndraRecognizer {
    /// Per-bucket single-line plans (batch=1), used by `recognize_line`.
    plans: Vec<(usize, Arc<TypedRunnableModel>)>,
    /// Batched plans: (batch_size, bucket_width, plan).
    batched_plans: Vec<(usize, usize, Arc<TypedRunnableModel>)>,
    target_height: u32,
    window: u32,
    vocab: IndraVocab,
}

impl IndraRecognizer {
    /// Initializes SVTR recognizer with custom height and context window.
    /// Pre-compiles single-line and batched plans for all bucket widths.
    pub fn new_with_config(
        model_bytes: &[u8],
        vocab: IndraVocab,
        target_height: u32,
        window: u32,
    ) -> Result<Self> {
        let mut plans = Vec::with_capacity(BUCKET_WIDTHS.len());
        let mut batched_plans = Vec::with_capacity(BUCKET_WIDTHS.len() * BATCH_SIZES.len());

        for &width in &BUCKET_WIDTHS {
            // Single-line plan (batch=1).
            {
                let mut reader = Cursor::new(model_bytes);
                let model = tract_onnx::onnx()
                    .model_for_read(&mut reader)
                    .context("Failed to parse SVTR ONNX model")?
                    .with_input_fact(
                        0,
                        InferenceFact::dt_shape(
                            f32::datum_type(),
                            tvec!(1, 1, target_height as usize, width),
                        ),
                    )?
                    .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
                    .into_typed()
                    .context("Failed to type SVTR model")?
                    .into_runnable()
                    .context("Failed to optimize SVTR into runnable plan")?;

                plans.push((width, model));
            }

            // Batched plans (batch > 1) for multi-line inference.
            for &batch in &BATCH_SIZES {
                if batch == 1 {
                    continue;
                }
                let mut reader = Cursor::new(model_bytes);
                let model = tract_onnx::onnx()
                    .model_for_read(&mut reader)
                    .context("Failed to parse SVTR ONNX model")?
                    .with_input_fact(
                        0,
                        InferenceFact::dt_shape(
                            f32::datum_type(),
                            tvec!(batch, 1, target_height as usize, width),
                        ),
                    )?
                    .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(batch)))?
                    .into_typed()
                    .context("Failed to type SVTR model")?
                    .into_runnable()
                    .context("Failed to optimize SVTR into runnable plan")?;

                batched_plans.push((batch, width, model));
            }
        }

        Ok(Self {
            plans,
            batched_plans,
            target_height,
            window,
            vocab,
        })
    }

    /// Initializes base SVTR recognizer (rec.onnx: 96px height, window=3).
    pub fn new(model_bytes: &[u8], vocab: IndraVocab) -> Result<Self> {
        Self::new_with_config(model_bytes, vocab, 96, 3)
    }

    /// Initializes world SVTR recognizer (rec_world.onnx: 32px height, window=1).
    pub fn new_world(model_bytes: &[u8], vocab: IndraVocab) -> Result<Self> {
        Self::new_with_config(model_bytes, vocab, 32, 1)
    }

    pub fn vocab(&self) -> &IndraVocab {
        &self.vocab
    }

    pub fn target_height(&self) -> u32 {
        self.target_height
    }

    pub fn window(&self) -> u32 {
        self.window
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

    /// Selects the best batched plan for the given batch size and line width.
    fn select_batched_plan(
        &self,
        batch_size: usize,
        width: usize,
    ) -> Option<(usize, usize, &Arc<TypedRunnableModel>)> {
        let bucket_w = BUCKET_WIDTHS
            .iter()
            .find(|&&bw| width <= bw)
            .copied()
            .unwrap_or(*BUCKET_WIDTHS.last().unwrap());
        let batch = BATCH_SIZES
            .iter()
            .find(|&&b| batch_size <= b)
            .copied()
            .unwrap_or(*BATCH_SIZES.last().unwrap());
        self.batched_plans
            .iter()
            .find(|(b, bw, _)| *b == batch && *bw == bucket_w)
            .map(|(b, bw, plan)| (*b, *bw, plan))
    }

    /// Extracts a line ink-map from a grayscale page image, resized to the recognizer's
    /// target height. Returns (ink_map, width, height).
    fn line_ink_map(&self, gray: &GrayImage, bbox: &TextBoundingBox) -> (Vec<f32>, usize, usize) {
        let page_w = gray.width();
        let page_h = gray.height();
        let x0 = bbox.x.min(page_w);
        let y0 = bbox.y.min(page_h);
        let x1 = (bbox.x + bbox.width).min(page_w);
        let y1 = (bbox.y + bbox.height).min(page_h);
        let cw = x1.saturating_sub(x0);
        let ch = y1.saturating_sub(y0);
        if cw == 0 || ch == 0 {
            return (Vec::new(), 0, 0);
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
        let dark = if total > 0 {
            let median_idx = total / 2;
            let mut cum = 0u32;
            let mut median = 128u8;
            for (i, &count) in hist.iter().enumerate() {
                cum += count;
                if cum > median_idx {
                    median = i as u8;
                    break;
                }
            }
            median < 128
        } else {
            false
        };
        let mut crop = GrayImage::new(cw, ch);
        for y in y0..y1 {
            let dst_y = y - y0;
            for x in x0..x1 {
                let dst_x = x - x0;
                crop.put_pixel(dst_x, dst_y, *gray.get_pixel(x, y));
            }
        }
        let target_h = self.target_height;
        let out_w = ((cw as f32 * target_h as f32 / ch as f32).round() as u32).clamp(8, 4096);
        let resized = image::imageops::resize(
            &crop,
            out_w,
            target_h,
            image::imageops::FilterType::Triangle,
        );
        let mut ink_map = Vec::with_capacity((out_w as usize) * (target_h as usize));
        for y in 0..target_h {
            for x in 0..out_w {
                let p = resized.get_pixel(x, y)[0] as f32 / 255.0;
                let val = if dark { p } else { 1.0 - p };
                ink_map.push(val);
            }
        }
        (ink_map, out_w as usize, target_h as usize)
    }

    /// Recognizes text from a line on a page given its bounding box and the grayscale page.
    pub fn recognize_line(&self, gray: &GrayImage, bbox: &TextBoundingBox) -> Result<String> {
        let (text, _) = self.recognize_line_with_conf(gray, bbox)?;
        Ok(text)
    }

    /// Recognizes text and returns the mean confidence score of the non-blank tokens.
    pub fn recognize_line_with_conf(
        &self,
        gray: &GrayImage,
        bbox: &TextBoundingBox,
    ) -> Result<(String, f32)> {
        self.recognize_line_with_mask(gray, bbox, None)
    }

    /// Recognizes text with an optional script token constraint mask (e.g. for Telugu/Indic).
    pub fn recognize_line_with_mask(
        &self,
        gray: &GrayImage,
        bbox: &TextBoundingBox,
        mask: Option<&[bool]>,
    ) -> Result<(String, f32)> {
        let (ink_map, line_w, line_h) =
            crop_line_ink_map(gray, bbox, self.target_height, self.window);
        if line_w == 0 || line_h == 0 {
            return Ok((String::new(), 0.0));
        }

        let (bucket_w, plan) = self.select_plan(line_w);

        // NCHW tensor (1, 1, target_height, bucket_w) right-padded with 0.0 (paper background)
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

        // Apply Constrained CTC Masking if requested
        let mut final_tokens: Vec<i64> = active_tokens.to_vec();
        if let Some(mask_slice) = mask {
            let topi_view = if outputs.len() > 5 {
                outputs[5].to_plain_array_view::<i64>().ok()
            } else {
                None
            };
            let topi_slice = topi_view.as_ref().and_then(|v| v.as_slice());

            for (t, tok) in final_tokens.iter_mut().enumerate() {
                let idx = *tok as usize;
                if idx < mask_slice.len() && mask_slice[idx] {
                    continue;
                }

                // Top-1 token is outside the permitted script whitelist.
                // Inspect top-8 candidates (topi) for highest-ranking valid in-script token.
                let mut valid_cand = None;
                if let Some(topi) = topi_slice {
                    let k_start = t * 8;
                    if k_start + 8 <= topi.len() {
                        for k in 0..8 {
                            let cand = topi[k_start + k];
                            let cand_idx = cand as usize;
                            if cand_idx < mask_slice.len() && mask_slice[cand_idx] {
                                valid_cand = Some(cand);
                                break;
                            }
                        }
                    }
                }

                *tok = valid_cand.unwrap_or(self.vocab.blank_idx as i64);
            }
        }

        // Output 1 contains token confidences
        let conf_view = if outputs.len() > 1 {
            outputs[1].to_plain_array_view::<f32>().ok()
        } else {
            None
        };
        let confs = conf_view.as_ref().and_then(|v| v.as_slice());

        let mut sum_conf = 0.0f32;
        let mut count_conf = 0usize;
        let mut prev = self.vocab.blank_idx as i64;

        for (i, &tok) in final_tokens.iter().enumerate() {
            if tok != self.vocab.blank_idx as i64
                && tok != prev
                && let Some(c) = confs
                && let Some(&conf) = c.get(i)
            {
                sum_conf += conf;
                count_conf += 1;
            }
            prev = tok;
        }

        let mean_conf = if count_conf > 0 {
            sum_conf / count_conf as f32
        } else {
            0.0
        };

        let decoded = self.vocab.decode_tokens(&final_tokens);
        Ok((decoded, mean_conf))
    }

    /// Recognizes multiple text lines in batched SVTR inference per bucket group.
    pub fn recognize_lines_batch(
        &self,
        gray: &GrayImage,
        boxes: &[TextBoundingBox],
        mask: Option<&[bool]>,
    ) -> Result<Vec<(String, f32)>> {
        let mut results: Vec<Option<(String, f32)>> = vec![None; boxes.len()];
        let groups: Vec<(usize, Vec<usize>)> = {
            let mut tmp: std::collections::BTreeMap<usize, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (i, box_) in boxes.iter().enumerate() {
                let ink_w = box_.width as usize;
                let bucket_w = BUCKET_WIDTHS
                    .iter()
                    .find(|&&bw| ink_w <= bw)
                    .copied()
                    .unwrap_or(*BUCKET_WIDTHS.last().unwrap());
                tmp.entry(bucket_w).or_default().push(i);
            }
            tmp.into_iter().collect()
        };
        self.recognize_lines_batch_groups(gray, boxes, mask, groups, &mut results)?;
        Ok(results.into_iter().map(|r| r.unwrap_or_default()).collect())
    }

    fn recognize_lines_batch_groups(
        &self,
        gray: &GrayImage,
        boxes: &[TextBoundingBox],
        mask: Option<&[bool]>,
        groups: Vec<(usize, Vec<usize>)>,
        results: &mut [Option<(String, f32)>],
    ) -> Result<()> {
        for (bucket_w, indices) in groups {
            let n = indices.len();
            let (batch_size, bucket_w, plan) = self
                .select_batched_plan(n, bucket_w)
                .unwrap_or_else(|| (1, bucket_w, self.select_plan(bucket_w).1));

            let target_h = self.target_height as usize;
            let mut tensor_data = vec![0.0f32; batch_size * target_h * bucket_w];
            let mut actual_widths: Vec<i64> = Vec::with_capacity(batch_size);

            for (row, &idx) in indices.iter().enumerate().take(batch_size) {
                let (ink_map, line_w, line_h) = self.line_ink_map(gray, &boxes[idx]);
                if line_w == 0 || line_h == 0 {
                    actual_widths.push(1);
                    continue;
                }
                let copy_w = line_w.min(bucket_w).min(target_h);
                actual_widths.push(copy_w as i64);
                for y in 0..line_h.min(target_h) {
                    let src_base = y * line_w;
                    let dst_base = row * target_h * bucket_w + y * bucket_w;
                    tensor_data[dst_base..(dst_base + copy_w)]
                        .copy_from_slice(&ink_map[src_base..(src_base + copy_w)]);
                }
            }

            if actual_widths.iter().all(|&w| w <= 1) {
                for &idx in &indices {
                    if let Ok((text, conf)) = self.recognize_line_with_mask(gray, &boxes[idx], mask)
                    {
                        results[idx] = Some((text, conf));
                    }
                }
                continue;
            }

            while actual_widths.len() < batch_size {
                actual_widths.push(1);
            }

            let input_tensor = tract_ndarray::Array4::from_shape_vec(
                (batch_size, 1, target_h, bucket_w),
                tensor_data,
            )?;
            let tract_input: Tensor = input_tensor.into();
            let lengths_tensor = tract_ndarray::Array1::from_vec(actual_widths.clone());
            let tract_lengths: Tensor = lengths_tensor.into();

            let outputs = plan.run(tvec!(tract_input.into(), tract_lengths.into()))?;

            self.decode_batch_results(
                gray,
                boxes,
                mask,
                &indices,
                batch_size,
                bucket_w,
                target_h,
                &outputs,
                &actual_widths,
                results,
            )?;
        }
        Ok(())
    }

    /// Decodes batched SVTR outputs into per-line results, applying constrained CTC masking.
    #[allow(clippy::too_many_arguments)]
    fn decode_batch_results(
        &self,
        _gray: &GrayImage,
        _boxes: &[TextBoundingBox],
        mask: Option<&[bool]>,
        indices: &[usize],
        batch_size: usize,
        _bucket_w: usize,
        _target_h: usize,
        outputs: &[TValue],
        actual_widths: &[i64],
        results: &mut [Option<(String, f32)>],
    ) -> Result<()> {
        let token_view = outputs[0].to_plain_array_view::<i64>()?;
        let tokens = token_view.as_slice().context("Tokens not contiguous")?;

        let valid_lens: Vec<usize> = if outputs.len() > 2 {
            let v = outputs[2]
                .to_plain_array_view::<i64>()
                .context("Valid lengths not contiguous")?;
            let slice = v.as_slice().context("valid lengths not contiguous")?;
            slice.iter().take(batch_size).map(|&x| x as usize).collect()
        } else {
            actual_widths
                .iter()
                .take(batch_size)
                .map(|&w| w as usize)
                .collect()
        };

        let conf_views: Option<Vec<Vec<f32>>> = if outputs.len() > 1 {
            let cv = outputs[1].to_plain_array_view::<f32>().ok();
            cv.and_then(|v| {
                let slice = v.as_slice()?;
                let seq_len = tokens.len() / batch_size;
                Some(
                    slice
                        .chunks(seq_len)
                        .take(batch_size)
                        .map(|c| c.to_vec())
                        .collect(),
                )
            })
        } else {
            None
        };

        let seq_len = tokens.len() / batch_size;

        for (row, &idx) in indices.iter().enumerate().take(batch_size) {
            if row >= valid_lens.len() {
                break;
            }
            let valid_len = valid_lens[row].min(actual_widths[row] as usize);
            if valid_len == 0 {
                continue;
            }

            let row_start = row * seq_len;
            let row_tokens = &tokens[row_start..row_start + seq_len];
            let mut final_tokens: Vec<i64> = row_tokens[..valid_len].to_vec();

            if let Some(mask_slice) = mask {
                let topi_view = if outputs.len() > 5 {
                    outputs[5].to_plain_array_view::<i64>().ok()
                } else {
                    None
                };
                let topi_slice = topi_view.as_ref().and_then(|v| v.as_slice());

                for (t, tok) in final_tokens.iter_mut().enumerate() {
                    let tok_idx = *tok as usize;
                    if tok_idx < mask_slice.len() && mask_slice[tok_idx] {
                        continue;
                    }
                    let mut valid_cand = None;
                    if let Some(topi) = topi_slice {
                        let k_start = t * 8;
                        if k_start + 8 <= topi.len() {
                            for k in 0..8 {
                                let cand = topi[k_start + k];
                                let cand_idx = cand as usize;
                                if cand_idx < mask_slice.len() && mask_slice[cand_idx] {
                                    valid_cand = Some(cand);
                                    break;
                                }
                            }
                        }
                    }
                    *tok = valid_cand.unwrap_or(self.vocab.blank_idx as i64);
                }
            }

            let mut sum_conf = 0.0f32;
            let mut count_conf = 0usize;
            let mut prev = self.vocab.blank_idx as i64;
            if let Some(confs) = &conf_views
                && let Some(row_conf) = confs.get(row)
            {
                for (i, &tok) in final_tokens.iter().enumerate() {
                    if tok != self.vocab.blank_idx as i64
                        && tok != prev
                        && let Some(&c) = row_conf.get(i)
                    {
                        sum_conf += c;
                        count_conf += 1;
                    }
                    prev = tok;
                }
            }
            let mean_conf = if count_conf > 0 {
                sum_conf / count_conf as f32
            } else {
                0.0
            };

            let decoded = self.vocab.decode_tokens(&final_tokens);
            results[idx] = Some((decoded, mean_conf));
        }

        Ok(())
    }

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
    let filter = image::imageops::FilterType::Triangle;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_rec_world_tract() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let model_path = std::path::Path::new(&manifest_dir)
            .join("models")
            .join("indra")
            .join("rec_world.onnx");
        if !model_path.exists() {
            return;
        }
        let bytes = std::fs::read(model_path).unwrap();
        let mut reader = Cursor::new(&bytes);
        let model = tract_onnx::onnx()
            .model_for_read(&mut reader)
            .expect("Failed to parse rec_world ONNX model")
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 32, 160)),
            )
            .expect("input 0 fact")
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))
            .expect("input 1 fact")
            .into_typed()
            .expect("typed")
            .into_runnable()
            .expect("runnable");

        let input_tensor =
            tract_ndarray::Array4::from_shape_vec((1, 1, 32, 160), vec![0.0f32; 32 * 160]).unwrap();
        let lengths_tensor = tract_ndarray::Array1::from_vec(vec![160i64]);
        let tract_input: Tensor = input_tensor.into();
        let tract_lengths: Tensor = lengths_tensor.into();
        let outputs = model
            .run(tvec!(tract_input.into(), tract_lengths.into()))
            .expect("run model");
        assert!(!outputs.is_empty());
    }

    #[test]
    fn test_indra_recognizer_world_initialization() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let meta_path = std::path::Path::new(&manifest_dir)
            .join("models")
            .join("indra")
            .join("meta.json");
        let model_path = std::path::Path::new(&manifest_dir)
            .join("models")
            .join("indra")
            .join("rec_world.onnx");
        if !model_path.exists() {
            return;
        }
        let meta_bytes = std::fs::read(meta_path).unwrap();
        let vocab = IndraVocab::from_world_json_bytes(&meta_bytes).unwrap();
        let model_bytes = std::fs::read(model_path).unwrap();
        let rec = IndraRecognizer::new_world(&model_bytes, vocab).expect("new_world");
        assert_eq!(rec.target_height(), 32);
        assert_eq!(rec.window(), 1);
        assert!(rec.vocab().contains_telugu());
    }
}
