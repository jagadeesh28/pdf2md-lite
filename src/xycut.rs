//! Recursive XY-Cut++: Deterministic spatial projection profile partitioning
//! for robust multi-column reading order and text block sequencing.

use crate::backends::indra::order::TextLine;

/// Represents a 2D bounding rectangle in pixel coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
}

impl Rect {
    pub fn from_coords(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x + width,
            max_y: y + height,
        }
    }

    pub fn width(&self) -> u32 {
        self.max_x.saturating_sub(self.min_x)
    }

    pub fn height(&self) -> u32 {
        self.max_y.saturating_sub(self.min_y)
    }
}

/// Applies Recursive XY-Cut++ to a slice of text lines in-place,
/// sorting them into true reading order across multi-column layouts,
/// titles, headers, and body paragraphs.
pub fn recursive_xy_cut(lines: &mut [TextLine]) {
    if lines.len() <= 1 {
        return;
    }

    // Indices into the original slice
    let mut indices: Vec<usize> = (0..lines.len()).collect();
    let ordered_indices = xy_cut_recursive(lines, &mut indices);

    // Reorder original slice based on ordered_indices
    let mut temp = Vec::with_capacity(lines.len());
    for &idx in &ordered_indices {
        temp.push(lines[idx].clone());
    }
    lines.clone_from_slice(&temp);
}

/// Recursive partitioning step.
fn xy_cut_recursive(lines: &[TextLine], indices: &mut [usize]) -> Vec<usize> {
    if indices.len() <= 1 {
        return indices.to_vec();
    }

    // 1. Compute overall bounding box of current set
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    let mut total_height = 0u64;

    for &idx in indices.iter() {
        let b = &lines[idx].bbox;
        min_x = min_x.min(b.x);
        min_y = min_y.min(b.y);
        max_x = max_x.max(b.x + b.width);
        max_y = max_y.max(b.y + b.height);
        total_height += b.height as u64;
    }

    let avg_height = ((total_height / indices.len() as u64) as u32).max(12);

    // 2. In XY-Cut++, first check if a clean column gutter exists that vertically
    // separates the entire block into two or more distinct columns (no spanning boxes).
    let min_v_gap = (avg_height * 3 / 2).max(20);
    if let Some(partitions) = find_vertical_cuts(lines, indices, min_x, max_x, min_v_gap) {
        if partitions.len() > 1 {
            let mut result = Vec::with_capacity(indices.len());
            for mut part in partitions {
                let sub = xy_cut_recursive(lines, &mut part);
                result.extend(sub);
            }
            return result;
        }
    }

    // 3. If no vertical column gutter splits the block (e.g. a title/banner spans across
    // columns or it is a single-column block), look for horizontal cuts (paragraphs / banners).
    // Section/banner gaps are substantially larger than inter-line gaps (>= 1.75 * avg_height).
    let min_h_gap = (avg_height * 7 / 4).max(28);
    if let Some(partitions) = find_horizontal_cuts(lines, indices, min_y, max_y, min_h_gap) {
        if partitions.len() > 1 {
            let mut result = Vec::with_capacity(indices.len());
            for mut part in partitions {
                let sub = xy_cut_recursive(lines, &mut part);
                result.extend(sub);
            }
            return result;
        }
    }

    // 4. Base case: Atomic cohesive block. Sort top-to-bottom, left-to-right.
    indices.sort_by(|&a, &b| {
        let la = &lines[a].bbox;
        let lb = &lines[b].bbox;
        let a_y_mid = la.y + la.height / 2;
        let b_y_mid = lb.y + lb.height / 2;
        let tolerance = (la.height.min(lb.height) / 2).max(6);

        if (a_y_mid as i32 - b_y_mid as i32).abs() <= tolerance as i32 {
            la.x.cmp(&lb.x)
        } else {
            la.y.cmp(&lb.y)
        }
    });

    indices.to_vec()
}

/// Finds horizontal projection valleys (Y-axis cuts).
fn find_horizontal_cuts(
    lines: &[TextLine],
    indices: &[usize],
    min_y: u32,
    max_y: u32,
    min_gap: u32,
) -> Option<Vec<Vec<usize>>> {
    if max_y <= min_y {
        return None;
    }

    let range = (max_y - min_y) as usize;
    if range == 0 {
        return None;
    }

    let mut hist = vec![0u16; range + 1];
    for &idx in indices {
        let b = &lines[idx].bbox;
        let y0 = b.y.saturating_sub(min_y) as usize;
        let y1 = (b.y + b.height).saturating_sub(min_y).min(max_y - min_y) as usize;
        for bin in y0..=y1 {
            if bin < hist.len() {
                hist[bin] = hist[bin].saturating_add(1);
            }
        }
    }

    // Find valleys of consecutive 0s with length >= min_gap
    let mut cut_points = Vec::new();
    let mut zero_start: Option<usize> = None;

    for (y, &count) in hist.iter().enumerate() {
        if count == 0 {
            if zero_start.is_none() {
                zero_start = Some(y);
            }
        } else if let Some(start) = zero_start {
            let gap_len = y - start;
            if gap_len >= min_gap as usize {
                cut_points.push(min_y + (start + y) as u32 / 2);
            }
            zero_start = None;
        }
    }

    if cut_points.is_empty() {
        return None;
    }

    // Partition indices into segments by cut points
    let mut partitions = Vec::new();
    let mut lower = min_y;

    for &cut in &cut_points {
        let mut group = Vec::new();
        for &idx in indices {
            let b = &lines[idx].bbox;
            let mid_y = b.y + b.height / 2;
            if mid_y >= lower && mid_y < cut {
                group.push(idx);
            }
        }
        if !group.is_empty() {
            partitions.push(group);
        }
        lower = cut;
    }

    // Remaining segment after last cut
    let mut last_group = Vec::new();
    for &idx in indices {
        let b = &lines[idx].bbox;
        let mid_y = b.y + b.height / 2;
        if mid_y >= lower {
            last_group.push(idx);
        }
    }
    if !last_group.is_empty() {
        partitions.push(last_group);
    }

    if partitions.len() > 1 {
        Some(partitions)
    } else {
        None
    }
}

