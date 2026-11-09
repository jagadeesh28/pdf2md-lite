//! In-process sentence transformer embeddings and semantic sidecars for Obsidian RAG.
//!
//! Provides 100% offline text embedding generation, section-aware semantic chunking,
//! and decentralized `<slug>.embeddings.jsonl` sidecars adhering to ADR 0009 and ADR 0010.

pub mod chunking;
pub mod engine;
pub mod sidecar;

pub use chunking::{SemanticChunk, chunk_markdown_document};
pub use engine::{
    EMBEDDING_DIM, EmbeddingEngine, cosine_similarity, embedding_models_exist,
    get_embedding_engine, resolve_embedding_paths,
};
pub use sidecar::{
    EmbeddingRecord, read_embeddings_sidecar, should_update_embeddings, sidecar_path_for_markdown,
    write_embeddings_sidecar,
};

use anyhow::{Context, Result};
use std::path::Path;

/// Generates a `.embeddings.jsonl` sidecar for a given Markdown note content and writes it adjacent to `md_path`.
pub fn generate_and_save_sidecar(
    content: &str,
    slug: &str,
    md_path: &Path,
    engine: &EmbeddingEngine,
) -> Result<usize> {
    let chunks = chunk_markdown_document(content, slug);
    if chunks.is_empty() {
        return Ok(0);
    }

    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let embeddings = engine
        .embed_batch(&texts)
        .context("Failed to compute embeddings for chunks")?;

    let records: Vec<EmbeddingRecord> = chunks
        .into_iter()
        .zip(embeddings)
        .map(|(chunk, embedding)| EmbeddingRecord {
            chunk_id: chunk.chunk_id,
            page_num: chunk.page_num,
            heading: chunk.heading,
            text: chunk.text,
            embedding,
        })
        .collect();

    let sidecar_path = sidecar_path_for_markdown(md_path);
    let count = records.len();
    write_embeddings_sidecar(&sidecar_path, &records)?;

    Ok(count)
}
