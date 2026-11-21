//! SLANet: Structure-Location Architecture Network for Table Recognition.
//! Predicts table HTML structure tokens and cell bounding boxes via ONNX Runtime.

#[cfg(feature = "slanet-ort")]
use crate::backends::indra::order::TextLine;
#[cfg(feature = "slanet-ort")]
use image::{DynamicImage, GenericImageView};
#[cfg(feature = "slanet-ort")]
use std::path::Path;
#[cfg(feature = "slanet-ort")]
use crate::figure::session_pool::SessionPool;
#[cfg(feature = "slanet-ort")]
use std::sync::Arc;

/// A recognized table cell with spatial coordinates and text.
#[derive(Debug, Clone)]
pub struct TableCell {
    pub bbox: [f32; 4], // [x1, y1, x2, y2] in cropped table coordinates
    pub text: String,
}

/// Result of neural table structure extraction containing both Markdown and exact HTML representations.
#[derive(Debug, Clone)]
pub struct TableExtractionResult {
    /// Portable GitHub Flavored Markdown table.
    pub markdown: String,
    /// Exact HTML `<table>` representation preserving rowspan, colspan, and exact tags.
    pub html: Option<String>,
    /// Whether multi-row or multi-column cell spans were present.
    pub has_spans: bool,
}

fn extract_span_num(attrs: &str, span_type: &str) -> Option<usize> {
    if let Some(pos) = attrs.find(span_type) {
        let after = &attrs[pos + span_type.len()..];
        if let Some(eq) = after.find('=') {
            let rest = after[eq + 1..].trim_start();
            let trimmed = rest.trim_matches(|c| c == '"' || c == '\'');
            let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            return num_str.parse().ok();
        }
    }
    None
}

#[cfg(feature = "slanet-ort")]
pub struct SlaNetEngine {
    session_pool: SessionPool<ort::session::Session>,
    vocab: Vec<String>,
}

#[cfg(feature = "slanet-ort")]
static SLANET_ENGINE: std::sync::OnceLock<Option<Arc<SlaNetEngine>>> = std::sync::OnceLock::new();

/// Returns a shared, lazily initialized singleton instance of the SLANet Table Engine,
/// or `None` if the model/dictionary files are not provisioned.
#[cfg(feature = "slanet-ort")]
pub fn get_slanet_engine() -> Option<Arc<SlaNetEngine>> {
    let engine = SLANET_ENGINE.get_or_init(|| {
        let model_path = {
            let plus = crate::resolve_model_path("models/layout/slanet_plus.onnx");
            if plus.exists() {
                plus
            } else {
                crate::resolve_model_path("models/layout/slanet.onnx")
            }
        };
        let dict_path = crate::resolve_model_path("models/layout/table_structure_dict.txt");
        if model_path.exists() && dict_path.exists() {
            match SlaNetEngine::new(&model_path, &dict_path) {
                Ok(engine) => Some(Arc::new(engine)),
                Err(err) => {
                    tracing::warn!("Failed to initialize SLANet table engine: {err:#}");
                    None
                }
            }
        } else {
            tracing::warn!(
                "SLANet table models not found (checked: {}, {}). Neural table structure recognition disabled.",
                model_path.display(),
                dict_path.display()
            );
            None
        }
    });
    engine.as_ref().cloned()
}

#[cfg(not(feature = "slanet-ort"))]
pub fn get_slanet_engine() -> Option<()> {
    None
}

#[cfg(feature = "slanet-ort")]
impl SlaNetEngine {
    /// Initializes SLANet from onnx model path and dictionary file.
    pub fn new<P: AsRef<Path>>(model_path: P, dict_path: P) -> anyhow::Result<Self> {
        let env = crate::figure::models::get_shared_ort_environment()?;

        let create_session = || -> anyhow::Result<ort::session::Session> {
            let mut builder = crate::figure::models::create_optimized_session_builder(&env, Some(2), false)?;
            builder
                .commit_from_file(model_path.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to commit SLANet model from file: {e}"))
        };

        // Bounded session pool: provision 2 sessions to allow concurrent page execution without mutex contention
        let mut sessions = vec![create_session()?];
        if let Ok(second) = create_session() {
            sessions.push(second);
        }

        let dict_content = std::fs::read_to_string(dict_path.as_ref())?;
        let mut vocab = Vec::new();
        for line in dict_content.lines() {
            vocab.push(line.to_string());
        }
        // Add standard special tokens if needed
        vocab.push("sos".to_string());
        vocab.push("eos".to_string());

        Ok(Self {
            session_pool: SessionPool::new(sessions),
            vocab,
        })
    }

    /// Predicts table structure and cell coordinates from cropped table image,
    /// populates each cell with overlapping OCR text lines, and renders Markdown table.
    pub fn predict_table_markdown(
        &self,
        table_crop: &DynamicImage,
        ocr_lines: &[TextLine],
        crop_offset_x: u32,
        crop_offset_y: u32,
    ) -> anyhow::Result<String> {
        let res = self.predict_table_hybrid(table_crop, ocr_lines, crop_offset_x, crop_offset_y)?;
        Ok(res.markdown)
    }

    /// Predicts both clean GitHub Flavored Markdown and exact HTML `<table>` representations,
    /// implementing Hybrid Table Serialization (ADR 0016).
    pub fn predict_table_hybrid(
        &self,
        table_crop: &DynamicImage,
        ocr_lines: &[TextLine],
        crop_offset_x: u32,
        crop_offset_y: u32,
    ) -> anyhow::Result<TableExtractionResult> {
        let (orig_w, orig_h) = table_crop.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(TableExtractionResult {
                markdown: String::new(),
                html: None,
                has_spans: false,
            });
        }

        // 1. Preprocess: Resize to 488x488 RGB, ImageNet normalization
        let resized = table_crop.resize_exact(488, 488, image::imageops::FilterType::Triangle);
        let rgb = resized.to_rgb8();

        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];

