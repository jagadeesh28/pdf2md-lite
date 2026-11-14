# pdfmd-lite — Audit & Recommendations

**Audited by:** Claude (static code review)
**Date:** 2026-11-14
**Commit audited:** `0e82730` (single-commit history)
**Repo size:** ~15.6k lines of Rust, 94 unit tests, no `tests/` directory

---

## 0. Scope and limits

**What this is:** a static read of the cloned repo.

**Read closely:** `pipeline.rs` (page workers), `concurrency.rs`, `config.rs`, `backends/indra/models.rs`, `figure/models.rs`, `figure/latex.rs`, Indra engine and recognizer setup, `pdf.rs` (panic guard), `build.rs`, `Cargo.toml`, CI, `deny.toml`, the CLI and batch structure, and the docs (`README.md`, `IMPROVEMENTS.md`, `FUTURE_IMPROVEMENTS.md`).

**Skimmed only:** `markdown.rs` (2,886 lines), `figure/table.rs`, `xycut.rs`, `metadata/gliner.rs`, `embeddings/`, `watch.rs`. No findings are offered on their internals.

**Not done:**
- Nothing was built or run (no Rust toolchain in the review sandbox).
- No benchmarks were executed.
- Performance statements are inferences from the code. Each one should be confirmed with `cargo bench --bench ocr_bench` before and after the change.

**Overall verdict:** the architecture is sound (bounded-channel render thread, BTreeMap page reorderer, batched SVTR recognition, hardware-aware governor). The problems are reliability gaps and shipping gaps, not design flaws.

---

## 1. Priority summary

| # | Finding | Severity | Effort |
|---|---------|----------|--------|
| 1 | `panic = "abort"` disables every `catch_unwind` safety net | P0 | S |
| 2 | Global panic-hook swap races across threads | P0 | S |
| 3 | Models located via runtime `CARGO_MANIFEST_DIR`; LaTeX/SLANet/GLiNER not embedded; failures silent | P0 | M |
| 4 | Config fields parsed but never used; config errors swallowed | P0 | S |
| 5 | Rasterization errors drop pages silently (multi-worker) or abort the doc (single-worker) | P0 | S |
| 6 | 42 tract plans compiled eagerly at startup (incl. unused world recognizer) | P1 | M |
| 7 | LaTeX decoder: per-step buffer clones, no KV cache, `Level1` optimization | P1 | M–L |
| 8 | 3–4 parses of every PDF; needless byte clone in `get_pdf_page_count` | P1 | S |
| 9 | `get_indra_engine()` check-then-set race | P1 | S |
| 10 | Governor: heavy `System::new_all()`, `println!`, no cgroup awareness, README/code mismatch | P1 | S |
| 11 | Embedded model bytes copied to heap; CWD `models/` silently shadows embedded weights | P1 | S |
| 12 | `build.rs` CPU-feature cfgs are unused (and risky if used) | P1 | S |
| 13 | `clap` is a dependency but unused; 650-line hand-rolled CLI | P2 | M |
| 14 | 8-argument page wrappers; warning block copy-pasted 3× | P2 | S |
| 15 | No end-to-end / golden tests | P2 | M |
| 16 | 167 MB of model weights tracked in git despite `.gitignore` | P2 | M |
| 17 | README overstates "pure Rust" | P2 | S |
| 18 | CI gaps (non-blocking cargo-deny, no release smoke test) | P2 | S |
| 19 | Stale docs (`FUTURE_IMPROVEMENTS.md`, `IMPROVEMENTS.md`) | P2 | S |

Effort: S = under a day, M = a few focused sessions, L = a week or more.

---

## 2. P0 — Reliability bugs

### 2.1 Panic safety net is dead code in release builds
- **Where:** `Cargo.toml` (`[profile.release] panic = "abort"`), `src/pipeline.rs:674`, `src/pdf.rs:202`.
- **Problem:** `catch_unwind` does nothing under `panic = "abort"`. The comment on `safe_extract_text_by_pages` states that `pdf-extract` panics on malformed font tables. In a release build, one such PDF terminates the whole process — including a long batch run.
- **Fix:**
  - Remove `panic = "abort"` from the release profile (keep `lto = "thin"`, `codegen-units = 1`).
  - For client-facing batch work, run each document in a child process (re-exec self per document). `catch_unwind` cannot contain a native crash inside ONNX Runtime; a process boundary can.
- [ ] Remove `panic = "abort"`
- [ ] Add a regression test with a deliberately malformed PDF fixture

