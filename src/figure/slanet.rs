//! SLANet: Structure-Location Architecture Network for Table Recognition.
//! Predicts table HTML structure tokens and cell bounding boxes via ONNX Runtime.

#[cfg(feature = "slanet-ort")]
use std::path::Path;
#[cfg(feature = "slanet-ort")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "slanet-ort")]
use image::{DynamicImage, GenericImageView};
#[cfg(feature = "slanet-ort")]
use crate::backends::indra::order::TextLine;

/// A recognized table cell with spatial coordinates and text.
#[derive(Debug, Clone)]
pub struct TableCell {
    pub bbox: [f32; 4], // [x1, y1, x2, y2] in cropped table coordinates
    pub text: String,
}

#[cfg(feature = "slanet-ort")]
pub struct SlaNetEngine {
    session: Mutex<ort::session::Session>,
    vocab: Vec<String>,
}

#[cfg(feature = "slanet-ort")]
static SLANET_ENGINE: std::sync::OnceLock<Option<Arc<SlaNetEngine>>> = std::sync::OnceLock::new();

/// Returns a shared, lazily initialized singleton instance of the SLANet Table Engine,
/// or `None` if the model/dictionary files are not provisioned.
#[cfg(feature = "slanet-ort")]
pub fn get_slanet_engine() -> Option<Arc<SlaNetEngine>> {
    let engine = SLANET_ENGINE.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let model_path = std::path::Path::new(&manifest_dir).join("models").join("layout").join("slanet.onnx");
        let dict_path = std::path::Path::new(&manifest_dir).join("models").join("layout").join("table_structure_dict.txt");
        if model_path.exists() && dict_path.exists() {
            SlaNetEngine::new(&model_path, &dict_path).ok().map(Arc::new)
        } else {
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
        let env = ort::environment::init()
            .with_name("slanet")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to initialize ORT environment: {e}"))?;

        let session = ort::session::Session::builder(&env)
            .map_err(|e| anyhow::anyhow!("Failed to create session builder: {e}"))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .map_err(|e| anyhow::anyhow!("Failed to set optimization level: {e}"))?
            .commit_from_file(model_path.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to commit model from file: {e}"))?;

        let dict_content = std::fs::read_to_string(dict_path.as_ref())?;
        let mut vocab = Vec::new();
        for line in dict_content.lines() {
            vocab.push(line.to_string());
        }
        // Add standard special tokens if needed
        vocab.push("sos".to_string());
        vocab.push("eos".to_string());

        Ok(Self {
            session: Mutex::new(session),
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
        let (orig_w, orig_h) = table_crop.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(String::new());
        }

        // 1. Preprocess: Resize to 488x488 RGB, ImageNet normalization
        let resized = table_crop.resize_exact(488, 488, image::imageops::FilterType::Triangle);
        let rgb = resized.to_rgb8();

        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];

        let mut input_tensor = vec![0.0f32; 1 * 3 * 488 * 488];
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
        let tensor = ort::value::Tensor::from_array(([1usize, 3, 488, 488], input_tensor.into_boxed_slice()))?;
        let inputs = ort::inputs!["x" => tensor];
        let mut session = self
            .session
            .lock()
            .map_err(|e| anyhow::anyhow!("SLANet mutex poisoned: {e}"))?;
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
                let x1 = loc_slice[t * 4 + 0] * orig_w as f32;
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

        // 5. Reconstruct rows into Markdown table
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut cell_idx = 0;

        for tag in &predicted_tags {
            if tag == "<tr>" {
                current_row.clear();
            } else if tag == "</tr>" {
                if !current_row.is_empty() {
                    rows.push(current_row.clone());
                    current_row.clear();
                }
            } else if tag == "<td>" || tag.starts_with("<td") {
                let text = if cell_idx < cells_with_text.len() {
                    cells_with_text[cell_idx].text.clone()
                } else {
                    String::new()
                };
                cell_idx += 1;
                // Escape pipe characters inside table cells
                let sanitized = text.replace('|', "\\|").replace('\n', " ");
                current_row.push(sanitized);
            }
        }

        if !current_row.is_empty() {
            rows.push(current_row);
        }

        if rows.is_empty() {
            return Ok(String::new());
        }

        let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if max_cols == 0 {
            return Ok(String::new());
        }

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

        Ok(md)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "slanet-ort")]
    use super::*;

    #[test]
    #[cfg(feature = "slanet-ort")]
    fn test_slanet_load_and_dummy_predict() -> anyhow::Result<()> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let model_path = std::path::Path::new(&manifest_dir).join("models").join("layout").join("slanet.onnx");
        let dict_path = std::path::Path::new(&manifest_dir).join("models").join("layout").join("table_structure_dict.txt");

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
