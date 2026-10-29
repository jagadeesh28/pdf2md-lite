//! Build script for pdfmd-lite.
//! 1. Detects host hardware extensions (AVX2, AVX-512, FMA, NEON) for SIMD acceleration.
//! 2. Validates that offline neural model weights are present in `models/`.

use std::fs;
use std::path::PathBuf;

const INDRA_FILES: &[&str] = &[
    "det.onnx",
    "rec.onnx",
    "order.onnx",
    "meta.json",
    "config.json",
];

const LAYOUT_FILES: &[&str] = &[
    "yolov8n-doclaynet.onnx",
];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_avx2)");
    println!("cargo::rustc-check-cfg=cfg(has_fma)");
    println!("cargo::rustc-check-cfg=cfg(has_avx512)");
    println!("cargo::rustc-check-cfg=cfg(has_neon)");

    detect_hardware_features();
    check_model_weights();
}

fn detect_hardware_features() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch == "x86_64" {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                println!("cargo:rustc-cfg=has_avx2");
            }
            if is_x86_feature_detected!("fma") {
                println!("cargo:rustc-cfg=has_fma");
            }
            if is_x86_feature_detected!("avx512f") {
                println!("cargo:rustc-cfg=has_avx512");
            }
        }
    } else if target_arch == "aarch64" {
        #[cfg(target_arch = "aarch64")]
        {
            println!("cargo:rustc-cfg=has_neon");
        }
    }
}

fn check_model_weights() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()),
    );

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
