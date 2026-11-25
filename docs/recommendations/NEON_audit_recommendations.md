# `pdfmd-lite` Repository Audit & Implementation Roadmap (`audit_recommendations.md`)

## 1. Audit Metadata & Scope

| Field | Detail |
|---|---|
| **Repository** | `pdfmd-lite` v0.7.2 (`pdfmd-lite.zip` + nested `ui_src.zip`) |
| **Audit Objectives** | (1) Alignment with stated objectives & ADRs 0001–0016, (2) Correctness of offline neural model loading & execution, (3) **Reading order & text parsing accuracy on simple single-page and multi-column PDFs**, (4) UI completeness & Desktop/Web shell parity, (5) UX ergonomics & air-gapped workflows, (6) Phased remediation with deterministic tests & passing gates |
| **Directly Audited Files** | Workspace & crate `Cargo.toml` manifests, `README.md`, `AGENTS.md`, `Audit_2026_11_19.md`, `REFERENCES.md`, `implementedphases.md`, all 16 ADRs (`docs/adr/0001`–`0016`), `build.rs`, `pdfmd-lite.toml`, `crates/desktop/tauri.conf.json`, `crates/desktop/capabilities/default.json`, `crates/engine/src/{lib,config,concurrency,model_locator,vault}.rs`, `crates/engine/src/backends/indra/*`, `crates/engine/src/figure/{models,session_pool,yolo,slanet,latex}.rs`, `crates/engine/src/metadata/gliner.rs`, `crates/engine/src/embeddings/engine.rs`, plus structural scans of `pipeline.rs`, `pdf.rs`, `xycut.rs`, `markdown.rs`, `batch.rs`, `cli.rs`, and the `ui_src.zip` file tree (`App.tsx`, `bridge/api.ts`, `hooks/useConversionProgress.ts`, and all 9 UI components) |
| **Targeted Deep-Inspection Gates** | Line-by-line instrumentation of `pdf.rs`, `xycut.rs`, `pipeline.rs`, and `markdown.rs` is scheduled as **Phase A Gate 0** (for single-page reading order & text parsing bugs), and line-by-line UI/Server contract inspection (`crates/server/src/lib.rs`, `crates/desktop/src/main.rs`, `ui_src/src/**/*.tsx`) is scheduled as **Phase E Gate 0** |

---

## 2. Executive Summary

`pdfmd-lite` establishes a clean four-crate Cargo workspace (`engine`, `cli`, `server`, `desktop`), enforces zero runtime environment-variable reads (confirmed across all four crates), provides deterministic pure-Rust fallbacks (`xycut.rs`, geometric tables, regex metadata), and uses SHA-256 frontmatter idempotency.

However, the audit identified **four high-impact problem areas** that directly affect output quality, offline correctness, and usability:

1. **Single-Page Reading Order & Text Parsing Regressions (Observed in Practice)**: Even on simple single-page PDFs, failures in hybrid PDF-to-Markdown pipelines typically stem from the handoff between four stages—`pdf.rs` (Hybrid DOM glyph/baseline clustering & CMap validation), `figure/yolo.rs` (overlapping or fragmented layout boxes), `xycut.rs` (XY-Cut++ falsely splitting single-column pages on indentation, short lines, or tight gutters), and `markdown.rs` (line-joining, spacing, and block deduplication). Phase A below establishes a deterministic diagnostic harness and fixes these failure modes first.
2. **Offline Model Loading & Execution Defects**:
   - **GLiNER (`gliner_small.onnx`) is a 183 MB no-op**: Loaded into an ONNX Runtime session marked `#[allow(dead_code)]` and never invoked; when absent, `pipeline.rs` skips even the pure-Rust heuristic metadata extractor.
   - **`ModelLocator` violates ADR 0014**: `load_bytes()` checks embedded slices *before* exe-adjacent disk models (contradicting `--doctor`) and omits the platform user-data directory.
   - **Custom `--layout-model` path is broken**: `LayoutModelSource::load_layout_bytes()` passes an `.onnx` file path as `ModelLocator`'s directory argument.
   - **Memory budget & hardware controls violate ADR 0011 and ADR 0015**: RapidLaTeXOCR spawns 2 encoder + 2 decoder sessions (~268 MB alone) and SLANet spawns 2 sessions, exceeding the 350 MB base assumption in `concurrency.rs`. DirectML is hardcoded per call site with no `--device auto|cpu|directml` CLI/config control.
   - **Default Cargo features break clean offline builds**: `embed-models` is enabled by default and conflates `dep:tokenizers` with `include_bytes!`.
3. **Unsafe & Non-Portable Model Provisioning (`vault.rs`)**: Hardcodes Windows-only `curl.exe`, omits `--fail` and SHA-256 verification, writes to CWD instead of the resolved model directory, and omits core Slim MSI weights.
4. **Desktop/Web UI Parity & Runtime Reload Gaps**: Process-wide `OnceLock` singletons prevent UI settings changes (`models_dir`, `device`) from taking effect without a full restart; `tauri.conf.json` hardcodes `npm.cmd` and sets `"csp": null`; `ModelsModal` displays unused/unimplemented weights (`order.onnx`, `unimernet.onnx`, `bge_small_int8.onnx`).

