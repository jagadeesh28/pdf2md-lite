# Agent Operating Guidelines: pdfmd-lite

> **Target Environment:** Antigravity IDE + Gemini 3.8 Flash (Medium)  
> **Workspace:** `pdfmd-lite` Rust Workspace (`crates/engine`, `crates/cli`, `crates/server`, `crates/desktop`) + React 19 Frontend (`ui/`)

This document serves as the canonical operating manual, architectural invariant contract, and phased remediation roadmap for autonomous AI coding agents working on `pdfmd-lite`.

---

## 1. Target Environment & Workspace Header

- **Target Environment:** Antigravity IDE + Gemini 3.8 Flash (Medium)
- **Active Workspace:** `pdfmd-lite` (`v0.7.2`, Rust Edition `2024`, Cargo Workspace + Vite/React 19/TypeScript + WiX Toolset v5)
- **Mission Summary:** `pdfmd-lite` is an autonomous, 100% offline-inference document conversion engine that transforms scientific, technical, and administrative PDFs into Obsidian-flavored Markdown with native LaTeX equations, structured tables, and per-note vector sidecars without external LLMs or cloud APIs.

---

## 2. Project Mission & North Star

`pdfmd-lite` targets locked-down workstations and air-gapped environments where deterministic document conversion, strict filesystem containment, and verifiable offline neural model execution are non-negotiable.

### Component Breakdown

| Component | Path | Responsibility |
| :--- | :--- | :--- |
| **Core Engine** | [`crates/engine/`](crates/engine) | Headless PDF parsing (`lopdf`, `hayro`, `pdf-extract`), neural layout/OCR (`tract-onnx` + optional `ort`/DirectML), structured IR, reading order, Markdown serialization, embeddings, vault tooling, and model registry. |
| **CLI Binary** | [`crates/cli/`](crates/cli) | Typed `clap` derive CLI (`pdfmd-lite`), batch runner, `--doctor` diagnostics, `--install-models` / `--dry-run` / `--watch` / `--embed` / `--serve` entry points. |
| **Web Server Daemon** | [`crates/server/`](crates/server) | Localhost-first Axum HTTP + WebSocket daemon (`pdfmd-lite --serve`) serving `ui/dist`, upload sandboxing, and real-time conversion telemetry. |
| **Desktop Shell** | [`crates/desktop/`](crates/desktop) | Native Tauri v2 desktop container (WebView2 on Windows) executing `crates/engine` in-process via IPC commands and OS file/folder pickers. |
| **Shared Frontend** | [`ui/`](ui) | Shared React 19 + Vite + Tailwind CSS + KaTeX SPA powering both Tauri Desktop and Axum Web shells via a unified bridge. |
| **Packaging & Scripts** | [`wix/`](wix), [`scripts/`](scripts) | WiX v5 MSI manifests (`main.wxs` Slim, `full.wxs` Full Offline) and release automation (`package-msi.ps1`). |

---

## 3. Core Operating Principles & Execution Model

1. **Pure-Offline Conversion Inference & Desktop/CLI-First Vision**:
   - Standard document conversion executes **100% offline in-process** via `tract-onnx` (pure-Rust SIMD core) and optional `ort` (ONNX Runtime / DirectML) acceleration with **zero** external LLMs or network calls.
   - Network model downloaders must never ship in default or MSI release builds; gate any online provisioning behind an opt-in Cargo feature (`online-provisioning`) and use explicit offline bundle import (`--install-models <dir|zip>`).
2. **64-Bit Native Architecture & Resource-Aware Scale**:
   - Address large multi-hundred-page batches dynamically via the **Hardware-Aware Concurrency Governor** (`concurrency.rs`), capping raster pixel allocations (default 40 MP) and input/decompression sizes.
   - Store all mutable runtime artifacts (config, logs, uploads, caches, user-installed models) in per-user OS AppData directories (overridable via `--data-dir`), never CWD or `Program Files`.
