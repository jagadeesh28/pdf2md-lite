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
    pub device: Option<String>,
    pub allow_degraded: Option<bool>,
    pub debug_layout: Option<PathBuf>,
    pub max_pixels: Option<u64>,
    pub max_file_mb: Option<u64>,
    pub page_timeout_sec: Option<u64>,
}

/// Execution hardware target for ONNX neural models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceTarget {
    #[default]
    Auto,
    Cpu,
    Directml,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageCapability {
    pub code: String,
    pub name: String,
    pub script: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilitiesResponse {
    pub languages: Vec<LanguageCapability>,
    pub active_device: String,
    pub dom_extraction_modes: Vec<String>,
    pub is_tauri: bool,
}

/// Detects available language models and hardware execution targets.
pub fn get_capabilities(is_tauri: bool) -> CapabilitiesResponse {
    let config = AppConfig::load(None).unwrap_or_default();
    let active_device = config.device.unwrap_or_else(|| "auto".to_string());
    let models = crate::model_locator::get_model_status();
    let has_world_model = models
        .iter()
        .any(|m| (m.name.contains("rec_world") || m.subpath.contains("rec_world")) && m.is_valid);

    let languages = vec![
        LanguageCapability {
            code: "en".to_string(),
            name: "English (Latin)".to_string(),
            script: "Latin".to_string(),
            available: true,
        },
        LanguageCapability {
            code: "hi".to_string(),
            name: "Hindi (Devanagari)".to_string(),
            script: "Devanagari".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "te".to_string(),
            name: "Telugu".to_string(),
            script: "Telugu".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "kn".to_string(),
            name: "Kannada".to_string(),
            script: "Kannada".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "ta".to_string(),
            name: "Tamil".to_string(),
            script: "Tamil".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "ml".to_string(),
            name: "Malayalam".to_string(),
            script: "Malayalam".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "mr".to_string(),
            name: "Marathi".to_string(),
            script: "Devanagari".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "bn".to_string(),
            name: "Bengali".to_string(),
            script: "Bengali".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "gu".to_string(),
            name: "Gujarati".to_string(),
            script: "Gujarati".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "pa".to_string(),
            name: "Punjabi (Gurmukhi)".to_string(),
            script: "Gurmukhi".to_string(),
            available: has_world_model,
        },
        LanguageCapability {
            code: "or".to_string(),
            name: "Odia".to_string(),
            script: "Odia".to_string(),
            available: has_world_model,
        },
    ];

    CapabilitiesResponse {
        languages,
        active_device,
        dom_extraction_modes: vec![
            "auto".to_string(),
            "always".to_string(),
            "never".to_string(),
        ],
        is_tauri,
    }
}

impl std::str::FromStr for DeviceTarget {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(DeviceTarget::Auto),
            "cpu" => Ok(DeviceTarget::Cpu),
            "directml" | "dml" => Ok(DeviceTarget::Directml),
            other => anyhow::bail!(
                "Unknown device target '{other}'. Supported devices: auto, cpu, directml"
            ),
        }
    }
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

    /// Merges non-None fields from `other` into `self`, ensuring UI saves do not clobber
    /// backend-only fields such as `models_dir`, `layout_model`, or `max_depth`.
    pub fn merge_patch(&mut self, other: AppConfig) {
        if other.concurrency.is_some() {
            self.concurrency = other.concurrency;
        }
        if other.max_depth.is_some() {
            self.max_depth = other.max_depth;
        }
        if other.models_dir.is_some() {
            self.models_dir = other.models_dir;
        }
        if other.layout_model.is_some() {
            self.layout_model = other.layout_model;
        }
        if other.language.is_some() {
            self.language = other.language;
        }
        if other.image_format.is_some() {
            self.image_format = other.image_format;
        }
        if other.image_quality.is_some() {
            self.image_quality = other.image_quality;
        }
        if other.save_table_images.is_some() {
            self.save_table_images = other.save_table_images;
        }
        if other.extract_tables.is_some() {
            self.extract_tables = other.extract_tables;
        }
        if other.embed_images_base64.is_some() {
            self.embed_images_base64 = other.embed_images_base64;
        }
        if other.log_file.is_some() {
            self.log_file = other.log_file;
        }
        if other.log_level.is_some() {
            self.log_level = other.log_level;
        }
        if other.dom_extraction.is_some() {
            self.dom_extraction = other.dom_extraction;
        }
        if other.embed.is_some() {
            self.embed = other.embed;
        }
        if other.output_dir.is_some() {
            self.output_dir = other.output_dir;
        }
        if other.academic_structure.is_some() {
            self.academic_structure = other.academic_structure;
        }
        if other.device.is_some() {
            self.device = other.device;
        }
        if other.allow_degraded.is_some() {
            self.allow_degraded = other.allow_degraded;
        }
        if other.debug_layout.is_some() {
            self.debug_layout = other.debug_layout;
        }
        if other.max_pixels.is_some() {
            self.max_pixels = other.max_pixels;
        }
        if other.max_file_mb.is_some() {
            self.max_file_mb = other.max_file_mb;
        }
        if other.page_timeout_sec.is_some() {
            self.page_timeout_sec = other.page_timeout_sec;
        }
    }

    /// Serializes and writes configuration to a TOML file, ensuring parent directory exists.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory at {}", parent.display())
            })?;
        }
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

    #[test]
    fn test_config_merge_patch_preserves_unmanaged_fields() {
        let mut original = AppConfig {
            models_dir: Some(PathBuf::from("E:/custom/models")),
            max_depth: Some(5),
            layout_model: Some(PathBuf::from("E:/custom/layout.onnx")),
            language: Some("en".to_string()),
            image_quality: Some(85),
            save_table_images: Some(true),
            ..Default::default()
        };

        let patch = AppConfig {
            language: Some("te".to_string()),
            image_quality: Some(95),
            save_table_images: Some(false),
            ..Default::default()
        };

        original.merge_patch(patch);

        // Updated fields
        assert_eq!(original.language.as_deref(), Some("te"));
        assert_eq!(original.image_quality, Some(95));
        assert_eq!(original.save_table_images, Some(false));

        // Preserved unmanaged backend fields
        assert_eq!(original.models_dir, Some(PathBuf::from("E:/custom/models")));
        assert_eq!(original.max_depth, Some(5));
        assert_eq!(
            original.layout_model,
            Some(PathBuf::from("E:/custom/layout.onnx"))
        );
    }
}
