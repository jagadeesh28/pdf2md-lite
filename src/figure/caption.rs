//! Automated proximity caption matching for detected figures.
//!
//! Connects visual figure crops to their author captions without vision model API queries
//! by identifying caption prefix anchors in recognized text lines and computing spatial
//! Euclidean and vertical proximity thresholds.
//!
//! Implements Milestone 3 of the Pure-Rust Hybrid Figure Extraction Architecture (ADR 0006).

use crate::backends::indra::order::TextLine;
use crate::figure::pdf_stream::StreamFigure;

/// A parsed caption prefix anchor (e.g., "Figure 1", "Fig. 4.2", "Diagram 3").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionAnchor {
    /// Canonical figure identifier (e.g., "Figure 1", "Fig. 4.2").
    pub label: String,
    /// Detailed caption text trailing the identifier.
    pub description: Option<String>,
    /// Full verbatim line text.
    pub raw_text: String,
}

/// Bounding rectangle for figure geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FigureRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl FigureRect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }


    pub fn from_stream_figure(s: &StreamFigure) -> Self {
        let (x, y, w, h) = s.pixel_box;
        Self {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[inline]
    pub fn xmin(&self) -> u32 {
        self.x
    }

    #[inline]
    pub fn ymin(&self) -> u32 {
        self.y
    }

    #[inline]
    pub fn xmax(&self) -> u32 {
        self.x.saturating_add(self.width)
    }

    #[inline]
    pub fn ymax(&self) -> u32 {
        self.y.saturating_add(self.height)
    }
}

/// Result of matching a candidate caption to a figure crop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedCaption {
    /// Figure label (e.g. "Figure 1" or "Fig. 3.2").
    pub label: String,
    /// Descriptive caption text if present.
    pub description: Option<String>,
    /// Formatted full title string (e.g., "Figure 1 - System Overview").
    pub full_title: String,
    /// Vertical distance in pixels from the figure boundary.
    pub vertical_distance_px: u32,
    /// Whether the caption was positioned below (true) or above (false) the figure.
    pub is_below: bool,
}

/// Configuration for proximity caption matching.
#[derive(Debug, Clone)]
pub struct CaptionMatchConfig {
    /// Maximum vertical distance (in pixels) between figure edge and caption (default: 60px).
    pub max_vertical_distance: u32,
    /// Maximum horizontal gap between figure column span and caption (default: 50px).
    pub max_horizontal_gap: u32,
    /// Multi-line caption lookahead: maximum vertical gap for continuation lines (default: 25px).
    pub continuation_line_gap: u32,
}

impl Default for CaptionMatchConfig {
    fn default() -> Self {
        Self {
            max_vertical_distance: 60,
            max_horizontal_gap: 50,
            continuation_line_gap: 25,
        }
    }
}

/// Identifies if a text line starts with a caption prefix anchor:
/// `r"^(Figure|Fig\.|Fig|Diagram|Chart|Table)\s*([0-9A-Za-z\.\-]+)"i`.
pub fn parse_caption_anchor(line: &str) -> Option<CaptionAnchor> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let prefixes = [
        "figure", "fig.", "fig", "diagram", "chart", "table", "plate",
    ];

    let lower = trimmed.to_lowercase();
    let mut matched_prefix_len = 0;

    for prefix in &prefixes {
        if lower.starts_with(prefix) {
            // Ensure next char is whitespace or punctuation (not e.g. "figurative" or "figures")
            if lower.len() == prefix.len() {
                matched_prefix_len = prefix.len();
                break;
            }
            let next_char = lower.as_bytes()[prefix.len()];
            if next_char == b' ' || next_char == b'\t' || next_char == b'.' || next_char == b':' {
                matched_prefix_len = prefix.len();
                break;
            }
        }
    }

    if matched_prefix_len == 0 {
        return None;
    }

    // Extract prefix slice
    let after_prefix = trimmed[matched_prefix_len..].trim_start_matches(|c: char| {
        c == ' ' || c == '\t' || c == '.' || c == ':'
    });

    if after_prefix.is_empty() {
        return None;
    }

    // Next must be identifier characters: digits, letters, dots, hyphens
    let mut id_end = 0;
    for (i, c) in after_prefix.char_indices() {
        if c.is_alphanumeric() || c == '.' || c == '-' || c == '(' || c == ')' {
            id_end = i + c.len_utf8();
        } else {
            break;
        }
    }

    if id_end == 0 {
        return None;
    }

    let raw_identifier = &after_prefix[..id_end];
    let identifier = raw_identifier.trim_end_matches(|c: char| c == '.' || c == ':' || c == '-');
    if identifier.is_empty() {
        return None;
    }

    // Must contain a digit, roman numeral, or alphanumeric section key (e.g. 1, 4.2, II-A, A)
    let has_valid_id = identifier
        .split(|c: char| c == '-' || c == '.' || c == '(' || c == ')')
        .any(|part| {
            let p = part.trim();
            if p.is_empty() {
                return false;
            }
            if p.chars().any(|c| c.is_ascii_digit()) {
                return true;
            }
            let upper = p.to_uppercase();
            matches!(
                upper.as_str(),
                "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX" | "X" | "XI" | "XII"
                    | "A" | "B" | "C" | "D" | "E" | "F"
            )
        });

    if !has_valid_id {
        return None;
    }

    let prefix_str = trimmed[..matched_prefix_len].trim_end_matches('.');
    let canonical_label = format!("{prefix_str} {identifier}");

    // The remaining text after identifier is the description
    let remainder = after_prefix[identifier.len()..].trim_start_matches(|c: char| {
        c == ' ' || c == '\t' || c == ':' || c == '.' || c == '-' || c == '—' || c == '–'
    }).trim();

    let description = if remainder.is_empty() {
        None
    } else {
        Some(remainder.to_string())
    };

    Some(CaptionAnchor {
        label: canonical_label,
        description,
        raw_text: trimmed.to_string(),
    })
}

