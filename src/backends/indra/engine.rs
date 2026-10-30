//! Top-level Indra OCR engine coordinator.

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage};
use std::path::PathBuf;

use super::detect::{IndraDetector, TextBoundingBox};
use super::models::IndraModelSource;
use super::order::{sort_and_format_markdown, TextLine};
use super::recognize::IndraRecognizer;
use super::vocab::{count_telugu_chars, IndraVocab};

/// Target script or routing strategy for Indra OCR text recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndraScript {
    /// Pure English/Latin script (rec.onnx).
    /// Prevents hallucinating non-Latin characters (Ethiopic, Georgian, Lao, Telugu, etc.) in English papers.
    #[default]
    English,
    /// Telugu and Indic script (rec_world.onnx).
    Telugu,
    /// Automatic script selection: prioritizes Telugu when detected, otherwise defaults to base English.
    Auto,
    /// Backward-compatible alias for English.
    Latin,
    /// Backward-compatible alias for Telugu/World.
    World,
}

impl std::str::FromStr for IndraScript {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "english" | "en" | "latin" => Ok(IndraScript::English),
            "telugu" | "te" | "world" => Ok(IndraScript::Telugu),
            "auto" => Ok(IndraScript::Auto),
            other => Err(anyhow::anyhow!(
                "Unknown language/script '{other}'. Supported options: --english, --telugu, --auto (or --lang <english|telugu|auto>)"
            )),
        }
    }
}

impl std::fmt::Display for IndraScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndraScript::English | IndraScript::Latin => write!(f, "English"),
            IndraScript::Telugu | IndraScript::World => write!(f, "Telugu"),
            IndraScript::Auto => write!(f, "Auto"),
        }
    }
}

pub struct IndraEngine {
    detector: IndraDetector,
    recognizer: IndraRecognizer,
    world_recognizer: Option<IndraRecognizer>,
    telugu_mask: Option<Vec<bool>>,
    default_script: IndraScript,
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

        // Attempt loading optional world recognizer for Telugu & Indic scripts
        let mut telugu_mask = None;
        let world_recognizer = match source.load_file("rec_world.onnx") {
            Ok(rec_world_bytes) => {
                let world_vocab = IndraVocab::from_world_json_bytes(&meta_bytes)
                    .context("Failed to load Indra world vocabulary")?;
                telugu_mask = Some(world_vocab.build_telugu_mask());
                let rec = IndraRecognizer::new_world(&rec_world_bytes, world_vocab)
                    .context("Failed to initialize Indra world recognizer")?;
                Some(rec)
            }
            Err(_) => None,
        };

