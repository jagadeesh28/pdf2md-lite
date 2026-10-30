//! Explicit configuration loader for pdfmd-lite.
//!
//! Loads typed settings from `pdfmd-lite.toml` or custom config files.
//! Strictly zero environment variable reads per project architectural rules.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub concurrency: Option<usize>,
    pub max_depth: Option<usize>,
    pub models_dir: Option<PathBuf>,
    pub layout_model: Option<PathBuf>,
    pub language: Option<String>,
}

impl AppConfig {
    /// Loads configuration from an explicit path or `./pdfmd-lite.toml` if present.
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit_path {
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file at {}", path.display()))?;
            let config: AppConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse TOML configuration at {}", path.display()))?;
            return Ok(config);
        }

        let default_config = Path::new("pdfmd-lite.toml");
        if default_config.exists() {
            let content = fs::read_to_string(default_config)
                .with_context(|| format!("Failed to read default config at {}", default_config.display()))?;
            let config: AppConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse default config at {}", default_config.display()))?;
            return Ok(config);
        }

        Ok(Self::default())
    }
}
