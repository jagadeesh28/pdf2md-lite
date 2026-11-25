//! Structured conversion telemetry, exit codes, and frontmatter reporting (Phase 12, ADR 0017/0018).

use serde::{Deserialize, Serialize};

/// High-level lifecycle status of a document or batch conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConversionStatus {
    #[default]
    Completed,
    CompletedWithWarnings,
    Failed,
    Cancelled,
    Skipped,
}

impl ConversionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::CompletedWithWarnings => "completed_with_warnings",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

/// Comprehensive telemetry report for an executed document conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversionReport {
    /// Conversion completion status.
    pub status: ConversionStatus,
    /// Total pages in document / slice.
    pub total_pages: usize,
    /// Number of pages successfully extracted.
    pub extracted_pages: usize,
    /// Number of pages extracted via fast baseline-aware hybrid DOM.
    pub hybrid_dom_pages: usize,
    /// Number of pages falling back to neural vision OCR.
    pub ocr_fallback_pages: usize,
    /// Total structured table grids extracted.
    pub tables_extracted: usize,
    /// Total display and inline formulas extracted via RapidLaTeXOCR.
    pub formulas_extracted: usize,
    /// Total images/figures written to disk or embedded.
    pub images_saved: usize,
    /// Non-fatal diagnostics and degradation warnings encountered.
    pub warnings: Vec<String>,
    /// Wall-clock conversion time in seconds.
    pub elapsed_seconds: f32,
    /// Model execution provider / device used (e.g. `cpu`, `directml`).
    pub model_device: Option<String>,
}

impl Default for ConversionReport {
    fn default() -> Self {
        Self {
            status: ConversionStatus::Completed,
            total_pages: 0,
            extracted_pages: 0,
            hybrid_dom_pages: 0,
            ocr_fallback_pages: 0,
            tables_extracted: 0,
            formulas_extracted: 0,
            images_saved: 0,
            warnings: Vec::new(),
            elapsed_seconds: 0.0,
            model_device: None,
        }
    }
}

impl ConversionReport {
    /// Creates a successful conversion report with zero warnings.
    pub fn clean(total_pages: usize, elapsed_seconds: f32) -> Self {
        Self {
            status: ConversionStatus::Completed,
            total_pages,
            extracted_pages: total_pages,
            hybrid_dom_pages: total_pages,
            ocr_fallback_pages: 0,
            tables_extracted: 0,
            formulas_extracted: 0,
            images_saved: 0,
            warnings: Vec::new(),
            elapsed_seconds,
            model_device: None,
        }
    }

    /// Creates a report with degradation or fallback warnings.
    pub fn with_warnings(total_pages: usize, warnings: Vec<String>, elapsed_seconds: f32) -> Self {
        Self {
            status: ConversionStatus::CompletedWithWarnings,
            total_pages,
            extracted_pages: total_pages,
            hybrid_dom_pages: 0,
            ocr_fallback_pages: total_pages,
            tables_extracted: 0,
            formulas_extracted: 0,
            images_saved: 0,
            warnings,
            elapsed_seconds,
            model_device: None,
        }
    }

    /// Creates a failed conversion report.
    pub fn failed(
        total_pages: usize,
        error_message: impl Into<String>,
        elapsed_seconds: f32,
    ) -> Self {
        Self {
            status: ConversionStatus::Failed,
            total_pages,
            extracted_pages: 0,
            hybrid_dom_pages: 0,
            ocr_fallback_pages: 0,
            tables_extracted: 0,
            formulas_extracted: 0,
            images_saved: 0,
            warnings: vec![error_message.into()],
            elapsed_seconds,
            model_device: None,
        }
    }

    /// Creates a skipped conversion report.
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: ConversionStatus::Skipped,
            total_pages: 0,
            extracted_pages: 0,
            hybrid_dom_pages: 0,
            ocr_fallback_pages: 0,
            tables_extracted: 0,
            formulas_extracted: 0,
            images_saved: 0,
            warnings: vec![reason.into()],
            elapsed_seconds: 0.0,
            model_device: None,
        }
    }

    /// Returns `true` if this report contains any warnings or fallback events.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty() || self.status == ConversionStatus::CompletedWithWarnings
    }

    /// Returns the standard CLI exit code for this report:
    /// - `0`: Clean success without warnings
    /// - `2`: Success with warnings or fallbacks
    /// - `1`: Fatal failure or cancellation
    pub fn exit_code(&self) -> i32 {
        match self.status {
            ConversionStatus::Completed => {
                if self.warnings.is_empty() {
                    0
                } else {
                    2
                }
            }
            ConversionStatus::CompletedWithWarnings => 2,
            ConversionStatus::Failed | ConversionStatus::Cancelled => 1,
            ConversionStatus::Skipped => 0,
        }
    }

    /// Generates a compact representation suitable for Obsidian note YAML frontmatter.
    pub fn to_frontmatter(&self) -> ConversionReportFrontmatter {
        ConversionReportFrontmatter {
            status: self.status.as_str().to_string(),
            pages: self.total_pages,
            hybrid_dom_pages: if self.hybrid_dom_pages > 0 {
                Some(self.hybrid_dom_pages)
            } else {
                None
            },
            ocr_fallback_pages: if self.ocr_fallback_pages > 0 {
                Some(self.ocr_fallback_pages)
            } else {
                None
            },
            warnings: self.warnings.len(),
            elapsed_sec: (self.elapsed_seconds * 100.0).round() / 100.0,
        }
    }
}

/// Compact YAML frontmatter payload representing conversion telemetry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionReportFrontmatter {
    pub status: String,
    pub pages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hybrid_dom_pages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_fallback_pages: Option<usize>,
    pub warnings: usize,
    pub elapsed_sec: f32,
}

impl Eq for ConversionReportFrontmatter {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_report_exit_codes() {
        let clean = ConversionReport::clean(5, 1.25);
        assert_eq!(clean.exit_code(), 0);

        let with_warn =
            ConversionReport::with_warnings(5, vec!["fallback to OCR".to_string()], 2.5);
        assert_eq!(with_warn.exit_code(), 2);

        let failed = ConversionReport::failed(5, "corrupt file", 0.1);
        assert_eq!(failed.exit_code(), 1);

        let skipped = ConversionReport::skipped("already exists");
        assert_eq!(skipped.exit_code(), 0);
    }

    #[test]
    fn test_frontmatter_serialization() {
        let report = ConversionReport {
            status: ConversionStatus::Completed,
            total_pages: 10,
            extracted_pages: 10,
            hybrid_dom_pages: 8,
            ocr_fallback_pages: 2,
            tables_extracted: 1,
            formulas_extracted: 3,
            images_saved: 2,
            warnings: vec!["Page 2 OCR fallback".to_string()],
            elapsed_seconds: 3.421,
            model_device: Some("cpu".to_string()),
        };

        let fm = report.to_frontmatter();
        assert_eq!(fm.status, "completed");
        assert_eq!(fm.pages, 10);
        assert_eq!(fm.hybrid_dom_pages, Some(8));
        assert_eq!(fm.ocr_fallback_pages, Some(2));
        assert_eq!(fm.warnings, 1);
        assert_eq!(fm.elapsed_sec, 3.42);
    }
}