---

## 3. Detailed Audit Findings

### 3.1 Reading Order & Single-Page Text Parsing Diagnostics (P0)

When a **simple, single-page PDF** exhibits reading-order swaps or garbled/mis-spaced text, the root cause almost always lives at the boundaries between DOM extraction, layout detection, geometric sorting, and Markdown assembly. The following six failure modes must be audited and gated in `crates/engine/src/{pdf.rs, xycut.rs, pipeline.rs, markdown.rs, figure/yolo.rs, backends/indra/*}`:

| ID | Severity | Modules Involved | Failure Mechanism & What to Audit |
|---|---|---|---|
| **PARSE-1** | **Critical** | `crates/engine/src/pdf.rs` (Hybrid DOM extraction, ADR 0012) | **Baseline clustering & intra-line word ordering**: PDF content streams do not guarantee left-to-right or top-to-bottom operator order (e.g., kerning adjustments, superscripts/subscripts, bold spans, or text drawn in reverse `Tm`/`Td` order). If baseline grouping uses a fixed pixel/point tolerance instead of a font-size-relative tolerance (e.g., $|\Delta y| \le 0.35 \times \text{font\_size}$), slightly shifted spans split into separate lines and sort out of order. Similarly, if horizontal gap thresholds for inserting spaces are too tight or too loose, words either concatenate (`helloworld`) or shatter (`h e l l o`). |
| **PARSE-2** | **Critical** | `crates/engine/src/pdf.rs` & `pipeline.rs` | **CMap / `/ToUnicode` corruption & silent fallback failure**: PDFs using custom font encodings, subsetted Type1/CID fonts without complete `/ToUnicode` tables, or ligatures (`fi`, `fl`, `ﬀ`) can emit replacement glyphs (`\u{FFFD}`), Private Use Area (PUA) codepoints, or shifted ASCII (e.g., Caesar-shifted characters) while still passing a naive "non-empty text" check—causing Hybrid DOM to win over Indra OCR even though the extracted text is unreadable. |
| **PARSE-3** | **Critical** | `crates/engine/src/xycut.rs` | **XY-Cut++ over-splitting single-column pages**: On a simple single-page document with indented paragraphs, bulleted lists, key-value headers, or a short centered title, recursive vertical projection can mistakenly detect a "column gutter" in whitespace and split a single-column page into phantom left/right columns—scrambling reading order across paragraphs. Conversely, minor bounding-box $y$-overlap (1–3 px) between consecutive lines can block horizontal cuts and trigger an erroneous vertical cut. |
| **PARSE-4** | **High** | `crates/engine/src/figure/yolo.rs` & `pipeline.rs` | **Overlapping / fragmented YOLO boxes & region drop/duplication**: If YOLO emits overlapping `Text` / `Section-header` / `List-item` boxes without class-agnostic NMS (or with a too-high IoU threshold), the same line can be extracted twice or split mid-sentence into two separate regions that later sort out of order. Conversely, text lines falling slightly outside YOLO's predicted boxes (e.g., page titles, short captions, or margin-hugging first lines) can be silently dropped or appended at the bottom as "orphan" blocks. |
| **PARSE-5** | **High** | `crates/engine/src/backends/indra/{detect.rs, recognize.rs, order.rs}` | **OCR line-box jitter & coordinate-system mismatch**: When Indra OCR runs on a page or cropped region, slight vertical jitter between two text fragments on the same visual line causes naive `(y_min, x_min)` sorting to interleave fragments. Additionally, PDF user-space coordinates (bottom-left origin, $y$ increasing upward) vs. raster/YOLO image coordinates (top-left origin, $y$ increasing downward) must be normalized consistently before matching DOM spans to YOLO regions; any $y$-inversion or page `MediaBox`/`CropBox` offset mismatch scrambles block order or clips text. |
| **PARSE-6** | **High** | `crates/engine/src/markdown.rs` | **Line-joining, hyphenation & heading heuristics**: Over-aggressive paragraph merging or heading detection can merge a title into the first body line, break inline equations/list items, or strip meaningful line breaks in single-page structured documents (letters, invoices, resumes, spec sheets). |

---

### 3.2 Stated Objectives & ADR Compliance

