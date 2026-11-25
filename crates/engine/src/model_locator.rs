//! Unified Cascading Model Locator and Zero-Copy Provisioning (ADR 0014, ADR 0017, ADR 0018).
//!
//! Discovers and loads offline neural model weights in strict descending precedence:
//! 1. Explicit `--models-dir` CLI flag or `AppConfig.models_dir` setting.
//! 2. Binary-adjacent directory (`std::env::current_exe().parent()/models`).
//! 3. Per-user OS AppData models directory (`pdfmd_lite::config::default_data_dir().join("models")`).
//! 4. Development workspace paths (`./models`, `../../models`).
//! 5. Embedded static binaries (`Cow::Borrowed(&'static [u8])`) when feature `bundle-weights` is active.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Embedded `MANIFEST.toml` defining exact SHA-256 hashes, sizes, and tensor signatures.
pub const MANIFEST_TOML: &str = include_str!("../../../models/MANIFEST.toml");

/// Model manifest schema matching `models/MANIFEST.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    #[serde(rename = "models")]
    pub models: Vec<ModelManifestEntry>,
}

/// Metadata and integrity signature for a single model in `MANIFEST.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifestEntry {
    pub id: String,
    pub name: String,
    pub subpath: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub license: String,
    pub required: bool,
    #[serde(default)]
    pub inputs: Vec<TensorSpec>,
    #[serde(default)]
    pub outputs: Vec<TensorSpec>,
}

/// ONNX input/output tensor specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorSpec {
    pub name: String,
    pub shape: Vec<i64>,
    pub dtype: String,
}

impl ModelManifest {
    /// Parses the compile-time embedded `models/MANIFEST.toml`.
    pub fn embedded() -> Result<Self> {
        toml::from_str(MANIFEST_TOML).context("Failed to parse embedded models/MANIFEST.toml")
    }

    /// Finds a manifest entry matching the given subpath.
    pub fn find_by_subpath(&self, subpath: &str) -> Option<&ModelManifestEntry> {
        let clean = ModelLocator::strip_models_prefix(subpath);
        self.models.iter().find(|m| {
            let entry_clean = ModelLocator::strip_models_prefix(&m.subpath);
            entry_clean == clean || m.subpath == subpath
        })
    }
}

static GLOBAL_MODELS_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Sets the process-wide explicit model directory override.
pub fn set_global_models_dir(dir: PathBuf) {
    if let Ok(mut lock) = GLOBAL_MODELS_DIR.write() {
        *lock = Some(dir);
    }
}

/// Retrieves the process-wide explicit model directory override if configured.
pub fn get_global_models_dir() -> Option<PathBuf> {
    GLOBAL_MODELS_DIR.read().ok().and_then(|p| p.clone())
}

#[cfg(feature = "bundle-weights")]
const EMBEDDED_DET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/det.onnx"
));
#[cfg(feature = "bundle-weights")]
const EMBEDDED_REC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/rec.onnx"
));
#[cfg(feature = "bundle-weights")]
const EMBEDDED_REC_WORLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/rec_world.onnx"
));
#[cfg(feature = "bundle-weights")]
const EMBEDDED_META: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/meta.json"
));
#[cfg(feature = "bundle-weights")]
const EMBEDDED_LAYOUT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/layout/yolov8n-doclaynet.onnx"
));

/// Status record for a single neural model used in diagnostic reports and UI/REST APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub name: String,
    pub subpath: String,
    pub required: bool,
    pub resolved_path: Option<PathBuf>,
    pub is_embedded: bool,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub expected_size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub expected_sha256: Option<String>,
    pub is_valid: bool,
    pub error: Option<String>,
}