/// Finds vertical projection valleys (X-axis column cuts / gutters).
fn find_vertical_cuts(
    lines: &[TextLine],
    indices: &[usize],
    min_x: u32,
    max_x: u32,
    min_gap: u32,
) -> Option<Vec<Vec<usize>>> {
    if max_x <= min_x {
        return None;
    }

    let range = (max_x - min_x) as usize;
    if range == 0 {
        return None;
    }

    let mut hist = vec![0u16; range + 1];
    for &idx in indices {
        let b = &lines[idx].bbox;
        let x0 = b.x.saturating_sub(min_x) as usize;
        let x1 = (b.x + b.width).saturating_sub(min_x).min(max_x - min_x) as usize;
        for bin in x0..=x1 {
            if bin < hist.len() {
                hist[bin] = hist[bin].saturating_add(1);
            }
        }
    }

    // Find valleys of consecutive 0s with length >= min_gap
    let mut cut_points = Vec::new();
    let mut zero_start: Option<usize> = None;

    for (x, &count) in hist.iter().enumerate() {
        if count == 0 {
            if zero_start.is_none() {
                zero_start = Some(x);
            }
        } else if let Some(start) = zero_start {
            let gap_len = x - start;
            if gap_len >= min_gap as usize {
                cut_points.push(min_x + (start + x) as u32 / 2);
            }
            zero_start = None;
        }
    }

    if cut_points.is_empty() {
        return None;
    }

    // Partition indices into column segments from left to right
    let mut partitions = Vec::new();
    let mut lower = min_x;

    for &cut in &cut_points {
        let mut group = Vec::new();
        for &idx in indices {
            let b = &lines[idx].bbox;
            let mid_x = b.x + b.width / 2;
            if mid_x >= lower && mid_x < cut {
                group.push(idx);
            }
        }
        if !group.is_empty() {
            partitions.push(group);
        }
        lower = cut;
    }

    let mut last_group = Vec::new();
    for &idx in indices {
        let b = &lines[idx].bbox;
        let mid_x = b.x + b.width / 2;
        if mid_x >= lower {
            last_group.push(idx);
        }
    }
    if !last_group.is_empty() {
        partitions.push(last_group);
    }

    if partitions.len() > 1 {
        Some(partitions)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::indra::detect::TextBoundingBox;

    fn make_line(x: u32, y: u32, width: u32, height: u32, text: &str) -> TextLine {
        TextLine {
            bbox: TextBoundingBox {
                x,
                y,
                width,
                height,
                score: 0.99,
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn test_xy_cut_two_column_reading_order() {
        // Document with 2 columns:
        // Left Column: X in [50..250], Y = 100, 150, 200
        // Right Column: X in [300..500], Y = 100, 150, 200
        // Gutter between columns: X in [250..300] (width = 50px)
        let mut lines = vec![
            make_line(300, 100, 200, 20, "Right Col Line 1"),
            make_line(50, 100, 200, 20, "Left Col Line 1"),
            make_line(300, 150, 200, 20, "Right Col Line 2"),
            make_line(50, 150, 200, 20, "Left Col Line 2"),
            make_line(300, 200, 200, 20, "Right Col Line 3"),
            make_line(50, 200, 200, 20, "Left Col Line 3"),
        ];

        recursive_xy_cut(&mut lines);

        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Left Col Line 1",
                "Left Col Line 2",
                "Left Col Line 3",
                "Right Col Line 1",
                "Right Col Line 2",
                "Right Col Line 3",
            ],
            "Multi-column reading order must read entire left column before right column!"
        );
    }

    #[test]
    fn test_xy_cut_title_spanning_two_columns() {
        // Title banner spanning X=[50..500] at Y=[20..50]
        // Below title: Left column X=[50..240], Right column X=[280..500]
        let mut lines = vec![
            make_line(280, 100, 200, 20, "Right Col Line 1"),
            make_line(50, 20, 450, 30, "Document Main Title"),
            make_line(50, 100, 190, 20, "Left Col Line 1"),
            make_line(50, 140, 190, 20, "Left Col Line 2"),
            make_line(280, 140, 200, 20, "Right Col Line 2"),
        ];

        recursive_xy_cut(&mut lines);

        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Document Main Title",
                "Left Col Line 1",
                "Left Col Line 2",
                "Right Col Line 1",
                "Right Col Line 2",
            ]
        );
    }
}
