//! Character vocabulary and CTC decoding for Indra OCR.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VocabMeta {
    vocab: String,
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

/// In-memory vocabulary for decoding CTC logits into text.
#[derive(Debug, Clone)]
pub struct IndraVocab {
    pub blank_idx: usize,
    pub chars: Vec<String>,
    pub det_long_side: u32,
    pub rec_height: u32,
}

impl IndraVocab {
    /// Loads vocabulary from meta.json byte slice.
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
        })
    }

    /// Decodes a sequence of token IDs into text with CTC blank stripping and deduplication.
    pub fn decode_tokens(&self, tokens: &[i64]) -> String {
        let mut result = String::new();
        let mut prev = self.blank_idx as i64;

        for &tok in tokens {
            if tok != self.blank_idx as i64 {
                if tok != prev {
                    if tok > 0 && (tok as usize - 1) < self.chars.len() {
                        result.push_str(&self.chars[tok as usize - 1]);
                    }
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
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let meta_path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("meta.json");
        let bytes = std::fs::read(meta_path).unwrap();
        let vocab = IndraVocab::from_json_bytes(&bytes).unwrap();
        assert_eq!(vocab.blank_idx, 0);
        assert!(!vocab.chars.is_empty());
        assert_eq!(vocab.det_long_side, 1024);
        assert_eq!(vocab.rec_height, 32); // meta specifies 32 or 96
    }
}