3. **Cross-Platform Parity for Main OSes (Windows, Linux, macOS)**:
   - Never hardcode Windows path separators (`\`), `curl.exe`, or `npm.cmd`. Use `std::path::Path` / `PathBuf` everywhere and normalize logical/UI asset URIs with `/`.
   - Handle `\r\n` (CRLF) and `\n` (LF) transparently in all text, CMap, frontmatter, and Markdown parsers.
   - Isolate unavoidable OS-specific behavior behind clean trait facades gated with `#[cfg(target_os = ...)]`.
4. **Evidence-Based Engineering & Honest Testing**:
   - Verify all extraction, reading-order, and throughput claims against checked-in synthetic PDF fixtures (`tests/fixtures/`), golden Markdown/report snapshots, and measured `tests/baseline.json` metrics.
   - Never write tests that return early when a developer-local file or model is absent; use deterministic synthetic fixtures or explicit `#[ignore = "reason"]` annotations tracked in CI.
5. **Zero-Copy & Linear-Time Architecture**:
   - Load static weights zero-copy via `Cow::Borrowed(&'static [u8])` when `bundle-weights` is active, read input PDFs once into `Arc<[u8]>`, mask figures in-place on 8-bit `GrayImage` buffers, reuse bounded `SessionPool<T>` instances, and stream page output to append-only `.partial` files before atomic finalization.
6. **Strict Crate Isolation & Zero Runtime Environment Variables**:
   - **Never use `std::env::var` or system environment variables** for application settings, paths, or runtime flags (only build-time `CARGO_MANIFEST_DIR` in build scripts/feature-gated assets is permitted).
   - `crates/engine` must remain strictly headless: zero dependencies on `cli`, `server`, `desktop`, or terminal I/O (`#![deny(clippy::print_stdout, clippy::print_stderr)]`).
7. **Pure, Idiomatic Error Handling (Zero-Unwrap Policy)**:
   - **Zero `unwrap()`, `expect()`, or `panic!()`** in production code paths across `engine`, `cli`, `server`, and `desktop` (allowed strictly inside `#[cfg(test)]` modules and integration test files).
   - Propagate typed errors via `Result<T, E>` (`thiserror` for structured engine errors, `anyhow` at CLI/handler boundaries).
   - Surface missing capabilities and runtime failures transparently via `ConversionReport` and explicit `--allow-degraded` flags—never coerce unsupported scripts or swallow render-thread errors.
8. **Verify, Never Assume**:
   - Run `rg -n` across the workspace before claiming any symbol, model weight, or configuration key is unused or wired.
   - Check [`docs/recommendations/implementedphases.md`](docs/recommendations/implementedphases.md) before opening or re-implementing previously completed Phase 0–8 milestones.
9. **Surgical Edits Only**:
   - Apply targeted search-and-replace edits to existing modules. Never overwrite, truncate, or rewrite large files from scratch or leave placeholder comments.
10. **Patient Builds & Zero Warning Suppression**:
    - Run `cargo check`, `cargo clippy`, `cargo test`, and frontend builds to completion.
    - Fix compiler and clippy diagnostics at their root cause; never add `#[allow(...)]` without a brief inline justification comment.
11. **Single-Phase Discipline & Local Git Checkpoints**:
    - Execute **one phase at a time** from Section 10.
    - **HARD STOP:** Do NOT begin Phase $N+1$ until every Passing Gate command in Phase $N$ exits with status code `0`, the Phase Sign-Off report is produced, and a local `git commit` checkpoint is created.
    - Request explicit user confirmation before executing `git push`.
12. **Domain Vocabulary & ADR Compliance**:
    - Enforce canonical domain terms in [`GLOSSARY.md`](GLOSSARY.md) and avoid listed `_Avoid_` synonyms.
    - Consult ADRs under [`docs/adr/`](docs/adr) **on demand only** when modifying that specific subsystem.

---

## 4. Workspace Architecture & Boundaries

```text
pdfmd-lite/
├── Cargo.toml                  # Root virtual workspace manifest ([workspace.package] & [workspace.dependencies])
├── GLOSSARY.md                 # Canonical domain vocabulary and avoided synonyms
├── deny.toml                   # cargo-deny advisory, license, ban, and source policy
├── docs/
│   ├── adr/                    # Architectural Decision Records (0001 – 0018)
│   └── recommendations/        # Audit reports, REFERENCES.md, and implementedphases.md
├── models/                     # Gitignored local ONNX weights & manifest
├── scripts/                    # Packaging automation (package-msi.ps1)
├── wix/                        # WiX v5 MSI installer manifests (main.wxs, full.wxs)
├── ui/                         # Shared React 19 + Vite + Tailwind + KaTeX SPA
└── crates/
    ├── engine/                 # Core library crate (`pdfmd-lite`):
    │   ├── benches/            # Criterion OCR/pipeline benchmarks (ocr_bench.rs)
    │   └── src/
    │       ├── backends/indra/ # DBNet detection, SVTR recognition, CTC masking, vocab
    │       ├── embeddings/     # all-MiniLM-L6-v2 engine, token-aware chunking, JSONL sidecars
    │       ├── figure/         # YOLOv8n layout, SLANet tables, RapidLaTeXOCR, SessionPool, PDF crops
    │       ├── metadata/       # Deterministic metadata extractor (+ deferred GLiNER module)
    │       ├── batch.rs        # Batch orchestration, dry-run, OutputPlanner
    │       ├── concurrency.rs  # Hardware-Aware Concurrency Governor
    │       ├── config.rs       # Typed AppConfig (`#[serde(deny_unknown_fields)]`)
    │       ├── markdown.rs     # Markdown formatting, frontmatter, LaTeX/table normalization
    │       ├── model_locator.rs# Cascading ModelLocator / ModelRegistry & manifest verification
    │       ├── pdf.rs          # Hybrid DOM parser, CMap decoder, hayro raster streaming
    │       ├── pipeline.rs     # Document pipeline orchestrator & Structured Document IR
    │       ├── progress.rs     # ProgressEmitter & ETA tracking
    │       ├── telemetry.rs    # Tracing subscriber & log configuration
    │       ├── vault.rs        # Vault formatting, image conversion, offline model installation
    │       ├── watch.rs        # Directory watcher with stability probing
    │       └── xycut.rs        # Recursive XY-Cut++ spatial ordering
    ├── cli/                    # `pdfmd-lite` CLI binary (`cli.rs`, `main.rs`)
    ├── server/                 # `pdfmd-lite-server` Axum HTTP/WebSocket daemon (`lib.rs`)
    └── desktop/                # `pdfmd-lite-desktop` Tauri v2 shell (`main.rs`, `tauri.conf.json`, `build.rs`)
```

### Dependency Flow

```text
  ┌──────────────────────────┐        ┌──────────────────────────┐
  │    ui/ (React 19 SPA)    │        │  crates/cli (pdfmd-lite) │
  └──────┬─────────────┬─────┘        └──────┬────────────┬──────┘
    Tauri│IPC      HTTP│WS                   │            │
         ▼             ▼                     ▼            │
  ┌─────────────┐  ┌───────────────┐         │            │
  │   desktop   │  │    server     │◄────────┘            │
  └──────┬──────┘  └───────┬───────┘                      │
         │                 │                              │
         └────────┬────────┴──────────────────────────────┘
                  ▼
      ┌───────────────────────┐
      │     crates/engine     │  (Headless core; zero UI/HTTP/CLI/stdout deps)
      └───────────────────────┘
```

### Strict Crate & Workspace Invariants

- **Unified `[workspace.dependencies]` & `[workspace.package]`**: Pin all shared external crates, `version`, `edition = "2024"`, `authors`, and `license` in root [`Cargo.toml`](Cargo.toml). Member crates MUST inherit via `{ workspace = true }` and share the root `target/` directory to prevent duplicate builds and version skew.
- **Engine Purity**: `crates/engine` must never import `axum`, `tauri`, `clap`, or emit `println!`/`eprintln!`. All progress and warnings flow through `ProgressEmitter`, `ConversionReport`, and `tracing`.
- **Single Bridge Contract**: `ui/` communicates with Rust solely through `ui/src/bridge/api.ts`, backed by identical schemas across Tauri IPC (`crates/desktop`) and Axum REST/WS (`crates/server`).
- **Path Policy Boundary**: Both `crates/server` and `crates/desktop` must validate every user- or PDF-derived filesystem path through `engine`'s canonicalizing `PathPolicy` before reading or mutating files.

---

## 5. Engineering & Toolchain Standards

1. **Rust Toolchain & Two-Speed Check Discipline**:
   - Target Rust Edition `2024` (`Cargo.toml` workspace).
   - Use `rg -n` instead of `grep` for all workspace symbol and call-site discovery.
   - **Inner loop:** Run crate-scoped checks (`cargo check -p pdfmd-lite` or `cargo test -p pdfmd-lite <filter>`) for rapid iteration.
   - **Phase gates:** Run workspace-wide verification (`cargo check --workspace --all-targets`, `cargo check --benches`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`).
