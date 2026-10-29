//! Model weight provisioning and loading for the YOLO Document Layout Engine.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

static CUSTOM_LAYOUT_PATH: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Sets a global custom weights path for the YOLO Document Layout Engine.
pub fn set_custom_layout_path(path: PathBuf) {
    if let Ok(mut lock) = CUSTOM_LAYOUT_PATH.write() {
        *lock = Some(path);
    }
}

/// Retrieves the global custom weights path if configured.
pub fn get_custom_layout_path() -> Option<PathBuf> {
    CUSTOM_LAYOUT_PATH.read().ok().and_then(|p| p.clone())
}

#[cfg(feature = "embed-models")]
const EMBEDDED_LAYOUT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/layout/yolov8n-doclaynet.onnx"
));

/// Provides raw ONNX bytes for the YOLO Document Layout Engine.
pub struct LayoutModelSource {
    custom_path: Option<PathBuf>,
}

impl LayoutModelSource {
    /// Creates a new model source with an optional custom weights path.
    pub fn new(custom_path: Option<PathBuf>) -> Self {
        let custom_path = custom_path.or_else(get_custom_layout_path);
        Self { custom_path }
    }

    /// Loads the YOLO layout detector model bytes from custom path, workspace directory, or embedded binary.
    pub fn load_layout_model(&self) -> Result<Vec<u8>> {
        // 1. Check custom override path if specified
        if let Some(ref path) = self.custom_path {
            if path.exists() {
                return fs::read(path)
                    .with_context(|| format!("Failed to read custom layout model at {:?}", path));
            }
        }

        // 2. Check local workspace models/layout/ directory
        let local_path = Path::new("models/layout/yolov8n-doclaynet.onnx");
        if local_path.exists() {
            return fs::read(local_path)
                .with_context(|| format!("Failed to read layout model at {:?}", local_path));
        }

        // 3. Fallback to embedded static binary slice if feature = "embed-models" is active
        #[cfg(feature = "embed-models")]
        {
            return Ok(EMBEDDED_LAYOUT.to_vec());
        }

        #[cfg(not(feature = "embed-models"))]
        {
            anyhow::bail!(
                "YOLO Document Layout model not found at '{}' and embed-models feature is disabled",
                local_path.display()
            );
        }
    }
}