/// Known model manifest for all offline neural pipelines (12 active models).
pub const KNOWN_MODELS: &[(&str, &str, bool)] = &[
    ("Indra OCR DBNet Detection", "models/indra/det.onnx", true),
    (
        "Indra OCR SVTR Latin Recognition",
        "models/indra/rec.onnx",
        true,
    ),
    (
        "Indra OCR SVTR World/Telugu Recognition",
        "models/indra/rec_world.onnx",
        false,
    ),
    (
        "Indra OCR Metadata and Vocab",
        "models/indra/meta.json",
        true,
    ),
    (
        "DocLayNet YOLOv8n Layout Detector",
        "models/layout/yolov8n-doclaynet.onnx",
        true,
    ),
    (
        "SLANet Table Structure Recognizer",
        "models/layout/slanet.onnx",
        false,
    ),
    (
        "SLANet Table Structure Vocab",
        "models/layout/table_structure_dict.txt",
        false,
    ),
    ("RapidLaTeXOCR Encoder", "models/latex/encoder.onnx", false),
    ("RapidLaTeXOCR Decoder", "models/latex/decoder.onnx", false),
    (
        "RapidLaTeXOCR Tokenizer",
        "models/latex/tokenizer.json",
        false,
    ),
    (
        "all-MiniLM-L6-v2 Embeddings",
        "models/embeddings/model.onnx",
        false,
    ),
    (
        "all-MiniLM-L6-v2 Tokenizer",
        "models/embeddings/tokenizer.json",
        false,
    ),
];

/// Unified Model Locator resolving offline model weights.
pub struct ModelLocator {
    custom_dir: Option<PathBuf>,
}

impl ModelLocator {
    /// Creates a new locator with an optional explicit custom directory.
    pub fn new(custom_dir: Option<PathBuf>) -> Self {
        Self {
            custom_dir: custom_dir.or_else(get_global_models_dir),
        }
    }

    /// Normalizes subpath by stripping leading `models/` if searching inside a `models` root.
    pub fn strip_models_prefix(subpath: &str) -> &str {
        subpath.strip_prefix("models/").unwrap_or(subpath)
    }

    /// Resolves the filesystem path for a model subpath, if present on disk.
    pub fn locate(&self, subpath: &str) -> Option<PathBuf> {
        let clean_sub = Self::strip_models_prefix(subpath);

        // 1. Explicit override directory
        if let Some(ref dir) = self.custom_dir {
            let candidate = dir.join(clean_sub);
            if candidate.exists() {
                return Some(candidate);
            }
            let candidate_full = dir.join(subpath);
            if candidate_full.exists() {
                return Some(candidate_full);
            }
        }

        // 2. Executable-adjacent models/ directory
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let candidate = exe_dir.join("models").join(clean_sub);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // 3. Per-user OS AppData models directory (ADR 0014, ADR 0017)
        let appdata_models = crate::config::default_data_dir()
            .join("models")
            .join(clean_sub);
        if appdata_models.exists() {
            return Some(appdata_models);
        }

        // 4. Current working directory / workspace relative
        let cwd_candidate = Path::new(subpath);
        if cwd_candidate.exists() {
            return Some(cwd_candidate.to_path_buf());
        }
        let cwd_models_candidate = Path::new("models").join(clean_sub);
        if cwd_models_candidate.exists() {
            return Some(cwd_models_candidate);
        }

        // 5. Upstream workspace directory (for tests and sub-crates)
        let upstream_candidate = Path::new("../../").join(subpath);
        if upstream_candidate.exists() {
            return Some(upstream_candidate);
        }
        let upstream_models = Path::new("../../models").join(clean_sub);
        if upstream_models.exists() {
            return Some(upstream_models);
        }

        None
    }