2. **Cargo Feature Hygiene**:
   - Decouple tokenizer support (`tokenizers-support`, default) from compile-time binary embedding (`bundle-weights`, opt-in release-only) and network provisioning (`online-provisioning`, opt-in only).
   - Gate `directml` under Windows target dependencies so Linux/macOS and clean checkouts compile via `cargo check --workspace --all-targets --no-default-features` without `models/` present.
3. **Cross-Platform Standards (Windows, Linux, macOS)**:
   - Use `Path`/`PathBuf` everywhere; treat output paths as directories unless ending in `.md` and not an existing directory; handle Windows reserved names (`CON`, `NUL`, trailing dots) in `OutputPlanner`.
   - Use portable frontend commands (`npm run build`, never hardcoded `npm.cmd`) in `crates/desktop/tauri.conf.json` and `crates/desktop/build.rs`, failing hard when UI build fails.
4. **Error, Security & Data-Integrity Discipline**:
   - **Zero-Unwrap Policy:** Never use `.unwrap()`, `.expect()`, or `panic!()` in non-test production code. Replace mutex `.lock().unwrap()` in `crates/server` and string/path `.unwrap()` calls with structured error propagation.
   - **WebView & Server Security:** Render Markdown via a maintained parser + `DOMPurify` (no regex `dangerouslySetInnerHTML`); enforce strict Tauri CSP (`withGlobalTauri: false`); bind `--serve` to loopback by default with per-run random bearer token, `Origin`/`Host` validation, no `CorsLayer::permissive()`, and bounded streaming uploads.
   - **Safe Mutations:** Write outputs to `.partial` files and atomically rename on completion; back up modified vault files under `.pdfmd-backup/<timestamp>/` and operate only on `generator: pdfmd-lite` notes by default.
5. **Parsing, Structured IR & Neural Standards**:
   - Normalize all PDF coordinates against `CropBox` and `/Rotate` into canonical top-left page space before clustering or YOLO region matching.
   - Retain `Page -> Region -> Block -> Line -> Span` IR with provenance and span-ID addressing through reading order (`xycut.rs`) and Markdown serialization—never perform global `replacen` string substitution for formulas or delete body text using corpus-specific date rules.
   - Validate all models against `MANIFEST.toml` (SHA-256, byte size, ONNX I/O shapes) via one reloadable `RwLock` registry honoring [ADR 0014](docs/adr/0014-cascading-model-locator.md) and [ADR 0015](docs/adr/0015-hybrid-dual-runtime-and-directml-acceleration.md) (`--device auto|cpu|directml`).

---

## 6. Reference Map

| Feature / Domain | Primary Reference Source | Key Topics Covered |
| :--- | :--- | :--- |
| **Domain Ubiquitous Language** | [`GLOSSARY.md`](GLOSSARY.md) | Canonical terms (`Raw Crop`, `Structured Document IR`, `Path Policy`, `Conversion Report`, `Model Manifest`, `Output Planner`, `Run Controller`) and avoided synonyms. |
| **Neural Models & Academic Citations** | [`docs/recommendations/REFERENCES.md`](docs/recommendations/REFERENCES.md) | YOLOv8n DocLayNet, Indra DBNet/SVTR, Recursive XY-Cut++, SLANet, RapidLaTeXOCR, and deferred GLiNER architecture. |
| **Workspace & Build Manifest** | [`Cargo.toml`](Cargo.toml), [`crates/engine/Cargo.toml`](crates/engine/Cargo.toml) | `[workspace.package]`, `[workspace.dependencies]`, feature flags, release LTO/unwind profile. |
| **Completed Historical Work** | [`docs/recommendations/implementedphases.md`](docs/recommendations/implementedphases.md) | Phases 0–8 baselines, Criterion bench, embeddings, CMap decoder, Tauri/Axum shell, WiX MSI packaging. |
| **Audit Findings & Remediation Basis** | [`claude_audit_recommendations.md`](docs/recommendations/claude_audit_recommendations.md), [`chatgpt_audit_recommendations.md`](docs/recommendations/chatgpt_audit_recommendations.md), [`NEON_audit_recommendations.md`](docs/recommendations/NEON_audit_recommendations.md) | Security (`SEC-*`), offline model integrity (`OFF-*`), reading order & DOM/CMap (`COR-*`/`PARSE-*`), run control (`REL-*`), config & UI parity (`CFG-*`/`UI-*`/`UX-*`). |
| **Desktop & Installer Config** | [`crates/desktop/tauri.conf.json`](crates/desktop/tauri.conf.json), [`wix/main.wxs`](wix/main.wxs), [`scripts/package-msi.ps1`](scripts/package-msi.ps1) | CSP, `withGlobalTauri`, frontend build hooks, Slim vs Full Offline WiX manifests. |

