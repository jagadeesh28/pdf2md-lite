//! Unified Cascading Model Locator and Zero-Copy Provisioning (ADR 0014).
//!
//! Discovers and loads offline neural model weights in strict descending precedence:
//! 1. Explicit `--models-dir` CLI flag or `AppConfig.models_dir` setting.
//! 2. Binary-adjacent directory (`std::env::current_exe().parent()/models`).
//! 3. Development workspace paths (`./models`, `../../models`).
//! 4. Embedded static binaries (`Cow::Borrowed(&'static [u8])`) when feature `embed-models` is active.

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

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

#[cfg(feature = "embed-models")]
const EMBEDDED_DET: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/det.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_REC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/rec.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_REC_WORLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/rec_world.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_ORDER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/order.onnx"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_META: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/indra/meta.json"
));
#[cfg(feature = "embed-models")]
const EMBEDDED_LAYOUT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../models/layout/yolov8n-doclaynet.onnx"
));

/// Status record for a single neural model used in diagnostic reports.
#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub name: &'static str,
    pub subpath: &'static str,
    pub required: bool,
    pub resolved_path: Option<PathBuf>,
    pub is_embedded: bool,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

/// Known model manifest for all offline neural pipelines.
pub const KNOWN_MODELS: &[(&str, &str, bool)] = &[
    ("Indra OCR Detection", "models/indra/det.onnx", true),
    ("Indra OCR Recognition (Latin)", "models/indra/rec.onnx", true),
    ("Indra OCR Recognition (World)", "models/indra/rec_world.onnx", false),
    ("Indra Reading Order", "models/indra/order.onnx", true),
    ("Indra OCR Metadata/Vocab", "models/indra/meta.json", true),
    ("DocLayNet YOLO Layout", "models/layout/yolov8n-doclaynet.onnx", true),
    ("SLANet Table Structure", "models/layout/slanet.onnx", false),
    ("SLANet Table Vocab", "models/layout/table_structure_dict.txt", false),
    ("RapidLaTeXOCR Encoder", "models/latex/encoder.onnx", false),
    ("RapidLaTeXOCR Decoder", "models/latex/decoder.onnx", false),
    ("RapidLaTeXOCR Tokenizer", "models/latex/tokenizer.json", false),
    ("GLiNER Metadata Extractor", "models/metadata/gliner_small.onnx", false),
    ("MiniLM Embedding Model", "models/embeddings/model.onnx", false),
    ("MiniLM Embedding Tokenizer", "models/embeddings/tokenizer.json", false),
    // Next-Gen Quantized Models (ADR 0014, ADR 0015, ADR 0016)
    ("PP-StructureV2 SLANet_Plus", "models/layout/slanet_plus.onnx", false),
    ("UniMERNet Formula OCR", "models/latex/unimernet.onnx", false),
    ("BGE-Small Embeddings (INT8)", "models/embeddings/bge_small_int8.onnx", false),
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
    fn strip_models_prefix(subpath: &str) -> &str {
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

        // 3. Current working directory / workspace relative
        let cwd_candidate = Path::new(subpath);
        if cwd_candidate.exists() {
            return Some(cwd_candidate.to_path_buf());
        }
        let cwd_models_candidate = Path::new("models").join(clean_sub);
        if cwd_models_candidate.exists() {
            return Some(cwd_models_candidate);
        }

        // 4. Upstream workspace directory (for tests and sub-crates)
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

    /// Returns embedded static byte slice if compiled with `embed-models`.
    #[allow(unused_variables)]
    fn get_embedded_slice(subpath: &str) -> Option<&'static [u8]> {
        #[cfg(feature = "embed-models")]
        {
            let clean = Self::strip_models_prefix(subpath);
            match clean {
                "indra/det.onnx" | "det.onnx" => Some(EMBEDDED_DET),
                "indra/rec.onnx" | "rec.onnx" => Some(EMBEDDED_REC),
                "indra/rec_world.onnx" | "rec_world.onnx" => Some(EMBEDDED_REC_WORLD),
                "indra/order.onnx" | "order.onnx" => Some(EMBEDDED_ORDER),
                "indra/meta.json" | "meta.json" => Some(EMBEDDED_META),
                "layout/yolov8n-doclaynet.onnx" | "yolov8n-doclaynet.onnx" => Some(EMBEDDED_LAYOUT),
                _ => None,
            }
        }
        #[cfg(not(feature = "embed-models"))]
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
            "Model file '{}' not found in custom path, exe-adjacent 'models/', workspace, or embedded binary.",
            subpath
        ))
    }

    /// Collects status for all known offline models for diagnostic inspection.
    pub fn doctor_report(&self) -> Vec<ModelStatus> {
        let mut report = Vec::with_capacity(KNOWN_MODELS.len());

        for &(name, subpath, required) in KNOWN_MODELS {
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
                (Some(slice.len() as u64), Some(format!("{:x}", hasher.finalize())))
            } else {
                (None, None)
            };

            report.push(ModelStatus {
                name,
                subpath,
                required,
                resolved_path: disk_path,
                is_embedded,
                exists,
                size_bytes,
                sha256,
            });
        }

        report
    }
}
