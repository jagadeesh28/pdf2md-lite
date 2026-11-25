//! In-Process Sentence Transformer Embedding Engine.
//!
//! Evaluates `all-MiniLM-L6-v2` (quantized INT8 ONNX) to produce 384-dimensional unit-normalized
//! dense vector embeddings for semantic retrieval, chunk sidecars, and Obsidian RAG plugins.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tract_onnx::prelude::*;

type TractEmbeddingPlan = TypedRunnableModel;

#[cfg(feature = "ort")]
use std::sync::Mutex;

#[cfg(feature = "ort")]
use ort::session::Session;

/// Dimension of dense vectors produced by all-MiniLM-L6-v2.
pub const EMBEDDING_DIM: usize = 384;

static GLOBAL_EMBEDDING_ENGINE: std::sync::RwLock<Option<Option<Arc<EmbeddingEngine>>>> =
    std::sync::RwLock::new(None);
static EMBEDDING_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Invalidates cached singleton instance of the EmbeddingEngine.
pub fn invalidate_embedding_engine() {
    if let Ok(mut lock) = GLOBAL_EMBEDDING_ENGINE.write() {
        *lock = None;
    }
}

/// Evaluates text embedding models locally in-process without network access.
pub struct EmbeddingEngine {
    #[cfg(feature = "tokenizers-support")]
    tokenizer: tokenizers::Tokenizer,
    #[cfg(feature = "ort")]
    session: Option<Mutex<Session>>,
    tract_plan: Option<Arc<TractEmbeddingPlan>>,
}

