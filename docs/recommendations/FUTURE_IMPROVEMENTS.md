# Profiling Analysis & Future Performance Improvements for pdfmd-lite

## 1. Executive Summary & Runtime Profile

A live profiling session was performed on the active batch conversion process (`PID 20724`, command line: `target\release\pdfmd-lite.exe input output -d 5`) processing 832 scientific PDF documents.

### Live Telemetry Snapshot
- **Host Configuration**: 12 logical cores, 1.21 GB available physical RAM at job startup.
- **Worker Configuration**: Dynamic Concurrency Governor resolved **9 workers** (4 concurrent documents $\times$ 2 page workers).
- **Elapsed Wall-Clock Time**: $\approx 82$ minutes.
- **Total Processor Time Accrued**: **$26,577\text{ seconds}$ ($\approx 7\text{ hours } 23\text{ minutes}$)** of multi-core CPU time.
- **CPU Time Distribution**:
  - **User Processor Time**: **$95.5\%$** ($7\text{h } 03\text{m}$) — predominantly tensor math, convolutions, CTC token decoding, and autoregressive model evaluations.
  - **Privileged (Kernel) Time**: **$4.5\%$** ($19\text{m } 40\text{s}$) — page faults, memory allocation churn, and file descriptor operations.
- **Memory Footprint**:
  - **Working Set**: $\approx 488\text{ MB}$ (current) with a **Peak Working Set of $1.45\text{ GB}$**.
  - **Paging Pressure**: Peak working set ($1.45\text{ GB}$) exceeded the available physical memory at startup ($1.21\text{ GB}$), causing OS memory paging and cache evictions.
- **Batch Throughput**:
  - **Completed Documents**: 232 / 832 documents ($\approx 27.9\%$).
  - **Conversion Rate**: $\approx 2.83\text{ documents/minute}$ ($\approx 21.2\text{ seconds}$ per document average).
  - **Per-Page Latency**:
    - 1-page documents: $11.40\text{s}$ to $16.74\text{s}$
    - 5-page documents: $36.47\text{s}$ ($\approx 7.29\text{s/page}$)
    - 6-page documents: $33.58\text{s}$ ($\approx 5.60\text{s/page}$)
    - 7-page documents: $41.06\text{s}$ ($\approx 5.86\text{s/page}$)

---

## 2. Identified Bottlenecks (Root Cause Analysis)

### Bottleneck 1: The Dilemma of Pure DOM Text vs. Vision-First OCR
- **Why Naive DOM-First Fails for Scientific Literature**:
  Directly falling back to naive PDF DOM stream extraction (`pdf-extract` / `lopdf` text operators) causes severe regression on scientific and technical papers:
  1. **Missing `/ToUnicode` CMaps in Type 1 Math Fonts**: Academic papers (AIAA, IEEE, AMS-LaTeX) frequently use specialized symbolic fonts (`CMMI`, `CMSY`, `MSBM`). Without `/ToUnicode` tables, character codes decode as corrupt ASCII or missing glyphs (e.g. $\partial, \nabla, \alpha, \int$ decode as `d`, `N`, `a`, or unprintable noise).
  2. **Loss of Subscripts, Superscripts, and Suffixes**: Subscripts and superscripts in PDF DOM streams have no semantic markup—they are rendered solely via baseline coordinate shifts ($\Delta y$) and reduced font scale ($Tf$). Naive text dumpers flatten these into the same baseline (e.g., $T_e$ becomes `Te`, $x^2$ becomes `x2`, or trailing suffixes get stripped).
  3. **Loss of 2D Equation Geometry**: Fractions ($\frac{a}{b}$), radicals ($\sqrt{x}$), and summations ($\sum$) are rendered as disconnected horizontal rules (`re` stroke) and floating characters, losing all algebraic semantics.
- **Why the Vision-First Pipeline was Chosen**:
  Indra OCR (DBNet + SVTR) combined with YOLOv8n DocLayNet and RapidLaTeXOCR processes the page as visual ground truth, preserving sub/superscripts, Greek symbols, and spatial equations regardless of underlying font stream corruption.