| ID | Severity | Location | Finding |
|---|---|---|---|
| **OBJ-1** | **High** | `crates/engine/Cargo.toml:26–33`, `model_locator.rs:30–59` | `default` features include `embed-models`, which triggers `include_bytes!` for 6 files under `../../models/`. Any clean checkout without pre-populated weights fails to compile. Furthermore, `embed-models` is overloaded to enable `dep:tokenizers`, preventing builds that use external disk models with MiniLM embeddings. |
| **OBJ-2** | **High** | `crates/engine/src/config.rs:11–30`, `crates/cli/src/cli.rs` | **ADR 0015** mandates typed `--device auto\|cpu\|directml` CLI/config controls. `AppConfig` uses `#[serde(deny_unknown_fields)]` and lacks a `device` field; `cli.rs` has no `--device` flag. |
| **OBJ-3** | **Medium** | `crates/engine/src/embeddings/engine.rs:52–64, 186–189` | **ADR 0010** specifies `all-MiniLM-L6-v2` evaluated via pure-Rust `tract-onnx` by default with optional `ort` acceleration. Current implementation only executes under `#[cfg(all(feature = "embed-models", feature = "ort"))]` and bails without `ort`. |
| **OBJ-4** | **Medium** | `build.rs` (root) | Root `Cargo.toml` is a virtual workspace (no `[package]` section), so the root `build.rs` weight-validation script is **never executed** by Cargo. |
| **OBJ-5** | **Low** | `pdfmd-lite.toml`, root `*.log` / `*.txt`, `AGENTS.md:54` | `pdfmd-lite.toml` contains a hardcoded local workstation path (`output_dir = 'E:\Rust\NASTRAN\Reference\NASTRAN-95-master'`). Five UTF-16/text build logs (`check.log`, `check_output.txt`, `outputlog.txt`, `test.log`, `test_lib.log`) are committed at repo root. `AGENTS.md` links to `./docs/adr/0003-hardware-aware-concurrency-and-memory-budget.md`, while the actual filename is `0003-resource-aware-dynamic-concurrency.md`. |
| **OBJ-6** | **Pass** | Workspace-wide scan | **Zero runtime `std::env::var` calls** exist in `crates/engine`, `crates/cli`, `crates/server`, or `crates/desktop`, satisfying Non-Negotiable Rule #2. |

---

### 3.3 Offline Model Correctness, Loading & Execution

| ID | Severity | Location | Finding |
|---|---|---|---|
| **MOD-1** | **Critical** | `crates/engine/src/metadata/gliner.rs:27–30, 85–121`, `pipeline.rs:297` | `GlinerEngine` loads `gliner_small.onnx` into `session: Mutex<ort::session::Session>` marked `#[allow(dead_code)]` and **never runs ONNX inference**. `extract_metadata()` only runs regex/string heuristics. Meanwhile, `pipeline.rs:297` gates those heuristics behind `if let Some(gliner) = get_gliner_engine()`, so users without the 183 MB file lose even the zero-dependency regex metadata extraction. |
| **MOD-2** | **Critical** | `crates/engine/src/model_locator.rs:115–206, 215–254` | **Precedence inversion & doctor mismatch**: `load_bytes()` checks `get_embedded_slice()` *before* `self.locate()` when `custom_dir` is `None`, whereas `doctor_report()` prefers `disk_path` over `embedded`. Exe-adjacent `models/` weights are silently ignored by `load_bytes()` while `--doctor` reports them as active. Additionally, step 3 of ADR 0014 (platform AppData / `~/.local/share/pdfmd-lite/models`) is missing from `locate()`. |
| **MOD-3** | **Critical** | `crates/engine/src/figure/models.rs:113–116` | `LayoutModelSource::load_layout_bytes()` passes `self.custom_path` (an `.onnx` **file path** from `--layout-model` / `AppConfig.layout_model`) into `ModelLocator::new(custom_dir)`, which treats it as a **directory** and probes `<file_path>/layout/yolov8n-doclaynet.onnx`. Custom layout models silently fail to load. |
| **MOD-4** | **High** | `crates/engine/src/embeddings/engine.rs:202–228`, `pipeline.rs:480` | `resolve_embedding_paths()` bypasses `ModelLocator` completely, ignores `get_global_models_dir()` and `exe_dir/models`, and only probes CWD-relative paths. `pipeline.rs:480` calls `get_embedding_engine(None)` instead of passing `options.models_dir`. Also, `embed_batch()` runs batch size 1 in a loop while holding the `Mutex<Session>` lock for the entire batch. |
| **MOD-5** | **High** | `crates/engine/src/figure/latex.rs:71–94`, `slanet.rs:100–111`, `concurrency.rs:36` | `RapidLatexEngine::new()` eagerly allocates **2 encoder + 2 decoder sessions** (~268 MB) and `SlaNetEngine::new()` allocates **2 sessions**, while `concurrency.rs:36` still assumes a 350 MB total base footprint (and ADR 0011 explicitly mandates 1 encoder + 1 decoder session). On 8–16 GB RAM hosts or 4 GB VRAM GPUs, this risks OOM or thrashing. |
| **MOD-6** | **High** | `crates/engine/src/figure/models.rs:77–83`, `latex.rs:72,79`, `slanet.rs:101`, `gliner.rs:75`, `embeddings/engine.rs:55` | `allow_directml` is hardcoded per call site (`true` for LaTeX/GLiNER/Embeddings, `false` for SLANet). There is no central `ExecutionDevice` (`Auto` / `Cpu` / `DirectMl`) setting, no way for users on Windows 8.1 or unstable DX12 drivers to force CPU mode, and no logging indicating whether DirectML or CPU EP actually activated. |
| **MOD-7** | **High** | `crates/engine/src/vault.rs:457–545` | `download_embedding_models()`: (a) invokes `curl.exe` directly—fails on Linux/macOS and **Windows 8.1**; (b) omits `-f`/`--fail` so HTTP 404/503 responses are saved as corrupt `.onnx` files; (c) performs no SHA-256 checksum validation; (d) writes to CWD-relative `models/` instead of a writable target dir; (e) downloads the unused 183 MB `gliner_small.onnx`; (f) does not provision Indra/YOLO/SLANet/RapidLaTeXOCR weights needed for Slim MSI (ADR 0013). |
| **MOD-8** | **Medium** | `crates/engine/src/model_locator.rs:46–49, 79, 91–93` | `models/indra/order.onnx` is marked `required = true` in `KNOWN_MODELS` and embedded via `include_bytes!`, yet `IndraEngine` never loads or uses it (`xycut.rs` handles reading order). Meanwhile, `unimernet.onnx` and `bge_small_int8.onnx` appear in `KNOWN_MODELS` (making `--doctor` / ModelsModal show them missing) even though neither `latex.rs` nor `embeddings/engine.rs` attempts to load them. |
| **MOD-9** | **Medium** | `crates/engine/src/backends/indra/engine.rs:157` vs `243` | `IndraScript::World` applies `telugu_mask` in `transcribe_line()` (line 157), but passes `mask = None` in `transcribe_gray_with_script()` (line 243). Single-line and page-level batch transcription behave inconsistently for `IndraScript::World`. |
| **MOD-10** | **Medium** | `pipeline.rs:24`, `slanet.rs:53`, `latex.rs:23`, `gliner.rs:33`, `embeddings/engine.rs:20` | All neural engines are stored in process-wide `OnceLock` singletons. In long-lived Desktop (`tauri`) or Web Server (`axum`) sessions, changing `models_dir` or `device` in Settings cannot take effect until the entire process is restarted, and failed initializations (`OnceLock<Option<...>>` caching `None`) permanently disable an engine for the rest of the session even after the user provisions the missing model. |

