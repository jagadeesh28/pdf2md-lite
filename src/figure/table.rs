//! Pure-Rust geometric table grid extraction and Markdown table formatting.
//!
//! Reconstructs 2D tabular rows and columns from Indra OCR text line coordinates
//! within detected table boundaries without extra neural weights.
//!
//! Implements ADR 0010: Pure-Rust Geometric Table Grid Extraction.

use crate::backends::indra::order::TextLine;
use crate::figure::caption::FigureRect;

/// A structured 2D table grid of text cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableGrid {
    pub rows: Vec<Vec<String>>,
    pub num_cols: usize,
}

impl TableGrid {
    /// Formats the table grid into GitHub Flavored Markdown table syntax.
    pub fn to_markdown(&self) -> String {
        if self.rows.is_empty() || self.num_cols == 0 {
            return String::new();
        }

        let mut out = String::new();

        // Header row (first row)
        let header = &self.rows[0];
        out.push('|');
        for c in 0..self.num_cols {
            let cell = header.get(c).map(|s| s.trim()).unwrap_or("");
            out.push(' ');
            out.push_str(&escape_cell(cell));
            out.push_str(" |");
        }
        out.push('\n');

        // Separator row
        out.push('|');
        for _ in 0..self.num_cols {
            out.push_str(" :--- |");
        }
        out.push('\n');

        // Data rows (subsequent rows)
        for row in self.rows.iter().skip(1) {
            // Skip rows where all cells are empty
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }

            out.push('|');
            for c in 0..self.num_cols {
                let cell = row.get(c).map(|s| s.trim()).unwrap_or("");
                out.push(' ');
                out.push_str(&escape_cell(cell));
                out.push_str(" |");
            }
            out.push('\n');
        }

        out
    }
}

/// Escapes markdown pipe symbols within cell text to prevent broken table syntax.
fn escape_cell(cell: &str) -> String {
    cell.replace('|', "\\|").replace('\n', " ")
}

/// Extracts a structured Markdown table from text lines located within a detected table boundary.
pub fn extract_markdown_table(table_rect: &FigureRect, text_lines: &[TextLine]) -> Option<String> {
    let (md, _) = extract_table_with_lines(table_rect, text_lines)?;
    Some(md)
}