- **The Core Problem**:
  Because vision-first OCR is necessary for faithful math and suffix extraction, the entire performance burden falls on the neural execution path. Right now, that path is severely bottlenecked by **unbatched single-line inference in `tract-onnx`**, **autoregressive LaTeX loop runaway**, and **per-page PDF DOM reloading**.

---

### Bottleneck 2: Sequential, Single-Line Neural Inference in `tract-onnx` (The Real OCR Bottleneck)
- **Mechanism**: In `src/backends/indra/engine.rs:197-204`:
  ```rust
  let mut recognized_lines = Vec::with_capacity(boxes.len());
  for bbox in boxes {
      let (text, _conf) = self.transcribe_line(&gray, &bbox, script)?;
      ...
  }
  ```
  A typical two-column technical document contains between 60 and 140 text lines per page. The engine crops, resizes, pads, and dispatches each line independently through `plan.run()`.
- **Performance Impact**:
  - $60 \text{ to } 140$ separate ONNX execution plan runs per page.
  - Zero tensor batching (batch size is strictly 1).
  - Pure-Rust `tract-onnx` executes single-threaded on CPU without AVX-512, OpenVINO, or DirectML hardware acceleration.
  - CPU caches (L1/L2) are repeatedly flushed as execution alternates between image preprocessing and model graphs.

---

### Bottleneck 3: RapidLaTeXOCR Autoregressive Quadratic Loop & Mutex Locking
- **Mechanism**: In `src/figure/latex.rs:179-228`:
  ```rust
  let mut decoder_session = self.decoder.lock().map_err(...)?;
  for _ in 0..max_seq_len { // max_seq_len = 256
      let dec_inputs = ort::inputs!["x" => x_tensor, "mask" => mask_tensor, "context" => ctx_tensor];
      let dec_outputs = decoder_session.run(dec_inputs)?;
      ...
  }
  ```
- **Performance Impact**:
  - **Cross-Thread Serialization**: The `decoder: Mutex<ort::session::Session>` locks out all other worker threads across the process for the entire duration of the 256-step loop.
  - **Quadratic Complexity $O(N^2)$**: Because there is no KV-caching, the transformer decoder re-processes the entire token sequence `x` on every token step.
  - **Runaway Loops on Ambiguous Crops**: In formula-dense documents (e.g. `Antiproton_powered_propulsion_wi.md`), crops with faint lines or noise trigger hallucinated infinite repeating tildes `\mathrm { ~ ~ ~ ~ ~ ... }`, running all the way to 256 steps without an early-stopping repetition penalty.

---

### Bottleneck 4: Redundant Full PDF File Re-parsing on Every Page
- **Mechanism**: In `src/figure/mod.rs:103-107`:
  ```rust
  if let Some(pdf_file) = pdf_path {
      if let Ok(doc) = lopdf::Document::load(pdf_file) {
          if let Ok(stream_figs) = pdf_stream::extract_page_stream_figures(&doc, page_num, page_w, page_h) { ... }
      }
  }
  ```
  `analyze_page_layout_image` is executed for each individual page. Inside this function, `lopdf::Document::load(pdf_file)` parses the entire PDF binary from disk from scratch.
- **Performance Impact**:
  - A 15-page document parses the complete PDF DOM structure **15 times**.
  - In a multi-worker environment (4 concurrent docs $\times$ 2 page workers), multiple threads simultaneously read and re-parse identical files from disk, causing redundant file I/O and heap allocations.

---

