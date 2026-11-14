//! Model weight provisioning and loading for Indra OCR (ADR 0014).

use anyhow::Result;
use std::borrow::Cow;
use std::path::PathBuf;

/// Provides raw ONNX bytes and metadata for the Indra OCR pipeline.
pub struct IndraModelSource {
    custom_path: Option<PathBuf>,
}

impl IndraModelSource {
    /// Creates a new model source with an optional custom weights directory.
    pub fn new(custom_path: Option<PathBuf>) -> Self {
        Self { custom_path }
    }

    /// Loads model bytes as a zero-copy static slice (if embedded) or owned vector.
    pub fn load_bytes(&self, file_name: &str) -> Result<Cow<'static, [u8]>> {
        let subpath = format!("models/indra/{file_name}");
        let locator = crate::model_locator::ModelLocator::new(self.custom_path.clone());
        locator.load_bytes(&subpath)
    }

    /// Retrieves byte slice or owned vector for a model file for backward compatibility.
    pub fn load_file(&self, file_name: &str) -> Result<Vec<u8>> {
        self.load_bytes(file_name).map(|cow| cow.into_owned())
    }
}
