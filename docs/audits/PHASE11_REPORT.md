# Phase 11 Sign-Off Report: PDF Text Fidelity, Structured IR, Reading Order & Hybrid Routing

> **Status:** Completed & Verified  
> **Workspace:** `pdfmd-lite` (`crates/engine`, `crates/cli`, `crates/server`, `crates/desktop`, `ui/`)  
> **Phase Milestone:** Phase 11 (per `AGENTS.md`)  
> **Date:** 2026-11-25

---

## 1. Executive Summary

Phase 11 remediates core reading-order anomalies, PDF text-state fidelity gaps, coordinate transform inaccuracies, and table/prose duplication. It introduces a hierarchical Structured Document IR (`Page -> Region -> Block -> Line -> Span`), an end-to-end diagnostic layout trace (`--debug-layout <dir>`), comprehensive CTM and text operator support (`q`/`Q`/`cm`, `'`/`"`, `Tc`/`Tw`/`Tz`), ligature decoding via `/Encoding /Differences`, font-size-relative baseline clustering, single-column gutter validation with sustained dual-side text mass in XY-Cut++, DocLayNet Section-header and Title signal integration, and pure-Rust `tract-onnx` support for `all-MiniLM-L6-v2` embeddings.

---

## 2. Key Accomplishments by Task

### Task 1: Diagnostic Trace (`--debug-layout <dir>`) & Structured Document IR
- **IR Architecture (`crates/engine/src/ir.rs`):** Implemented `PageIr`, `RegionIr`, `BlockIr`, `LineIr`, and `SpanIr` with exact `Provenance` tracking (`Dom`, `Ocr`, `Specialist`) and geometric `BoundingBox`.
- **Diagnostic Export:** Implemented `DebugLayoutTrace` emitting per-page JSON traces capturing `MediaBox`/`CropBox`/`Rotate`, raw DOM spans, raw and post-NMS YOLO boxes, XY-Cut++ splits, and final block order.
- **CLI & Config Integration:** Added `--debug-layout <DIR>` to CLI arguments and `AppConfig`, wired into `ImageOptions` and document processing pipelines.

### Task 2: PDF Text-State & Coordinate Fidelity (`crates/engine/src/pdf.rs`)
- **CTM Graphics Stack:** Implemented graphics state stack (`q`/`Q`) and Current Transformation Matrix (`cm`) transforms via `mult_matrix`.
- **Text Operators:** Implemented full support for `'` (advance to next line and show text) and `"` (set word spacing, char spacing, advance and show text), along with `Tc` (character spacing), `Tw` (word spacing), and `Tz` (horizontal scaling).
- **Coordinate & Rotation Normalization:** Handled `CropBox`/`MediaBox` origin offsets and page rotation ($0^\circ, 90^\circ, 180^\circ, 270^\circ$) in `parse_content_text_lines_with_geometry`.
- **Font Ligatures & CMap:** Added `/Encoding /Differences` array decoding and mapped standard Adobe Glyph List ligatures (`fi`, `fl`, `ff`, `ffi`, `ffl`, dashes, quotes, math symbols). Enhanced `PdfFontDecoder` with variable-length 1-byte/2-byte decoding.
- **Baseline Clustering:** Refined baseline clustering using font-size-relative thresholds ($|\Delta y| \le 0.35 \cdot \min(h_1, h_2)$ for standard text and up to $0.6 \cdot \max(h_1, h_2)$ for sub/superscript candidate spans).
- **DOM Sanity Gate:** Implemented automated page-level DOM sanity check fallback to OCR if replacement (`\u{FFFD}`) or Unicode PUA characters exceed 10%.

### Task 3: Structured Document IR & Pipeline Integration
- **Layout Exclusion:** Excluded `Table`, `Formula`, and `Picture` regions from prose lines in both DOM extraction and OCR fallback in `crates/engine/src/pipeline.rs`.
- **Prose De-duplication:** Filtered text lines overlapping table rectangles to eliminate duplicated table text in body prose.
- **Formula Placeholder Removal:** Eliminated `$?$` formula placeholder emission; formula regions are routed exclusively to LaTeX extraction.
- **Protected De-hyphenation:** Guarded `dehyphenate_text` in `crates/engine/src/markdown.rs` against inline code, fenced code blocks, and LaTeX math blocks (`$..$`, `$$..$$`).

### Task 4: XY-Cut++ & YOLO Region Integrity
- **XY-Cut++ Gutter Validation (`crates/engine/src/xycut.rs`):**
  - Added vertical overlap shrinkage with 10% line height margin.
  - Implemented single-column gutter validation requiring sustained dual-side text mass ($\ge 15\%$ on each partition) to prevent phantom column splits from indented paragraphs, dates, or bullet lists.
- **DocLayNet Class Support (`crates/engine/src/figure/yolo.rs`):**
  - Enabled `Section-header` (class 7) and `Title` (class 10) in `LayoutClass` and YOLO detector.
  - Implemented class-agnostic containment pruning: `Table` boxes suppress overlapping `Picture` boxes exceeding 60% IoU or containment.

### Task 5: Embeddings & Metadata Cleanup
- **Pure-Rust `tract-onnx` MiniLM:** Added pure-Rust fallback in `crates/engine/src/embeddings/engine.rs` enabling `all-MiniLM-L6-v2` inference without ORT dynamic libraries.
- **Batched Execution:** Implemented batched tensor evaluation in `OrtEmbeddingBackend` for multi-line chunks.
- **Token-Count Chunking:** Refactored `chunk_markdown_document` in `chunking.rs` from character lengths to token bounds (`MIN_CHUNK_TOKENS: 64`, `TARGET_CHUNK_TOKENS: 384`, `MAX_CHUNK_TOKENS: 512`) using `estimate_tokens()`.
- **Sidecar Schema Header:** Added `_schema: pdfmd-lite/embeddings/v1` metadata header in `embeddings/sidecar.rs`.
- **Deterministic Metadata:** Replaced fabricated domain tags with script-ratio-based language classification (`lang/telugu`, `lang/multilingual`, `lang/english`).

---

## 3. Passing Gate Verification

All Phase 11 passing gates passed with exit code `0`:

```powershell
# 1. Workspace targets check
cargo check --workspace --all-targets
# Exit code: 0

# 2. Benchmarks check
cargo check --benches
# Exit code: 0

# 3. Full test suite (117 total unit and integration tests)
cargo test --workspace --all-targets
# Exit code: 0 (111 engine tests + 6 server tests)

# 4. Workspace Clippy audit (strict -D warnings)
cargo clippy --workspace --all-targets -- -D warnings
# Exit code: 0 (0 warnings)

# 5. Rust formatting audit
cargo fmt --all -- --check
# Exit code: 0
```

---

## 4. Architectural Invariants Maintained

- **Zero-Unwrap Policy:** No production `unwrap()` or `expect()` introduced.
- **Zero Runtime Environment Variables:** Configuration and flags adhere strictly to CLI args and `AppConfig`.
- **Crate Purity:** `crates/engine` remains strictly headless with zero UI/CLI dependencies.