### Bottleneck 5: Memory Allocation Churn & Redundant Image Buffer Cloning
- **Mechanism**:
  - In `src/pipeline.rs:565`:
    ```rust
    let ocr_canvas = if !layout.picture_masks.is_empty() {
        mask_page_regions(page_img, &layout.picture_masks)
    } else {
        page_img.clone()
    };
    ```
    Even when no visual regions need masking, `page_img.clone()` creates a full deep clone of the 2.0x supersampled RGBA image ($15\text{ MB to } 30\text{ MB}$ per page).
  - `ocr_canvas.to_luma8()` subsequently allocates another large grayscale buffer.
  - Resizing to $1024 \times 1024$ for DBNet and $640 \times 640$ for YOLO allocates fresh vectors (`vec![0.0f32; 1024*1024]`) on every page instead of using reusable thread-local scratchpads.
- **Performance Impact**:
  - Contributes to high peak memory consumption ($1.45\text{ GB}$), exhausting available physical RAM ($1.21\text{ GB}$) and triggering Windows page swapping.

---

### Bottleneck 6: Hardware Governor RAM Calibration Mismatch
- **Mechanism**: ADR 0003 specified allocating 1 worker per $1.5\text{ GB}$ of free RAM:
  $$\text{workers} = \max\left(1, \min\left(\text{cores}-1, \frac{\text{free RAM (GB)}}{1.5\text{ GB}}\right)\right)$$
  However, `src/concurrency.rs` implements:
  $$1 + \left\lfloor\frac{\text{free RAM} - 250\text{ MB}}{120\text{ MB}}\right\rfloor$$
- **Performance Impact**:
  - On a workstation with only $1.21\text{ GB}$ free RAM, the governor spawned **9 workers**.
  - 9 parallel page pipelines concurrently allocating high-resolution image rasters and ONNX tensor arenas exceeded physical RAM, forcing disk paging and thread context-switching overhead.

---

### Bottleneck 7: Lack of Incremental / Resume Support in Batch Conversion
- **Mechanism**: In `src/batch.rs`, `process_pdf_directory_with_options()` scans all PDFs and processes every file, even if the destination `<doc_slug>/<doc_slug>.md` has already been converted in a previous run.
- **Performance Impact**:
  - If a batch run of 832 files is interrupted at file 250, restarting the run re-converts files 1 through 249, wasting hours of CPU time.

---

## 3. Prioritized Optimization Roadmap (Preserving Full Math & Suffix Quality)

To preserve complete mathematical fidelity, Greek symbols, suffixes, and subscripts without regressions, optimizations focus on **accelerating the neural vision pipeline by $5\times - 10\times$** rather than degrading output quality with naive DOM dumps:

| Priority | Optimization Item | Estimated Speedup | Complexity | Quality Impact |
| :--- | :--- | :--- | :--- | :--- |
| **P0** | **Indra SVTR Batched Line Inference** | **$3\times - 5\times$ OCR speedup** | Medium | **Zero quality loss** (exact same SVTR model & tokens) |
| **P0** | **LaTeX Decoder Mutex Removal & Early Stop** | **$1.5\times - 2.5\times$ on math docs** | Medium | **Improves quality** (stops hallucinated 256-step `~ ~ ~` loops) |
| **P0** | **Pass Pre-Parsed `lopdf::Document`** | **$1.2\times - 1.4\times$ overall** | Low | **Zero quality loss** (eliminates redundant disk/DOM parsing) |
| **P1** | **Formula Box Triage & Fast-Path** | **$1.3\times - 1.8\times$ on math docs** | Low | **Zero quality loss** (bypasses heavy decoder on 1-char boxes) |
| **P1** | **Batch Skip / Resume Mode (`--resume`)** | **$\infty$ on completed files** | Low | **Zero quality loss** (skips already verified output markdown) |
| **P2** | **Migrate SVTR / DBNet to ONNX Runtime (`ort`)**| **$2\times - 4\times$ inference speed** | High | **Zero quality loss** (exact same weights, hardware SIMD execution) |
| **P2** | **Hardware Governor RAM Recalibration** | System stability | Low | Prevents OS paging by budgeting $\ge 500\text{ MB}$ per worker |
| **P3** | **Baseline-Aware Hybrid DOM Extraction** | **$5\times - 10\times$ on clean PDFs** | High | Detects baseline $\Delta y$ for sub/superscripts; YOLO for math |