/// Matches detected figure boundaries with the nearest recognized text line caption.
pub fn match_proximity_caption(
    figure: &FigureRect,
    lines: &[TextLine],
    config: &CaptionMatchConfig,
    default_page_index: (usize, usize), // (page_num, figure_idx)
) -> MatchedCaption {
    let mut best_match: Option<(usize, CaptionAnchor, u32, bool)> = None;
    let mut min_distance = u32::MAX;

    for (idx, line) in lines.iter().enumerate() {
        if let Some(anchor) = parse_caption_anchor(&line.text) {
            let l_xmin = line.bbox.x;
            let l_ymin = line.bbox.y;
            let l_xmax = line.bbox.x + line.bbox.width;
            let l_ymax = line.bbox.y + line.bbox.height;

            // Check horizontal column overlap/gap
            let h_inter = (figure.xmax().min(l_xmax) as i32) - (figure.xmin().max(l_xmin) as i32);
            let h_gap = if h_inter >= 0 {
                0
            } else {
                (-h_inter) as u32
            };

            if h_gap > config.max_horizontal_gap {
                continue;
            }

            // Determine if caption is below or above the figure
            let (is_below, v_dist) = if l_ymin >= figure.ymax() {
                // Below figure
                (true, l_ymin - figure.ymax())
            } else if l_ymax <= figure.ymin() {
                // Above figure
                (false, figure.ymin() - l_ymax)
            } else {
                // Overlapping vertically (e.g. inside margin of figure)
                (true, 0)
            };

            if v_dist <= config.max_vertical_distance {
                // Weight captions below slightly more favorably (1.0 vs 1.2)
                let weighted_dist = if is_below { v_dist } else { v_dist + 5 };

                if weighted_dist < min_distance {
                    min_distance = weighted_dist;
                    best_match = Some((idx, anchor, v_dist, is_below));
                }
            }
        }
    }

    if let Some((anchor_idx, anchor, v_dist, is_below)) = best_match {
        // Multi-line continuation: check if subsequent text lines immediately below continue the caption
        let mut full_desc = anchor.description.unwrap_or_default();
        let mut prev_bottom = lines[anchor_idx].bbox.y + lines[anchor_idx].bbox.height;

        for line in lines.iter().skip(anchor_idx + 1) {
            // If the next line is within continuation gap and doesn't start a new anchor/header
            let gap = line.bbox.y.saturating_sub(prev_bottom);
            if gap <= config.continuation_line_gap {
                let trimmed = line.text.trim();
                if parse_caption_anchor(trimmed).is_some() {
                    break;
                }
                if !trimmed.is_empty() {
                    if !full_desc.is_empty() {
                        full_desc.push(' ');
                    }
                    full_desc.push_str(trimmed);
                    prev_bottom = line.bbox.y + line.bbox.height;
                }
            } else {
                break;
            }
        }

        let desc_opt = if full_desc.is_empty() {
            None
        } else {
            Some(full_desc)
        };

        let full_title = match &desc_opt {
            Some(desc) => format!("{} - {}", anchor.label, desc),
            None => anchor.label.clone(),
        };

        MatchedCaption {
            label: anchor.label,
            description: desc_opt,
            full_title,
            vertical_distance_px: v_dist,
            is_below,
        }
    } else {
        // Fallback default label: Figure {page}.{idx}
        let fallback_label = format!("Figure {}.{}", default_page_index.0, default_page_index.1);
        MatchedCaption {
            label: fallback_label.clone(),
            description: None,
            full_title: fallback_label,
            vertical_distance_px: 0,
            is_below: true,
        }
    }
}

