# Phase 10 Sign-Off Report: Offline Model Integrity, Registry & Device Control

**Document Status:** Complete & Verified  
**Date:** 2026-11-25  
**Workspace:** `pdfmd-lite` (`v0.7.2`, Rust Edition `2024`, Cargo Virtual Workspace + Vite/React 19/TypeScript)  
**Execution Environment:** Antigravity IDE + Gemini 3.8 Flash (Medium)

---

## 1. Executive Summary & Scope Definition

Phase 10 unifies offline neural model discovery, cryptographic SHA-256 integrity verification, execution provider hardware device selection (`--device auto|cpu|directml`), capability reporting, and reloadable in-memory model caches across the CLI binary (`pdfmd-lite`), Web daemon (`pdfmd-lite-server`), Desktop container (`pdfmd-lite-desktop`), and shared React 19 UI (`ui/`).

### Key Deliverables & Files Modified

- **Model Manifest & Cryptographic Self-Verification (`models/MANIFEST.toml`, `crates/engine/src/model_locator.rs`):**
  - Created [`models/MANIFEST.toml`](file:///e:/Rust/pdfmd-lite/models/MANIFEST.toml) capturing exact byte sizes, licenses, input/output tensor shapes, and SHA-256 hashes for all 12 active models (Required: `det.onnx`, `rec.onnx`, `meta.json`, `yolov8n-doclaynet.onnx`; Optional: `rec_world.onnx`, `slanet.onnx`, `table_structure_dict.txt`, `encoder.onnx`, `decoder.onnx`, `tokenizer.json`, `model.onnx`, `tokenizer.json`).
  - Embedded `MANIFEST.toml` at compile-time via `include_str!` in `model_locator.rs`, parsing into strongly typed `ModelManifest` and `ModelManifestEntry`.
  - Added user AppData models directory (`pdfmd_lite::config::default_data_dir().join("models")`) to `ModelLocator::locate` cascading resolution (`--models-dir` > exe-adjacent > AppData > dev workspace > `bundle-weights`) honoring [ADR 0014](file:///e:/Rust/pdfmd-lite/docs/adr/0014-cascading-model-locator.md).
  - Enhanced `ModelStatus` to capture `expected_sha256`, `size_bytes`, `expected_size_bytes`, `is_valid`, and explicit error descriptions.
  - Implemented `ModelLocator::check_required_models()` verifying that all required models exist and pass checksum validation.
- **Purge Dead/Phantom Weights & Custom Layout File Loading:**
  - Removed unused and phantom model entries (`order.onnx`, `unimernet.onnx`, `bge_small_int8.onnx`, `gliner_small.onnx`, `slanet_plus.onnx`) from `KNOWN_MODELS`, `bundle-weights`, and manifest.
  - Fixed `LayoutModelSource::load_layout_bytes()` in [`crates/engine/src/figure/models.rs`](file:///e:/Rust/pdfmd-lite/crates/engine/src/figure/models.rs) to load file paths directly via `fs::read` when `path.is_file()` instead of treating the file path as a custom directory.
  - Decoupled `populate_metadata_from_text` in [`crates/engine/src/metadata/gliner.rs`](file:///e:/Rust/pdfmd-lite/crates/engine/src/metadata/gliner.rs) to execute deterministic header metadata extraction directly without loading `gliner_small.onnx`.
- **Strict Capability & `--allow-degraded` Gating (`crates/cli/`, `crates/engine/`):**
  - Added `--allow-degraded` boolean flag to `CliArgs` and `AppConfig.allow_degraded`.
  - Added strict language validation in CLI: unsupported languages fail immediately with code 1 unless `--allow-degraded` is passed (logging a warning and falling back to English).
  - Gated Telugu/World script OCR on presence of `models/indra/rec_world.onnx`: fails immediately with code 1 unless `--allow-degraded` is explicitly passed.
  - Aligned `IndraScript::World` masking in `transcribe_line` in [`crates/engine/src/backends/indra/engine.rs`](file:///e:/Rust/pdfmd-lite/crates/engine/src/backends/indra/engine.rs) to pass `None` for mask (matching `transcribe_gray_with_script`) rather than constraining World recognition to Telugu Unicode range.
- **Offline Importer, `--device` DirectML Execution Provider & Reloadable Engines:**
  - Implemented `pdfmd-lite --install-models <dir|zip>` in CLI and [`crates/engine/src/vault.rs`](file:///e:/Rust/pdfmd-lite/crates/engine/src/vault.rs): extracts/copies models into AppData `models/` directory, protects against zip-slip directory traversal, verifies models against `MANIFEST.toml`, and invalidates caches.
  - Wired `--device auto|cpu|directml` end-to-end across `AppConfig`, `CliArgs`, and ORT session builders, configuring DirectML execution provider on Windows and logging the active EP via `tracing::info!`.
  - Scaled ORT session pool counts dynamically by RAM via [`crates/engine/src/concurrency.rs`](file:///e:/Rust/pdfmd-lite/crates/engine/src/concurrency.rs): provisions 1 session when `< 8 GB` and 2 sessions when `>= 8 GB`.
  - Replaced `OnceLock<Option<...>>` singletons with `RwLock` in `figure/yolo.rs`, `figure/slanet.rs`, `figure/latex.rs`, `embeddings/engine.rs`, and `pipeline.rs`, unifying them under `pdfmd_lite::invalidate_model_caches()`.
- **Truthful Model Status in Server REST API & Desktop UI (`crates/server/`, `crates/desktop/`, `ui/`):**
  - Added `GET /api/models` endpoint in [`crates/server/src/lib.rs`](file:///e:/Rust/pdfmd-lite/crates/server/src/lib.rs) and `get_model_status` Tauri IPC command in [`crates/desktop/src/main.rs`](file:///e:/Rust/pdfmd-lite/crates/desktop/src/main.rs).
  - Updated `ui/src/bridge/api.ts` with `getModelStatus()`.
  - Replaced hardcoded dummy cards in [`ui/src/components/ModelsModal.tsx`](file:///e:/Rust/pdfmd-lite/ui/src/components/ModelsModal.tsx) with live verified backend status, showing required/optional badges, sizes, truncated SHA-256 hashes, and verified/mismatch/missing badges.

---

## 2. Passing Gate Execution Log

All 7 mandatory Phase 10 Passing Gate commands exited with status code `0`:

| # | Command | Exit Code | Result Summary |
|---|---|---|---|
| 1 | `cargo check --workspace --all-targets` | `0` | All workspace member crates compile cleanly with zero errors |
| 2 | `cargo check --benches` | `0` | Criterion ocr_bench benchmarks compile cleanly |
| 3 | `cargo test --workspace --all-targets` | `0` | 117 tests passed (111 engine + 6 server + 0 failures) |
| 4 | `cargo clippy --workspace --all-targets -- -D warnings` | `0` | Zero compiler warnings or clippy lints across all targets |
| 5 | `cargo fmt --all -- --check` | `0` | Workspace formatting strictly conforms to Rust style standards |
| 6 | `npm.cmd --prefix ui run build` | `0` | TypeScript type-check and Vite production bundle build succeed |
| 7 | `npm.cmd --prefix ui run test` | `0` | All Vitest UI test suites pass |

---

## 3. Diagnostic Verification (`--doctor`)

Verification of `pdfmd-lite --doctor` output:

```text
pdfmd-lite Offline Neural Model Diagnostics
==========================================================================================
Model Name                          Type       Size       SHA256     Valid      Status / Location
------------------------------------------------------------------------------------------
Indra OCR DBNet Detection           REQUIRED   0.37 MB    f2bd4cba   YES        [FOUND] models/indra/det.onnx
Indra OCR SVTR Latin Recognition    REQUIRED   4.81 MB    475972a2   YES        [FOUND] models/indra/rec.onnx
Indra OCR SVTR World/Telugu Recognition OPTIONAL   5.37 MB    c8d1e59f   YES        [FOUND] models/indra/rec_world.onnx
Indra OCR Metadata and Vocab        REQUIRED   0.20 MB    4ce4c94b   YES        [FOUND] models/indra/meta.json
DocLayNet YOLOv8n Layout Detector   REQUIRED   11.71 MB   6b9137bd   YES        [FOUND] models/layout/yolov8n-doclaynet.onnx
SLANet Table Structure Recognizer   OPTIONAL   7.32 MB    8a8aa31b   YES        [FOUND] models/layout/slanet.onnx
SLANet Table Structure Vocab        OPTIONAL   0.00 MB    1e2375e0   YES        [FOUND] models/layout/table_structure_dict.txt
RapidLaTeXOCR Encoder               OPTIONAL   84.88 MB   01bf5dc2   YES        [FOUND] models/latex/encoder.onnx
RapidLaTeXOCR Decoder               OPTIONAL   48.59 MB   bd695497   YES        [FOUND] models/latex/decoder.onnx
RapidLaTeXOCR Tokenizer             OPTIONAL   0.02 MB    1dc27b18   YES        [FOUND] models/latex/tokenizer.json
all-MiniLM-L6-v2 Embeddings         OPTIONAL   21.91 MB   afdb6f1a   YES        [FOUND] models/embeddings/model.onnx
all-MiniLM-L6-v2 Tokenizer          OPTIONAL   0.44 MB    be50c362   YES        [FOUND] models/embeddings/tokenizer.json
==========================================================================================
All required offline neural models are provisioned and verified.
```

---

## 4. Architectural Invariant Verifications

- [x] **Zero Runtime Environment Variables:** Zero `std::env::var` calls used for configuration or model paths.
- [x] **Zero-Unwrap Policy:** Zero unhandled panics, unwrap, or expect in production paths.
- [x] **Cascading Precedence (ADR 0014):** `--models-dir` > exe-adjacent > AppData > dev workspace > embedded slices.
- [x] **DirectML Acceleration (ADR 0015):** User-selectable device EP (`--device auto|cpu|directml`) with active EP logged.
- [x] **Truthful Reporting & Strict Gating (ADR 0018):** Missing required models or checksum mismatches exit with status code 1.
- [x] **Single-Phase Discipline (Rule 11):** Only Phase 10 was executed and verified before committing.

---

## 5. Phase Sign-Off & Checkpoint Commit

Phase 10 is complete, verified, and signed off.