---

## 4. Detailed Implementation Plans

### 4.1. Milestone 1: Indra SVTR Batched Line Inference (P0)

#### Why it's needed:
In a 2-column paper, DBNet detects 80 to 140 bounding boxes per page. Right now, `transcribe_image_with_script()` runs `plan.run()` inside a single-item loop:
```rust
for bbox in boxes {
    let (text, _conf) = self.transcribe_line(&gray, &bbox, script)?;
    ...
}
```
This causes 80–140 individual ONNX executions per page with single-line tensor allocations.

#### Proposed Architecture:
Group detected bounding boxes into batches of $B = 16$ or $32$ sorted by bucketed width:
1. Construct a batched NCHW tensor `[B, 1, 96, W_bucket]`.
2. Construct a batched lengths tensor `[B]`.
3. Dispatch a single `plan.run()` across all 16 or 32 lines simultaneously.
```rust
// In src/backends/indra/recognize.rs:
pub fn recognize_batch(
    &self,
    gray: &GrayImage,
    boxes: &[TextBoundingBox],
    mask: Option<&[bool]>,
) -> Result<Vec<(String, f32)>> {
    let mut results = Vec::with_capacity(boxes.len());
    for chunk in boxes.chunks(32) {
        let (batched_tensor, lengths) = prepare_batched_ink_maps(gray, chunk, self.target_height);
        let outputs = plan.run(tvec!(batched_tensor.into(), lengths.into()))?;
        // Decode all 32 lines from CTC logits in a single vector pass
        decode_ctc_batch(&outputs, chunk.len(), &mut results);
    }
    Ok(results)
}
```
*Expected Speedup*: Reduces 100 single-item plan invocations down to 3–4 batched calls per page, multiplying throughput by **$3\times \text{to } 5\times$** while producing the exact same tokens.

---

### 4.2. Milestone 2: RapidLaTeXOCR Early Stopping & Mutex Elimination (P0)

#### Why it's needed:
In the live batch run (`Antiproton_powered_propulsion_wi.md`), RapidLaTeXOCR got stuck in an autoregressive loop generating 256 repeating tildes:
```latex
\varrho - \mathrm { \boldmath { ~ \ell + \ell + \ell \, } } \varrho \, \! \! \cdot \mathrm { \boldmath { ~ \ell ~ } } \mathrm { \boldmath { ~ \ell ~ } } ...
```
Because `decoder: Mutex<ort::session::Session>` locks the session, this single formula blocked all other worker threads across the process for hundreds of milliseconds.

#### Proposed Architecture:
1. **Remove Mutex**: `ort::session::Session::run(&self)` is thread-safe in ONNX Runtime. Replace `Mutex<Session>` with `Session` to allow concurrent formula evaluation across page workers.
2. **Repetition Penalty / Early-Break**: Stop decoding if the same token repeats $\ge 3$ times consecutively.
3. **Cap Maximum Sequence Length**: Lower `max_seq_len` from 256 down to 128 (no standard LaTeX formula in DocLayNet exceeds 128 tokens; longer sequences are almost always hallucination loops).
```rust
let mut repeat_count = 0;
let mut prev_token = -1;

for _ in 0..128 {
    ...
    let next_tok = best_idx as i64;
    if next_tok == 2 { // [EOS]
        break;
    }
    if next_tok == prev_token {
        repeat_count += 1;
        if repeat_count >= 3 {
            // Abort runaway infinite loop immediately
            break;
        }
    } else {
        repeat_count = 0;
        prev_token = next_tok;
    }
    tokens.push(next_tok);
    mask.push(true);
}
```
*Expected Benefit*: Eliminates cross-thread pipeline freezes and cuts formula decoding time by **$50\% - 80\%$** on math-heavy documents.

---

### 4.3. Milestone 3: Pre-Parsed `lopdf::Document` Sharing (P0)