---

## 7. Architectural Decision Records (ADRs)

> [!IMPORTANT]
> **Do NOT read all ADRs up front.** Only consult a specific ADR on demand when actively implementing or modifying that specific functional domain.

| Functional Domain / Task Context | Relevant ADR | Scope & Invariants |
| :--- | :--- | :--- |
| **Offline Execution & Zero-LLM Mandate** | [`0001-pure-offline-zero-llm-architecture.md`](docs/adr/0001-pure-offline-zero-llm-architecture.md) | No cloud APIs or external LLM runtimes; offline inference only (clarified by ADR 0017). |
| **Layout Class Routing (Picture / Table / Formula)** | [`0002-three-way-yolo-layout-routing.md`](docs/adr/0002-three-way-yolo-layout-routing.md) | DocLayNet class routing: Picture (6), Table (8), Formula (2) (amended by ADR 0018). |
| **Worker Concurrency & Memory Governor** | [`0003-resource-aware-dynamic-concurrency.md`](docs/adr/0003-resource-aware-dynamic-concurrency.md) | RAM/core-based worker sizing, zero env vars, ascending `BTreeMap` page ordering. |
| **Batch Directory Layout & Page Anchors** | [`0004-short-slug-batch-directory-layout.md`](docs/adr/0004-short-slug-batch-directory-layout.md) | Sanitized short slugs, `<slug>_images/` assets, `<!-- Page N -->` anchors, 1-based page slices. |
| **YOLOv8n Pure-Rust Retention** | [`0005-yolov8n-pure-rust-retention.md`](docs/adr/0005-yolov8n-pure-rust-retention.md) | Retain YOLOv8n over YOLO11 so layout runs on `tract-onnx`. |
| **Indic Script Masking & Progressive Metadata** | [`0006-constrained-script-decoding-and-progressive-metadata.md`](docs/adr/0006-constrained-script-decoding-and-progressive-metadata.md) | Constrained CTC Unicode masking (`U+0C00..=U+0C7F`), inline vs display math gating (metadata amended by ADR 0018). |
| **Global Worker Pool & Raster Streaming** | [`0007-global-worker-pool-and-pipelined-rasterization.md`](docs/adr/0007-global-worker-pool-and-pipelined-rasterization.md) | Shared worker pool, bounded channel streaming (`DynamicImage`), no intermediate PNG disk spooling. |
| **Post-Extraction Note Formatter** | [`0008-unified-post-extraction-note-formatter.md`](docs/adr/0008-unified-post-extraction-note-formatter.md) | Spurious codeblock unwrapping, LaTeX normalization, vault formatting. |
| **Embedding Sidecars & Frontmatter Idempotency** | [`0009-per-note-embedding-sidecars-and-frontmatter-hash.md`](docs/adr/0009-per-note-embedding-sidecars-and-frontmatter-hash.md) | `<slug>.embeddings.jsonl` sidecars, `pdf_sha256` frontmatter, opt-in `--embed-images-base64`. |
| **In-Process MiniLM Embeddings & RAG Schema** | [`0010-inprocess-minilm-sentence-embeddings.md`](docs/adr/0010-inprocess-minilm-sentence-embeddings.md) | `all-MiniLM-L6-v2` 384-dim vectors, `tract-onnx` default + `ort` option, standard JSONL schema (amended by ADR 0017/0018). |
| **Fine-Grained Mutexing & Grayscale Masking** | [`0011-fine-grained-session-mutexes-and-inplace-grayscale-masking.md`](docs/adr/0011-fine-grained-session-mutexes-and-inplace-grayscale-masking.md) | Step-level decoder mutex scoping and in-place 8-bit `GrayImage` masking. |
| **Hybrid DOM Extraction & Baseline Sub/Super** | [`0012-baseline-aware-hybrid-dom-extraction.md`](docs/adr/0012-baseline-aware-hybrid-dom-extraction.md) | `--dom-extraction auto\|always\|never`, $\Delta y$ sub/superscript detection, CMap check, Table/Formula exclusion (Table is Class 8). |
| **Unified Tauri Desktop & Axum Web Shell** | [`0013-tauri-axum-unified-desktop-web-architecture.md`](docs/adr/0013-tauri-axum-unified-desktop-web-architecture.md) | Shared `ui/` SPA across Tauri v2 IPC and `axum` `--serve`, WiX dual-flavor MSI (hardened by ADR 0017). |
| **Cascading Model Locator & Zero-Copy Assets** | [`0014-cascading-model-locator.md`](docs/adr/0014-cascading-model-locator.md) | Precedence: `--models-dir` > exe-adjacent > AppData > dev > embedded `Cow::Borrowed`, `--doctor` diagnostics (refined by ADR 0017/0018). |
| **Hybrid Dual-Runtime & DirectML EP** | [`0015-hybrid-dual-runtime-and-directml-acceleration.md`](docs/adr/0015-hybrid-dual-runtime-and-directml-acceleration.md) | `tract-onnx` core + Windows DirectML/`Level3` CPU fallback, `--device auto\|cpu\|directml` (wired per ADR 0018). |
| **Hybrid Table Serialization & Span HTML** | [`0016-hybrid-table-serialization-and-span-reconstruction.md`](docs/adr/0016-hybrid-table-serialization-and-span-reconstruction.md) | Clean GFM table in main body + exact HTML `rowspan`/`colspan` inside `> [!table]-` callout. |
| **Offline Provisioning, AppData & Server Boundary** | [`0017-offline-provisioning-and-local-server-containment.md`](docs/adr/0017-offline-provisioning-and-local-server-containment.md) | `bundle-weights`/`online-provisioning` split, `--install-models`, AppData state, bearer token/`PathPolicy`/CSP/DOMPurify. |
| **Structured Document IR & Manifest Lifecycle** | [`0018-structured-document-ir-and-explicit-model-manifest.md`](docs/adr/0018-structured-document-ir-and-explicit-model-manifest.md) | Hierarchical Page IR with provenance, `MANIFEST.toml`, drop unused `order.onnx`/dead `gliner_small.onnx`, `--allow-degraded`, `tract-onnx` MiniLM. |