        let mut input_tensor = vec![0.0f32; 3 * 488 * 488];
        for y in 0..488 {
            for x in 0..488 {
                let p = rgb.get_pixel(x, y);
                for c in 0..3 {
                    let val = (p[c] as f32 / 255.0 - mean[c]) / std[c];
                    let idx = c * 488 * 488 + (y as usize) * 488 + (x as usize);
                    input_tensor[idx] = val;
                }
            }
        }

        // 2. Run inference via ORT
        let tensor = ort::value::Tensor::from_array((
            [1usize, 3, 488, 488],
            input_tensor.into_boxed_slice(),
        ))?;
        let inputs = ort::inputs!["x" => tensor];
        let mut session = self.session_pool.acquire()?;
        let outputs = session.run(inputs)?;

        // Output 0: loc_preds [1, seq_len, 4]
        // Output 1: structure_probs [1, seq_len, num_tokens]
        let (loc_shape, loc_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Failed to extract loc tensor: {e}"))?;
        let (struct_shape, struct_slice) = outputs[1]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Failed to extract struct tensor: {e}"))?;

        let seq_len = loc_shape[1] as usize;
        let num_tokens = struct_shape[2] as usize;

        // 3. Decode structure sequence and bounding boxes
        let mut predicted_tags: Vec<String> = Vec::new();
        let mut cell_boxes: Vec<[f32; 4]> = Vec::new();

        for t in 0..seq_len {
            // Argmax across token probabilities
            let mut max_idx = 0;
            let mut max_val = f32::NEG_INFINITY;
            for k in 0..num_tokens {
                let val = struct_slice[t * num_tokens + k];
                if val > max_val {
                    max_val = val;
                    max_idx = k;
                }
            }

            let token = if max_idx < self.vocab.len() {
                self.vocab[max_idx].clone()
            } else {
                continue;
            };

            if token == "eos" || token == "</table>" {
                predicted_tags.push("</table>".to_string());
                break;
            }

            if token == "<td>" || token.starts_with("<td") {
                let x1 = loc_slice[t * 4] * orig_w as f32;
                let y1 = loc_slice[t * 4 + 1] * orig_h as f32;
                let x2 = loc_slice[t * 4 + 2] * orig_w as f32;
                let y2 = loc_slice[t * 4 + 3] * orig_h as f32;
                cell_boxes.push([x1, y1, x2, y2]);
            }

            predicted_tags.push(token);
        }

        // 4. Map OCR text into cells based on bounding box intersection
        let mut cells_with_text: Vec<TableCell> = Vec::new();
        for bbox in &cell_boxes {
            // Absolute coordinates on original page
            let cell_min_x = crop_offset_x + bbox[0].max(0.0) as u32;
            let cell_min_y = crop_offset_y + bbox[1].max(0.0) as u32;
            let cell_max_x = crop_offset_x + bbox[2].min(orig_w as f32) as u32;
            let cell_max_y = crop_offset_y + bbox[3].min(orig_h as f32) as u32;

            let mut cell_text_parts = Vec::new();
            for line in ocr_lines {
                let b = &line.bbox;
                let line_mid_x = b.x + b.width / 2;
                let line_mid_y = b.y + b.height / 2;

                if line_mid_x >= cell_min_x
                    && line_mid_x <= cell_max_x
                    && line_mid_y >= cell_min_y
                    && line_mid_y <= cell_max_y
                {
                    let trimmed = line.text.trim();
                    if !trimmed.is_empty() {
                        cell_text_parts.push(trimmed);
                    }
                }
            }

            let text = cell_text_parts.join(" ");
            cells_with_text.push(TableCell { bbox: *bbox, text });
        }

        // 5. Reconstruct rows into Markdown table (with replicated spans) and exact HTML table
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut cell_idx = 0;
        let mut in_td = false;
        let mut current_td_attrs = String::new();
        let mut has_spans = false;

        let mut html_out = String::from("<table>\n");
        let mut cell_idx_html = 0;
        let mut in_td_html = false;
        let mut current_td_attrs_html = String::new();

        for tag in &predicted_tags {
            // HTML reconstruction
            if tag == "<tr>" {
                html_out.push_str("  <tr>\n");
            } else if tag == "</tr>" {
                html_out.push_str("  </tr>\n");
            } else if tag == "<td>" {
                let text = if cell_idx_html < cells_with_text.len() {
                    &cells_with_text[cell_idx_html].text
                } else {
                    ""
                };
                cell_idx_html += 1;
                html_out.push_str(&format!(
                    "    <td>{}</td>\n",
                    crate::figure::table::html_escape(text)
                ));
            } else if tag == "<td" {
                in_td_html = true;
                current_td_attrs_html.clear();
            } else if in_td_html && (tag.contains("colspan") || tag.contains("rowspan")) {
                current_td_attrs_html.push(' ');
                current_td_attrs_html.push_str(tag.trim());
            } else if in_td_html && tag.contains('>') {
                let text = if cell_idx_html < cells_with_text.len() {
                    &cells_with_text[cell_idx_html].text
                } else {
                    ""
                };
                cell_idx_html += 1;
                html_out.push_str(&format!(
                    "    <td{}>{}</td>\n",
                    current_td_attrs_html,
                    crate::figure::table::html_escape(text)
                ));
                in_td_html = false;
                current_td_attrs_html.clear();
            }

            // GFM row reconstruction with span replication (ADR 0016)
            if tag == "<tr>" {
                current_row.clear();
            } else if tag == "</tr>" {
                if !current_row.is_empty() {
                    rows.push(current_row.clone());
                    current_row.clear();
                }
            } else if tag == "<td>" {
                let text = if cell_idx < cells_with_text.len() {
                    cells_with_text[cell_idx].text.clone()
                } else {
                    String::new()
                };
                cell_idx += 1;
                let formatted = crate::figure::table::format_table_cell_with_latex(&text);
                current_row.push(formatted);
            } else if tag == "<td" {
                in_td = true;
                current_td_attrs.clear();
            } else if in_td && (tag.contains("colspan") || tag.contains("rowspan")) {
                has_spans = true;
                current_td_attrs.push(' ');
                current_td_attrs.push_str(tag.trim());
            } else if in_td && tag.contains('>') {
                let text = if cell_idx < cells_with_text.len() {
                    cells_with_text[cell_idx].text.clone()
                } else {
                    String::new()
                };
                cell_idx += 1;
                let formatted = crate::figure::table::format_table_cell_with_latex(&text);
                let col_span = extract_span_num(&current_td_attrs, "colspan").unwrap_or(1);
                for _ in 0..col_span {
                    current_row.push(formatted.clone());
                }
                in_td = false;
                current_td_attrs.clear();
            }
        }

        if !current_row.is_empty() {
            rows.push(current_row);
        }

        if rows.is_empty() {
            return Ok(TableExtractionResult {
                markdown: String::new(),
                html: None,
                has_spans: false,
            });
        }

        let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if max_cols <= 1 {
            // Reject degenerate 1-column tables from SLANet so pipeline falls back to geometric extraction
            return Ok(TableExtractionResult {
                markdown: String::new(),
                html: None,
                has_spans: false,
            });
        }

        html_out.push_str("</table>");

        // Format as GitHub Flavored Markdown table
        let mut md = String::new();
        for (i, row) in rows.iter().enumerate() {
            md.push('|');
            for col_idx in 0..max_cols {
                let val = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                md.push(' ');
                md.push_str(val);
                md.push_str(" |");
            }
            md.push('\n');

            if i == 0 {
                md.push('|');
                for _ in 0..max_cols {
                    md.push_str(" --- |");
                }
                md.push('\n');
            }
        }

        Ok(TableExtractionResult {
            markdown: md,
            html: Some(html_out),
            has_spans,
        })
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "slanet-ort")]
    use super::*;

    #[test]
    #[cfg(feature = "slanet-ort")]
    fn test_slanet_load_and_dummy_predict() -> anyhow::Result<()> {
        let model_path = crate::resolve_model_path("models/layout/slanet.onnx");
        let dict_path = crate::resolve_model_path("models/layout/table_structure_dict.txt");

        if !model_path.exists() || !dict_path.exists() {
            println!("SLANet models not found, skipping");
            return Ok(());
        }

        let engine = SlaNetEngine::new(&model_path, &dict_path)?;
        let dummy_crop = DynamicImage::new_rgb8(200, 100);
        let md = engine.predict_table_markdown(&dummy_crop, &[], 0, 0)?;
        println!("Generated SLANet markdown:\n{}", md);
        Ok(())
    }
}
