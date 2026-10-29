//! Model weight provisioning and loading for Indra OCR.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(feature = "embed-models")]
const EMBEDDED_DET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/indra/det.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_REC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/indra/rec.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_ORDER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/indra/order.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_META: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/indra/meta.json"
));

/// Provides raw ONNX bytes and metadata for the Indra OCR pipeline.
pub struct IndraModelSource {
    custom_path: Option<PathBuf>,
}

impl IndraModelSource {
    /// Creates a new model source with an optional custom weights directory.
    pub fn new(custom_path: Option<PathBuf>) -> Self {
        Self { custom_path }
    }

    /// Retrieves byte slice or owned vector for a model file.
    pub fn load_file(&self, file_name: &str) -> Result<Vec<u8>> {
        // 1. Check custom path override first if provided
        if let Some(ref dir) = self.custom_path {
            let path = dir.join(file_name);
            if path.exists() {
                return fs::read(&path)
                    .with_context(|| format!("Failed to read custom weight file at {:?}", path));
            }
        }

        // 2. Check local workspace models/indra/ directory
        let local_path = Path::new("models").join("indra").join(file_name);
        if local_path.exists() {
            return fs::read(&local_path)
                .with_context(|| format!("Failed to read local weight file at {:?}", local_path));
        }

        // 3. Check embedded models if enabled
        #[cfg(feature = "embed-models")]
        {
            match file_name {
                "det.onnx" => return Ok(EMBEDDED_DET.to_vec()),
                "rec.onnx" => return Ok(EMBEDDED_REC.to_vec()),
                "order.onnx" => return Ok(EMBEDDED_ORDER.to_vec()),
                "meta.json" => return Ok(EMBEDDED_META.to_vec()),
                _ => {}
            }
        }

        anyhow::bail!(
            "Model file '{}' not found in custom path, local 'models/indra/', or embedded binary.",
            file_name
        )
    }
}