---

## 8. Mandatory Edge-Case Test Matrix

Every audited or newly added module MUST include deterministic unit, property, or fixture integration tests covering all applicable categories:

| Category | Required Edge Cases & Fixtures |
| :--- | :--- |
| **Numerical & Geometry** | `f32`/`f64::NAN`, `INFINITY`, `NEG_INFINITY`, `-0.0`, zero-width/height bboxes, non-zero `CropBox`/`MediaBox` origins, `/Rotate` ($0^\circ, 90^\circ, 180^\circ, 270^\circ$), huge MediaBox (`14400x14400 pt`) pixel-cap downscaling. |
| **Cardinality & Slugs** | `0`-page PDFs, `1`-line pages, single-column vs uneven `2`-column pages, duplicate same-stem PDFs across subdirectories (`a/report.pdf` vs `b/report.pdf`), `0`/`1`/repeated short tokens (`(1)`, `x`) near formulas. |
| **Domain Algorithms** | XY-Cut++ single-column indented/bulleted/date-header pages (zero phantom column splits), overlapping YOLO `Table`/`Picture`/`Text` boxes (single owner), corrupt/partial `/ToUnicode` CMaps (`\u{FFFD}`, PUA, OT1 ligatures), `rowspan`/`colspan` tables, Telugu vs Latin CTC masking, de-hyphenation skipping `$...$` and fenced code. |
| **Parsers & I/O** | Empty/truncated PDFs, encrypted PDFs, malformed compressed content streams, UTF-8 BOM, CRLF (`\r\n`) vs LF (`\n`), Telugu/mixed UTF-8 token-count chunking (`<= max_seq_len`), TOML `deny_unknown_fields`, invalid `--pages` & `--lang` inputs. |
| **Concurrency, State & Recovery** | Render-thread `Err` & panic propagation (never swallowed), `SessionPool` RAII drop under panic, `RunController` double-start (`409`/Busy) & cancel latency, kill-mid-run `.partial` resume vs changed options, WebSocket `Lagged` resync, `RwLock` model reload after install. |
| **Security & UI Parity** | Hostile HTML/XSS PDF text (`<script>`, `<img onerror>`, `svg onload`, `javascript:`/`file:` links), `PathPolicy` traversal (`..\`, UNC, symlink, junction, `CON`/`NUL`), Axum missing token (`401`)/bad `Origin` (`403`)/bad `Host` (`421`), browser drag-drop & folder byte uploads, UI `AppConfig` full round-trip preserving `models_dir`. |

---

## 9. Historical Project Status

Completed foundational milestones (verified through `v0.7.2`; details in [`docs/recommendations/implementedphases.md`](docs/recommendations/implementedphases.md)):

| Phase | Description | Status |
| :---: | :--- | :---: |
| **0 & 1** | Criterion benchmarking (`ocr_bench`), batched SVTR OCR, formula fast-path, concurrency governor, batch oversubscription fix, RGBA→RGB slice flattening, basic `--resume`, note formatter | ✅ Complete |
| **2** | In-process `all-MiniLM-L6-v2` embeddings (`384`-dim), section-aware chunking, `<slug>.embeddings.jsonl` sidecars, retroactive vault pass | ✅ Complete |
| **3** | `pdf_sha256` frontmatter idempotency, opt-in `--embed-images-base64`, `indicatif` progress/ETA, `--watch`, `--dry-run`, `deny.toml` | ✅ Complete |
| **4** | In-place grayscale masking (`mask_gray_regions_mut`), fine-grained decoder mutexing, `tracing` telemetry, baseline-aware hybrid DOM extraction | ✅ Complete |
| **5** | Workspace decoupling (`engine`, `cli`, `server`, `desktop`), React 19 SPA (`ui/`), Axum `--serve`, Tauri v2 desktop shell, WiX v5 MSI installers | ✅ Complete |
| **6** | Thread-safe panic hook isolation, cascading `ModelLocator`, `AppConfig` strict unknown-field rejection, unified raster error boundaries, `--doctor` | ✅ Complete |
| **7** | DirectML EP & `SessionPool<T>`, Split-Pane live page preview (`GET /api/page-preview`), Sandboxed Upload Guard, cancellation tokens, `clap` derive CLI, `TableGrid::to_html` hybrid serialization | ✅ Complete |
| **8** | Adobe `/ToUnicode` CMap & PDF encoding parser, `Auto` hybrid DOM default, in-place masking gating, release `v0.7.2` & MSI artifacts | ✅ Complete |

---

## 10. Phased Audit & Improvement Roadmap

> [!IMPORTANT]
> **Single-Phase Discipline:** Complete only the active phase. Do NOT advance to Phase $N+1$ until every Passing Gate command in Phase $N$ exits with `0`, the Sign-Off Report is written, and changes are committed locally.

### Phase 9: Security Containment, Clean Builds & Reproducible Baseline
**Objective:** Close all P0 WebView/server/filesystem security hazards, decouple compile-time model embedding for clean checkouts, eliminate silent data-loss bugs (`COR-01`, `COR-02`, `COR-04`, `COR-05`), and commit a deterministic synthetic PDF corpus with measured baselines.

- **Tasks:**
  1. **WebView & CSP Hardening (`ui/`, `crates/desktop/`):** Replace regex HTML rendering in [`MarkdownViewer.tsx`](ui/src/components/MarkdownViewer.tsx) with a maintained Markdown parser + `DOMPurify` (supporting callouts, wikilinks, KaTeX, safe URL protocols only); set strict CSP and `"withGlobalTauri": false` in [`tauri.conf.json`](crates/desktop/tauri.conf.json); remove hardcoded `npm.cmd` and fail hard on frontend build errors in [`crates/desktop/build.rs`](crates/desktop/build.rs).
  2. **Axum Server & Path Containment (`crates/server/`, `crates/engine/`):** Implement canonicalizing `PathPolicy` in `engine` (deleting `input/` basename scanning); remove `CorsLayer::permissive()`; bind `--serve` to loopback by default (require explicit `--allow-remote` + auth for non-loopback); enforce per-run random bearer token + `Origin`/`Host` validation on `/api/*` and WebSocket; stream bounded uploads into per-run AppData temp directories with automatic cleanup.
  3. **Per-User AppData & Repo Hygiene:** Move config (`pdfmd-lite.toml`), logs, uploads, caches, and user models from CWD/`Program Files` to OS AppData (overridable via `--data-dir`, zero `std::env::var`), defaulting output to user Documents; delete root `pdfmd-lite.toml`, dead root `build.rs`, and committed `*.log`/`*.txt` logs; fix root `.gitignore`.
  4. **Offline Feature Split:** Split `embed-models` in [`crates/engine/Cargo.toml`](crates/engine/Cargo.toml) into `tokenizers-support` (default) and `bundle-weights` (opt-in release-only for `include_bytes!`), gate `directml` to Windows targets, and gate `vault::download_embedding_models` behind opt-in `online-provisioning`.
  5. **Stop Silent Output Corruption:** Write notes to `<slug>.md.partial` + atomic rename (`pdfmd_schema: 2`, `pdfmd_status: complete`, `conversion_fingerprint`); join `render_thread` in `pipeline.rs` and propagate errors/panics (`lopdf` canonical page count); replace AIAA date/month/`TECHNICAL NOTES` line deletion in `markdown.rs` with repetition-based header/footer detection; make heuristic heading promotion opt-in (`--academic-structure`); guard destructive vault operations with dry-run counts, `generator: pdfmd-lite` scoping, atomic writes, and `.pdfmd-backup/<timestamp>/`.
  6. **Synthetic Golden Corpus & Baseline:** Replace all tests referencing local `F:\` paths with checked-in synthetic PDF fixtures (`tests/fixtures/` covering digital, two-column, ligatures, rotated/CropBox, corrupt CMap, encrypted/corrupt xref, table/formula, XSS payload, gov-order dates/lists) and record `tests/baseline.json`.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets --no-default-features
  cargo check --workspace --all-targets
  cargo check --benches
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  npm --prefix ui run build
  npm --prefix ui run test
  ```

---

### Phase 10: Offline Model Integrity, Registry & Device Control
**Objective:** Make offline model discovery, SHA-256 integrity validation, hardware device selection, and capability reporting unified, reloadable, and truthful across CLI, Desktop, and Web.

- **Tasks:**
  1. **Model Manifest & Unified Registry:** Create `models/MANIFEST.toml` (SHA-256, byte size, license, tensor I/O signatures, required/optional status); unify `ModelLocator`, `resolve_model_path`, and `resolve_embedding_paths` into one registry respecting [ADR 0014](docs/adr/0014-cascading-model-locator.md) (`--models-dir` > exe-adjacent > AppData > dev workspace > `bundle-weights`) so `--doctor` and `load_bytes` always agree; make `--doctor` exit non-zero on missing/mismatched required weights.
  2. **Remove Dead/Phantom Weights & Fix Custom Layout Path:** Remove unused `order.onnx` (and phantom `unimernet.onnx`, `bge_small_int8.onnx`) from active `KNOWN_MODELS`/`bundle-weights`; decouple `Deterministic Metadata Extractor` from GLiNER and remove dead `gliner_small.onnx` loading/downloading (`#[allow(dead_code)]`); fix `LayoutModelSource::load_layout_bytes()` in [`figure/models.rs`](crates/engine/src/figure/models.rs) so file paths load directly instead of acting as `custom_dir`.
  3. **Strict Capability & `--allow-degraded` Gating:** Return an error when requested capabilities (e.g., `Telugu` without `rec_world.onnx` or unknown `--lang`) are unavailable unless `--allow-degraded` is explicitly passed (recording warnings in `ConversionReport` and note frontmatter); align `IndraScript::World` masking between `transcribe_line` and `transcribe_gray_with_script`.
  4. **Offline Importer, `--device`, & Reloadable Engines:** Implement `pdfmd-lite --install-models <dir|zip>` validating `MANIFEST.toml` (and make any `online-provisioning` downloader cross-platform, pinned-hash, and atomic); wire `--device auto|cpu|directml` end-to-end across `AppConfig`, CLI, UI, and all ORT builders (logging active EP); scale ORT session pool counts by RAM (`1` when `< 8 GB`, `2` when `>= 8 GB`); replace `OnceLock<Option<...>>` with `RwLock`-backed reloadable holders (`invalidate_model_caches()`); replace hardcoded `ModelsModal.tsx` statuses with backend `get_model_status` / `GET /api/models` data.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets
  cargo check --benches
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  npm --prefix ui run build
  npm --prefix ui run test
  ```

---

### Phase 11: PDF Text Fidelity, Structured IR, Reading Order & Hybrid Routing
**Objective:** Resolve single-page and multi-column reading-order swaps, CMap/coordinate extraction bugs, and table/prose duplication using `--debug-layout` diagnostics, a hierarchical Document IR, and region-aware hybrid DOM/OCR routing.

- **Tasks:**
  1. **Diagnostic Trace (`--debug-layout <dir>`):** Emit per-page JSON traces capturing `MediaBox`/`CropBox`/`Rotate`, raw DOM spans, YOLO boxes (raw + post-NMS), XY-Cut++ splits, and final block order.
  2. **PDF Text-State & Coordinate Fidelity (`crates/engine/src/pdf.rs`):** Handle `'`/`"` operators, `q`/`Q`/`cm` CTM transforms, `CropBox`/`MediaBox` origins, `/Rotate`, `Tc`/`Tw`/`Tz`, `/Encoding /Differences` ligatures, variable-length CMap codespace ranges + array `bfrange`, font-size-relative baseline clustering (`|\Delta y| <= 0.35 * min(h1, h2)`), dynamic advance-based space insertion, and a DOM sanity gate (`\u{FFFD}` / PUA > threshold triggers per-page/region OCR fallback).
  3. **Structured Document IR (`Page -> Region -> Block -> Line -> Span`):** Retain structured blocks with provenance (`dom`, `ocr`, `specialist`) and span-ID addressing through ordering and Markdown serialization; eliminate global `replacen` formula substitution (`$?$` placeholder removal) and preserve standalone short-line breaks in `markdown.rs`.
  4. **XY-Cut++ & YOLO Region Integrity (`xycut.rs`, `yolo.rs`, `pipeline.rs`):** Add single-column gutter validation (minimum gutter fraction + sustained dual-side text mass) and vertical overlap shrinkage in `xycut.rs`; enforce class-agnostic NMS / containment pruning (`Table` beats `Picture`, exclude `Table` & `Formula` lines from prose), enable DocLayNet `Section-header` (7) and `Title` (10) signals, and rescue orphan spans falling outside YOLO boxes.
  5. **Embeddings & Metadata Cleanup:** Implement pure-Rust `tract-onnx` support for `all-MiniLM-L6-v2` (honoring [ADR 0010](docs/adr/0010-inprocess-minilm-sentence-embeddings.md)) with real tensor batching benchmarked against serial inference, switch chunking from byte lengths to token counts, record embedding model/dim in sidecar headers, evaluate multilingual retrieval on Telugu fixtures, strip fabricated domain/author tags in favor of script ratio + optional `taxonomy.toml`, and guard de-hyphenation against prose compounds/code/math.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets
  cargo check --benches
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  ```

---

### Phase 12: Run Control, Output Planning & Reporting
**Objective:** Deliver deterministic job identity, panic-resilient run lifecycle management, collision-free output planning, and accurate conversion/batch accounting across CLI, Server, and Desktop.

- **Tasks:**
  1. **`ConversionReport` & Exit Codes:** Return structured `ConversionReport` (`completed`, `completed_with_warnings`, `failed`, `cancelled`, `skipped`, page/fallback/model stats) written into compact frontmatter and emitted as CLI exit codes `0` (clean), `2` (warnings), `1` (failed).
  2. **`RunController` & ID-Routed Events:** Replace `is_running` booleans and global cancel flags with an RAII `RunController` (`Idle -> Running -> Cancelling -> Idle`), per-document panic isolation, cooperative worker cancellation tokens, run/job IDs on all progress events (removing fuzzy slug matching in `useConversionProgress.ts`), WebSocket `Lagged` resync + `GET /api/run-state`, and `tracing` warning forwarding to UI.
  3. **`OutputPlanner`, Calibrated ETA & Watch Readiness:** Unify single/batch output layout (`<out>/<slug>/<slug>.md` with deterministic path-hash disambiguation on collisions and Windows-safe sanitization); track calibrated EMA seconds/page per mode; require multi-poll size/mtime stability + exclusive-open probe in `watch.rs`.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets
  cargo check --benches
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  npm --prefix ui run build
  npm --prefix ui run test
  ```

---

### Phase 13: UI Contract Parity, Accessibility & UX
**Objective:** Achieve full Tauri Desktop and Axum Web workflow parity, schema-locked Rust/TypeScript contracts, honest UI affordances, and accessible keyboard/screen-reader interactions.

- **Tasks:**
  1. **Contract Parity & Config Round-Trip:** Generate/verify shared Rust⇄TS types (`specta` or `ts-rs` contract tests) so `AppConfigData`, `DryRunReport`, `ModelStatus`, and `BatchSummary` match 1:1; merge UI config saves without clobbering `models_dir`/`max_depth`/`layout_model` or inverting `save_table_images`; wire or remove dead settings (`extract_tables`, UI `log_level`/`log_file`, `device`, `image_quality` bounds); debounce saves on blur.
  2. **Browser Mode Parity & Capability UI:** Add `GET /api/note` and `GET /api/asset` scoped by `PathPolicy`; upload dropped files and directory selections as bytes with explicit error feedback (no bare-filename fallback); drive language dropdown from backend `get_capabilities`; show whole-queue multi-file `DryRunReport` tables; export full loaded Markdown documents; add per-job degradation/pipeline badges in `BatchQueue.tsx` and synchronized page scrolling in `SplitPane.tsx`.
  3. **Honest Controls, Guardrails & Accessibility:** Wire real Export Markdown and Watch Directory (or clearly disable unsupported actions—never log fake success); add confirmation + backup dialogs before destructive vault ops; add accessible modal focus traps/Escape/`role="dialog"`/`aria-*` semantics, selectable viewer text, readable contrast, non-colliding shortcuts, and Vitest + Playwright + `axe-core` coverage.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  npm --prefix ui run typecheck
  npm --prefix ui run build
  npm --prefix ui run test
  ```

---

### Phase 14: Performance Resource Caps, Hygiene & Release Certification
**Objective:** Enforce strict resource/memory caps, linear-time streaming, zero-print/zero-unwrap engine hygiene, offline reproducible builds, and end-to-end MSI release verification.

- **Tasks:**
  1. **Resource Caps & Linear Performance:** Enforce configurable pixel caps (`40 MP` default), `image::Limits`, decompression size limits, `max_file_mb`, and per-page watchdogs (`COR-17`); consolidate duplicate PDF opening/counting into one `PdfSession` (`<= 2` parses per document); verify linear scaling on a 300-page fixture benchmark.
  2. **Static Hygiene Gates:** Eliminate all `println!`/`eprintln!` from `crates/engine/src` and enforce `#![deny(clippy::print_stdout, clippy::print_stderr)]`; eliminate all remaining non-test `.unwrap()`/`.expect()` across `engine`, `cli`, `server`, and `desktop`.
  3. **Supply Chain, Offline Builds & Release Verification:** Refresh [`deny.toml`](deny.toml) and run `cargo deny check` / `cargo audit`; verify offline vendored compilation (`cargo vendor` + `cargo build --offline --workspace --locked`) and vendored `npm ci --ignore-scripts`; unify version single-source checks across `Cargo.toml`, `tauri.conf.json`, `ui/package.json`, and `wix/*.wxs`; validate non-admin Windows MSI install + offline network-denied conversion smoke test + SBOM/SHA256SUMS + doc link/loader-coverage checks.
- **Passing Gate (ALL must pass with exit code `0`):**
  ```powershell
  cargo check --workspace --all-targets --no-default-features
  cargo check --workspace --all-targets
  cargo check --benches
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  cargo deny check
  npm --prefix ui run typecheck
  npm --prefix ui run build
  npm --prefix ui run test
  ```

---

## 11. Phase Sign-Off Report Template

At the conclusion of every phase, write a completion report (`docs/audits/PHASE{N}_REPORT.md` or in the final phase response) before advancing:

```markdown
# Phase {N} Completion Report

## 1. Summary of Changes
- `[file/path]`: Concise summary of implementation, fixes, and tests added.

## 2. Issues Resolved
| ID | File / Crate | Issue Type | Root Cause | Fix & Test Added |
| :--- | :--- | :--- | :--- | :--- |
| P{N}-01 | `...` | Security / Correctness / Perf | `...` | `...` |

## 3. Edge-Case Coverage Checklist
- [ ] Numerical & Geometry (`NaN`, `Inf`, `-0.0`, `CropBox`/rotation, pixel caps)
- [ ] Cardinality (`0`, `1`, `2`-col, same-stem collision)
- [ ] Domain Algorithms (XY-Cut++, YOLO overlap, CMaps, tables, CTC masking)
- [ ] Parsers & I/O (encrypted/corrupt PDF, BOM, CRLF/LF, TOML, token chunking)
- [ ] Concurrency & Recovery (render join, `SessionPool`, `RunController`, `.partial` resume)
- [ ] Security & UI Parity (XSS/CSP, `PathPolicy`, bearer/Origin, byte uploads, config round-trip)

## 4. Passing Gate Verification
| Command | Exit Code | Notes |
| :--- | :---: | :--- |
| `cargo check --workspace --all-targets` | `0` | 0 errors, 0 warnings |
| `cargo test --workspace --all-targets` | `0` | X passed, 0 failed, 0 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | `0` | 0 warnings |

## 5. Git Checkpoint Verification
- **Local Commit:** `git commit -m "<conventional commit summary>"` (SHA: `<hash>`)
- **Push Status:** Pending user confirmation (or `git push` SHA confirmed by user)
```

---

## 12. Review & Verification Checklist for Agents

Before claiming any phase or task is complete, verify all items:

- [ ] `cargo check --workspace --all-targets` passes with `0` errors and `0` warnings.
- [ ] `cargo check --benches` compiles cleanly.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes with `0` warnings.
- [ ] `cargo test --workspace --all-targets` passes all unit, fixture, and integration tests without vacuous skips.
- [ ] `cargo fmt --all -- --check` passes cleanly.
- [ ] `npm --prefix ui run typecheck && npm --prefix ui run build && npm --prefix ui run test` passes whenever `ui/` or bridge contracts are modified.
- [ ] Zero `.unwrap()`, `.expect()`, or `panic!()` in non-test production code.
- [ ] Zero `println!`/`eprintln!` in `crates/engine/src` and zero runtime `std::env::var` reads across the workspace.
- [ ] Mandatory Edge-Case Test Matrix (Section 8) adhered to for all modified modules.
- [ ] Phase Sign-Off report produced and local `git commit` checkpoint recorded before moving to Phase $N+1$.
