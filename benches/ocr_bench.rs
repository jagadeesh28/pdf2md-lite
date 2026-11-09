//! Performance benchmarks for Indra OCR and pipeline stages.
//!
//! Run with: `cargo bench --bench ocr_bench`

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use image::{GrayImage, Luma};
use pdfmd_lite::backends::indra::recognize::IndraRecognizer;
use pdfmd_lite::backends::indra::vocab::IndraVocab;
use pdfmd_lite::backends::indra::{IndraEngine, IndraScript};
use std::path::Path;

const BUCKET_WIDTHS: [usize; 7] = [160, 320, 480, 640, 800, 1024, 1536];

fn load_base_recognizer() -> Option<IndraRecognizer> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let meta_path = Path::new(&manifest_dir)
        .join("models")
        .join("indra")
        .join("meta.json");
    let rec_path = Path::new(&manifest_dir)
        .join("models")
        .join("indra")
        .join("rec.onnx");
    if !meta_path.exists() || !rec_path.exists() {
        return None;
    }
    let meta_bytes = std::fs::read(&meta_path).ok()?;
    let vocab = IndraVocab::from_json_bytes(&meta_bytes).ok()?;
    let model_bytes = std::fs::read(&rec_path).ok()?;
    IndraRecognizer::new(&model_bytes, vocab).ok()
}

fn synthetic_line_crop(width: usize, height: usize) -> GrayImage {
    let mut img = GrayImage::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            if y > height / 3 && y < 2 * height / 3 {
                img.put_pixel(x as u32, y as u32, Luma([0u8]));
            } else {
                img.put_pixel(x as u32, y as u32, Luma([255u8]));
            }
        }
    }
    img
}

fn bench_serial_recognition(c: &mut Criterion) {
    let Some(rec) = load_base_recognizer() else {
        return;
    };
    let widths = [200usize, 600, 1200];
    for &w in &widths {
        let img = synthetic_line_crop(w, 96);
        let bbox = pdfmd_lite::backends::indra::detect::TextBoundingBox {
            x: 0,
            y: 0,
            width: w as u32,
            height: 96,
            score: 1.0,
        };
        c.bench_function(&format!("serial_recognize_w{}", w), |b| {
            b.iter(|| {
                let _ = black_box(rec.recognize_line(&img, &bbox).unwrap());
            });
        });
    }
}

fn bench_batched_recognition(c: &mut Criterion) {
    let Some(rec) = load_base_recognizer() else {
        return;
    };
    let lines: Vec<(usize, GrayImage)> = vec![
        (200, synthetic_line_crop(200, 96)),
        (250, synthetic_line_crop(250, 96)),
        (400, synthetic_line_crop(400, 96)),
        (500, synthetic_line_crop(500, 96)),
        (700, synthetic_line_crop(700, 96)),
        (900, synthetic_line_crop(900, 96)),
        (1200, synthetic_line_crop(1200, 96)),
        (1400, synthetic_line_crop(1400, 96)),
    ];
    let bboxes: Vec<pdfmd_lite::backends::indra::detect::TextBoundingBox> = lines
        .iter()
        .enumerate()
        .map(
            |(i, (w, _))| pdfmd_lite::backends::indra::detect::TextBoundingBox {
                x: 0,
                y: i as u32,
                width: *w as u32,
                height: 96,
                score: 1.0,
            },
        )
        .collect();
    let grays: Vec<&GrayImage> = lines.iter().map(|(_, img)| img).collect();
    c.bench_function("batched_recognize_8_lines", |b| {
        b.iter(|| {
            let results = recognize_lines_batched(&rec, &grays, &bboxes);
            black_box(results);
        });
    });
}

fn recognize_lines_batched(
    rec: &IndraRecognizer,
    grays: &[&GrayImage],
    boxes: &[pdfmd_lite::backends::indra::detect::TextBoundingBox],
) -> Vec<(String, f32)> {
    let mut results: Vec<Option<(String, f32)>> = vec![None; boxes.len()];
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, box_) in boxes.iter().enumerate() {
        let ink_w = box_.width as usize;
        let bucket_w = BUCKET_WIDTHS
            .iter()
            .find(|&&bw| ink_w <= bw)
            .copied()
            .unwrap_or(*BUCKET_WIDTHS.last().unwrap());
        groups.entry(bucket_w).or_default().push(i);
    }
    for (_bucket_w, indices) in groups {
        for &idx in &indices {
            if let Ok((text, conf)) = rec.recognize_line_with_conf(grays[idx], &boxes[idx]) {
                results[idx] = Some((text, conf));
            }
        }
    }
    results.into_iter().map(|r| r.unwrap_or_default()).collect()
}

fn bench_page_transcription(c: &mut Criterion) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let img_path = Path::new(&manifest_dir).join("images").join("page-1.png");
    if !img_path.exists() {
        return;
    }
    let img = image::open(&img_path).unwrap().into_luma8();
    let dynamic = image::DynamicImage::ImageLuma8(img);
    let engine = IndraEngine::new(None).unwrap();
    c.bench_function("transcribe_page", |b| {
        b.iter(|| {
            let _ = black_box(
                engine
                    .transcribe_image_with_script(&dynamic, IndraScript::English)
                    .unwrap(),
            );
        });
    });
}

criterion_group!(
    benches,
    bench_serial_recognition,
    bench_batched_recognition,
    bench_page_transcription
);
criterion_main!(benches);