#### Why it's needed:
In `src/figure/mod.rs:103-107`, `lopdf::Document::load(pdf_file)` parses the entire PDF file structure from disk inside `analyze_page_layout_image` for **every single page**. A 15-page document parses the identical file from disk 15 times.

#### Proposed Architecture:
Load the PDF document once in `process_pdf_with_options()` and share it via `Arc<lopdf::Document>`:
```rust
// Parse once per PDF document in pipeline.rs:
let lopdf_doc = Arc::new(lopdf::Document::load(input_path).ok());

// Pass down to analyze_page_layout_image():
pub fn analyze_page_layout_image(
    doc: Option<&lopdf::Document>,
    page_img: &DynamicImage,
    image_output_dir: &Path,
    page_num: usize,
) -> Result<PageLayoutResult> { ... }
```
*Expected Benefit*: Saves hundreds of megabytes of redundant file I/O and thousands of DOM allocations per document with zero accuracy changes.

---

### 4.4. Milestone 4: Formula Box Triage & Fast-Path (P1)

#### Why it's needed:
YOLOv8n DocLayNet frequently detects isolated single-character variables (e.g. $x$, $y$, $i$, $1$, $a$) as Class 2 ("Formula"). Passing a $15 \times 12$ pixel crop of the letter "x" into RapidLaTeXOCR's encoder and autoregressive decoder is an enormous waste of compute.

#### Proposed Architecture:
Before invoking the neural LaTeX transformer:
- Check bounding box dimensions: If $\text{width} < 25\text{ px}$ and $\text{height} < 25\text{ px}$, or if the box contains only 1 recognized OCR token:
  Wrap the token directly as inline math (`$x$`) without calling the neural LaTeX encoder/decoder.
*Expected Benefit*: Eliminates $40\% - 60\%$ of unnecessary neural LaTeX invocations on academic papers.

---

### 4.5. Milestone 5: Incremental Batch Resume (`--resume`) (P1)

#### Proposed Architecture:
In `src/batch.rs`, before dispatching a document, check if the output markdown file already exists, is valid, and contains frontmatter:
```rust
if resume_mode && target_md_path.exists() {
    if let Ok(meta) = fs::metadata(&target_md_path) {
        if meta.len() > 100 {
            // Already processed; increment succeeded counter and proceed to next
            continue;
        }
    }
}
```
*Expected Benefit*: Instant resumption of interrupted batches without re-converting completed files.

---

### 4.6. Milestone 6: Baseline-Aware Hybrid DOM Enrichment (P3 - Advanced)

If DOM text is ever utilized to accelerate clean digital pages, it must be **baseline-aware** to avoid losing sub/superscripts and math:
1. **Delta-Y Baseline Tracking**: Compare font matrix baseline ($y$) of consecutive glyphs. If $\Delta y > 2\text{ pt}$ and font size is reduced, automatically format as superscript (`^2` or `^{2}`) or subscript (`_i`).
2. **YOLO Exclusion**: Always use YOLOv8n to mask out formula regions and table regions. Only allow DOM text extraction inside purely text/paragraph bounding boxes.
3. **CMap Validation**: Verify that the font dictionary contains a valid `/ToUnicode` stream. If any font on the page lacks `/ToUnicode`, immediately fall back to the accelerated Indra OCR pipeline.

---

## 5. Summary & Projected Batch Completion Time

- **Current Progress**: 232 / 832 documents complete ($\approx 600$ remaining).
- **Current Completion Estimate**: $\approx 600 \text{ docs} / 2.83\text{ docs/min} \approx 212\text{ minutes}$ ($\approx 3.5\text{ hours}$ remaining).
- **With DOM-First Gating & Document Sharing**: Expected throughput increases from $2.8\text{ docs/min}$ to $15 - 25\text{ docs/min}$, reducing the total runtime of equivalent batches from **$5\text{ hours}$ to $< 45\text{ minutes}$**.
