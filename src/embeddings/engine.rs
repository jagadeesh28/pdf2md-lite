//! In-Process Sentence Transformer Embedding Engine.
//!
//! Evaluates `all-MiniLM-L6-v2` (quantized INT8 ONNX) to produce 384-dimensional unit-normalized
//! dense vector embeddings for semantic retrieval, chunk sidecars, and Obsidian RAG plugins.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "ort")]
use ort::session::Session;

/// Dimension of dense vectors produced by all-MiniLM-L6-v2.
pub const EMBEDDING_DIM: usize = 384;

/// Global singleton instance of the EmbeddingEngine.
static GLOBAL_EMBEDDING_ENGINE: OnceLock<Option<Arc<EmbeddingEngine>>> = OnceLock::new();

/// Evaluates text embedding models locally in-process without network access.
pub struct EmbeddingEngine {
    #[cfg(feature = "embed-models")]
    tokenizer: tokenizers::Tokenizer,
    #[cfg(feature = "ort")]
    session: Mutex<Session>,
}

impl EmbeddingEngine {
    /// Attempts to load the embedding engine from the specified model directory or default paths.
    pub fn new(custom_models_dir: Option<&Path>) -> Result<Self> {
        let (model_path, tokenizer_path) = resolve_embedding_paths(custom_models_dir)?;

        #[cfg(feature = "embed-models")]
        let tokenizer = {
            let mut tok = tokenizers::Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| anyhow::anyhow!("Failed to load tokenizer from {}: {e}", tokenizer_path.display()))?;
            // Truncate to maximum 512 tokens (standard BERT/MiniLM context window)
            let _ = tok.with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }));
            tok
        };

        #[cfg(feature = "ort")]
        let session = {
            let env = crate::figure::models::get_shared_ort_environment()?;
            let s = Session::builder(&env)
                .map_err(|e| anyhow::anyhow!("Failed to create embedding session builder: {e}"))?
                .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
                .map_err(|e| anyhow::anyhow!("Failed to set embedding optimization level: {e}"))?
                .commit_from_file(&model_path)
                .map_err(|e| anyhow::anyhow!("Failed to load embedding model from {}: {e}", model_path.display()))?;
            Mutex::new(s)
        };

        Ok(Self {
            #[cfg(feature = "embed-models")]
            tokenizer,
            #[cfg(feature = "ort")]
            session,
        })
    }

    /// Embeds a single text string into a 384-dimensional unit L2-normalized vector.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text])?;
        results.into_iter().next().context("No embedding produced for text")
    }

    /// Embeds a batch of texts into 384-dimensional unit L2-normalized vectors.
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        #[cfg(all(feature = "embed-models", feature = "ort"))]
        {
            let mut session_guard = self.session.lock().map_err(|e| anyhow::anyhow!("Embedding mutex poisoned: {e}"))?;

            for &text in texts {
                let encoding = self.tokenizer.encode(text, true)
                    .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;

                let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
                let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&x| x as i64).collect();
                let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();

                let seq_len = input_ids.len();
                if seq_len == 0 {
                    all_embeddings.push(vec![0.0f32; EMBEDDING_DIM]);
                    continue;
                }

                let input_ids_tensor = ort::value::Tensor::from_array((
                    [1usize, seq_len],
                    input_ids.into_boxed_slice(),
                ))?;
                let attention_mask_tensor = ort::value::Tensor::from_array((
                    [1usize, seq_len],
                    attention_mask.clone().into_boxed_slice(),
                ))?;
                let token_type_ids_tensor = ort::value::Tensor::from_array((
                    [1usize, seq_len],
                    token_type_ids.into_boxed_slice(),
                ))?;

                let inputs = ort::inputs![
                    "input_ids" => input_ids_tensor,
                    "attention_mask" => attention_mask_tensor,
                    "token_type_ids" => token_type_ids_tensor,
                ];

                let outputs = session_guard.run(inputs)?;

                // Output 0: last_hidden_state [1, seq_len, 384]
                let (hidden_shape, hidden_slice) = outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| anyhow::anyhow!("Failed to extract last_hidden_state tensor: {e}"))?;

                let hidden_dim = if hidden_shape.len() >= 3 {
                    hidden_shape[2] as usize
                } else {
                    EMBEDDING_DIM
                };

                // Mean pooling across non-masked tokens
                let mut pooled = vec![0.0f32; hidden_dim];
                let mut mask_sum = 0.0f32;

                for (t, &m) in attention_mask.iter().enumerate() {
                    if m == 1 {
                        mask_sum += 1.0;
                        let offset = t * hidden_dim;
                        if offset + hidden_dim <= hidden_slice.len() {
                            for d in 0..hidden_dim {
                                pooled[d] += hidden_slice[offset + d];
                            }
                        }
                    }
                }

                if mask_sum > 0.0 {
                    for d in 0..hidden_dim {
                        pooled[d] /= mask_sum;
                    }
                }

                // L2 normalization
                let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
                let scale = if norm > 1e-12 { 1.0 / norm } else { 1.0 };
                for d in 0..hidden_dim {
                    pooled[d] *= scale;
                }

                all_embeddings.push(pooled);
            }
        }

        #[cfg(not(all(feature = "embed-models", feature = "ort")))]
        {
            anyhow::bail!("embed-models and ort features are required to compute embeddings");
        }

        #[allow(unreachable_code)]
        Ok(all_embeddings)
    }
}

/// Calculates the cosine similarity between two unit-normalized embedding vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Resolves filesystem paths for the ONNX embedding model and tokenizer.
pub fn resolve_embedding_paths(custom_dir: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    let candidate_dirs = [
        custom_dir.map(PathBuf::from),
        Some(PathBuf::from("models/embeddings")),
        Some(PathBuf::from("models")),
    ];

    for dir_opt in candidate_dirs.into_iter().flatten() {
        let model_path = dir_opt.join("model.onnx");
        let tokenizer_path = dir_opt.join("tokenizer.json");

        if model_path.is_file() && tokenizer_path.is_file() {
            return Ok((model_path, tokenizer_path));
        }

        // Also check if named all-MiniLM-L6-v2.onnx
        let alt_model = dir_opt.join("all-MiniLM-L6-v2.onnx");
        if alt_model.is_file() && tokenizer_path.is_file() {
            return Ok((alt_model, tokenizer_path));
        }
    }

    anyhow::bail!(
        "Embedding model not found. Expected 'models/embeddings/model.onnx' and 'models/embeddings/tokenizer.json'.\nRun 'pdfmd-lite --download-models' to download official weights."
    )
}

/// Checks if embedding models exist locally without raising an error.
pub fn embedding_models_exist(custom_dir: Option<&Path>) -> bool {
    resolve_embedding_paths(custom_dir).is_ok()
}

/// Gets or initializes the shared global EmbeddingEngine singleton.
pub fn get_embedding_engine(custom_dir: Option<&Path>) -> Option<Arc<EmbeddingEngine>> {
    GLOBAL_EMBEDDING_ENGINE
        .get_or_init(|| {
            EmbeddingEngine::new(custom_dir)
                .map(Arc::new)
                .map_err(|e| {
                    eprintln!("Note: In-process embedding engine inactive: {e}");
                    e
                })
                .ok()
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![0.57735, 0.57735, 0.57735];
        let v2 = vec![0.57735, 0.57735, 0.57735];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![-1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - (-1.0)).abs() < 1e-4);
    }
}