---

### 3.4 UI Completeness & Desktop/Web Shell Ergonomics

| ID | Severity | Area | Finding & Gap |
|---|---|---|---|
| **UI-1** | **High** | `crates/desktop/tauri.conf.json:8` | `"beforeBuildCommand": "cd ../../ui && npm.cmd run build"` hardcodes Windows-only `npm.cmd`, breaking Linux/macOS Tauri builds. Also `"csp": null` disables Content Security Policy entirely in the WebView. |
| **UI-2** | **High** | `ModelsModal.tsx` ↔ `model_locator.rs` | Because `KNOWN_MODELS` includes unused `order.onnx` (as required) and unimplemented `unimernet.onnx` / `bge_small_int8.onnx`, and `load_bytes()` inverts precedence, the Models diagnostics modal reports misleading Ready/Missing states and cannot distinguish "actively loaded" vs "on disk" vs "embedded". |
| **UI-3** | **High** | `SettingsModal.tsx` ↔ `config.rs` & `OnceLock` | Settings UI lacks `device` (`auto` / `cpu` / `directml`) and `session_pool_size` controls, and changing `models_dir` at runtime silently fails to reload already-initialized `OnceLock` engines (no hot-reload and no "Restart required" banner). |
| **UI-4** | **Medium** | `SplitPane.tsx` & `MarkdownViewer.tsx` | Needs end-to-end verification of source PDF page canvas streaming (`previewUrl`), page-number synchronization between PDF canvas and `<!-- Page N -->` anchors in Markdown, and zoom/pan ergonomics across both Tauri IPC and Axum WebSocket transports. |
| **UI-5** | **Medium** | `BatchQueue.tsx` & `LogConsole.tsx` | In air-gapped workflows, users need per-file degradation badges (e.g., *"Formula OCR unavailable — fell back to text OCR"*, *"SLANet missing — used Geometric Table Grid"*, *"Hybrid DOM active"*) instead of burying model fallback warnings inside `LogConsole`. |
| **UI-6** | **Medium** | `MenuBar.tsx`, `ShortcutsModal.tsx`, `App.tsx` | Audit parity between Tauri Desktop and Axum Web (`--serve`) in `bridge/api.ts`: file/folder pickers (native `rfd` dialog in Tauri vs browser file/path input in Axum), drag-and-drop multi-PDF ingestion, cancellation of active batch jobs, and keyboard shortcut conflicts with browser defaults when running under `--serve`. |

---

## 4. Phased Implementation Plan, Tests & Passing Gates

Each phase is self-contained and **must pass its Verification Gate** before proceeding to the next phase.

---

### Phase A: Single-Page Reading Order & Text Parsing Stabilization (P0 — Immediate Priority)

**Objective:** Eliminate reading-order swaps, dropped/duplicated lines, and garbled text on simple single-page PDFs (both digital DOM and scanned OCR paths) before touching downstream features.

