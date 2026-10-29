//! RapidLaTeXOCR: Mathematical Formula Recognition using ONNX Runtime.
//! Predicts LaTeX code from cropped formula images using an encoder-decoder architecture.

#[cfg(feature = "latex-ort")]
use std::collections::HashMap;
#[cfg(feature = "latex-ort")]
use std::path::Path;
#[cfg(feature = "latex-ort")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "latex-ort")]
use image::{DynamicImage, GenericImageView};

#[cfg(feature = "latex-ort")]
pub struct RapidLatexEngine {
    encoder: Mutex<ort::session::Session>,
    decoder: Mutex<ort::session::Session>,
    vocab: HashMap<i64, String>,
}

#[cfg(feature = "latex-ort")]
static LATEX_ENGINE: std::sync::OnceLock<Option<Arc<RapidLatexEngine>>> = std::sync::OnceLock::new();

/// Returns a shared, lazily initialized singleton instance of the RapidLaTeXOCR Engine,
/// or `None` if the model/tokenizer files are not provisioned.
#[cfg(feature = "latex-ort")]
pub fn get_latex_engine() -> Option<Arc<RapidLatexEngine>> {
    let engine = LATEX_ENGINE.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let enc_path = std::path::Path::new(&manifest_dir).join("models").join("latex").join("encoder.onnx");
        let dec_path = std::path::Path::new(&manifest_dir).join("models").join("latex").join("decoder.onnx");
        let tok_path = std::path::Path::new(&manifest_dir).join("models").join("latex").join("tokenizer.json");

        if enc_path.exists() && dec_path.exists() && tok_path.exists() {
            RapidLatexEngine::new(&enc_path, &dec_path, &tok_path).ok().map(Arc::new)
        } else {
            None
        }
    });
    engine.as_ref().cloned()
}

#[cfg(not(feature = "latex-ort"))]
pub fn get_latex_engine() -> Option<()> {
    None
}

#[cfg(feature = "latex-ort")]
impl RapidLatexEngine {
    /// Initializes RapidLaTeXOCR from encoder, decoder, and tokenizer paths.
    pub fn new<P: AsRef<Path>>(encoder_path: P, decoder_path: P, tokenizer_path: P) -> anyhow::Result<Self> {
        let env = ort::environment::init()
            .with_name("rapid_latex")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to initialize ORT environment: {e}"))?;

        let encoder = ort::session::Session::builder(&env)
            .map_err(|e| anyhow::anyhow!("Failed to create encoder session builder: {e}"))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .map_err(|e| anyhow::anyhow!("Failed to set encoder optimization level: {e}"))?
            .commit_from_file(encoder_path.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to load encoder model: {e}"))?;

        let decoder = ort::session::Session::builder(&env)
            .map_err(|e| anyhow::anyhow!("Failed to create decoder session builder: {e}"))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .map_err(|e| anyhow::anyhow!("Failed to set decoder optimization level: {e}"))?
            .commit_from_file(decoder_path.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to load decoder model: {e}"))?;

        let tok_str = std::fs::read_to_string(tokenizer_path.as_ref())?;
        let tok_json: serde_json::Value = serde_json::from_str(&tok_str)?;

        let mut vocab = HashMap::new();
        if let Some(vocab_obj) = tok_json.get("model").and_then(|m| m.get("vocab")).and_then(|v| v.as_object()) {
            for (token_str, id_val) in vocab_obj {
                if let Some(id) = id_val.as_i64() {
                    vocab.insert(id, token_str.clone());
                }
            }
        }

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            vocab,
        })
    }

    /// Predicts LaTeX formula from a cropped formula image.
    pub fn predict_latex(&self, formula_crop: &DynamicImage) -> anyhow::Result<String> {
        let (orig_w, orig_h) = formula_crop.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(String::new());
        }

        // 1. Convert to grayscale and find tight formula bounding box
        let gray = formula_crop.to_luma8();
        let total_pixels = orig_w as usize * orig_h as usize;
        let sum_intensity: usize = gray.pixels().map(|p| p[0] as usize).sum();
        let mean_intensity = (sum_intensity / total_pixels.max(1)) as u8;

        let is_white_background = mean_intensity > 128;

        let mut min_x = orig_w;
        let mut min_y = orig_h;
        let mut max_x = 0;
        let mut max_y = 0;
        let mut found = false;

        for y in 0..orig_h {
            for x in 0..orig_w {
                let val = gray.get_pixel(x, y)[0];
                let is_ink = if is_white_background {
                    val < 128
                } else {
                    val > 128
                };
                if is_ink {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    found = true;
                }
            }
        }

        let cropped = if found && max_x >= min_x && max_y >= min_y {
            let bw = (max_x - min_x + 1).min(orig_w - min_x);
            let bh = (max_y - min_y + 1).min(orig_h - min_y);
            formula_crop.crop_imm(min_x, min_y, bw, bh)
        } else {
            formula_crop.clone()
        };

