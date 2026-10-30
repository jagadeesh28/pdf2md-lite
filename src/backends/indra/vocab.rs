//! Character vocabulary and CTC decoding for Indra OCR.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VocabMeta {
    vocab: String,
    #[serde(default)]
    vocab_world: Option<String>,
    #[serde(default)]
    det_long_side: Option<u32>,
    #[serde(default)]
    rec_height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct VocabDef {
    blank: usize,
    chars: Vec<String>,
}

/// Unicode block range for Telugu: U+0C00..=U+0C7F
pub const TELUGU_RANGE: std::ops::RangeInclusive<u32> = 0x0C00..=0x0C7F;

/// Returns true if the character is in the Telugu Unicode block.
#[inline]
pub fn is_telugu_char(c: char) -> bool {
    let code = c as u32;
    (0x0C00..=0x0C7F).contains(&code)
}

/// Returns true if the text contains any Telugu character.
pub fn has_telugu(text: &str) -> bool {
    text.chars().any(is_telugu_char)
}

/// Returns true if the count of Telugu characters in the given text.
pub fn count_telugu_chars(text: &str) -> usize {
    text.chars().filter(|&c| is_telugu_char(c)).count()
}

/// Returns true if a character is valid in Telugu document text (Telugu, ASCII/Latin, numerals, typography/punctuation).
#[inline]
pub fn is_valid_telugu_doc_char(c: char) -> bool {
    let cp = c as u32;
    // 1. Telugu Unicode Block (U+0C00..=U+0C7F)
    if (0x0C00..=0x0C7F).contains(&cp) {
        return true;
    }
    // 2. Standard ASCII (letters, digits, whitespace, common punctuation)
    if cp <= 127 {
        return true;
    }
    // 3. Permitted Indic and typography punctuation: dandas (।, ॥), ZWNJ, ZWJ, quotes, dashes, bullets, ellipses, rupee
    matches!(
        cp,
        0x0964 | 0x0965 | 0x200C | 0x200D | 0x20B9 | 0x2013 | 0x2014 | 0x2018 | 0x2019 | 0x201C | 0x201D | 0x2022 | 0x2026
    )
}

/// Returns true if all characters in the token string are valid in Telugu document text.
pub fn is_valid_telugu_token(token_str: &str) -> bool {
    token_str.chars().all(is_valid_telugu_doc_char)
}

/// In-memory vocabulary for decoding CTC logits into text.
#[derive(Debug, Clone)]
pub struct IndraVocab {
    pub blank_idx: usize,
    pub chars: Vec<String>,
    pub det_long_side: u32,
    pub rec_height: u32,
    pub is_world: bool,
}

impl IndraVocab {
    /// Loads base Latin/math vocabulary from meta.json byte slice.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        let meta: VocabMeta = serde_json::from_slice(bytes)
            .context("Failed to parse Indra meta.json metadata")?;
        let def: VocabDef = serde_json::from_str(&meta.vocab)
            .context("Failed to parse inner vocab definition string")?;

