//! Model weight provisioning and loading for the YOLO Document Layout Engine.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::RwLock;

static CUSTOM_LAYOUT_PATH: RwLock<Option<PathBuf>> = RwLock::new(None);

#[cfg(any(
    feature = "slanet-ort",
    feature = "latex-ort",
    feature = "gliner-ort",
    feature = "ort"
))]
static ORT_ENVIRONMENT: std::sync::OnceLock<std::sync::Arc<ort::environment::Environment>> =
    std::sync::OnceLock::new();
#[cfg(any(
    feature = "slanet-ort",
    feature = "latex-ort",
    feature = "gliner-ort",
    feature = "ort"
))]
static ORT_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Returns a shared, process-wide ONNX Runtime Environment singleton.
/// Only one Environment is permitted per process by ONNX Runtime.
#[cfg(any(
    feature = "slanet-ort",
    feature = "latex-ort",
    feature = "gliner-ort",
    feature = "ort"
))]
pub fn get_shared_ort_environment() -> anyhow::Result<std::sync::Arc<ort::environment::Environment>>
{
    if let Some(env) = ORT_ENVIRONMENT.get() {
        return Ok(env.clone());
    }
    let _guard = ORT_INIT_LOCK.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(env) = ORT_ENVIRONMENT.get() {
        return Ok(env.clone());
    }
    let env = ort::environment::init()
        .with_name("pdfmd_lite")
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to initialize shared ORT environment: {e}"))?;
    let arc = std::sync::Arc::new(env);
    let _ = ORT_ENVIRONMENT.set(arc.clone());
    Ok(arc)
}

/// Configures and returns an optimized ONNX Runtime SessionBuilder configured with
/// `GraphOptimizationLevel::Level3`, explicit thread budgeting, and
/// DirectML hardware acceleration on Windows with automatic zero-failure fallback to CPU.
#[cfg(any(
    feature = "slanet-ort",
    feature = "latex-ort",
    feature = "gliner-ort",
    feature = "ort"
))]
pub fn create_optimized_session_builder(
    env: &std::sync::Arc<ort::environment::Environment>,
    intra_threads: Option<usize>,
    allow_directml: bool,
) -> anyhow::Result<ort::session::builder::SessionBuilder> {
    let mut builder = ort::session::Session::builder(env)
        .map_err(|e| anyhow::anyhow!("Failed to create ORT session builder: {e}"))?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow::anyhow!("Failed to set ORT optimization level Level3: {e}"))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!("Failed to set ORT inter_threads: {e}"))?;

    let intra = intra_threads.unwrap_or(2);
    builder = builder
        .with_intra_threads(intra)
        .map_err(|e| anyhow::anyhow!("Failed to set ORT intra_threads ({intra}): {e}"))?;

    #[cfg(all(target_os = "windows", feature = "directml"))]
    if allow_directml {
        // DirectML acceleration with automatic silent fallback to CPU
        builder = builder
            .with_execution_providers([ort::ep::DirectML::default().build().fail_silently()])
            .map_err(|e| anyhow::anyhow!("Failed to configure DirectML execution provider: {e}"))?;
    }

    Ok(builder)
}

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

    /// Loads the YOLO layout detector model bytes without heap duplication.
    pub fn load_layout_bytes(&self) -> Result<std::borrow::Cow<'static, [u8]>> {
        let locator = crate::model_locator::ModelLocator::new(self.custom_path.clone());
        locator.load_bytes("models/layout/yolov8n-doclaynet.onnx")
    }

    /// Loads the YOLO layout detector model bytes as owned vector for backward compatibility.
    pub fn load_layout_model(&self) -> Result<Vec<u8>> {
        self.load_layout_bytes().map(|cow| cow.into_owned())
    }
}