        Ok(Self {
            detector,
            recognizer,
            world_recognizer,
            telugu_mask,
            default_script: IndraScript::English,
        })
    }

    /// Configures the default script strategy builder-style.
    pub fn with_script(mut self, script: IndraScript) -> Self {
        self.default_script = script;
        self
    }

    /// Sets the default script strategy.
    pub fn set_script(&mut self, script: IndraScript) {
        self.default_script = script;
    }

    /// Returns the currently configured default script strategy.
    pub fn default_script(&self) -> IndraScript {
        self.default_script
    }

    /// Returns whether the multilingual world model (rec_world.onnx) is loaded.
    pub fn has_world_model(&self) -> bool {
        self.world_recognizer.is_some()
    }

    /// Returns a reference to the text detector.
    pub fn detector(&self) -> &IndraDetector {
        &self.detector
    }

    /// Returns a reference to the base Latin recognizer.
    pub fn base_recognizer(&self) -> &IndraRecognizer {
        &self.recognizer
    }

    /// Returns a reference to the world multilingual recognizer, if loaded.
    pub fn world_recognizer(&self) -> Option<&IndraRecognizer> {
        self.world_recognizer.as_ref()
    }

    /// Recognizes a single text bounding box line with a specific script strategy.
    pub fn transcribe_line(
        &self,
        gray: &GrayImage,
        bbox: &TextBoundingBox,
        script: IndraScript,
    ) -> Result<(String, f32)> {
        match script {
            IndraScript::Telugu | IndraScript::World => {
                if let Some(world) = &self.world_recognizer {
                    world.recognize_line_with_mask(gray, bbox, self.telugu_mask.as_deref())
                } else {
                    self.recognizer.recognize_line_with_conf(gray, bbox)
                }
            }
            IndraScript::English | IndraScript::Latin => {
                self.recognizer.recognize_line_with_conf(gray, bbox)
            }
            IndraScript::Auto => {
                if let Some(world) = &self.world_recognizer {
                    let (world_text, world_conf) = world.recognize_line_with_mask(gray, bbox, self.telugu_mask.as_deref())?;

                    // If world text contains genuine Telugu characters, route to world model
                    let telugu_count = count_telugu_chars(&world_text);
                    if telugu_count >= 2 || (telugu_count == 1 && world_text.chars().count() <= 6) {
                        return Ok((world_text, world_conf));
                    }

                    // Otherwise prioritize base Latin recognizer to avoid hallucinated world glyphs
                    let (base_text, base_conf) = self.recognizer.recognize_line_with_conf(gray, bbox)?;
                    if base_text.trim().is_empty() && !world_text.trim().is_empty() && world_conf > 0.6 {
                        Ok((world_text, world_conf))
                    } else {
                        Ok((base_text, base_conf))
                    }
                } else {
                    self.recognizer.recognize_line_with_conf(gray, bbox)
                }
            }
        }
    }

    /// Transcribes an entire document page image using an explicit script strategy.
    pub fn transcribe_image_with_script(
        &self,
        img: &DynamicImage,
        script: IndraScript,
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

        // 2. Crop each line and run character recognition
        let mut recognized_lines = Vec::with_capacity(boxes.len());
        for bbox in boxes {
            let (text, _conf) = self.transcribe_line(&gray, &bbox, script)?;

            if !text.trim().is_empty() {
                recognized_lines.push(TextLine { bbox, text });
            }
        }

        // 3. Format lines into reading order Markdown
        let mut lines_for_markdown = recognized_lines.clone();
        let markdown = sort_and_format_markdown(&mut lines_for_markdown);
        Ok((markdown, recognized_lines))
    }

    /// Transcribes an entire document page image into Markdown and returns recognized lines using the default script.
    pub fn transcribe_image_with_lines(
        &self,
        img: &DynamicImage,
    ) -> Result<(String, Vec<TextLine>)> {
        self.transcribe_image_with_script(img, self.default_script)
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
    fn test_indra_engine_initializes_with_world_model() -> Result<()> {
        let engine = IndraEngine::new(None).context("Failed to initialize IndraEngine")?;
        assert!(engine.has_world_model(), "IndraEngine should detect and load rec_world.onnx");
        let world = engine.world_recognizer().expect("World recognizer must be present");
        assert!(world.vocab().contains_telugu(), "World recognizer vocab must contain Telugu script");
        assert_eq!(world.target_height(), 32);
        assert_eq!(world.window(), 1);
        assert_eq!(engine.base_recognizer().target_height(), 96);
        assert_eq!(engine.base_recognizer().window(), 3);
        Ok(())
    }

    #[test]
    fn test_indra_engine_script_configuration() -> Result<()> {
        let engine = IndraEngine::new(None)?
            .with_script(IndraScript::World);
        assert_eq!(engine.default_script(), IndraScript::World);

        let eng_script: IndraScript = "english".parse()?;
        assert_eq!(eng_script, IndraScript::English);
        let te_script: IndraScript = "telugu".parse()?;
        assert_eq!(te_script, IndraScript::Telugu);
        let auto_script: IndraScript = "auto".parse()?;
        assert_eq!(auto_script, IndraScript::Auto);
        Ok(())
    }

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
