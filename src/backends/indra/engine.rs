//! Top-level Indra OCR engine coordinator.

use anyhow::{Context, Result};
use image::DynamicImage;
use std::path::PathBuf;

use super::detect::IndraDetector;
use super::models::IndraModelSource;
use super::order::{sort_and_format_markdown, TextLine};
use super::recognize::IndraRecognizer;
use super::vocab::IndraVocab;

pub struct IndraEngine {
    detector: IndraDetector,
    recognizer: IndraRecognizer,
}

impl IndraEngine {
    /// Loads the Indra OCR engine using embedded models or an optional custom directory.
    pub fn new(custom_path: Option<PathBuf>) -> Result<Self> {
        let source = IndraModelSource::new(custom_path);

        let meta_bytes = source.load_file("meta.json")?;
        let vocab = IndraVocab::from_json_bytes(&meta_bytes)?;

        let det_bytes = source.load_file("det.onnx")?;
        let detector = IndraDetector::new(&det_bytes, vocab.det_long_side)?;

        let rec_bytes = source.load_file("rec.onnx")?;
        let recognizer = IndraRecognizer::new(&rec_bytes, vocab)?;

        Ok(Self {
            detector,
            recognizer,
        })
    }

    /// Transcribes an entire document page image into Markdown and returns recognized lines.
    pub fn transcribe_image_with_lines(
        &self,
        img: &DynamicImage,
    ) -> Result<(String, Vec<TextLine>)> {
        // 1. Detect all text bounding boxes
        let boxes = self
            .detector
            .detect(img)
            .context("Indra text detection failed")?;

        if boxes.is_empty() {
            return Ok((String::new(), Vec::new()));
        }

        let gray = img.to_luma8();

        // 2. Crop each line with SVTR tall context window and run character recognition
        let mut recognized_lines = Vec::with_capacity(boxes.len());
        for bbox in boxes {
            let text = self.recognizer.recognize_line(&gray, &bbox)?;

            if !text.trim().is_empty() {
                recognized_lines.push(TextLine { bbox, text });
            }
        }

        // 3. Format lines into reading order Markdown
        let mut lines_for_markdown = recognized_lines.clone();
        let markdown = sort_and_format_markdown(&mut lines_for_markdown);
        Ok((markdown, recognized_lines))
    }

    /// Transcribes an entire document page image into Markdown.
    pub fn transcribe_image(&self, img: &DynamicImage) -> Result<String> {
        let (markdown, _) = self.transcribe_image_with_lines(img)?;
        Ok(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_indra_engine_e2e() -> Result<()> {
        let img_path = Path::new("images/page-1.png");
        if !img_path.exists() {
            println!("Test image not found, skipping");
            return Ok(());
        }

        let img = image::open(img_path).context("Failed to open test image")?;
        let engine = IndraEngine::new(None).context("Failed to initialize IndraEngine")?;
        let markdown = engine.transcribe_image(&img)?;

        println!("=== Transcribed Markdown ===");
        println!("{}", markdown);
        println!("============================");

        assert!(!markdown.trim().is_empty(), "Transcribed output should not be empty");
        Ok(())
    }
}