/// Extracts a structured Markdown table and the list of raw text lines inside the table bounds.
pub fn extract_table_with_lines(
    table_rect: &FigureRect,
    text_lines: &[TextLine],
) -> Option<(String, Vec<String>)> {
    let grid = extract_table_grid(table_rect, text_lines)?;
    let md = grid.to_markdown();
    if md.trim().is_empty() {
        return None;
    }

    let margin = 6i32;
    let min_x = (table_rect.x as i32 - margin).max(0) as u32;
    let max_x = table_rect.x + table_rect.width + margin as u32;
    let min_y = (table_rect.y as i32 - margin).max(0) as u32;
    let max_y = table_rect.y + table_rect.height + margin as u32;

    let inside_texts: Vec<String> = text_lines
        .iter()
        .filter(|l| {
            let cx = l.bbox.x + l.bbox.width / 2;
            let cy = l.bbox.y + l.bbox.height / 2;
            cx >= min_x && cx <= max_x && cy >= min_y && cy <= max_y
        })
        .map(|l| l.text.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    Some((md, inside_texts))
}

/// Substitutes raw unstructured table lines in `body_text` with the structured Markdown table and optional callout.
pub fn integrate_table_into_markdown(
    body_text: &str,
    markdown_table: &str,
    table_line_texts: &[String],
    table_label: &str,
    table_callout: Option<&str>,
) -> String {
    let mut table_block = String::new();
    table_block.push('\n');
    table_block.push_str(markdown_table.trim());
    table_block.push_str("\n\n");
    if let Some(callout) = table_callout {
        table_block.push_str(callout.trim());
        table_block.push_str("\n\n");
    }

    // 1. Remove raw table text lines from body_text to prevent duplicated unstructured text
    let mut cleaned_lines: Vec<&str> = Vec::new();
    for line in body_text.lines() {
        let trimmed = line.trim();
        let is_table_line = table_line_texts.iter().any(|t| {
            if t.len() >= 6 && trimmed.contains(t.as_str()) {
                true
            } else {
                trimmed == t.as_str()
            }
        });
        if !is_table_line {
            cleaned_lines.push(line);
        }
    }
    let cleaned_text = cleaned_lines.join("\n");

    // 2. Insert table block at caption location or append
    let lower_text = cleaned_text.to_lowercase();
    let lower_label = table_label.to_lowercase();

    if let Some(pos) = lower_text.find(&lower_label) {
        let caption_end = cleaned_text[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(cleaned_text.len());

        let mut res = String::with_capacity(cleaned_text.len() + table_block.len());
        res.push_str(&cleaned_text[..caption_end]);
        res.push('\n');
        res.push_str(&table_block);
        res.push_str(&cleaned_text[caption_end..]);
        res
    } else {
        let mut res = cleaned_text;
        if !res.ends_with("\n\n") {
            if res.ends_with('\n') {
                res.push('\n');
            } else {
                res.push_str("\n\n");
            }
        }
        res.push_str(&table_block);
        res
    }
}

/// Extracts a 2D `TableGrid` by spatially clustering text lines within a table rectangle.
pub fn extract_table_grid(table_rect: &FigureRect, text_lines: &[TextLine]) -> Option<TableGrid> {
    // 1. Filter text lines whose midpoint lies inside the table rectangle (with 6px margin)
    let margin = 6i32;
    let min_x = (table_rect.x as i32 - margin).max(0) as u32;
    let max_x = table_rect.x + table_rect.width + margin as u32;
    let min_y = (table_rect.y as i32 - margin).max(0) as u32;
    let max_y = table_rect.y + table_rect.height + margin as u32;

    let mut inside_lines: Vec<&TextLine> = text_lines
        .iter()
        .filter(|l| {
            let cx = l.bbox.x + l.bbox.width / 2;
            let cy = l.bbox.y + l.bbox.height / 2;
            cx >= min_x && cx <= max_x && cy >= min_y && cy <= max_y
        })
        .collect();

    if inside_lines.len() < 2 {
        return None;
    }

    // 2. Row clustering
    // Sort lines by vertical center
    inside_lines.sort_by_key(|l| l.bbox.y + l.bbox.height / 2);

    let mut row_groups: Vec<Vec<&TextLine>> = Vec::new();
    for line in inside_lines {
        let y_center = line.bbox.y + line.bbox.height / 2;
        let mut matched_row = false;

        for group in &mut row_groups {
            let avg_y = group.iter().map(|l| l.bbox.y + l.bbox.height / 2).sum::<u32>()
                / group.len() as u32;
            let avg_h = group.iter().map(|l| l.bbox.height).sum::<u32>() / group.len() as u32;
            let tol = (avg_h / 2).max(6);

            if (y_center as i32 - avg_y as i32).abs() <= tol as i32 {
                group.push(line);
                matched_row = true;
                break;
            }
        }

        if !matched_row {
            row_groups.push(vec![line]);
        }
    }

    if row_groups.is_empty() {
        return None;
    }

    // Sort rows from top to bottom
    row_groups.sort_by_key(|g| {
        g.iter().map(|l| l.bbox.y + l.bbox.height / 2).sum::<u32>() / g.len() as u32
    });

    // Sort items within each row from left to right
    for group in &mut row_groups {
        group.sort_by_key(|l| l.bbox.x);
    }

    // 3. Check if text lines inside rows contain multi-token columns separated by significant whitespace
    // (In case DBNet detected an entire row as one line)
    let expanded_rows = expand_single_line_tokens(&row_groups);

    // 4. Column interval clustering
    // Determine the global columns across all rows
    let column_centers = derive_column_centers(&expanded_rows, table_rect.width);
    if column_centers.is_empty() {
        return None;
    }

    let num_cols = column_centers.len();

    // 5. Populate 2D grid
    let mut grid_rows = Vec::new();
    for row in expanded_rows {
        let mut cells = vec![String::new(); num_cols];

        for item in row {
            let item_cx = item.x + item.width / 2;
            // Find closest column center
            let mut best_col = 0;
            let mut min_dist = u32::MAX;

            for (col_idx, &col_cx) in column_centers.iter().enumerate() {
                let dist = (item_cx as i32 - col_cx as i32).unsigned_abs();
                if dist < min_dist {
                    min_dist = dist;
                    best_col = col_idx;
                }
            }

            let cell = &mut cells[best_col];
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&item.text);
        }

        // Only include rows that have at least one non-empty cell
        if cells.iter().any(|c| !c.trim().is_empty()) {
            grid_rows.push(cells);
        }
    }

    if grid_rows.is_empty() {
        return None;
    }

    Some(TableGrid {
        rows: grid_rows,
        num_cols,
    })
}

#[derive(Debug, Clone)]
struct TableCellToken {
    x: u32,
    width: u32,
    text: String,
}

/// Expands rows where DBNet may have captured multiple table columns in a single `TextLine`.
/// If a line contains multi-space gaps (`  +`), it splits into sub-cell tokens with interpolated X coordinates.
fn expand_single_line_tokens(row_groups: &[Vec<&TextLine>]) -> Vec<Vec<TableCellToken>> {
    let mut result = Vec::new();

    for row in row_groups {
        let mut tokens_in_row = Vec::new();

        for line in row {
            let text = line.text.trim();
            // Check if line contains 2 or more consecutive spaces indicating column gutter
            if text.contains("  ") {
                let sub_chunks: Vec<&str> = text
                    .split("  ")
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();

                if sub_chunks.len() > 1 {
                    let total_chars: usize = sub_chunks.iter().map(|s| s.len()).sum();
                    let char_width = if total_chars > 0 {
                        (line.bbox.width as f32) / (total_chars as f32 * 1.3)
                    } else {
                        10.0
                    };

                    let mut current_x = line.bbox.x;
                    let spacing = (line.bbox.width.saturating_sub((total_chars as f32 * char_width) as u32))
                        / (sub_chunks.len() as u32);

                    for chunk in sub_chunks {
                        let w = ((chunk.len() as f32) * char_width).max(15.0) as u32;
                        tokens_in_row.push(TableCellToken {
                            x: current_x,
                            width: w,
                            text: chunk.to_string(),
                        });
                        current_x = current_x.saturating_add(w).saturating_add(spacing);
                    }
                    continue;
                }
            }

            tokens_in_row.push(TableCellToken {
                x: line.bbox.x,
                width: line.bbox.width,
                text: text.to_string(),
            });
        }

        tokens_in_row.sort_by_key(|t| t.x);
        result.push(tokens_in_row);
    }

    result
}

/// Derives canonical horizontal column center coordinates from all table tokens.
fn derive_column_centers(rows: &[Vec<TableCellToken>], _table_width: u32) -> Vec<u32> {
    // 1. Gather all token centers
    let mut all_centers: Vec<u32> = rows
        .iter()
        .flat_map(|r| r.iter().map(|t| t.x + t.width / 2))
        .collect();

    if all_centers.is_empty() {
        return Vec::new();
    }

    all_centers.sort_unstable();

    // 2. Estimate column separation threshold from intra-row gaps
    let mut intra_gaps = Vec::new();
    for row in rows {
        for window in row.windows(2) {
            let left_edge = window[0].x + window[0].width;
            let right_edge = window[1].x;
            if right_edge > left_edge {
                intra_gaps.push(right_edge - left_edge);
            }
        }
    }

    let min_separation = if !intra_gaps.is_empty() {
        intra_gaps.sort_unstable();
        // Use median intra-row gap divided by 2 as grouping tolerance
        let median_gap = intra_gaps[intra_gaps.len() / 2];
        (median_gap / 2).max(18)
    } else {
        25
    };

    // 3. Cluster column centers
    let mut clusters: Vec<Vec<u32>> = Vec::new();
    for center in all_centers {
        let mut added = false;
        for cluster in &mut clusters {
            let cluster_avg = cluster.iter().sum::<u32>() / cluster.len() as u32;
            if (center as i32 - cluster_avg as i32).abs() <= min_separation as i32 {
                cluster.push(center);
                added = true;
                break;
            }
        }
        if !added {
            clusters.push(vec![center]);
        }
    }

    clusters.sort_by_key(|c| c.iter().sum::<u32>() / c.len() as u32);
    clusters
        .into_iter()
        .map(|c| c.iter().sum::<u32>() / c.len() as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::indra::detect::TextBoundingBox;

    fn make_line(x: u32, y: u32, w: u32, h: u32, text: &str) -> TextLine {
        TextLine {
            bbox: TextBoundingBox {
                x,
                y,
                width: w,
                height: h,
                score: 0.99,
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn test_table_grid_to_markdown() {
        let grid = TableGrid {
            rows: vec![
                vec!["Header 1".to_string(), "Header 2".to_string(), "Header 3".to_string()],
                vec!["Val 1".to_string(), "Val 2".to_string(), "Val 3".to_string()],
                vec!["Val 4".to_string(), "Val 5 | Pipe".to_string(), "Val 6".to_string()],
            ],
            num_cols: 3,
        };

        let md = grid.to_markdown();
        assert!(md.contains("| Header 1 | Header 2 | Header 3 |"));
        assert!(md.contains("| :--- | :--- | :--- |"));
        assert!(md.contains("| Val 1 | Val 2 | Val 3 |"));
        assert!(md.contains("| Val 4 | Val 5 \\| Pipe | Val 6 |"));
    }

    #[test]
    fn test_extract_table_grid_multi_column() {
        let table_rect = FigureRect::new(100, 100, 600, 200);

        // Row 1 (Header): Y ~ 110
        let l1 = make_line(120, 110, 80, 20, "Station");
        let l2 = make_line(300, 110, 100, 20, "Pressure (atm)");
        let l3 = make_line(500, 110, 120, 20, "Temperature (K)");

        // Row 2: Y ~ 150
        let l4 = make_line(120, 150, 20, 20, "1");
        let l5 = make_line(300, 150, 50, 20, "1.000");
        let l6 = make_line(500, 150, 60, 20, "300.0");

        // Row 3: Y ~ 190
        let l7 = make_line(120, 190, 20, 20, "2");
        let l8 = make_line(300, 190, 50, 20, "0.528");
        let l9 = make_line(500, 190, 60, 20, "250.0");

        let lines = vec![l1, l2, l3, l4, l5, l6, l7, l8, l9];
        let grid = extract_table_grid(&table_rect, &lines).expect("Grid should be extracted");

        assert_eq!(grid.num_cols, 3);
        assert_eq!(grid.rows.len(), 3);
        assert_eq!(grid.rows[0], vec!["Station", "Pressure (atm)", "Temperature (K)"]);
        assert_eq!(grid.rows[1], vec!["1", "1.000", "300.0"]);
        assert_eq!(grid.rows[2], vec!["2", "0.528", "250.0"]);

        let md = grid.to_markdown();
        assert!(md.contains("| Station | Pressure (atm) | Temperature (K) |"));
    }

    #[test]
    fn test_extract_table_sparse_cells() {
        let table_rect = FigureRect::new(50, 50, 500, 150);

        // Row 1: 3 columns
        let l1 = make_line(60, 60, 50, 15, "Col A");
        let l2 = make_line(220, 60, 50, 15, "Col B");
        let l3 = make_line(400, 60, 50, 15, "Col C");

        // Row 2: missing middle column (Col B empty)
        let l4 = make_line(60, 95, 40, 15, "Data A1");
        let l5 = make_line(400, 95, 40, 15, "Data C1");

        let lines = vec![l1, l2, l3, l4, l5];
        let grid = extract_table_grid(&table_rect, &lines).expect("Grid should extract sparse table");

        assert_eq!(grid.num_cols, 3);
        assert_eq!(grid.rows.len(), 2);
        assert_eq!(grid.rows[1][0], "Data A1");
        assert_eq!(grid.rows[1][1], ""); // Empty cell
        assert_eq!(grid.rows[1][2], "Data C1");
    }

    #[test]
    fn test_integrate_table_into_markdown() {
        let body = "Intro paragraph.\n\nTable 1 Equilibrium states.\n\nCol1 Col2\nVal1 Val2\n\nNext section.";
        let table_md = "| Col1 | Col2 |\n| :--- | :--- |\n| Val1 | Val2 |";
        let raw_lines = vec!["Col1 Col2".to_string(), "Val1 Val2".to_string()];
        let callout = "> [!table]- Original Table Image: Table 1\n> ![Table 1](images/Table-1.png)";

        let res = integrate_table_into_markdown(body, table_md, &raw_lines, "Table 1", Some(callout));
        assert!(res.contains("Table 1 Equilibrium states."));
        assert!(res.contains("| Col1 | Col2 |"));
        assert!(res.contains("> [!table]- Original Table Image: Table 1"));
        assert!(!res.contains("\nCol1 Col2\n"));
        assert!(!res.contains("\nVal1 Val2\n"));
    }
}