### 2.2 Global panic-hook swap is not thread-safe
- **Where:** `src/pdf.rs`, `safe_extract_text_by_pages`.
- **Problem:** `take_hook()` / `set_hook()` mutate process-global state. With concurrent documents in batch mode, two threads can interleave and leave the silent hook installed permanently, hiding real panics elsewhere.
- **Fix:** install one process-wide hook at startup that logs to `tracing` and consults a thread-local "suppress" flag. `safe_extract_text_by_pages` sets that flag instead of swapping the hook.
- [ ] Single hook at startup
- [ ] Thread-local suppress flag

### 2.3 Models resolved through `CARGO_MANIFEST_DIR` at runtime
- **Where:** `src/figure/latex.rs:29`, `src/figure/slanet.rs:34` and `:285`, `src/metadata/gliner.rs:40`, plus several test helpers in `recognize.rs`, `vocab.rs`, `inspect.rs`.
- **Problem:**
  - `CARGO_MANIFEST_DIR` exists only at compile time or under `cargo run`. In a shipped binary the lookup falls back to `"."`, i.e. the current working directory.
  - Only the Indra and YOLO models are embedded via `include_bytes!`. LaTeX, SLANet and GLiNER are not, so the "single offline binary / MSI" plan in ADR 0013 does not work for formulas, tables or metadata.
  - Failures end in `.ok()` → `None` with no log line, so formulas and tables degrade silently on a client machine.
  - It also contradicts the README's "zero environment variables" rule.
- **Fix:** one `ModelLocator` used by every engine, with this resolution order:
  1. `--models-dir` / `models_dir` from config
  2. `std::env::current_exe()`-adjacent `models/`
  3. Platform data directory
  4. Embedded bytes (where available)
- Log which source each model resolved from. Emit a `warn!` when an engine is unavailable. Add a `--doctor` (or `models check`) command that prints each model, its resolved path, size and SHA-256.
- [ ] `ModelLocator` implementation
- [ ] Replace all `CARGO_MANIFEST_DIR` runtime reads (tests may keep `env!`)
- [ ] `--doctor` command

### 2.4 Config fields parsed but never used; errors swallowed
- **Where:** `src/config.rs`, `src/cli.rs:880`.
- **Problem:**
  - `models_dir`, `layout_model`, `language`, `image_format`, `dom_extraction` have **zero** references in `cli.rs`. `set_custom_layout_path` is never called. The README documents `models_dir` and `layout_model` as working settings.
  - `AppConfig::load(...).unwrap_or_default()` swallows TOML parse errors, so a typo silently runs on defaults.
- **Fix:**
  - Wire every field through, or delete the unused ones and the README lines.
  - Add `#[serde(deny_unknown_fields)]` to `AppConfig`.
  - Fail loudly on parse errors.
  - Document precedence: CLI flag > config file > default.
- [ ] Wire or remove each dead field
- [ ] `deny_unknown_fields`
- [ ] Hard error on malformed config

### 2.5 Inconsistent handling of rasterization failures
- **Where:** `src/pipeline.rs`, `process_pages_stream_with_options`.
- **Problem:**
  - Multi-worker path: a rasterization `Err` hits `continue`. The page vanishes — no warning callout, no `<!-- Page N -->` marker — and the output has a silent gap.
  - Single-worker path: `msg?` aborts the entire document on the first bad page.
- **Fix:** both paths should emit the same `> [!WARNING] Page N Processing Incomplete` block that other failures already produce, keeping the page marker so ordering stays intact.
- [ ] Unify error handling in both paths

---

## 3. P1 — Performance