        Ok(Self {
            blank_idx: def.blank,
            chars: def.chars,
            det_long_side: meta.det_long_side.unwrap_or(1024),
            rec_height: meta.rec_height.unwrap_or(96),
            is_world: false,
        })
    }

    /// Loads world vocabulary (including Telugu, Devanagari, etc.) from meta.json byte slice.
    pub fn from_world_json_bytes(bytes: &[u8]) -> Result<Self> {
        let meta: VocabMeta = serde_json::from_slice(bytes)
            .context("Failed to parse Indra meta.json metadata")?;
        let vocab_str = meta
            .vocab_world
            .as_deref()
            .unwrap_or(&meta.vocab);
        let def: VocabDef = serde_json::from_str(vocab_str)
            .context("Failed to parse world vocab definition string")?;

        Ok(Self {
            blank_idx: def.blank,
            chars: def.chars,
            det_long_side: meta.det_long_side.unwrap_or(1024),
            rec_height: 32, // rec_world operates on 32px height
            is_world: true,
        })
    }

    /// Checks if this vocabulary contains any Telugu script characters.
    pub fn contains_telugu(&self) -> bool {
        self.chars.iter().any(|s| has_telugu(s))
    }

    /// Builds a boolean mask where `mask[token_idx]` is true if the token is valid for Telugu text.
    /// Index 0 is the CTC blank token (always valid).
    pub fn build_telugu_mask(&self) -> Vec<bool> {
        let mut mask = Vec::with_capacity(self.chars.len() + 1);
        mask.push(true); // Token index 0 is CTC blank
        for s in &self.chars {
            mask.push(is_valid_telugu_token(s));
        }
        mask
    }

    /// Decodes a sequence of token IDs into text with CTC blank stripping and deduplication.
    pub fn decode_tokens(&self, tokens: &[i64]) -> String {
        let mut result = String::new();
        let mut prev = self.blank_idx as i64;

        for &tok in tokens {
            if tok != self.blank_idx as i64 && tok != prev {
                if tok > 0 && (tok as usize - 1) < self.chars.len() {
                    result.push_str(&self.chars[tok as usize - 1]);
                }
            }
            prev = tok;
        }

        result
    }

    /// Performs CTC greedy decoding on a 2D matrix of logits/probabilities.
    /// `logits`: sequence_length x vocab_size (row-major).
    pub fn ctc_greedy_decode(&self, logits: &[f32], seq_len: usize, vocab_size: usize) -> String {
        let mut decoded_tokens: Vec<usize> = Vec::with_capacity(seq_len);
        let mut prev_token = usize::MAX;

        for t in 0..seq_len {
            let row_start = t * vocab_size;
            let row = &logits[row_start..row_start + vocab_size];

            // Argmax
            let mut best_idx = 0;
            let mut best_val = f32::NEG_INFINITY;
            for (idx, &val) in row.iter().enumerate() {
                if val > best_val {
                    best_val = val;
                    best_idx = idx;
                }
            }

            // CTC collapse rule: skip blanks (0) and consecutive duplicates
            if best_idx != self.blank_idx && best_idx != prev_token {
                decoded_tokens.push(best_idx);
            }
            prev_token = best_idx;
        }

        // Map token indices to characters
        let mut result = String::with_capacity(decoded_tokens.len());
        for &tok in &decoded_tokens {
            // Note: in Indra vocab, token index 0 is blank. Token index 1 corresponds to chars[0].
            if tok > 0 && tok - 1 < self.chars.len() {
                result.push_str(&self.chars[tok - 1]);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meta_json_vocab() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let meta_path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("meta.json");
        let bytes = std::fs::read(meta_path).unwrap();
        let vocab = IndraVocab::from_json_bytes(&bytes).unwrap();
        assert_eq!(vocab.blank_idx, 0);
        assert!(!vocab.chars.is_empty());
        assert_eq!(vocab.det_long_side, 1024);
        assert!(!vocab.is_world);
    }

    #[test]
    fn parses_meta_json_world_vocab_and_telugu() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let meta_path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("meta.json");
        let bytes = std::fs::read(meta_path).unwrap();
        let world_vocab = IndraVocab::from_world_json_bytes(&bytes).unwrap();
        assert_eq!(world_vocab.blank_idx, 0);
        assert!(world_vocab.is_world);
        assert_eq!(world_vocab.rec_height, 32);
        assert!(world_vocab.contains_telugu(), "World vocabulary must contain Telugu script");
        assert_eq!(world_vocab.chars.len(), 2935);

        // Verify Telugu character recognition helper
        assert!(is_telugu_char('అ')); // Telugu Letter A
        assert!(is_telugu_char('క')); // Telugu Letter Ka
        assert!(is_telugu_char('్')); // Telugu Sign Virama
        assert!(!is_telugu_char('A'));
        assert!(!is_telugu_char('1'));

        let telugu_sample = "తెలుగు లిపి";
        assert!(has_telugu(telugu_sample));
        assert_eq!(count_telugu_chars(telugu_sample), 10);

        let mask = world_vocab.build_telugu_mask();
        assert_eq!(mask.len(), world_vocab.chars.len() + 1);
        assert!(mask[0], "Blank token must be valid in mask");
        // Georgian, Ethiopic, Sinhala should NOT be valid
        assert!(!is_valid_telugu_token("ტ"));
        assert!(!is_valid_telugu_token("ዮ"));
        assert!(!is_valid_telugu_token("ක"));
        assert!(is_valid_telugu_token("తె"));
        assert!(is_valid_telugu_token("A"));
        assert!(is_valid_telugu_token("123"));
    }
}
