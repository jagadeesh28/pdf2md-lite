//! Explicit configuration loader for pdfmd-lite.
//!
//! Loads typed settings from `pdfmd-lite.toml` or custom config files.
//! Strictly zero environment variable reads per project architectural rules.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub concurrency: Option<usize>,
    pub max_depth: Option<usize>,
    pub models_dir: Option<PathBuf>,
    pub layout_model: Option<PathBuf>,
    pub language: Option<String>,
    pub image_format: Option<String>,
    pub image_quality: Option<u8>,
    #[serde(alias = "table_crops")]
    pub save_table_images: Option<bool>,
    pub extract_tables: Option<bool>,
    pub embed_images_base64: Option<bool>,
    pub log_file: Option<PathBuf>,
    pub log_level: Option<String>,
    pub dom_extraction: Option<String>,
    pub embed: Option<bool>,
    pub output_dir: Option<PathBuf>,
    pub academic_structure: Option<bool>,
}

impl AppConfig {
    /// Returns the default per-user configuration path under OS config directory.
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join("pdfmd-lite").join("pdfmd-lite.toml"))
            .unwrap_or_else(|| PathBuf::from("pdfmd-lite.toml"))
    }

    /// Returns the default per-user data directory for models, caches, and logs.
    pub fn default_data_dir() -> PathBuf {
        dirs::data_dir()
            .map(|p| p.join("pdfmd-lite"))
            .unwrap_or_else(|| PathBuf::from("data"))
    }

    /// Returns the default output directory under the user's Documents folder.
    pub fn default_output_dir() -> PathBuf {
        dirs::document_dir()
            .map(|p| p.join("pdfmd-lite"))
            .unwrap_or_else(|| PathBuf::from("output"))
    }

    /// Loads configuration from an explicit path or OS AppData / CWD `pdfmd-lite.toml`.
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit_path {
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file at {}", path.display()))?;
            let config: AppConfig = toml::from_str(&content).with_context(|| {
                format!("Failed to parse TOML configuration at {}", path.display())
            })?;
            return Ok(config);
        }

        let user_config = Self::default_config_path();
        if user_config.exists() {
            let content = fs::read_to_string(&user_config).with_context(|| {
                format!("Failed to read user config at {}", user_config.display())
            })?;
            let config: AppConfig = toml::from_str(&content).with_context(|| {
                format!("Failed to parse user config at {}", user_config.display())
            })?;
            return Ok(config);
        }

        let cwd_config = Path::new("pdfmd-lite.toml");
        if cwd_config.exists() {
            let content = fs::read_to_string(cwd_config).with_context(|| {
                format!("Failed to read default config at {}", cwd_config.display())
            })?;
            let config: AppConfig = toml::from_str(&content).with_context(|| {
                format!("Failed to parse default config at {}", cwd_config.display())
            })?;
            return Ok(config);
        }

        Ok(Self::default())
    }

    /// Serializes and writes configuration to a TOML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("Failed to serialize AppConfig to TOML format")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write configuration to {}", path.display()))?;
        Ok(())
    }
}

/// Returns the default per-user configuration path under OS config directory.
pub fn default_config_path() -> PathBuf {
    AppConfig::default_config_path()
}

/// Returns the default per-user data directory for models, caches, and logs.
pub fn default_data_dir() -> PathBuf {
    AppConfig::default_data_dir()
}

/// Returns the default output directory under the user's Documents folder.
pub fn default_output_dir() -> PathBuf {
    AppConfig::default_output_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config_parsing() {
        let toml_str = r#"
            concurrency = 4
            models_dir = "/custom/models"
            language = "telugu"
            image_format = "jpeg"
            image_quality = 90
            save_table_images = true
            embed_images_base64 = false
            dom_extraction = "auto"
        "#;
        let config: Result<AppConfig, _> = toml::from_str(toml_str);
        assert!(config.is_ok());
        let cfg = config.unwrap();
        assert_eq!(cfg.concurrency, Some(4));
        assert_eq!(cfg.language.as_deref(), Some("telugu"));
        assert_eq!(cfg.image_quality, Some(90));
    }

    #[test]
    fn test_deny_unknown_fields_rejection() {
        let toml_str = r#"
            concurrency = 4
            unrecognized_bogus_key = "should fail"
        "#;
        let config: Result<AppConfig, _> = toml::from_str(toml_str);
        assert!(config.is_err(), "Unknown fields must be rejected");
        let err_msg = config.unwrap_err().to_string();
        assert!(err_msg.contains("unknown field `unrecognized_bogus_key`"));
    }
}