### 3.1 Eager compilation of 42 tract plans
- **Where:** `src/backends/indra/recognize.rs` (`BUCKET_WIDTHS` × `BATCH_SIZES`), `src/backends/indra/engine.rs` (`IndraEngine::new`).
- **Problem:** each recognizer compiles 7 widths × batch sizes {1, 4, 8} = 21 plans, and two recognizers (English + world) are built even in English mode. Each plan likely holds its own optimized graph, so weights may be duplicated. This probably explains the 350 MB base footprint hardcoded in the governor. Errors loading `rec_world.onnx` are also swallowed (`Err(_) => None`).
- **Fix:**
  - Compile plans lazily per bucket using `OnceLock`.
  - Load `rec_world` only when the script is not English.
  - Try a symbolic width dimension in tract to get one plan per batch size.
  - Log (don't swallow) a world-model load error when Telugu/Auto was requested.
- **Validate with:** `cargo bench --bench ocr_bench`, plus cold-start time and peak RSS.

### 3.2 LaTeX decoder
- **Where:** `src/figure/latex.rs`.
- **Status:** early stopping (repeat ≥ 3, max 128 steps) and narrow mutex scope are already in place. `FUTURE_IMPROVEMENTS.md` bottleneck 3 is therefore stale.
- **Remaining costs:**
  - Every decode step clones `tokens`, `mask` and the full encoder context (`ctx_data.clone().into_boxed_slice()`) — up to 128 times per formula. Use borrowed tensor views or reuse buffers (check the `ort` 2.0.0-rc.15 API for the right constructor).
  - Graph optimization is `Level1`; benchmark `Level3`.
  - No KV cache, so decoding is O(n²). Proper fix: re-export the decoder with a KV cache.
  - INT8 dynamic quantization of the encoder (89 MB) and decoder (51 MB) could cut size and CPU time. **Validate accuracy on your own formula set before adopting.**
  - Both sessions sit behind a `Mutex`, so formula decoding serializes across workers. If RAM allows, a small session pool (2 decoders ≈ +51 MB) lets two formulas decode concurrently.

### 3.3 Redundant PDF parsing
- **Where:** `src/pipeline.rs` (`process_pages_stream_with_options`), `src/pdf.rs` (`get_pdf_page_count`).
- **Problem:** each document is parsed by `fs::read` + `pdf-extract` (all pages, eagerly), `lopdf::Document::load` (by path), and `hayro` (rendering). `get_pdf_page_count` also does a full `pdf_bytes.clone()`.
- **Fix:** read once into `Arc<[u8]>`, share it, use `lopdf::Document::load_mem`, and avoid the clone.

### 3.4 Engine singleton race
- **Where:** `src/pipeline.rs:26` (`get_indra_engine`).
- **Problem:** check-then-set means two threads at batch start can each build a full engine and discard one.
- **Fix:** store the init result in the `OnceLock` via `get_or_init` (e.g. `OnceLock<Result<IndraEngine, String>>`).

### 3.5 Concurrency governor
- **Where:** `src/concurrency.rs`, `src/batch.rs`.
- **Problems and fixes:**
  - `System::new_all()` enumerates processes, disks and more just to read free memory. Use `System::new()` + `refresh_memory()`.
  - `println!` in library code fights the `indicatif` bar and the planned `ProgressEmitter`. Use `tracing`.
  - Memory detection ignores cgroup limits, so it over-provisions inside Docker or locked-down VMs. `sysinfo` exposes cgroup limits.
  - README and ADR 0003 say 1.5 GB per worker; code uses 200 MB per worker plus a 350 MB base. Pick one and update the other.
  - Batch mode divides workers across document threads. A single global worker budget (shared semaphore) is simpler and avoids idle workers when documents finish unevenly.

### 3.6 Embedded model byte copies and silent shadowing
- **Where:** `backends/indra/models.rs`, `figure/models.rs`.
- **Problem:** `EMBEDDED_*.to_vec()` copies 12+ MB of static data to the heap. The lookup order prefers `./models/` in the CWD over embedded weights, so running from the wrong directory silently loads different weights.
- **Fix:** read from the `&'static [u8]` through a `Cursor` where the loader allows it, and log which source was used (this folds into `ModelLocator`, §2.3).

### 3.7 Dead CPU-feature detection in `build.rs`
- **Problem:** `detect_hardware_features()` emits `has_avx2` / `has_fma` / `has_avx512` / `has_neon` cfgs, but nothing in `src/` reads them. If they were used, they would bake the **build machine's** CPU into binaries delivered to clients.
- **Fix:** delete the function. If you need tuned builds, use `RUSTFLAGS=-C target-cpu=...` per release target, or runtime dispatch.

---

## 4. P2 — Ergonomics, structure, hygiene

### 4.1 CLI
- `clap` is declared in `Cargo.toml` and has **zero** uses. `cli::run()` is ~650 lines of hand-parsing with 27 `process::exit` calls, and modes are detected with `args.iter().any(...)` anywhere on the line.
- Out-of-range values slip through (e.g. `--image-quality 200` parses as a valid `u8`).
- **Recommended subcommands:**
  - `convert <in> <out> [--pages 1-10]`
  - `batch <in-dir> <out-dir> [-j N] [-d DEPTH]`
  - `watch <in-dir> <out-dir>`
  - `fix-math`, `format-notes`
  - `embed [--force]`
  - `models download | check`
- Keep today's positional form as the default subcommand for backward compatibility. Gains: generated help, shell completions, validation, and a clean base for the planned `crates/cli` split.

### 4.2 Page-processing plumbing
- Four wrapper layers around `process_single_page_image_with_doc` with up to 8 parameters (`#[allow(clippy::too_many_arguments)]`), and the warning-callout block is copy-pasted three times.
- **Fix:** a `PageJob` / `PageContext` struct carrying PDF handle, image, output dir, page number, raw text, script and image options; one `warning_block(page, reason, raw_text)` function.

### 4.3 Tests
- 94 unit tests and no end-to-end coverage. `markdown.rs` is 2,886 lines of hand-rolled text processing and cannot be refactored safely without golden tests.
- **Add** 3–4 fixture PDFs with `insta` snapshots: a two-column paper, a scanned page, a table-heavy page, and a Telugu page. Include one deliberately malformed PDF for §2.1.

### 4.4 Model weights in git
- `.gitignore` lists `models`, yet 15 model files are tracked (force-added): ~167 MB in the working tree, `.git` is ~150 MB. Every future model update permanently grows history.
- **Fix:** move weights to Git LFS or GitHub Release assets, with a SHA-256 manifest verified by `models download` (the MiniLM download command already provides the pattern). Note that `embed-models` is a default feature and `include_bytes!` fails the build if the files are missing, so document the build prerequisite.

### 4.5 README accuracy
- It claims pure-Rust, no external runtime. The default feature set enables `ort` (native ONNX Runtime) for LaTeX, SLANet, GLiNER and embeddings, and `tokenizers` with `onig` (a C library).
- For clients who audit their supply chain, say this explicitly, and verify that an **air-gapped build** works — `ort` typically downloads binaries at build time unless pointed at a local copy.

### 4.6 CI
- `cargo-deny` runs with `continue-on-error: true`, so it can never fail the build. Also, `vulnerability = "deny"` and `unlicensed = "deny"` in `deny.toml` are, to my knowledge, keys removed from recent cargo-deny schemas — check whether that is why it was made non-blocking.
- No release-build smoke test. A job that runs `cargo build --release` and converts a fixture PDF would have caught §2.1 and §2.3.
- `matrix.apt_packages` and `matrix.linker_env` are referenced but never defined (harmless, but dead).

### 4.7 Docs drift
- `FUTURE_IMPROVEMENTS.md` bottlenecks 2–7 are already implemented; mark them resolved or delete.
- `IMPROVEMENTS.md` references `e:\Rust\pdfmd-lite` and says models are absent from the working copy.

---

## 5. UI/UX and future improvements

### 5.1 Phase 5 (Tauri + Axum + React) — recommendation
The planned stack (React + Vite + Tailwind + KaTeX, Tauri/WebView2, Axum daemon, WiX MSI) adds a Node toolchain and a large new surface area to a Rust-only offline product, while P0 reliability gaps remain.
- Ship §2.1–2.5 first.
- Then consider an Axum server embedded in the same binary serving a static HTML/htmx page (no Node, no WebView2 dependency).
- Defer Tauri until a client specifically asks for a desktop shell.
- Before any UI work, remove `println!` from library code and introduce the headless `ProgressEmitter` trait.

### 5.2 Highest-value UI feature: a review queue
- `recognize_line_with_conf` already returns OCR confidence, but the pipeline discards it.
- Persist per-line and per-page confidence plus flags (LaTeX repair applied, formula rejected, table fallback used, page-level warning) into `<slug>.quality.json`.
- UI: side-by-side PDF page and Markdown, sorted by lowest confidence. For users who must trust the output, this matters more than menus and settings panels.

### 5.3 Other improvements
- **Page-level resume.** Resume today works per document via the `pdf_sha256` frontmatter. A 300-page document that dies at page 280 restarts at page 1. Cache per-page Markdown keyed by page content hash.
- **Windows long paths.** Slug truncation to 32 characters avoids `MAX_PATH`, but the `\\?\` extended-path prefix is the proper fix for deep input trees.
- **Quantization.** INT8 LaTeX and SLANet models would shrink the installer (your MSI estimate is ~180 MB) and CPU time; benchmark accuracy first.
- **Structured logs.** A JSON log mode alongside `--log-file` makes batch runs auditable.

---

## 6. Suggested sequencing

1. **Decide:** panic strategy (§2.1) and `ModelLocator` design (§2.3). Everything else hangs on these.
2. **Reliability pass (one coding block):** §2.1, 2.2, 2.4, 2.5, then `ModelLocator` and `--doctor`.
3. **Measured performance work (separate sessions, bench before/after each):** §3.1, 3.2, 3.3, 3.4, 3.5.
4. **Cleanup:** §3.7, §4.2, §4.7.
5. **Structure:** clap migration (§4.1), golden tests (§4.3), model hosting (§4.4), CI hardening (§4.6).
6. **Product:** quality sidecar and review UI (§5.2), then the Axum UI (§5.1).

---

## 7. Verification checklist (after the reliability pass)

- [ ] Release build converts a malformed PDF without aborting the batch
- [ ] Release binary run from an unrelated directory finds all models (or reports exactly which are missing via `--doctor`)
- [ ] LaTeX, SLANet and GLiNER paths log a warning when their models are unavailable
- [ ] A typo in `pdfmd-lite.toml` fails with a clear error
- [ ] A forced rasterization failure yields a warning callout and keeps page ordering, in both single- and multi-worker modes
- [ ] 832-document batch memory and throughput compared against the numbers in `FUTURE_IMPROVEMENTS.md`