#### Implementation Tasks
1. **Gate 0 — Add Deterministic Layout Trace (`--debug-layout <dir>`)**:
   - Emit a per-page JSON sidecar (`page_001_trace.json`) capturing:
     - `page_box` (`MediaBox`, `CropBox`, rotation),
     - raw DOM spans (text, font size, normalized top-left bbox, CMap quality score),
     - raw YOLO boxes (class, confidence, bbox) + post-NMS boxes,
     - XY-Cut++ tree splits (horizontal vs. vertical cut coordinates and resulting block order),
     - final ordered blocks sent to `markdown.rs`.
   - Run this trace on 3–5 failing simple single-page PDFs to pinpoint whether the defect originates in `pdf.rs`, `yolo.rs`, `xycut.rs`, `indra`, or `markdown.rs`.
2. **Harden Hybrid DOM Extraction (`crates/engine/src/pdf.rs`)**:
   - Normalize all coordinates strictly against `CropBox` and page `/Rotate` ($0^\circ, 90^\circ, 180^\circ, 270^\circ$) into a single canonical top-left coordinate system before any grouping or YOLO matching.
   - Cluster glyphs/spans into lines using a **font-size-relative vertical tolerance** (e.g., baseline delta $|\Delta y| \le 0.35 \cdot \min(h_1, h_2)$ or vertical IoU $\ge 0.5$) rather than a fixed pixel threshold, then sort spans strictly left-to-right by $x_{\min}$ within each line.
   - Compute inter-word space insertion dynamically from each span's average character width / space advance width (e.g., gap $> 0.22 \times \text{font\_size}$ inserts a single space), preventing word gluing and mid-word splitting.
   - Add a **DOM text sanity gate**: compute the ratio of invalid/unmapped characters (`\u{FFFD}`, PUA `U+E000..U+F8FF`, control chars, or excessive isolated single-character tokens). If the ratio exceeds a strict threshold (e.g., $> 5\%$), automatically fall back that page (or region) to Indra OCR and log a structured warning.
3. **Fix XY-Cut++ Single-Column Guard & Overlap Handling (`crates/engine/src/xycut.rs`)**:
   - Before attempting a vertical (column) split, verify that the page/subtree actually exhibits a **true multi-column gutter**:
     - Gutter width must exceed a minimum fraction of page width (e.g., $\ge 2.5\%$ of page width and $\ge 1.5 \times$ median line height),
     - Both left and right partitions must contain sustained vertical text mass (not just a single indented line, bullet marker, or right-aligned date/page number).
   - Shrink block vertical bounds slightly (e.g., by $10\%$ of line height) during projection-profile computation so 1–2 px ascender/descender overlaps never block a horizontal cut on single-column text.
   - Sort single-column blocks cleanly by `(y_center_bucket, x_min)` rather than raw `y_min` to tolerate minor baseline jitter.
4. **Harden YOLO Region Assembly & Deduplication (`crates/engine/src/figure/yolo.rs`, `pipeline.rs`)**:
   - Apply **class-agnostic NMS / containment pruning** (e.g., if a `Text` box is $> 80\%$ contained inside a `Table`, `Formula`, or higher-confidence `Text` box, suppress the duplicate so text is never extracted twice).
   - Implement **orphan-span rescue**: any valid DOM line (or Indra detected line) whose center falls outside all YOLO boxes must be preserved as a `Text` block in its true geometric position before XY-Cut++ runs, rather than being dropped or appended at the end.
5. **Stabilize Markdown Line-Joining (`crates/engine/src/markdown.rs`)**:
   - Preserve explicit line breaks for short standalone lines (headings, address blocks, key-value lines, list items) instead of greedily collapsing them into a single paragraph.
   - Only de-hyphenate line endings (`word-\nnext` $\to$ `wordnext`) when the next line starts with a lowercase letter and the hyphen is not part of a known compound/code token.

#### Required Tests (`crates/engine/tests/reading_order_parsing.rs`)
- `test_single_page_simple_prose_order`: synthetic single-page PDF with title, 3 indented paragraphs, and a bullet list; asserts exact top-to-bottom paragraph order and zero concatenated/split words.
- `test_single_page_out_of_order_pdf_stream`: PDF whose content stream draws paragraph 3 before paragraph 1 and splits a word across kerning `TJ` operators; asserts canonical visual reading order and clean word spacing.
- `test_single_page_rotated_90_and_cropbox_offset`: PDF with non-zero `CropBox` origin and `/Rotate 90`; asserts text is not clipped or inverted.
- `test_xycut_does_not_split_indented_single_column`: unit test on `xycut.rs` with centered title, hanging-indent bullets, and right-aligned date; asserts zero phantom vertical splits and strict top-to-bottom order.
- `test_xycut_true_two_column_order`: unit test with spanning header + 2 balanced columns + spanning footer; asserts `Header -> Col 1 -> Col 2 -> Footer`.
- `test_corrupt_cmap_triggers_ocr_fallback`: PDF with broken `/ToUnicode` emitting `\u{FFFD}` / PUA glyphs; asserts DOM sanity gate rejects it and triggers OCR fallback.
- `test_yolo_overlap_deduplication_and_orphan_rescue`: overlapping text boxes do not duplicate output; an unboxed title line is rescued in its true top position.

