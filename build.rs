//! Build script for pdfmd-lite.
//! Validates that offline neural model weights are present in `models/`.

use std::fs;
use std::path::PathBuf;

const INDRA_FILES: &[&str] = &[
    "det.onnx",
    "rec.onnx",
    "rec_world.onnx",
    "order.onnx",
    "meta.json",
    "config.json",
];

const LAYOUT_FILES: &[&str] = &["yolov8n-doclaynet.onnx"];

fn main() {
    check_model_weights();
}

fn check_model_weights() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));

    println!("cargo:rerun-if-changed=models/indra");
    println!("cargo:rerun-if-changed=models/layout");

    let indra_dir = manifest_dir.join("models").join("indra");
    for file_name in INDRA_FILES {
        let file_path = indra_dir.join(file_name);
        if !file_path.exists() || fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0) == 0 {
            println!(
                "cargo:warning=Indra OCR model file '{}' is missing in models/indra/.",
                file_name
            );
        }
    }

    let layout_dir = manifest_dir.join("models").join("layout");
    for file_name in LAYOUT_FILES {
        let file_path = layout_dir.join(file_name);
        if !file_path.exists() || fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0) == 0 {
            println!(
                "cargo:warning=DocLayNet layout model file '{}' is missing in models/layout/.",
                file_name
            );
        }
    }
}
