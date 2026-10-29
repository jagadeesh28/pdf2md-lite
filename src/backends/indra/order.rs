//! Reading order sorting and layout structuring for Indra OCR.

use super::detect::TextBoundingBox;

/// A recognized text block with spatial coordinates and decoded string.
#[derive(Debug, Clone)]
pub struct TextLine {
    pub bbox: TextBoundingBox,
    pub text: String,
}

/// Sorts detected lines into natural reading order and formats them as Markdown.
pub fn sort_and_format_markdown(lines: &mut [TextLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    // 1. Sort primarily by Y (top-to-bottom), then by X (left-to-right)
    lines.sort_by(|a, b| {
        // If Y centers are within half a line height, consider them on the same line
        let a_y_mid = a.bbox.y + a.bbox.height / 2;
        let b_y_mid = b.bbox.y + b.bbox.height / 2;
        let tolerance = ((a.bbox.height.min(b.bbox.height)) as i32 / 2).max(6);

        if (a_y_mid as i32 - b_y_mid as i32).abs() <= tolerance {
            a.bbox.x.cmp(&b.bbox.x)
        } else {
            a.bbox.y.cmp(&b.bbox.y)
        }
    });

    // 2. Assemble lines into Markdown paragraphs
    let mut out = String::new();
    let mut prev_bottom: Option<u32> = None;
    let mut prev_height: u32 = 16;

    for line in lines.iter() {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(bottom) = prev_bottom {
            let gap = line.bbox.y.saturating_sub(bottom);
            // If gap exceeds 1.5x previous line height, treat as paragraph break
            if gap > (prev_height * 3 / 2) {
                out.push_str("\n\n");
            } else if gap > 2 {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }

        out.push_str(trimmed);
        prev_bottom = Some(line.bbox.y + line.bbox.height);
        prev_height = line.bbox.height;
    }

    out
}