    /// Returns embedded static byte slice if compiled with `bundle-weights`.
    #[allow(unused_variables)]
    fn get_embedded_slice(subpath: &str) -> Option<&'static [u8]> {
        #[cfg(feature = "bundle-weights")]
        {
            let clean = Self::strip_models_prefix(subpath);
            match clean {
                "indra/det.onnx" | "det.onnx" => Some(EMBEDDED_DET),
                "indra/rec.onnx" | "rec.onnx" => Some(EMBEDDED_REC),
                "indra/rec_world.onnx" | "rec_world.onnx" => Some(EMBEDDED_REC_WORLD),
                "indra/meta.json" | "meta.json" => Some(EMBEDDED_META),
                "layout/yolov8n-doclaynet.onnx" | "yolov8n-doclaynet.onnx" => Some(EMBEDDED_LAYOUT),
                _ => None,
            }
        }
        #[cfg(not(feature = "bundle-weights"))]
        {
            None
        }
    }

    /// Loads model bytes as a zero-copy static slice (if embedded) or owned vector (if read from disk).
    pub fn load_bytes(&self, subpath: &str) -> Result<Cow<'static, [u8]>> {
        // Check disk first if custom directory is explicitly set
        if self.custom_dir.is_some()
            && let Some(path) = self.locate(subpath)
        {
            let data = fs::read(&path)
                .with_context(|| format!("Failed to read model file at {}", path.display()))?;
            return Ok(Cow::Owned(data));
        }

        // Check embedded weights if available
        if let Some(slice) = Self::get_embedded_slice(subpath) {
            return Ok(Cow::Borrowed(slice));
        }

        // Fallback to disk discovery
        if let Some(path) = self.locate(subpath) {
            let data = fs::read(&path)
                .with_context(|| format!("Failed to read model file at {}", path.display()))?;
            return Ok(Cow::Owned(data));
        }

        Err(anyhow!(
            "Model file '{}' not found in custom path, exe-adjacent 'models/', AppData, workspace, or embedded binary.",
            subpath
        ))
    }

    /// Collects status and SHA-256 validation for all known offline models against `MANIFEST.toml`.
    pub fn doctor_report(&self) -> Vec<ModelStatus> {
        let manifest = ModelManifest::embedded().ok();
        let mut report = Vec::with_capacity(KNOWN_MODELS.len());

        for &(name, subpath, required) in KNOWN_MODELS {
            let manifest_entry = manifest.as_ref().and_then(|m| m.find_by_subpath(subpath));
            let expected_sha256 = manifest_entry.map(|e| e.sha256.clone());
            let expected_size_bytes = manifest_entry.map(|e| e.size_bytes);

            let disk_path = self.locate(subpath);
            let embedded = Self::get_embedded_slice(subpath);

            let exists = disk_path.is_some() || embedded.is_some();
            let is_embedded = disk_path.is_none() && embedded.is_some();

            let (size_bytes, sha256) = if let Some(ref path) = disk_path {
                let size = fs::metadata(path).ok().map(|m| m.len());
                let hash = fs::read(path).ok().map(|bytes| {
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    format!("{:x}", hasher.finalize())
                });
                (size, hash)
            } else if let Some(slice) = embedded {
                let mut hasher = Sha256::new();
                hasher.update(slice);
                (
                    Some(slice.len() as u64),
                    Some(format!("{:x}", hasher.finalize())),
                )
            } else {
                (None, None)
            };

            let (is_valid, error) = if exists {
                if let (Some(actual_hash), Some(expected_hash)) = (&sha256, &expected_sha256) {
                    if actual_hash.eq_ignore_ascii_case(expected_hash) {
                        (true, None)
                    } else {
                        (
                            false,
                            Some(format!(
                                "SHA-256 hash mismatch: expected {expected_hash}, got {actual_hash}"
                            )),
                        )
                    }
                } else {
                    (true, None)
                }
            } else if required {
                (
                    false,
                    Some("Required offline model weight is missing".to_string()),
                )
            } else {
                (true, None)
            };

            report.push(ModelStatus {
                name: name.to_string(),
                subpath: subpath.to_string(),
                required,
                resolved_path: disk_path,
                is_embedded,
                exists,
                size_bytes,
                expected_size_bytes,
                sha256,
                expected_sha256,
                is_valid,
                error,
            });
        }

        report
    }

    /// Checks required models and returns an error list if any required model is missing or corrupt.
    pub fn check_required_models(&self) -> Result<(), Vec<String>> {
        let report = self.doctor_report();
        let mut errors = Vec::new();

        for status in report {
            if status.required && (!status.exists || !status.is_valid) {
                errors.push(format!(
                    "Required model '{}' ({}) error: {}",
                    status.name,
                    status.subpath,
                    status.error.as_deref().unwrap_or("Missing or invalid")
                ));
            } else if status.exists && !status.is_valid {
                errors.push(format!(
                    "Model '{}' ({}) integrity check failed: {}",
                    status.name,
                    status.subpath,
                    status.error.as_deref().unwrap_or("Checksum mismatch")
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Convenience function to retrieve current model status using default locator.
pub fn get_model_status() -> Vec<ModelStatus> {
    ModelLocator::new(None).doctor_report()
}

/// Global function to invalidate all cached model singletons across engine subsystems (ADR 0018).
pub fn invalidate_model_caches() {
    tracing::info!("Invalidating in-memory model caches");
    crate::figure::yolo::invalidate_yolo_detector();
    #[cfg(feature = "slanet-ort")]
    crate::figure::slanet::invalidate_slanet_engine();
    #[cfg(feature = "latex-ort")]
    crate::figure::latex::invalidate_latex_engine();
    crate::embeddings::engine::invalidate_embedding_engine();
    crate::pipeline::invalidate_indra_engine();
}