#### Phase A Passing Gate
```powershell
cargo test --workspace -- reading_order xycut pdf_dom
cargo clippy --workspace --all-targets -- -D warnings
```
*Gate criteria:* 100% pass rate on all single-page, rotated, out-of-order stream, two-column, and corrupt-CMap fixtures; `--debug-layout` produces clean, verifiable order traces on the user's real single-page PDFs with zero reading-order inversions or word-spacing defects.

---

### Phase B: Model Locator, Cargo Features & Build Hygiene (P0)

**Objective:** Make clean builds succeed without bundled weights, fix ADR 0014 model precedence, fix `--layout-model`, and clean up repository hygiene.

#### Implementation Tasks
1. **Decouple Cargo features (`crates/engine/Cargo.toml`)**:
   - Split `embed-models` into `tokenizers-support` (enables `dep:tokenizers` for MiniLM) and `bundle-weights` (opt-in feature that enables `include_bytes!` for standalone single-binary builds; **OFF** by default).
   - Keep `default = ["slanet-ort", "latex-ort", "gliner-ort", "tokenizers-support", "embed-ort"]` and gate `directml` under Windows target dependencies so Linux/macOS and Windows 8.1 CPU builds stay clean.
2. **Fix `ModelLocator` (`crates/engine/src/model_locator.rs`)**:
   - Enforce strict ADR 0014 precedence in **both** `locate()` and `load_bytes()`:
     1. Explicit `--models-dir` / `AppConfig.models_dir`
     2. Executable-adjacent `<exe_dir>/models`
     3. Platform user data dir (`%LOCALAPPDATA%\pdfmd-lite\models` via Win32 `SHGetKnownFolderPath` / `dirs` crate — zero `std::env::var` reads)
     4. Dev workspace fallback (`#[cfg(debug_assertions)]` only: `./models`, `../../models`)
     5. Embedded static slices (`#[cfg(feature = "bundle-weights")]`)
   - Ensure `doctor_report()` and `load_bytes()` call the **exact same** resolution helper so `--doctor` never contradicts runtime behavior.
   - Remove `order.onnx` from `KNOWN_MODELS` and `EMBEDDED_ORDER`. Remove phantom `unimernet.onnx` and `bge_small_int8.onnx` from `KNOWN_MODELS` until inference loaders actually exist.
3. **Fix `LayoutModelSource` (`crates/engine/src/figure/models.rs`)**:
   - In `load_layout_bytes()`, if `self.custom_path` points to an existing file (`path.is_file()`), read that file directly instead of passing it to `ModelLocator::new(custom_dir)`.
4. **Fix `embeddings/engine.rs` & `pipeline.rs`**:
   - Route `resolve_embedding_paths()` through `ModelLocator::new(custom_dir)`.
   - Pass `options.models_dir.as_deref()` into `get_embedding_engine(...)` at `pipeline.rs:480`.
5. **Repo Hygiene**:
   - Remove dead root `build.rs`.
   - Replace hardcoded personal path in `pdfmd-lite.toml` with commented defaults.
   - Delete committed log files (`check.log`, `check_output.txt`, `outputlog.txt`, `test.log`, `test_lib.log`) and add `*.log` to `.gitignore`.
   - Fix broken ADR 0003 link in `AGENTS.md`.

#### Required Tests
- `test_model_locator_precedence_custom_over_exe_over_embedded`: temp dir with custom file overrides exe-adjacent and embedded; `doctor_report()` matches `load_bytes()` source.
- `test_layout_model_source_accepts_direct_file_path`: passing a temp `.onnx` file to `LayoutModelSource::new(Some(file_path))` loads exact file bytes.
- `test_embedding_paths_respect_global_models_dir`: `set_global_models_dir(temp)` resolves `model.onnx` + `tokenizer.json` without CWD dependency.

#### Phase B Passing Gate
```powershell
cargo check --workspace --all-targets --no-default-features
cargo check --workspace --all-targets
cargo test --workspace -- model_locator layout_model embedding_paths
cargo clippy --workspace --all-targets -- -D warnings
```
*Gate criteria:* Clean build succeeds on a checkout **without** `models/` present; 0 clippy warnings; all new unit tests pass.

---

### Phase C: Offline Model Correctness, Device Control & Memory Budgeting (P0)

**Objective:** Eliminate the GLiNER no-op, wire `--device` across all ORT engines, align session pools with the RAM budget, fix Indra script masking consistency, and make engines reloadable.

#### Implementation Tasks
1. **Fix GLiNER (`crates/engine/src/metadata/gliner.rs` & `pipeline.rs:297`)**:
   - **Immediate decoupling**: Extract the zero-dependency regex/heuristic metadata extractor (`extract_metadata_heuristic()`) so `pipeline.rs` and `vault.rs` **always** run it regardless of whether `gliner_small.onnx` is present.
   - **Neural decision**: Either (a) implement actual ONNX tokenization + span-scoring inference on `self.session` (with a unit test asserting non-empty entity output on a fixture header) OR (b) remove `gliner_small.onnx` loading/downloading until the tokenizer + span decoder are implemented, avoiding a 183 MB dead allocation.