        let (cw, ch) = cropped.dimensions();

        // 2. Compute target dimensions divisible by 32
        // Target height clamped to [32, 192], width clamped to [32, 672]
        let target_h = (((ch as f32).clamp(32.0, 192.0) / 32.0).round() as u32 * 32).clamp(32, 192);
        let scale = target_h as f32 / ch.max(1) as f32;
        let target_w = (((cw as f32 * scale / 32.0).round() as u32 * 32)).clamp(32, 672);

        let resized = cropped.resize_exact(target_w, target_h, image::imageops::FilterType::Triangle);
        let resized_gray = resized.to_luma8();

        // 3. Normalize image: mean = 0.7931, std = 0.1738
        let mean = 0.7931f32 * 255.0;
        let std = 0.1738f32 * 255.0;

        let mut tensor_buf = vec![0.0f32; target_h as usize * target_w as usize];
        for y in 0..target_h {
            for x in 0..target_w {
                let p = resized_gray.get_pixel(x, y)[0] as f32;
                let normalized = (p - mean) / std;
                tensor_buf[(y as usize) * (target_w as usize) + (x as usize)] = normalized;
            }
        }

        // 4. Run Encoder
        let (ctx_n, ctx_d, ctx_data) = {
            let enc_input = ort::value::Tensor::from_array((
                [1usize, 1, target_h as usize, target_w as usize],
                tensor_buf.into_boxed_slice(),
            ))?;
            let enc_inputs = ort::inputs!["input" => enc_input];
            let mut encoder_session = self.encoder.lock().map_err(|e| anyhow::anyhow!("Encoder mutex poisoned: {e}"))?;
            let enc_outputs = encoder_session.run(enc_inputs)?;

            let (ctx_shape, ctx_slice) = enc_outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow::anyhow!("Failed to extract context tensor: {e}"))?;

            (ctx_shape[1] as usize, ctx_shape[2] as usize, ctx_slice.to_vec())
        };

        // 5. Autoregressive Decoder Loop
        let mut tokens: Vec<i64> = vec![1]; // [BOS]
        let mut mask: Vec<bool> = vec![true];
        let max_seq_len = 256;

        let mut decoder_session = self.decoder.lock().map_err(|e| anyhow::anyhow!("Decoder mutex poisoned: {e}"))?;

        for _ in 0..max_seq_len {
            let x_tensor = ort::value::Tensor::from_array((
                [1usize, tokens.len()],
                tokens.clone().into_boxed_slice(),
            ))?;
            let mask_tensor = ort::value::Tensor::from_array((
                [1usize, mask.len()],
                mask.clone().into_boxed_slice(),
            ))?;
            let ctx_tensor = ort::value::Tensor::from_array((
                [1usize, ctx_n, ctx_d],
                ctx_data.clone().into_boxed_slice(),
            ))?;

            let dec_inputs = ort::inputs![
                "x" => x_tensor,
                "mask" => mask_tensor,
                "context" => ctx_tensor
            ];

            let dec_outputs = decoder_session.run(dec_inputs)?;
            let (out_shape, logits_slice) = dec_outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow::anyhow!("Failed to extract logits tensor: {e}"))?;

            let vocab_size = out_shape[2] as usize;
            let last_pos = tokens.len() - 1;
            let offset = last_pos * vocab_size;
            let step_logits = &logits_slice[offset..offset + vocab_size];

            // Greedy argmax
            let mut best_idx = 0;
            let mut best_val = f32::NEG_INFINITY;
            for (k, &val) in step_logits.iter().enumerate() {
                if val > best_val {
                    best_val = val;
                    best_idx = k;
                }
            }

            let next_tok = best_idx as i64;
            if next_tok == 2 { // [EOS]
                break;
            }

            tokens.push(next_tok);
            mask.push(true);
        }

        // 6. Decode token string
        let mut latex = String::new();
        for &tok_id in &tokens[1..] {
            if let Some(s) = self.vocab.get(&tok_id) {
                if s == "[EOS]" || s == "[BOS]" || s == "[PAD]" {
                    continue;
                }
                latex.push_str(s);
            }
        }

        // Replace ByteLevel space 'Ġ' (U+0120) with standard space
        let detok = latex.replace('\u{0120}', " ");
        let cleaned = Self::post_process(&detok);

        Ok(cleaned)
    }

    /// Cleans and formats raw LaTeX string produced by token decoding.
    fn post_process(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut last_was_space = false;

        for ch in s.chars() {
            if ch.is_whitespace() {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            } else {
                out.push(ch);
                last_was_space = false;
            }
        }

        out.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latex_engine_load_if_present() {
        #[cfg(feature = "latex-ort")]
        {
            if let Some(engine) = get_latex_engine() {
                // Test dummy image
                let dummy = DynamicImage::new_rgb8(100, 40);
                let result = engine.predict_latex(&dummy);
                assert!(result.is_ok());
            }
        }
    }
}
