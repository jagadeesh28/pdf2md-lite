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
        let env = crate::figure::models::get_shared_ort_environment()?;

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

        // 5. Autoregressive Decoder Loop with Repetition Early-Stop
        let mut tokens: Vec<i64> = vec![1]; // [BOS]
        let mut mask: Vec<bool> = vec![true];
        let max_seq_len = 128; // Standard formulas in DocLayNet do not exceed 128 tokens; prevents runaway quadratic iterations
        let mut repeat_count = 0;
        let mut prev_token = -1;

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

            // Repetition detection: abort immediately if identical token repeats 3+ times (e.g. infinite ~ / spaces)
            if next_tok == prev_token {
                repeat_count += 1;
                if repeat_count >= 3 {
                    break;
                }
            } else {
                repeat_count = 0;
                prev_token = next_tok;
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

        if let Some(repaired) = validate_and_repair_latex(&cleaned) {
            Ok(repaired)
        } else {
            Ok(String::new())
        }
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

/// Strips all unescaped dollar signs ($ and $$) from raw formula strings.
pub fn clean_stray_dollars(s: &str) -> String {
    s.replace('$', "")
}

/// Extracts trailing equation tags like `(6a)`, `(9)`, `7 (6a)` from candidate formula string.
pub fn extract_equation_tag(s: &str) -> (String, Option<String>) {
    let trimmed = s.trim();
    if let Some(open_paren) = trimmed.rfind('(') {
        if let Some(close_paren) = trimmed[open_paren..].find(')') {
            let tag_candidate = trimmed[open_paren + 1..open_paren + close_paren].trim();
            let after_tag = trimmed[open_paren + close_paren + 1..].trim();
            let is_valid_tag = !tag_candidate.is_empty()
                && tag_candidate.len() <= 8
                && tag_candidate.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');

            let is_at_end = after_tag.is_empty()
                || after_tag == "$"
                || after_tag == "$$"
                || after_tag == "dx"
                || after_tag == "dr"
                || after_tag.starts_with("where")
                || after_tag.starts_with("with");

            if is_valid_tag && is_at_end {
                let before = trimmed[..open_paren].trim_end();
                let before_tag = if let Some(last_space) = before.rfind(' ') {
                    let trailing_word = before[last_space + 1..].trim();
                    if trailing_word.chars().all(|c| c.is_ascii_digit()) {
                        before[..last_space].trim_end()
                    } else {
                        before
                    }
                } else {
                    before
                };

                if !before_tag.is_empty() {
                    return (before_tag.to_string(), Some(tag_candidate.to_string()));
                }
            }
        }
    }
    (trimmed.to_string(), None)
}

/// Replaces known OCR command corruptions and typos with standard LaTeX commands.
pub fn fix_common_latex_command_typos(s: &str) -> String {
    let mut res = s.to_string();

    res = res.replace(r"_{p\}", "_{p}")
             .replace(r"^{p\}", "^{p}")
             .replace(r"p_{p\}", "p_p")
             .replace(r"\}", "}");

    res = res.replace(r"\dotm_", r"\dot{m}_")
             .replace(r"\dotm", r"\dot{m}")
             .replace(r"\ddotm_", r"\ddot{m}_")
             .replace(r"\ddotm", r"\ddot{m}");

    res = res.replace("{n^{}}", "{n}")
             .replace("T{{}}", "T")
             .replace(r"\sigma_r{}", r"\sigma_r");

    let mut out = String::with_capacity(res.len());
    let mut i = 0;
    let chars: Vec<char> = res.chars().collect();

    while i < chars.len() {
        if i + 6 <= chars.len() && chars[i..i + 6].iter().collect::<String>() == r"\right" {
            let after_idx = i + 6;
            if after_idx < chars.len() {
                let c = chars[after_idx];
                let valid_delims = [')', ']', '}', '|', '.', '/'];
                if c.is_ascii_alphabetic() && !valid_delims.contains(&c) {
                    i += 6;
                    continue;
                }
            }
        }

        if i + 2 <= chars.len() && chars[i..i + 2].iter().collect::<String>() == "|(" {
            out.push_str("/(");
            i += 2;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Balances unescaped curly braces `{` and `}` in a LaTeX string.
pub fn balance_braces(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut depth: i32 = 0;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            if let Some(&next_c) = chars.peek() {
                out.push(next_c);
                chars.next();
            }
            continue;
        }

        if c == '{' {
            depth += 1;
            out.push(c);
        } else if c == '}' {
            if depth > 0 {
                depth -= 1;
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }

    while depth > 0 {
        out.push('}');
        depth -= 1;
    }

    out
}

/// Matches `\left` and `\right` delimiters and balances plain parentheses.
pub fn balance_delimiters(s: &str) -> String {
    let mut left_stack: Vec<char> = Vec::new();
    let mut out = String::with_capacity(s.len() + 32);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if i + 5 <= chars.len() && chars[i..i + 5].iter().collect::<String>() == r"\left" {
            let mut delim_idx = i + 5;
            while delim_idx < chars.len() && chars[delim_idx].is_whitespace() {
                delim_idx += 1;
            }
            if delim_idx < chars.len() {
                let delim = chars[delim_idx];
                let is_valid_delim = matches!(delim, '(' | '[' | '|' | '.' | '/' | '{');
                if is_valid_delim {
                    left_stack.push(delim);
                    out.push_str(r"\left");
                    out.push(delim);
                    i = delim_idx + 1;
                    continue;
                } else {
                    left_stack.push('.');
                    out.push_str(r"\left.");
                    i = i + 5;
                    continue;
                }
            } else {
                left_stack.push('.');
                out.push_str(r"\left.");
                i = chars.len();
                continue;
            }
        }

        if i + 6 <= chars.len() && chars[i..i + 6].iter().collect::<String>() == r"\right" {
            let mut delim_idx = i + 6;
            while delim_idx < chars.len() && chars[delim_idx].is_whitespace() {
                delim_idx += 1;
            }
            if delim_idx < chars.len() {
                let delim = chars[delim_idx];
                let is_valid_delim = matches!(delim, ')' | ']' | '|' | '.' | '/' | '}');
                if is_valid_delim {
                    if left_stack.pop().is_some() {
                        out.push_str(r"\right");
                        out.push(delim);
                    } else {
                        out.push(delim);
                    }
                    i = delim_idx + 1;
                    continue;
                } else {
                    i = i + 6;
                    continue;
                }
            } else {
                i = chars.len();
                continue;
            }
        }

        // If plain closing bracket matches unclosed \left, promote to \right
        if chars[i] == ')' && left_stack.last() == Some(&'(') {
            left_stack.pop();
            out.push_str(r"\right)");
            i += 1;
            continue;
        }

        if chars[i] == ']' && left_stack.last() == Some(&'[') {
            left_stack.pop();
            out.push_str(r"\right]");
            i += 1;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    for open_delim in left_stack.into_iter().rev() {
        let matching = match open_delim {
            '(' => r"\right)",
            '[' => r"\right]",
            '{' => r"\right\}",
            '|' => r"\right|",
            _ => r"\right.",
        };
        out.push_str(matching);
    }

    out
}

/// Checks whether a cleaned LaTeX string represents valid, non-trivial mathematical formula syntax.
pub fn is_valid_latex_formula(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.len() < 2 {
        return false;
    }

    if crate::markdown::is_likely_prose_sentence(trimmed) {
        return false;
    }

    // Explanatory table column descriptions like "= Reynolds number" without math commands/subscripts
    if trimmed.starts_with('=') && !trimmed.contains('\\') && !trimmed.contains('_') && !trimmed.contains('^') {
        return false;
    }

    const KNOWN_MATH_COMMANDS: &[&str] = &[
        r"\alpha", r"\beta", r"\gamma", r"\delta", r"\epsilon", r"\varepsilon", r"\zeta", r"\eta",
        r"\theta", r"\vartheta", r"\iota", r"\kappa", r"\lambda", r"\mu", r"\nu", r"\xi",
        r"\pi", r"\varpi", r"\rho", r"\varrho", r"\sigma", r"\varsigma", r"\tau", r"\upsilon",
        r"\phi", r"\varphi", r"\chi", r"\psi", r"\omega",
        r"\Gamma", r"\Delta", r"\Theta", r"\Lambda", r"\Xi", r"\Pi", r"\Sigma", r"\Upsilon",
        r"\Phi", r"\Psi", r"\Omega",
        r"\frac", r"\dfrac", r"\tfrac", r"\sqrt", r"\sum", r"\prod", r"\int", r"\iint", r"\oint",
        r"\partial", r"\nabla", r"\infty", r"\times", r"\pm", r"\mp", r"\cdot", r"\leq", r"\geq",
        r"\neq", r"\approx", r"\equiv", r"\propto", r"\to", r"\implies",
        r"\dot", r"\ddot", r"\hat", r"\bar", r"\tilde", r"\vec", r"\left", r"\right", r"\exp", r"\ln", r"\log",
    ];

    let has_cmd = KNOWN_MATH_COMMANDS.iter().any(|cmd| trimmed.contains(cmd));
    if has_cmd {
        return true;
    }

    // Subscripts and superscripts are explicit math notations in LaTeX
    if trimmed.contains('_') || trimmed.contains('^') {
        return true;
    }

    // Math delimiters like absolute values |x|
    if (trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 3)
        || (trimmed.starts_with(r"\|") && trimmed.ends_with(r"\|"))
    {
        return true;
    }

    let has_relation = trimmed.contains('=')
        || trimmed.contains(r"\approx")
        || trimmed.contains(r"\le")
        || trimmed.contains(r"\ge")
        || trimmed.contains(r"\neq");

    let math_chars = ['+', '-', '*', '/', '^', '_'];
    let has_op = trimmed.chars().any(|c| math_chars.contains(&c));

    if has_relation {
        return true;
    }

    if has_op && trimmed.contains('(') && trimmed.contains(')') {
        return true;
    }

    false
}

/// Validates, cleans, and auto-repairs a candidate LaTeX formula string.
/// Returns `None` if the formula contains unfixable fatal syntax or pure noise.
pub fn validate_and_repair_latex(raw: &str) -> Option<String> {
    let clean_dollars = clean_stray_dollars(raw);
    let (body, tag_opt) = extract_equation_tag(&clean_dollars);
    let typos_fixed = fix_common_latex_command_typos(&body);
    let delims_balanced = balance_delimiters(&typos_fixed);
    let braces_balanced = balance_braces(&delims_balanced);

    let trimmed = braces_balanced.trim();
    if !is_valid_latex_formula(trimmed) {
        return None;
    }

    if let Some(tag) = tag_opt {
        Some(format!("{trimmed} \\tag{{{tag}}}"))
    } else {
        Some(trimmed.to_string())
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

    #[test]
    fn test_validate_and_repair_unbalanced_braces() {
        // User screenshot 1 sample: unclosed braces
        let input = r"\dot{m}_{nn}=m_{tt0+\dotm_i0} 7 (6a)";
        let repaired = validate_and_repair_latex(input).expect("Should repair");
        assert!(repaired.contains(r"\dot{m}"));
        assert!(repaired.contains(r"\tag{6a}"));
        assert_eq!(repaired.matches('{').count(), repaired.matches('}').count());

        // Escaped brace sample
        let input2 = r"p_{p\}=\rho_{i0}";
        let repaired2 = validate_and_repair_latex(input2).expect("Should repair");
        assert_eq!(repaired2.matches('{').count(), repaired2.matches('}').count());
        assert!(!repaired2.contains(r"\}"));
    }

    #[test]
    fn test_validate_and_repair_unclosed_left_right() {
        // User screenshot 2 sample: \left( without \right)
        let input = r"$V_{n}=\left(m_{tk}+{m_{n}+{m_{n})RT_{n}|(PA)$";
        let repaired = validate_and_repair_latex(input).expect("Should repair");
        assert!(repaired.contains(r"\left("));
        assert!(repaired.contains(r"\right)"));
        assert!(!repaired.contains('$'));
        assert_eq!(repaired.matches('{').count(), repaired.matches('}').count());

        // Sample with stray $ and equation number
        let input2 = r"q=4\pi{m_{t}e\epsilon{\sigma_r}{(T_{n^{}}-T{{}}) \left(m_{j}V{n})$ $ (9)";
        let repaired2 = validate_and_repair_latex(input2).expect("Should repair");
        assert!(repaired2.contains(r"\tag{9}"));
        assert!(!repaired2.contains('$'));
        assert!(repaired2.contains(r"\right)"));
    }

    #[test]
    fn test_reject_pure_noise() {
        // User screenshot 2 OCR noise sample: dd)mc,(m
        let input = "dd)mc,(m";
        assert!(validate_and_repair_latex(input).is_none());

        // Stray numbers
        assert!(validate_and_repair_latex("00").is_none());
        assert!(validate_and_repair_latex("(8) dr").is_none());
    }
}