2. **Add `ExecutionDevice` (`Auto` / `Cpu` / `DirectMl`) per ADR 0015**:
   - Add `pub device: Option<String>` to `AppConfig` (`config.rs`) and `--device <auto|cpu|directml>` to `cli.rs`.
   - Store process-wide `ExecutionDevice` (default `Auto`) and pass it into `create_optimized_session_builder()` across **all** ORT engines (`latex.rs`, `slanet.rs`, `gliner.rs`, `embeddings/engine.rs`), eliminating hardcoded `true`/`false` flags.
   - Log the resolved execution provider at `tracing::info!` level on session creation.
3. **Align Session Pools & Concurrency Governor (`latex.rs`, `slanet.rs`, `concurrency.rs`)**:
   - Default `RapidLatexEngine` and `SlaNetEngine` pool size to **1** on hosts with $< 8\text{ GB}$ available RAM (preserving ADR 0011's 350 MB budget) and scale to `2` only when available RAM $\ge 8\text{ GB}$ or explicitly configured via `AppConfig`.
   - Update `concurrency.rs` base memory calculation to account for active ORT pools dynamically.
4. **Fix `IndraScript::World` Masking (`crates/engine/src/backends/indra/engine.rs:243`)**:
   - Align `transcribe_gray_with_script()` with `transcribe_line()` so `IndraScript::Telugu` and `IndraScript::World` apply consistent masking semantics.
5. **Replace `OnceLock<Option<...>>` with Reloadable Engine Registry**:
   - Wrap engine singletons in `RwLock<Option<Arc<...>>>` (keyed by `(models_dir, device)`) with an explicit `invalidate_model_caches()` API so Tauri/Axum Settings changes take effect without restarting and missing-model `None` states are not permanently cached.
6. **True MiniLM Batching (`crates/engine/src/embeddings/engine.rs`)**:
   - Pad sequences to batch max length (capped at 512) and run batched ORT inference (batch size 8–16) while releasing the mutex between batches.

#### Required Tests
- `test_heuristic_metadata_runs_without_gliner_model`: verifies DOI, date, author, org, journal extraction in `pipeline` when `gliner_small.onnx` is absent.
- `test_config_parses_device_option`: validates `device = "cpu" | "directml" | "auto"` in `AppConfig` and rejects invalid values.
- `test_indra_world_script_consistency`: asserts `transcribe_line` and `transcribe_gray_with_script` use identical mask selection for `Telugu` and `World`.
- `test_engine_registry_reloads_on_models_dir_change`: verifies cache invalidation reloads from a new directory.
- `test_minilm_batch_matches_single_embeddings`: cosine similarity $> 0.999$ between `embed_text` and `embed_batch` (guarded if model file present).

#### Phase C Passing Gate
```powershell
cargo test --workspace
cargo check --benches
cargo clippy --workspace --all-targets -- -D warnings
```
*Gate criteria:* All existing + new tests pass; zero `#[allow(dead_code)]` on ONNX `Session` fields; `--device cpu` runs end-to-end with zero DirectML calls.

---

### Phase D: Safe, Portable Offline Model Provisioner (P1)

**Objective:** Replace the unsafe `curl.exe` shell-out in `vault.rs` so Slim MSI and cross-platform users can provision and verify models reliably while keeping conversion 100% offline (ADR 0001).

#### Implementation Tasks
1. **Refactor `download_embedding_models()` $\to$ `provision_models()` (`crates/engine/src/vault.rs`)**:
   - Resolve target directory via `ModelLocator` (explicit `--models-dir` $\to$ writable user data dir, never hardcoded CWD).
   - Replace hardcoded `curl.exe` with cross-platform detection (`curl` on Unix/Win10+, PowerShell `Invoke-WebRequest` fallback on Windows 8.1, or an opt-in `provision` feature) with strict failure flags (`curl -fsSL --retry 3`).
   - Download into `<file>.tmp`, verify expected **SHA-256 checksum** from a versioned manifest table, and atomically rename to `<file>` only on hash match.
   - Support selective profiles: `--download-models core` (Indra + YOLO for Slim MSI), `--download-models embeddings` (MiniLM), `--download-models math-tables` (RapidLaTeXOCR + SLANet), `--download-models all`.
   - Remove automatic download of `gliner_small.onnx` unless neural GLiNER inference is implemented in Phase C.

#### Required Tests
- `test_checksum_verification_rejects_corrupt_download`: simulates an HTTP 404 HTML payload written to `.tmp`, asserts SHA-256 check fails and leaves no corrupt `.onnx` file behind.
- `test_provision_target_dir_respects_custom_models_dir`: verifies writes go to configured dir, not CWD.

#### Phase D Passing Gate
```powershell
cargo test --workspace -- provision checksum
cargo clippy --workspace --all-targets -- -D warnings
```
*Gate criteria:* Corrupt/HTML responses never overwrite or create `.onnx` files; zero `curl.exe` hardcoding remains; `pdfmd-lite --doctor` reports valid SHA-256 for all provisioned models.

---

### Phase E: UI Completeness, Tauri/Axum Parity & UX Ergonomics (P1)

**Objective:** Audit and close all frontend/backend contract gaps across `ui_src/`, `crates/desktop/`, and `crates/server/`.

#### Implementation Tasks
1. **Gate 0 — Line-by-Line UI/Server Contract Audit**:
   - Inspect `ui_src/src/bridge/api.ts`, `App.tsx`, `SplitPane.tsx`, `BatchQueue.tsx`, `SettingsModal.tsx`, `ModelsModal.tsx`, `MenuBar.tsx`, `LogConsole.tsx`, `crates/desktop/src/main.rs`, and `crates/server/src/lib.rs` to build a 1:1 endpoint matrix (Tauri `invoke` commands vs. Axum REST/WS routes).
2. **Cross-Platform Tauri Shell & Security (`crates/desktop/tauri.conf.json`)**:
   - Replace `"npm.cmd run build"` with portable `"npm run build"`.
   - Replace `"csp": null` with a strict offline CSP (`default-src 'self'; img-src 'self' data: blob: asset:; style-src 'self' 'unsafe-inline'; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:*`).
3. **Complete Settings & Models UX**:
   - Expose `device` (`Auto` / `CPU` / `DirectML`), `dom_extraction` (`Auto` / `Always` / `Never`), `embed` (MiniLM sidecars), and `resume` (SHA-256 idempotency) in `SettingsModal.tsx` and wire them into `AppConfig` save/load.
   - Trigger backend `invalidate_model_caches()` when `models_dir` or `device` changes in `SettingsModal.tsx`, displaying a non-blocking toast confirming reload.
   - Upgrade `ModelsModal.tsx` to display **Source** (`Custom Dir` / `App Dir` / `User Data` / `Embedded` / `Missing`), **Size**, **SHA-256 prefix**, **Active EP** (`DirectML` / `CPU` / `tract-onnx`), and clear **Offline Air-Gapped Import** instructions ("Place files in `<resolved_path>` and click Rescan").
4. **UX Ergonomics & Degradation Visibility**:
   - Add per-job status pills in `BatchQueue.tsx` showing active pipeline mode (`Hybrid DOM` vs `Indra OCR`, `SLANet` vs `Geometric Grid`, `LaTeX OCR` vs `Text Fallback`) so users immediately see why a page rendered a certain way without digging through `LogConsole.tsx`.
   - Ensure `SplitPane.tsx` synchronizes page navigation between the rendered PDF canvas and `<!-- Page N -->` anchors in `MarkdownViewer.tsx`, with clean empty/loading/error states.
   - Ensure active batch jobs support graceful **Cancel** from both Tauri and Axum WebSocket transports.
   - Audit `ShortcutsModal.tsx` vs `App.tsx` keybindings to prevent collisions with browser shortcuts (`Ctrl+W`, `Ctrl+R`, `Ctrl+T`) in `--serve` mode.

#### Required Tests
- **Backend API parity tests (`crates/server`)**: Axum `tower::ServiceExt::oneshot` integration tests for `/api/config`, `/api/models`, `/api/convert`, `/api/batch`, and WebSocket progress streaming.
- **Frontend unit/component tests (`ui/`)**: Add Vitest + Testing Library smoke tests for `bridge/api.ts` (mocking Tauri vs Axum transport), `SettingsModal` round-trip serialization, `ModelsModal` status rendering, and `SplitPane` page sync.

#### Phase E Passing Gate
```powershell
cd ui && npm run typecheck && npm run build && npm test
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
*Gate criteria:* Zero TypeScript errors; `ui/dist` builds cleanly on Windows and Linux; Tauri and Axum expose identical config/model/progress schemas; all frontend and backend tests pass.

---

### Phase F: End-to-End Regression Suite & Release Readiness (P2)

**Objective:** Lock in offline determinism, reading-order fidelity, memory bounds, and Windows/Linux CI verification.

#### Implementation Tasks
1. **Golden Corpus Regression Suite (`crates/engine/tests/`)**:
   - Simple single-page digital PDF (prose, headings, bullets, indents) $\to$ exact reading order & spacing assertion.
   - Digital scientific PDF (with `/ToUnicode` CMap + superscripts/subscripts) $\to$ verifies Hybrid DOM activation and clean `^`/`_` output.
   - Scanned / missing-CMap PDF $\to$ verifies automatic fallback to Indra OCR.
   - Multi-column PDF + table + formula $\to$ verifies XY-Cut++ reading order, `<!-- Page N -->` markers, and Hybrid Table Serialization (GFM + HTML callout per ADR 0016).
   - Incremental `--resume` test $\to$ verifies second run skips unchanged PDF via `pdf_sha256` frontmatter and re-converts on byte modification.
2. **Release Packaging Check**:
   - Validate WiX v5 manifests (`wix/main.wxs` for Slim MSI, `wix/full.wxs` for Full Offline MSI) match the updated `KNOWN_MODELS` manifest and omit unused `order.onnx` / unimplemented weights.

#### Phase F Passing Gate
```powershell
cargo test --workspace --all-targets
cargo test --workspace --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
cargo check --benches
```
*Gate criteria:* 100% pass rate across both default and `--no-default-features` builds, zero compiler/clippy warnings, and verified `--doctor` + single/batch conversion on a clean offline machine.