impl EmbeddingEngine {
    /// Attempts to load the embedding engine from the specified model directory or default paths.
    pub fn new(custom_models_dir: Option<&Path>) -> Result<Self> {
        #[allow(unused_variables)]
        let (model_path, tokenizer_path) = resolve_embedding_paths(custom_models_dir)?;

        #[cfg(feature = "tokenizers-support")]
        let tokenizer = {
            let mut tok = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to load tokenizer from {}: {e}",
                    tokenizer_path.display()
                )
            })?;
            // Truncate to maximum 512 tokens (standard BERT/MiniLM context window)
            let _ = tok.with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }));
            tok
        };

        #[cfg(feature = "ort")]
        let session = {
            if let Ok(env) = crate::figure::models::get_shared_ort_environment()
                && let Ok(mut builder) =
                    crate::figure::models::create_optimized_session_builder(&env, Some(2), true)
                && let Ok(s) = builder.commit_from_file(&model_path)
            {
                Some(Mutex::new(s))
            } else {
                None
            }
        };

        // Pure-Rust tract-onnx fallback plan (ADR 0010)
        let tract_plan = match std::fs::read(&model_path) {
            Ok(bytes) => {
                let mut reader = std::io::Cursor::new(bytes);
                tract_onnx::onnx()
                    .model_for_read(&mut reader)
                    .ok()
                    .and_then(|m| m.into_typed().ok())
                    .and_then(|m| m.into_runnable().ok())
            }
            Err(_) => None,
        };

        #[cfg(feature = "ort")]
        let has_model = session.is_some() || tract_plan.is_some();
        #[cfg(not(feature = "ort"))]
        let has_model = tract_plan.is_some();

        if !has_model {
            anyhow::bail!(
                "Failed to load embedding model from {}",
                model_path.display()
            );
        }

        Ok(Self {
            #[cfg(feature = "tokenizers-support")]
            tokenizer,
            #[cfg(feature = "ort")]
            session,
            tract_plan,
        })
    }

    /// Embeds a single text string into a 384-dimensional unit L2-normalized vector.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text])?;
        results
            .into_iter()
            .next()
            .context("No embedding produced for text")
    }

    /// Embeds a batch of texts into 384-dimensional unit L2-normalized vectors using real tensor batching.
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        #[cfg(feature = "tokenizers-support")]
        {
            let encodings = self
                .tokenizer
                .encode_batch(texts.to_vec(), true)
                .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;

            let batch_size = texts.len();
            let max_seq_len = encodings
                .iter()
                .map(|e| e.get_ids().len())
                .max()
                .unwrap_or(0);
            if max_seq_len == 0 {
                return Ok(vec![vec![0.0f32; EMBEDDING_DIM]; batch_size]);
            }

            #[cfg(feature = "ort")]
            if let Some(ref session_mutex) = self.session {
                let mut session_guard = session_mutex
                    .lock()
                    .map_err(|e| anyhow::anyhow!("Embedding mutex poisoned: {e}"))?;

                let mut input_ids_flat = vec![0i64; batch_size * max_seq_len];
                let mut attention_mask_flat = vec![0i64; batch_size * max_seq_len];
                let mut token_type_ids_flat = vec![0i64; batch_size * max_seq_len];

                for (i, enc) in encodings.iter().enumerate() {
                    let ids = enc.get_ids();
                    let mask = enc.get_attention_mask();
                    let type_ids = enc.get_type_ids();
                    for j in 0..ids.len() {
                        let idx = i * max_seq_len + j;
                        input_ids_flat[idx] = ids[j] as i64;
                        attention_mask_flat[idx] = mask[j] as i64;
                        token_type_ids_flat[idx] = type_ids[j] as i64;
                    }
                }

                let input_ids_tensor = ort::value::Tensor::from_array((
                    [batch_size, max_seq_len],
                    input_ids_flat.into_boxed_slice(),
                ))?;
                let attention_mask_tensor = ort::value::Tensor::from_array((
                    [batch_size, max_seq_len],
                    attention_mask_flat.into_boxed_slice(),
                ))?;
                let token_type_ids_tensor = ort::value::Tensor::from_array((
                    [batch_size, max_seq_len],
                    token_type_ids_flat.into_boxed_slice(),
                ))?;

                let inputs = ort::inputs![
                    "input_ids" => input_ids_tensor,
                    "attention_mask" => attention_mask_tensor,
                    "token_type_ids" => token_type_ids_tensor,
                ];

                let outputs = session_guard.run(inputs)?;

                // Output 0: last_hidden_state [batch_size, max_seq_len, hidden_dim]
                let (hidden_shape, hidden_slice) =
                    outputs[0].try_extract_tensor::<f32>().map_err(|e| {
                        anyhow::anyhow!("Failed to extract last_hidden_state tensor: {e}")
                    })?;

                let hidden_dim = if hidden_shape.len() >= 3 {
                    hidden_shape[2] as usize
                } else {
                    EMBEDDING_DIM
                };

                let mut all_embeddings = Vec::with_capacity(batch_size);

                for (i, enc) in encodings.iter().enumerate() {
                    let mask = enc.get_attention_mask();
                    let mut pooled = vec![0.0f32; hidden_dim];
                    let mut mask_sum = 0.0f32;

                    for (t, &m) in mask.iter().enumerate() {
                        if m == 1 {
                            mask_sum += 1.0;
                            let offset = (i * max_seq_len + t) * hidden_dim;
                            if offset + hidden_dim <= hidden_slice.len() {
                                for d in 0..hidden_dim {
                                    pooled[d] += hidden_slice[offset + d];
                                }
                            }
                        }
                    }

                    if mask_sum > 0.0 {
                        for val in &mut pooled {
                            *val /= mask_sum;
                        }
                    }

                    // L2 normalization
                    let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
                    let scale = if norm > 1e-12 { 1.0 / norm } else { 1.0 };
                    for val in &mut pooled {
                        *val *= scale;
                    }

                    all_embeddings.push(pooled);
                }

                return Ok(all_embeddings);
            }

            // Fallback to tract-onnx
            if let Some(ref plan) = self.tract_plan {
                let mut all_embeddings = Vec::with_capacity(batch_size);
                for enc in &encodings {
                    let ids: Vec<i64> = enc.get_ids().iter().map(|&x| x as i64).collect();
                    let mask: Vec<i64> =
                        enc.get_attention_mask().iter().map(|&x| x as i64).collect();
                    let type_ids: Vec<i64> = enc.get_type_ids().iter().map(|&x| x as i64).collect();
                    let seq_len = ids.len();

                    let input_ids: Tensor =
                        tract_ndarray::Array2::from_shape_vec((1, seq_len), ids)?.into();
                    let attention_mask: Tensor =
                        tract_ndarray::Array2::from_shape_vec((1, seq_len), mask.clone())?.into();
                    let token_type_ids: Tensor =
                        tract_ndarray::Array2::from_shape_vec((1, seq_len), type_ids)?.into();

                    let outputs = plan.run(tvec![
                        input_ids.into(),
                        attention_mask.into(),
                        token_type_ids.into()
                    ])?;
                    let hidden_view = outputs[0].to_plain_array_view::<f32>()?;
                    let hidden_slice = hidden_view.as_slice().unwrap_or(&[]);
                    let hidden_dim = EMBEDDING_DIM;

                    let mut pooled = vec![0.0f32; hidden_dim];
                    let mut mask_sum = 0.0f32;
                    for (t, &m) in mask.iter().enumerate() {
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
                        for val in &mut pooled {
                            *val /= mask_sum;
                        }
                    }
                    let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
                    let scale = if norm > 1e-12 { 1.0 / norm } else { 1.0 };
                    for val in &mut pooled {
                        *val *= scale;
                    }
                    all_embeddings.push(pooled);
                }
                return Ok(all_embeddings);
            }

            anyhow::bail!("No active neural runtime available for embedding generation");
        }

        #[cfg(not(feature = "tokenizers-support"))]
        {
            anyhow::bail!("tokenizers-support feature is required to compute embeddings")
        }
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
    let locator = crate::model_locator::ModelLocator::new(custom_dir.map(PathBuf::from));
    let model_path = locator
        .locate("models/embeddings/model.onnx")
        .or_else(|| locator.locate("models/embeddings/all-MiniLM-L6-v2.onnx"))
        .or_else(|| locator.locate("model.onnx"));
    let tokenizer_path = locator
        .locate("models/embeddings/tokenizer.json")
        .or_else(|| locator.locate("tokenizer.json"));

    if let (Some(m), Some(t)) = (model_path, tokenizer_path) {
        return Ok((m, t));
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
    if let Ok(guard) = GLOBAL_EMBEDDING_ENGINE.read()
        && let Some(ref cached) = *guard
    {
        return cached.clone();
    }
    let _lock = EMBEDDING_INIT_LOCK.lock().ok()?;
    if let Ok(guard) = GLOBAL_EMBEDDING_ENGINE.read()
        && let Some(ref cached) = *guard
    {
        return cached.clone();
    }

    let result = match EmbeddingEngine::new(custom_dir) {
        Ok(engine) => Some(Arc::new(engine)),
        Err(e) => {
            tracing::warn!("Note: In-process embedding engine inactive: {e}");
            None
        }
    };

    if let Ok(mut guard) = GLOBAL_EMBEDDING_ENGINE.write() {
        *guard = Some(result.clone());
    }
    result
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