/// Formats an Obsidian collapsible figure callout as specified in Milestone 3:
/// ```markdown
/// > [!figure]- Original Enhanced Image: Figure 1 - System Overview
/// > ![[figure_p1_1.png]]
/// ```
pub fn format_obsidian_callout(caption: &MatchedCaption, image_filename: &str) -> String {
    format!(
        "> [!figure]- Original Enhanced Image: {}\n> ![[{}]]\n",
        caption.full_title, image_filename
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::indra::detect::TextBoundingBox;

    #[test]
    fn test_parse_caption_anchor_variants() {
        let c1 = parse_caption_anchor("Figure 1: System Architecture Overview").unwrap();
        assert_eq!(c1.label, "Figure 1");
        assert_eq!(c1.description.as_deref(), Some("System Architecture Overview"));

        let c2 = parse_caption_anchor("Fig. 4.7 — Variation of species mole fractions").unwrap();
        assert_eq!(c2.label, "Fig 4.7");
        assert_eq!(c2.description.as_deref(), Some("Variation of species mole fractions"));

        let c3 = parse_caption_anchor("Diagram 3. Component Hierarchy").unwrap();
        assert_eq!(c3.label, "Diagram 3");
        assert_eq!(c3.description.as_deref(), Some("Component Hierarchy"));

        let c4 = parse_caption_anchor("Chart 2").unwrap();
        assert_eq!(c4.label, "Chart 2");
        assert_eq!(c4.description, None);

        let c5 = parse_caption_anchor("Figure II-A: Test matrix").unwrap();
        assert_eq!(c5.label, "Figure II-A");
        assert_eq!(c5.description.as_deref(), Some("Test matrix"));

        // Negative cases
        assert!(parse_caption_anchor("Figurative speech in ancient prose").is_none());
        assert!(parse_caption_anchor("Figures in the landscape were drawn").is_none());
        assert!(parse_caption_anchor("Normal body paragraph text").is_none());
    }

    #[test]
    fn test_proximity_caption_matching_below() {
        let fig = FigureRect::new(100, 200, 400, 300); // y: 200..500
        let lines = vec![
            TextLine {
                bbox: TextBoundingBox {
                    x: 100,
                    y: 100,
                    width: 400,
                    height: 20,
                    score: 0.9,
                },
                text: "Some preceding section text".to_string(),
            },
            TextLine {
                bbox: TextBoundingBox {
                    x: 100,
                    y: 520, // 20px below figure bottom (500)
                    width: 380,
                    height: 18,
                    score: 0.95,
                },
                text: "Figure 1 - High-level system architecture".to_string(),
            },
            TextLine {
                bbox: TextBoundingBox {
                    x: 100,
                    y: 542, // Continuation line
                    width: 350,
                    height: 18,
                    score: 0.92,
                },
                text: "and communication protocol between nodes.".to_string(),
            },
        ];

        let matched = match_proximity_caption(&fig, &lines, &CaptionMatchConfig::default(), (1, 1));
        assert_eq!(matched.label, "Figure 1");
        assert!(matched.is_below);
        assert_eq!(matched.vertical_distance_px, 20);
        assert_eq!(
            matched.full_title,
            "Figure 1 - High-level system architecture and communication protocol between nodes."
        );

        let callout = format_obsidian_callout(&matched, "figure_p1_1.png");
        assert_eq!(
            callout,
            "> [!figure]- Original Enhanced Image: Figure 1 - High-level system architecture and communication protocol between nodes.\n> ![[figure_p1_1.png]]\n"
        );
    }

    #[test]
    fn test_proximity_caption_matching_above() {
        let fig = FigureRect::new(100, 300, 400, 300); // y: 300..600
        let lines = vec![
            TextLine {
                bbox: TextBoundingBox {
                    x: 100,
                    y: 270, // 30px above figure top (300)
                    width: 300,
                    height: 20,
                    score: 0.95,
                },
                text: "Table 2: Experimental validation matrix".to_string(),
            },
        ];

        let matched = match_proximity_caption(&fig, &lines, &CaptionMatchConfig::default(), (2, 1));
        assert_eq!(matched.label, "Table 2");
        assert!(!matched.is_below);
        assert_eq!(matched.vertical_distance_px, 10); // 300 - 290 = 10px
    }

    #[test]
    fn test_proximity_caption_fallback() {
        let fig = FigureRect::new(100, 200, 400, 300);
        let lines = vec![TextLine {
            bbox: TextBoundingBox {
                x: 100,
                y: 700, // 200px away (> 60px max distance)
                width: 300,
                height: 20,
                score: 0.9,
            },
            text: "Figure 99 - Distant text".to_string(),
        }];

        let matched = match_proximity_caption(&fig, &lines, &CaptionMatchConfig::default(), (3, 2));
        assert_eq!(matched.label, "Figure 3.2");
        assert_eq!(matched.full_title, "Figure 3.2");
    }
}
