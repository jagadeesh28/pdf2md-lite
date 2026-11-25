//! Structured Document Intermediate Representation (IR) and Diagnostic Trace (ADR 0018).
//!
//! Provides a strict hierarchical document model preserving provenance and span-ID addressing:
//! Document -> Page -> Region -> Block -> Line -> Span
//!
//! Supports diagnostic layout serialization (`--debug-layout <dir>`) to dump geometric,
//! OCR/DOM provenance, and spatial ordering decisions for deterministic visual debugging.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Bounding rectangle in page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBox {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    pub fn intersection_area(&self, other: &BoundingBox) -> f32 {
        let x_overlap = (self.right().min(other.right()) - self.x.max(other.x)).max(0.0);
        let y_overlap = (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.0);
        x_overlap * y_overlap
    }

    pub fn area(&self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }

    pub fn iou(&self, other: &BoundingBox) -> f32 {
        let intersection = self.intersection_area(other);
        let union = self.area() + other.area() - intersection;
        if union > 0.0 {
            intersection / union
        } else {
            0.0
        }
    }

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
}

/// Provenance of extracted document content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    #[default]
    Dom,
    Ocr,
    Specialist,
}

/// Category of high-level document region (DocLayNet classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    #[default]
    Body,
    Title,
    SectionHeader,
    Table,
    Formula,
    Picture,
    Caption,
    Footnote,
    Header,
    Footer,
}

/// Block-level semantic element within a document region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    #[default]
    Paragraph,
    Heading {
        level: u8,
    },
    Table {
        rows: usize,
        cols: usize,
    },
    Formula {
        display: bool,
    },
    Picture {
        label: String,
    },
    ListItem,
    Callout {
        callout_type: String,
    },
}

/// Smallest addressed text unit with bounding geometry and font state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SpanIr {
    pub id: usize,
    pub text: String,
    pub bbox: BoundingBox,
    pub font_name: Option<String>,
    pub font_size: f32,
    pub provenance: Provenance,
}

/// Horizontally clustered line of spans sharing a baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LineIr {
    pub id: usize,
    pub bbox: BoundingBox,
    pub baseline_y: f32,
    pub spans: Vec<SpanIr>,
}

impl LineIr {
    pub fn full_text(&self) -> String {
        self.spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Cohesive paragraph, heading, table, or math block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BlockIr {
    pub id: usize,
    pub kind: BlockKind,
    pub bbox: BoundingBox,
    pub lines: Vec<LineIr>,
    pub provenance: Provenance,
}

impl BlockIr {
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.full_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Cohesive spatial region (e.g. column, full-width header, table area).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RegionIr {
    pub id: usize,
    pub kind: RegionKind,
    pub bbox: BoundingBox,
    pub blocks: Vec<BlockIr>,
}

/// Full hierarchical document page representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PageIr {
    pub page_number: usize,
    pub media_box: [f32; 4],
    pub crop_box: [f32; 4],
    pub rotate: i32,
    pub regions: Vec<RegionIr>,
}

/// Diagnostic layout trace for `--debug-layout <dir>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugLayoutTrace {
    pub page_number: usize,
    pub media_box: [f32; 4],
    pub crop_box: [f32; 4],
    pub rotate: i32,
    pub raw_dom_spans: Vec<SpanIr>,
    pub yolo_boxes_raw: Vec<LayoutBoxTrace>,
    pub yolo_boxes_post_nms: Vec<LayoutBoxTrace>,
    pub xy_cut_splits: Vec<f32>,
    pub final_block_order: Vec<BlockOrderTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutBoxTrace {
    pub class_name: String,
    pub class_id: usize,
    pub score: f32,
    pub bbox: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockOrderTrace {
    pub block_id: usize,
    pub region_kind: RegionKind,
    pub block_kind: BlockKind,
    pub text: String,
    pub bbox: BoundingBox,
    pub provenance: Provenance,
}

impl DebugLayoutTrace {
    /// Saves the diagnostic trace JSON to the specified directory.
    pub fn write_to_dir(&self, dir: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(dir)?;
        let file_name = format!("page_{:03}_layout.json", self.page_number);
        let path = dir.join(file_name);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;
        Ok(())
    }
}
