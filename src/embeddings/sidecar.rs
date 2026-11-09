//! Decentralized Embedding Sidecar serialization (`<slug>.embeddings.jsonl`).
//!
//! Stores semantically chunked text segments, physical page references, and dense vector
//! embeddings alongside converted notes matching the standard Obsidian RAG schema per ADR 0009.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// Standard RAG record schema matching Obsidian search & RAG plugins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingRecord {
    /// Unique identifier for this chunk (e.g. `<slug>#p1-c0`).
    pub chunk_id: String,
    /// Physical 1-based page number where the chunk originated.
    pub page_num: usize,
    /// Nearest enclosing Markdown heading if present.
    pub heading: Option<String>,
    /// Chunk text formatted in Markdown.
    pub text: String,
    /// 384-dimensional dense vector embedding (unit L2-normalized).
    pub embedding: Vec<f32>,
}

/// Derives the adjacent `.embeddings.jsonl` sidecar path for a given Markdown note path.
pub fn sidecar_path_for_markdown(md_path: &Path) -> std::path::PathBuf {
    let mut p = md_path.to_path_buf();
    p.set_extension("embeddings.jsonl");
    p
}

/// Checks whether an existing sidecar is up to date relative to its Markdown note.
/// Returns `true` if the sidecar should be created or updated (i.e. missing, empty, or older than the note).
pub fn should_update_embeddings(md_path: &Path, sidecar_path: &Path) -> bool {
    if !sidecar_path.is_file() {
        return true;
    }

    let sidecar_meta = match fs::metadata(sidecar_path) {
        Ok(m) => m,
        Err(_) => return true,
    };

    if sidecar_meta.len() == 0 {
        return true;
    }

    let md_meta = match fs::metadata(md_path) {
        Ok(m) => m,
        Err(_) => return true,
    };

    let md_modified = match md_meta.modified() {
        Ok(t) => t,
        Err(_) => return true,
    };

    let sidecar_modified = match sidecar_meta.modified() {
        Ok(t) => t,
        Err(_) => return true,
    };

    // If the Markdown file was modified after the sidecar, re-embedding is needed
    md_modified > sidecar_modified
}

/// Writes a collection of EmbeddingRecords to a JSON Lines sidecar file.
pub fn write_embeddings_sidecar(path: &Path, records: &[EmbeddingRecord]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(path)
        .with_context(|| format!("Failed to create embedding sidecar: {}", path.display()))?;
    let mut writer = BufWriter::new(file);

    for record in records {
        let json =
            serde_json::to_string(record).context("Failed to serialize EmbeddingRecord to JSON")?;
        writeln!(writer, "{json}")?;
    }

    writer.flush()?;
    Ok(())
}

/// Reads all EmbeddingRecords from a JSON Lines sidecar file.
pub fn read_embeddings_sidecar(path: &Path) -> Result<Vec<EmbeddingRecord>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open embedding sidecar: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (line_num, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let record: EmbeddingRecord = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "Failed to deserialize record at line {} in {}",
                line_num + 1,
                path.display()
            )
        })?;
        records.push(record);
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidecar_roundtrip_serialization() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sidecar_file = temp_dir.path().join("sample.embeddings.jsonl");

        let records = vec![
            EmbeddingRecord {
                chunk_id: "sample#p1-c0".to_string(),
                page_num: 1,
                heading: Some("Introduction".to_string()),
                text: "Text of chunk 0".to_string(),
                embedding: vec![0.1, -0.2, 0.5, 0.9],
            },
            EmbeddingRecord {
                chunk_id: "sample#p1-c1".to_string(),
                page_num: 1,
                heading: Some("Methodology".to_string()),
                text: "Text of chunk 1".to_string(),
                embedding: vec![0.3, 0.4, -0.1, 0.0],
            },
        ];

        write_embeddings_sidecar(&sidecar_file, &records).unwrap();
        assert!(sidecar_file.exists());

        let read_back = read_embeddings_sidecar(&sidecar_file).unwrap();
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[0].chunk_id, "sample#p1-c0");
        assert_eq!(read_back[1].heading.as_deref(), Some("Methodology"));
        assert_eq!(read_back[0].embedding, vec![0.1, -0.2, 0.5, 0.9]);
    }

    #[test]
    fn test_sidecar_path_derivation() {
        let md = Path::new("output/turbines/turbines.md");
        let sidecar = sidecar_path_for_markdown(md);
        assert_eq!(
            sidecar,
            Path::new("output/turbines/turbines.embeddings.jsonl")
        );
    }
}
