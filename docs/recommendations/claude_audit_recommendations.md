# pdfmd-lite — Full-Repo Audit & Implementation Plan

| | |
|---|---|
| **Repo version audited** | 0.7.2 (workspace, `tauri.conf.json`, `wix/main.wxs`, `ui/package.json`) |
| **Audit date** | 2026-11-25 |
| **Scope** | `crates/{engine,cli,server,desktop}`, `ui/` (from `ui_src.zip`), `docs/adr`, root docs, `wix/`, `scripts/`, `deny.toml`, `build.rs` |
| **Criteria (your brief)** | (1) stated objectives, (2) correctness of using offline models, (3) UI completeness, (4) UX ergonomics |
| **Deliverable** | This file: findings, fixes, tests, and pass/fail gates per phase |

> **Assumption:** I read "state objectives" as the project's *stated* objectives (README / AGENTS.md / ADRs): 100% offline, zero LLM, zero runtime env vars, "pure-Rust", deterministic, Obsidian-quality output, hardware-aware, built for locked-down government machines. If you meant something else, Section 1's scorecard is the part to re-point.

---

## 0. How to read this report (method, coverage, limits)

**What I could not do.** My sandbox has no Rust toolchain and the zip contains no `models/` directory. Nothing was compiled, run, or benchmarked. Every finding is from reading code and docs.

- **[V] Verified in code.** The source itself proves it. No runtime needed.
- **[L] Likely.** Depends on runtime or corpus behavior. The fix plan includes the fixture that confirms or kills it. Do not spend fix effort on an [L] item until its confirming test has run.

**Coverage.**

| Level | Files |
|---|---|
| Read line-by-line | `pipeline.rs`, `batch.rs`, `pdf.rs`, `config.rs`, `concurrency.rs`, `model_locator.rs`, `lib.rs`, `figure/{session_pool,models}.rs`, `figure/mod.rs` (analysis path), `backends/indra/{engine,detect,order,models,mod}.rs`, `desktop/src/main.rs`, `desktop/build.rs`, `tauri.conf.json`, `server/src/lib.rs`, `cli/src/cli.rs`, `ui/src/{App.tsx,bridge/api.ts,types,hooks,components/MarkdownViewer.tsx,components/ModelsModal.tsx}`, all root manifests, ADRs, root docs |
| Read in part | `markdown.rs` (frontmatter, dehyphenate, header stripping, heading promotion, metadata enrichment; ~1,700 lines of LaTeX/table normalization only sampled), `figure/yolo.rs`, `recognize.rs`, `vault.rs`, `watch.rs`, `xycut.rs`, `BatchQueue.tsx`, `SettingsModal.tsx`, `SplitPane.tsx` |
| Grep/structure-audited only | `figure/{caption,latex,slanet,table,pdf_stream}.rs`, `metadata/gliner.rs`, `embeddings/*`, `progress.rs`, `telemetry.rs`, `vocab.rs`, `inspect.rs`, `MenuBar.tsx`, `LogConsole.tsx`, About/Shortcuts modals, `wix/`, `scripts/package-msi.ps1` |
| Not reviewed | `desktop/gen/schemas/*` (generated), KaTeX dist assets, `Cargo.lock` contents, model weights (absent) |

The repo's earlier audit documents (`Audit_2026_11_19.md`, `IMPROVEMENTS.md`, `CLAUDE_RECOMMENDATIONS.md`, `implementedphases.md`) were read for context. I did not re-adjudicate them item by item. Where code contradicts a "done" claim, the finding says so.

---

## 1. Executive verdict

**Not ready for locked-down government deployment.**

The engine core is well structured: per-page panic isolation, a sound session pool, ordered page streaming, no `unsafe`, and no runtime env reads. But three problems block deployment:

1. The **security posture** lets a hostile PDF or web page act on local files.
2. The **"offline + verified models" story** is partly fiction. The UI hard-codes "Verified" and a download button contacts the internet.
3. Several **silent-wrong-output paths** (resume, swallowed render errors, corpus-specific text deletion, wrong-script OCR) corrupt notes while reporting success.

### Objectives scorecard

| Stated objective | Verdict | Why |
|---|---|---|
| 100% offline at runtime | **PARTIAL** | Inference is in-process and no HTTP client crate exists. But `vault.rs::download_embedding_models` shells out to `curl.exe` against huggingface.co and is exposed in GUI, web API, and CLI. The Models dialog says no external service is "ever contacted" (OFF-01, OFF-02). |
| Zero runtime env vars | **MET** | No runtime `env::var` reads found. Only build-time `CARGO_MANIFEST_DIR`. |
| Zero LLM | **MET** | No generative-model dependency. GLiNER is a small NER encoder. |
| "Pure-Rust" (README) | **NOT MET as worded** | `ort` (native ONNX Runtime, pre-release `2.0.0-rc.15`), `tokenizers` with `onig` (C), DirectML (OFF-07). |
| Correct, auditable use of offline models | **NOT MET** | No checksum manifest, no shape or class-map validation, silent fallbacks, three inconsistent model locators, nothing recorded in output (OFF-03..05). |
| Deterministic / reproducible output | **AT RISK** | DirectML `fail_silently()` makes numerics machine-dependent. No provenance in notes. No determinism test (OFF-08). |
| Works on locked-down gov machines | **AT RISK** | App writes config, crash log, uploads, model downloads, and default `./output` relative to CWD. The MSI sets CWD to `Program Files`. Standard users cannot write there (BLD-04). Build runs `npm install` (SEC-06). |
| Telugu / Indic support | **PARTIAL** | Engine routes only English/Telugu/Auto. UI offers 11 languages; 9 silently run English OCR (UI-03). Embedding model is English-only (OFF-06). |
| UI completeness | **PARTIAL** | Fake menu actions, contract mismatches, dead settings, web-mode gaps (UI-01..09). |
| UX ergonomics | **PARTIAL** | No accessibility semantics, no confirmation on destructive ops, misleading progress and ETA (UX-01..07). |

### What is solid (keep it)

- `panic = "unwind"` plus `catch_unwind` per page, so one bad page becomes a warning callout instead of a dead run.
- `SessionPool` uses an mpsc channel with RAII guards, so there is no mutex-poisoning cascade.
- Ordered page emission (`BTreeMap` reorder) is correct.
- `pdf_stream.rs` handles the CTM and page-origin offsets correctly. The text path should copy this (COR-08).
- `AppConfig` uses `deny_unknown_fields`.
- Upload handler strips path components and checks `%PDF-` magic.
- `Cargo.lock` is committed. A dry-run feature exists. The `ProgressEmitter` abstraction is the right seam.

---

## 2. Findings register

**Severity.** P0 = fix before any deployment. P1 = fix before pilot. P2 = fix before GA. P3 = polish.
Each finding lists **Evidence**, **Impact**, **Fix**, **Test**. The phase is in the heading.

### 2.1 Security (SEC)

#### SEC-01 · P0 · [V] · Phase 0 — Unsanitized HTML in the Markdown viewer, with no CSP
- **Evidence:** `ui/src/components/MarkdownViewer.tsx` runs regex replacements over raw note text and injects the result with `dangerouslySetInnerHTML`. Only fenced code is escaped. Callout bodies, table cells, headings, `alt`, wikilink `title="${target}"`, and `src="${src}"` are inserted raw. `tauri.conf.json` has `"csp": null` and `"withGlobalTauri": true`.
- **Impact:** Text in a hostile PDF is extracted verbatim into the note and rendered in the inspector. `<img src=x onerror="window.__TAURI__.core.invoke('read_markdown',…)">` gets full IPC. Reachable commands include `read_markdown` (arbitrary file read), `save_config`, `format_notes` and `convert_png_to_jpeg` (in-place file rewrites), and `open_output_folder`.
- **Fix:**
  1. Replace the regex renderer with a real parser (`markdown-it` or `remark`/`rehype`) and sanitize the output with DOMPurify. Add small plugins for callouts, wikilinks, and `<!-- Page N -->`.
  2. Set a strict CSP: `default-src 'self'; script-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost`.
  3. Set `withGlobalTauri: false` and import `@tauri-apps/api` (already a dependency).
- **Test:**
  - Vitest XSS corpus (≥30 payloads): `<script>`, `<img onerror>`, `<svg onload>`, `[[x" onmouseover="…]]`, `![a" onerror="…](x)`, `javascript:` links, a callout body with a `<form>`. Assert the rendered DOM has no `on*` attributes, no `<script>`, and no `javascript:` URLs.
  - Add fixture PDF F17 (text layer contains HTML) and assert the same.
  - CI assertion that `tauri.conf.json` has a non-null CSP and `withGlobalTauri == false`.

#### SEC-02 · P0 · [V] · Phase 0 — Web daemon is unauthenticated, has permissive CORS, and can bind anywhere
- **Evidence:** `server/src/lib.rs` uses `CorsLayer::permissive()` with no auth, no Origin/Host check, and endpoints that take filesystem paths (`/convert`, `/format-notes`, `/convert-png-to-jpeg`, `/embed-vault`, `/page-preview`, `/config` PUT, `/download-models`). `cli.rs` accepts `--serve 0.0.0.0:3000` without warning.
- **Impact:** Any web page the user visits can POST to `127.0.0.1:3000` (CSRF; DNS rebinding) and make the daemon read, convert, or rewrite local directories. On a LAN bind, any host can.
- **Fix:**
  1. Default to loopback and refuse non-loopback binds unless `--allow-remote` plus a token is set.
  2. Generate a random per-run bearer token and inject it into the served `index.html`. Require it on `/api/*` and on the WebSocket upgrade.
  3. Validate `Host` and `Origin` against the bound address.
  4. Remove `CorsLayer::permissive()`. The SPA is same-origin.
  5. Add an allow-listed root set (see SEC-03).
- **Test:** `tower::ServiceExt` integration tests:
  - `Origin: https://evil.example` returns 403.
  - A missing token returns 401.
  - A `Host: attacker.test` header returns 421.
  - A path outside the roots returns 403.
  - The WebSocket upgrade without a token is refused.

#### SEC-03 · P0 · [V] · Phase 0 — Unscoped path access in desktop commands and server
- **Evidence:** Desktop commands `read_markdown`, `get_page_preview`, `open_output_folder`, `format_notes`, `convert_png_to_jpeg`, `embed_vault` accept any string path. `resolve_document_path` (duplicated in desktop and server) also searches `./input/**` by file name and returns the first match.
- **Impact:** Arbitrary file read or modify (amplifies SEC-01 and SEC-02). A bare file name can silently resolve to a different same-named document.
- **Fix:**
  1. Introduce `PathPolicy` in `engine`. It holds "granted roots" added only by native pickers, native drop events, or the configured output dir.
  2. Canonicalize and require the path to be inside a root, rejecting symlink escapes.
  3. `read_markdown` returns only `.md` under an output root. Previews return only `.pdf` inside a granted root.
  4. Delete the `input/` name search.
  5. Use Tauri capability permissions to expose only the commands that are needed.
- **Test:** table-driven unit tests: `..\..\Windows\win.ini`, UNC paths, a symlink out of a root, a junction, an 8.3 short name, mixed separators, and a trailing-dot name. All must be denied.

#### SEC-04 · P1 · [V]/[L] · Phase 0 — Upload handler limits and retention
- **Evidence:** `upload_handler` buffers the whole file (`field.bytes()`). There is no size limit configured. Files go to `temp/uploads/<name>` relative to CWD, collide on same name, and are never deleted. `axum = "0.7"` applies a 2 MiB default body limit to `Multipart` [L: confirm with a 5 MB PDF].
- **Impact:** Larger PDFs are likely rejected in web mode. Uploaded government documents persist on disk indefinitely. Concurrent users overwrite each other.
- **Fix:**
  1. Configure `DefaultBodyLimit` explicitly (default 256 MB, configurable).
  2. Stream to disk.
  3. Use `<data-dir>/uploads/<run-id>/<uuid>.pdf`.
  4. Delete on run completion or after a TTL.
  5. Return the opaque id, not a path.
- **Test:** 5 MB and 300 MB upload tests, a concurrent same-name upload test, and a cleanup test.

#### SEC-05 · P1 · [V] · Phase 0 — Destructive vault operations have no confirmation, no backup, and no scoping
- **Evidence:** `vault.rs::format_notes_in_path*` rewrites every `.md` under a folder in place with a non-atomic `fs::write`. Write errors only `warn!`, yet the file still counts as modified. `convert_vault_png_to_jpeg*` converts PNGs in place (lossy, irreversible). Both are one click in the UI (`handleFormatNotes`, `handleConvertPngToJpeg`). The UI passes `outputDir`, which may contain hand-written notes. `_fix_latex` is ignored, so "Format Notes" and "Fix LaTeX" are the same operation.
- **Impact:** Silent corruption of user notes. No undo.
- **Fix:**
  1. Add `generator: pdfmd-lite` and `pdfmd_version` to frontmatter, and only touch notes carrying the marker unless `--include-foreign`.
  2. Always run a dry-run first and show counts in a confirm dialog.
  3. Back up changed files to `.pdfmd-backup/<timestamp>/` and write atomically (temp file plus rename).
  4. Count a file as modified only after a successful write.
  5. For PNG→JPEG, keep originals unless the user ticks "delete originals".
- **Test:** property test (any write failure leaves the original byte-identical), crash-injection test, a foreign-note-untouched test, and a backup-restore round trip.

#### SEC-06 · P1 · [V] · Phase 6 — Build-time network and stale-asset behavior
- **Evidence:** `desktop/build.rs` runs `npm install` when `node_modules` is missing and swallows build failure if any `dist/` exists. `tauri.conf.json` also runs `npm.cmd run build` in `beforeBuildCommand`, so the UI builds twice. `npm.cmd` is hard-coded.
- **Impact:** Non-reproducible, network-dependent builds on an air-gapped build host. A broken UI build can ship a stale `dist/`.
- **Fix:** Use `npm ci --ignore-scripts` from a vendored or offline cache. Fail hard if the build fails. Pick one place to build the UI (the Tauri hook). Drop `dist/` and `tsconfig.tsbuildinfo` from version control.
- **Test:** CI job with no network that builds from a vendored cache. A job that changes a UI source file and asserts the bundle hash changes.

#### SEC-07 · P1 · [V] · Phase 0 — Personal artifacts committed to the repo
- **Evidence:** root `pdfmd-lite.toml` contains `output_dir = 'E:\Rust\NASTRAN\Reference\NASTRAN-95-master'`. `AppConfig::load(None)` auto-loads it from CWD, so running from the repo root writes to that path. Tests in `pdf.rs` reference `F:\Jag\artech varsha documents\…` (property-deed and utility-bill PDFs) and `F:\Jag\AI\cornell\…`. UTF-16 logs (`check.log`, `test.log`, `test_lib.log`) are in the zip.
- **Impact:** Surprising output location. Personal document names leak. Tests silently pass when the file is absent (see BLD-03).
- **Fix:** Delete the stray config. Move personal-file tests to synthetic fixtures. Add logs to `.gitignore` and purge them.
- **Test:** CI grep gate for `[A-Z]:\\` absolute paths and for `pdfmd-lite.toml` at repo root.

### 2.2 Offline guarantee and model correctness (OFF)

#### OFF-01 · P0 · [V] · Phase 0 — Network provisioning contradicts the offline claim
- **Evidence:** `vault.rs::download_embedding_models` runs `Command::new("curl.exe")` against `huggingface.co/.../resolve/main/...` (unpinned revision) for MiniLM, its tokenizer, and GLiNER. It does no checksum verification and writes to CWD-relative `models/embeddings`. It is exposed as the CLI `--download-models`, the Tauri `download_models` command, and `POST /api/download-models`. Both GUI and server answer "downloaded and verified successfully" even though nothing is verified beyond `exists()`.
- **Impact:** Violates the primary objective. Supply-chain exposure from an unpinned remote `main`. `curl.exe` is Windows-only. A false "verified" claim.
- **Fix:**
  1. Remove the downloader from default builds. Gate it behind a cargo feature `online-provisioning` (off by default and never in MSI or desktop builds).
  2. Add `pdfmd-lite --install-models <dir|zip>`: an offline importer that validates `models/MANIFEST.toml` (OFF-03) and copies into the data dir.
  3. If the online path is kept: pin revision hashes, verify sha256, use a Rust HTTP client with timeouts, and require an explicit `--i-understand-this-uses-the-network`.
  4. Delete the UI "Download" button in offline builds.
- **Test:**
  - CI static gate: fail if `Command::new("curl`, `reqwest`, `ureq`, or a literal `http(s)://` appears in non-doc source outside an allow-list.
  - Dynamic gate: run `doctor` and a full conversion of F01 in a network-denied namespace (Linux `unshare -rn`; Windows firewall deny rule). Assert exit 0 and zero outbound connection attempts (strace or ETW).

#### OFF-02 · P0 · [V] · Phase 1 — Models dialog is hard-coded fiction
- **Evidence:** `ModelsModal.tsx` defines `defaultModels` with every model `status: 'installed'` and renders a green "Verified" check for all. Sizes are literals ("GLiNER 150 MB", "RapidLaTeXOCR 18 MB"). No backend command or route exposes `ModelLocator::doctor_report()`. The engine already computes presence, size, and sha256.
- **Impact:** The UI asserts verification that never happened. A machine missing the formula or table model shows green while output silently degrades.
- **Fix:** Add `get_model_status` (Tauri) and `GET /api/models` returning `{name, required, expected_sha256, actual_sha256, size, source: embedded|disk(path)|missing, status: ok|missing|mismatch}`. Render real state and the real path. Add a "Copy diagnostics" button. Remove hard-coded sizes and descriptions that the manifest can supply.
- **Test:**
  - Contract test: renders `missing`, `mismatch`, and `ok` rows correctly from a mocked bridge.
  - Rust test: tamper one byte of a model in a temp models dir and assert `mismatch`.

#### OFF-03 · P1 · [V] · Phase 1 — No model integrity manifest or shape validation
- **Evidence:** `KNOWN_MODELS` is `(name, path, required)` with no expected hash. `doctor` prints an 8-char hash and never compares. YOLO class indices are hard-coded (formula=2, picture=6, table=8, i.e. the 11-class DocLayNet order) with no check that the loaded or custom (`layout_model`) ONNX has that output shape. A mismatched index is guarded by `if idx + 4 < channels`, so it silently yields no detections.
- **Impact:** A wrong, corrupted, or swapped model produces plausible-looking bad output with no error.
- **Fix:**
  1. Add `models/MANIFEST.toml` per model: sha256, size, license, upstream source (informational), input/output tensor signature, and class map where relevant.
  2. Verify lazily on first load (cached).
  3. Required-model failure is a hard error. Optional-model failure is a reported degradation (OFF-04).
  4. Validate ONNX I/O shapes at load.
  5. Embed the manifest hash in the binary.
- **Test:** unit tests for truncated file, wrong-shape ONNX (tiny synthetic model), swapped class count. Doctor exits non-zero on any required mismatch (see CLI gate below).

#### OFF-04 · P0 · [V] · Phase 1 — Silent model fallbacks and cached failures
- **Evidence:**
  - `IndraEngine::transcribe_gray_with_script`: with `IndraScript::Telugu`/`Auto` and `rec_world.onnx` absent or failing to load, the code falls into `else { self.recognizer.recognize_lines_batch(...) }`, so the **Latin** recognizer reads Telugu. Load failures log at `debug!`/`warn!` only.
  - SLANet, LaTeX, GLiNER, and the embedding engine use `OnceLock<Option<…>>`, which caches `None` for the life of the process, so provisioning models later still needs a restart.
  - The desktop and server do not forward `tracing` to the UI, so warnings are invisible.
  - `rec_world.onnx` is `required: false` in `KNOWN_MODELS` although Telugu is a headline capability.
- **Impact:** Telugu documents silently produce garbage Latin text. Formulas and tables silently degrade by machine, with nothing in the note saying so.
- **Fix:**
  1. A requested capability that is unavailable is an **error** by default. Degrade only with an explicit `--allow-degraded`, and then say so in the report and the note.
  2. Replace `OnceLock<Option>` with a reloadable holder (`RwLock<Option<Arc<_>>>` plus `reload()` after install).
  3. Surface all degradations as `ConversionReport.warnings` and UI banners.
  4. Mark `rec_world.onnx` required whenever Telugu or Auto is selected.
- **Test:** unit test with `rec_world` withheld asserting `Telugu` returns `Err`. Degraded-mode test asserting a report warning. Reload test: install a model after first call, then assert it is picked up without restart.

#### OFF-05 · P1 · [V] · Phase 1 — Three inconsistent model locators; GUI ignores `models_dir`
- **Evidence:**
  - `ModelLocator` (Indra, YOLO) differs from `resolve_model_path` (SLANet/LaTeX/GLiNER; disk only) and from `resolve_embedding_paths` (CWD-relative candidates; ignores `--models-dir` and the global dir).
  - The desktop and server never call `set_global_models_dir`, so `models_dir` in config is dead there.
  - `run_doctor` receives only the CLI flag (config's `models_dir` is ignored).
  - `ModelLocator` precedence differs from ADR 0014: no platform data dir, and embedded beats exe-adjacent.
  - `doctor_report` reports the disk path (via `locate`) while `load_bytes` prefers embedded, so doctor can name a file the runtime never loads.
- **Impact:** "Which model did this run use?" has no reliable answer. Two machines with identical config can load different weights.
- **Fix:** One `ModelRegistry` API used by every consumer, with one documented precedence order. `doctor` reports the source that `load` will actually use. Apply `models_dir` identically in CLI, GUI, and server.
- **Test:** precedence matrix test (explicit, exe-adjacent, data dir, embedded) asserting load and doctor agree for every model. A GUI-level test that `models_dir` in config changes the resolved path.

#### OFF-06 · P1 · [V] for code, [L] for retrieval quality · Phase 3 — Embeddings are English-only and mis-sized
- **Evidence:** `embeddings/engine.rs` uses all-MiniLM-L6-v2 for every document, including those tagged `lang/telugu`. Chunking uses byte lengths (`MIN 200`, `TARGET 1200`, `MAX 2000` via `.len()`), so Telugu text (3 bytes per character) gets about 3× smaller chunks than intended. The tokenizer truncates at 512 tokens, while this model family was trained at a 256-token window [L: confirm model card].
- **Impact:** Retrieval over Telugu notes will be poor. Chunks near `MAX` degrade.
- **Fix:**
  1. Chunk by tokens and characters.
  2. Make the embedding model a manifest entry with `max_seq_len`, `dim`, `languages`.
  3. Evaluate a multilingual small model on a Telugu retrieval set before adopting one. I have not verified license or size for any candidate.
  4. Write model id and dim into the sidecar header and refuse to mix.
- **Test:** chunking invariants (property test: no chunk exceeds token limit for English, Telugu, and mixed). Retrieval smoke test on a small bilingual set (recall@5 baseline, then no-regression).

#### OFF-07 · P1 · [V] · Phase 6 — "Pure-Rust" is not accurate, and ORT is a pre-release native dependency
- **Evidence:** `ort = "2.0.0-rc.15"` (release candidate, native ONNX Runtime), `tokenizers` with `onig` (C library), DirectML. README line 3 says "pure-Rust".
- **Impact:** Inaccurate claim in a procurement context. Build hosts need a C toolchain. If `ort` fetches its runtime binary at build time (default behavior of its binary-download feature) [L: confirm features in `Cargo.lock` resolution], air-gapped builds fail.
- **Fix:** Correct the README. Document exactly what is native. Vendor the ORT library (dynamic load or pinned local path). Run `cargo vendor` plus a no-network build in CI. Decide whether to track a stable `ort` release.
- **Test:** CI job: `cargo vendor` then `cargo build --offline --workspace --locked`.

#### OFF-08 · P2 · [V] · Phase 1 — Execution provider is silent and ADR 0015's `--device` is missing
- **Evidence:** `create_optimized_session_builder` registers DirectML with `.fail_silently()`. No CLI/config/UI field selects or reports the device. `directml` is in the default feature set.
- **Impact:** CPU vs GPU numerics differ, so identical inputs can produce different notes on different machines. Nobody can tell which ran.
- **Fix:** Add `device = "auto|cpu|directml"` in config, CLI, and UI. Log and record the selected EP in provenance. Make `directml` an opt-in feature for Windows builds.
- **Test:** CPU-only golden determinism (byte-identical across two runs and across `-j 1` vs `-j N`).

### 2.3 Conversion correctness and data integrity (COR)

#### COR-01 · P0 · [V] · Phase 0 — Resume skips partial and failed notes
- **Evidence:** `pipeline.rs` rewrites the output file after every page, with frontmatter that already contains `pdf_sha256`. `batch.rs` resume logic starts with `should_skip = true` and only flips it if a hash is present and different. A note with a matching hash, or no hash at all, is skipped. `test_resume_mode_skips_existing_markdown` writes a hashless note and asserts it is skipped. That locks the bug in. `dry_run_pdf_directory` treats "no hash" as `WOULD_OVERWRITE`, so dry-run and real run disagree.
- **Impact:** A crash, cancel, or error mid-document leaves a truncated note that `--resume` treats as done, permanently. Changed page range, script, DOM mode, or tool/model version are also ignored.
- **Fix:**
  1. Write to `<name>.md.partial` and atomically rename on success.
  2. Add `pdfmd_status: complete`, `pages_ok`, `pages_failed`, and `conversion_fingerprint` to frontmatter. The fingerprint is sha256 over (pdf sha, page range, script, dom mode, image options, tool version, manifest hash).
  3. Skip only when status is complete and the fingerprint is equal.
  4. Dry-run and real run share one `plan_document()` function.
- **Test:**
  - Kill-mid-run test (spawn, kill at page k, resume, assert reconversion).
  - Resume matrix across changed options.
  - Replace the existing test with one asserting a hashless note is **not** skipped.
  - Dry-run/run parity test.

#### COR-02 · P0 · [V] · Phase 0 — Render-thread failure is swallowed, producing empty "successful" notes
- **Evidence:** `process_pages_stream_with_options` spawns `stream_rendered_pages` and ends with `let _ = render_thread.join();`. The inner function returns `Err` if hayro cannot parse the PDF or the requested range exceeds hayro's page count. The page count used for planning comes from a different library (`pdf-extract`), so the two can disagree. No pages arrive, the loop ends cleanly, and the function returns `Ok`.
- **Impact:** A PDF hayro cannot open (encrypted, unusual xref, unsupported features) yields a frontmatter-only note reported as success.
- **Fix:**
  1. Join the thread and propagate both `Err` and panic.
  2. Assert `pages_emitted == end - begin + 1`. Emit a warning callout for each missing page and mark the document failed if none were emitted.
  3. Use one canonical page-count source (`lopdf::Document::get_pages().len()`) everywhere, and validate against hayro at open.
- **Test:** fixtures F13 (encrypted), F14 (corrupt xref), F15 (zero pages). Each must produce a failed status with a clear error, never an empty success. Add a unit test that injects an erroring render thread.

#### COR-03 · P1 · [V] · Phase 2 — Document "success" ignores page failures; no conversion report
- **Evidence:** Page-level errors and panics become `> [!WARNING]` callouts and the function returns `Ok(())`. Batch counts it as succeeded. The desktop emits `"completed"`. There is no per-document structured outcome.
- **Impact:** A document where every page failed is "Succeeded" in CLI summary, toast, and queue.
- **Fix:** Return `ConversionReport { pages_total, pages_ok, pages_failed, pages_dom, pages_ocr, formulas: {found, neural, fallback}, tables: {found, neural, geometric}, models_used, warnings[] }`. Statuses: `completed`, `completed_with_warnings`, `failed`, `cancelled`, `skipped`. CLI exit codes: 0, 2 (warnings), 1 (failure). Write a compact report into frontmatter.
- **Test:** unit tests per status. Snapshot of report JSON on fixtures. CLI exit-code tests.

#### COR-04 · P0 · [V] · Phase 0 — Header/footer stripping deletes real content
- **Evidence:** `markdown.rs::strip_running_headers_and_footers` is tuned to an AIAA 1980s–90s corpus. It deletes any line under 80 characters that starts with `MARCH`/`JAN`/`FEB`/`APR`/`MAY`/`JUNE`/`JULY`/`AUG`/`SEPT`/`OCT`/`NOV`/`DEC` and contains `198`, `199`, `200`, or `TECHNICAL NOTES`. Because `starts_with("DEC")` also matches "DECLARATION", "DECISION", and `contains("200")` matches 2000–2009 and "2005", lines such as "DECLARATION … 2003", "AUGUST 12, 2008", or "OCTOBER 2004 ORDER NO. …" are removed silently. It also removes any line starting `VOL. ` and containing `NO. `.
- **Impact:** Silent loss of dates and references in government orders, the target document class.
- **Fix:** Delete these constants. Detect running headers/footers by **repetition**: take lines in the top and bottom bands of each page, normalize digits, and remove only text that repeats on at least N pages (N≥3 or ≥40% of pages). Record removed lines in the report. Optionally keep them as HTML comments.
- **Test:** fixture F06 (government order with dates in the body) must retain every dated line. Property test: for documents with unique page text, the function is the identity. F01/F09 verify real repeating headers are removed.

#### COR-05 · P0 · [V] · Phase 0 — Heading promotion turns lists and names into headings
- **Evidence:** `promote_headings` emits `## ` for any line under 60 characters that starts with a Roman numeral token (`I. `, `V. `, `X. `, …) or a digit followed by `. ` (e.g. `1. Submit Form 16`). It also promotes the first line that is 10–120 characters, does not end with `.`, and lacks "University/Company/Institute/Laboratory/@" as `# Title`.
- **Impact:** Numbered procedures become H2 headings. Indian names with initials ("V. Srinivas", "K. …" where the initial matches a Roman token) become H2s. A letterhead line becomes the document title, and `enrich_document_metadata` then copies it into frontmatter `title`.
- **Fix:** Drive headings from layout and font signals: enable DocLayNet classes 7 (Section-header) and 10 (Title) in `yolo.rs`, and use font size and weight from DOM text where available. Make the regex promotion opt-in (`--academic-structure`).
- **Test:** property: any line matching `^\d+\.\s` or `^[IVX]+\.\s` is unchanged when no layout signal is present. Fixture F06 numbered list preserved. F01 academic headings still promoted under the opt-in flag.

#### COR-06 · P1 · [V] · Phase 3 — Metadata enrichment fabricates tags and authors
- **Evidence:** `enrich_document_metadata`: Telugu characters imply `domain/history` and `domain/telugu-literature`; `contains("MACH")` implies aerodynamics (matches "machine", "machinery"); `REPORT` implies `type/technical-report`; `TRANSACTIONS` implies `type/journal-article`; author is taken from the line above one containing "University/Company/Institute/Laboratory".
- **Impact:** Wrong, confident metadata in every note (a Telugu government order tagged "history"). This conflicts with the "don't invent data" principle.
- **Fix:** Default tags: only `lang/<script>` detected by script-character ratio. Move domain and type taxonomy into an optional user `taxonomy.toml`, with word-boundary matching. Author only from PDF Info or GLiNER above a score threshold, with `metadata_source` recorded. Otherwise omit.
- **Test:** F06/F05 (Telugu government order) must produce no `domain/*` tags. Word-boundary tests ("machinery" does not match "mach").

#### COR-07 · P2 · [V] · Phase 3 — De-hyphenation breaks legitimate compounds
- **Evidence:** `dehyphenate_text` joins `word-\nlowercase` unless the previous word is in a 14-word list (`three`, `non`, `self`, …). "state-\nof-the-art" becomes "stateof-the-art"; "e-\nmail" becomes "email"; "pre-\nprocessing" becomes "preprocessing" (the last may be desired). It is not code- or math-aware.
- **Fix:** Prefer PDF signals (soft hyphen U+00AD, or DOM line break). Otherwise join only when the joined form appears elsewhere in the document's vocabulary, else keep the hyphen. Skip fenced code and `$…$`.
- **Test:** fixture F01 with known split words, plus a property test that content outside prose spans is untouched.

#### COR-08 · P1 · [L] · Phase 3 — Hybrid DOM text extraction fidelity gaps
- **Evidence (code):** the `pdf.rs` content interpreter handles only `BT/ET/Tf/Ts/TL/Tm/Td/TD/T*/Tj/TJ`.
  - **Not handled:** `'` and `"` text operators (although `can_use_hybrid_dom` counts them as proof of text), Form XObjects (`Do`), `cm/q/Q`, `Tc/Tw/Tz`, page `/Rotate`.
  - **Origin offset:** `get_page_bbox` returns `(x0,y0,x1,y1)` but the caller uses only width/height, so `CropBox`/`MediaBox` origin offsets are dropped. `pdf_stream.rs` handles the origin correctly, so figures and text can misalign.
  - **Word spaces:** words placed by separate `Tj`/`Td` with `|ty| ≤ 2` get no space inserted (only `TJ` kerning `< -180` does).
  - **Encodings:** without `/ToUnicode`, single-byte fonts are decoded as ASCII/Latin-1 with control characters dropped. `/Encoding /Differences` is ignored, so OT1 ligature codes (`fi`, `fl`) vanish.
  - **Completeness:** any non-empty DOM result is accepted. There is no check against the rest of the page text.
- **Impact:** Silent text loss or joined words on specific generators and layouts. Magnitude depends on the corpus, so confirm with fixtures first.
- **Fix:** Build the text-extraction pass on a real interpreter (evaluate hayro's interpreter hooks or full CTM handling in lopdf). Apply origin and rotation. Insert spaces from glyph advance geometry. Decode `Differences`. Add a per-page completeness check (DOM character count vs rendered-OCR sample or `pdf-extract` count) with automatic per-page OCR fallback, recorded in the report.
- **Test:** fixtures F01 (LaTeX OT1 ligatures), F02 (Word), F03 (LibreOffice), F07 (rotated), F08 (CropBox offset), F09 (two-column). Assert expected text including `ﬁ`/`fi`, word boundaries, and bbox alignment against rendered page within a pixel tolerance.

#### COR-09 · P1 · [V] · Phase 3 — Formula insertion fabricates `$?$` and uses first-match string replace
- **Evidence:** `pipeline.rs` Phase 5: tiny formula boxes with no OCR text become `$?$`. Inline and display insertion uses `formatted_text.replacen(needle, …, 1)` on OCR line text.
- **Impact:** Placeholder noise in notes and RAG indexes. Repeated short lines (`(1)`, `x`) get replaced at the wrong occurrence.
- **Fix:** Anchor insertions by line index and bbox order, not by string search. Unreadable formulas keep the original text and add `<!-- formula unreadable -->`.
- **Test:** fixture F11 with repeated short lines near formulas. Assert correct position and no `?` placeholders.

#### COR-10 · P1 · [V] · Phase 2 — Output layout is inconsistent; same-stem files overwrite each other
- **Evidence:**
  - Single file: `<out>/<slug>.md`, flat. Batch: `<out>/<slug>/<slug>.md`.
  - Desktop/server loop over `files` and compute `slug = sanitize_short_slug(stem)` with no collision handling, so `a/report.pdf` and `b/report.pdf` overwrite each other.
  - An output directory whose name contains a dot (`Reports.v2`) is treated as a file path (`extension().is_none()` test).
  - `--resume` is ignored for single files.
  - `sanitize_short_slug` truncates to 32 bytes. Collision suffixes depend on sort order, so adding a file can renumber existing folders. It also ignores Windows reserved names (`CON`, `NUL`), trailing dots, and control characters.
- **Fix:** One `OutputPlanner` shared by CLI, desktop, server, and dry-run. Slug = sanitized stem plus an 8-hex hash of the source-relative path (suffix only when needed, but stable across runs). Output is always `<out>/<slug>/<slug>.md`. Treat the output as a directory unless it ends in `.md` and is not an existing directory.
- **Test:** property tests (slugs are unique per source path, valid on Windows, and stable when unrelated files are added). Fixture F18 (same-stem files from two folders).

#### COR-11 · P2 · [L] · Phase 3 — Detector resolution and per-line cost
- **Evidence:** DBNet runs once on the whole page resized so the long side equals `det_long_side` (1024 per `IndraDetector::new`). No tiling or adaptive size. `Auto` script runs both recognizers per line, unbatched. `IndraRecognizer::new_with_config` re-parses the model and compiles 7 widths × 3 batch sizes = 21 plans per recognizer (42 with the world model) eagerly at startup.
- **Impact:** Small fonts and large sheets (A3, drawings) may lose lines. Startup time and RAM are probably higher than the governor's 350 MB assumption.
- **Fix:** Measure first (see gates). Then tile or choose detector size by page DPI/size, compile plans lazily per used bucket, and batch `Auto` by running the cheap script check once per page.
- **Test:** benchmark gate (criterion bench `ocr_bench` extended with startup time and peak RSS). OCR CER on fixtures F04 (A4 scan) and a dense-font fixture.

#### COR-12 · P2 · [L] · Phase 3 — Table/figure overlap, hard-coded thresholds, three of eleven classes used
- **Evidence:** YOLO score threshold 0.40, NMS 0.45, containment 0.85 are literals. Only Picture/Table/Formula are used. A region detected as both Picture and Table produces both a figure crop and a table (no reconciliation in `analyze_page_layout_image_with_doc_and_options`).
- **Fix:** Table beats Picture on high overlap. Expose thresholds in config. Use Caption/Footnote/Page-header/Page-footer classes to drive COR-04/05.
- **Test:** fixture F10 (ruled, unruled, merged-cell tables). Assert no duplicate figure for table regions.

#### COR-13 · P2 · [V] · Phase 2 — Watch mode readiness and the GUI stub
- **Evidence:** `watch.rs::is_file_ready` only checks size > 0, so a PDF still being copied passes. The initial batch pass discards errors (`let _ =`). The GUI menu item "Watch Directory" only logs text (UI-02).
- **Fix:** Require stable size and modification time over N polls plus an exclusive-open probe. Surface errors. Either implement watch in GUI/server or remove it there.
- **Test:** simulate a slow copy (growing file) and assert no conversion until stable.

#### COR-14 · P2 · [V] · Phase 6 — Stdout/stderr printing inside the library
- **Evidence:** about 50 `println!`/`eprintln!` in `engine/src` (batch 13, vault 12, watch 10, pdf_stream 4, indra engine 4, pipeline 2 including "Completed conversion", embeddings, slanet, inspect).
- **Impact:** Noise for embedders. Invisible in the GUI. Mixed with structured output.
- **Fix:** Replace with `tracing` and `ProgressEmitter`. Add `#![deny(clippy::print_stdout, clippy::print_stderr)]` to the engine crate.
- **Test:** clippy gate in CI.

#### COR-15 · P2 · [V] · Phase 6 — Quadratic progressive rewrite and non-atomic output
- **Evidence:** the per-page callback re-renders `frontmatter + raw_body`, runs `normalize_latex_delimiters` over the entire accumulated body, and `fs::write`s it, ignoring errors, after every page.
- **Impact:** O(pages²) work on long documents. A crash mid-write can truncate the note (see COR-01).
- **Fix:** Append-only `.partial` file with periodic flush. Finalize once with normalization.
- **Test:** benchmark a 300-page fixture (F19) and assert time grows ~linearly (e.g., 300-page time < 2× of 150-page time ×2 within tolerance).

#### COR-16 · P2 · [V] · Phase 6 — Repeated full parsing per PDF
- **Evidence:** `get_pdf_page_count` runs a full `pdf-extract` text extraction, only to count pages. It is called in the batch pre-pass, in `extract_pdf_metadata`, and by the desktop and server for progress totals. Separately, `process_pdf_full_with_emitter` extracts all pages' text for the header slice, and `process_pages_stream_with_options` extracts it again. `extract_pdf_metadata` does a fourth parse via `lopdf::Document::load`. The render thread parses again with hayro.
- **Fix:** One `PdfSession` (bytes read once, lopdf parsed once, page count from `get_pages()`, text extracted once and cached). Pass it through the pipeline.
- **Test:** counter-based test using an instrumented parser wrapper (assert parse count per document ≤ 2). Benchmark gate on startup per document.

#### COR-17 · P1 · [V] · Phase 6 — No resource caps (page size, file size, image decode, upload)
- **Evidence:** `stream_rendered_pages` renders every page at fixed 2.0× with no pixel cap. A MediaBox of 14400×14400 pt yields a pixmap of about 3.3 GB and aborts the process (not catchable). `pdf_stream.rs` calls `decompressed_content()` without output limits. Files are read whole (`fs::read`).
- **Impact:** A crafted or merely large-format PDF can crash the whole batch.
- **Fix:** Cap pixels (default 40 MP, configurable) by lowering scale per page (`scale = min(2.0, sqrt(cap/(w·h)))`) and record it in the report. Use `image::Limits` and a max decoded-size check. Add `max_file_mb`. Add a per-page time budget with a watchdog.
- **Test:** fixtures F12 (huge MediaBox) and F16 (image bomb). Assert the document completes or fails cleanly with a report, and peak RSS stays under the configured cap.

#### COR-18 · P2 · [V] · Phase 2 — Time and memory estimates are invented constants
- **Evidence:** dry-run and the desktop/server ETA seed use 0.8–1.8 s/page and 1.8 s/page. `memory_budget_mb = 350 + workers × 200` appears in the engine and again in `BatchQueue.tsx`. `test.log` in the zip shows a one-page document taking 15.85 s (this includes one-time model initialization, so it is not steady-state, but it is an order of magnitude above 1.8 s).
- **Impact:** Users get ETAs that are far too optimistic. Engine and UI formulas can diverge.
- **Fix:** Persist an exponential moving average of observed seconds per page per page type (DOM vs OCR) in the data dir. Before data exists, show "calculating". Return memory estimates from the backend only. Calibrate the 350/200 MB constants with measured peak RSS.
- **Test:** unit test for the estimator. Gate: estimate within a stated tolerance of actual on the benchmark corpus after warm-up.

#### COR-19 · P1 · [V] · Phase 2 — Batch result accounting is wrong
- **Evidence:** Skipped documents are counted as `succeeded`. Cancellation leaves the remaining documents uncounted. The CLI prints `Skipped = total − succeeded − failed`, which is always 0 unless cancelled. The desktop and server add `succeeded` into `completed` as well.
- **Fix:** `BatchConversionResult { succeeded, skipped, failed, cancelled, warnings }`, used by CLI, desktop, and server.
- **Test:** unit tests for each count. CLI summary golden test.

### 2.4 Reliability and run control (REL)

#### REL-01 · P1 · [V] · Phase 2 — Run state machine is missing
- **Evidence:**
  - Server: `convert_handler` sets `is_running` and clears it only at the end of the closure. A panic outside `catch_unwind` leaves it stuck true, so every later run returns 409 until restart.
  - Desktop: `start_conversion` returns immediately and has no run guard. A second call starts a concurrent run sharing one global `CANCEL_REQUESTED` that each run resets.
  - UI: `handleStopConversion` sets `isRunning=false` immediately while the backend continues until the next check. A later `batch_summary` then raises a "Completed" toast after the user pressed Stop.
  - If `batch_summary` never arrives (panic, WebSocket drop), the UI stays "running" forever.
  - Cancellation is only checked per page callback and per document; after cancel, worker threads are detached and keep processing up to `concurrency` pages.
  - Desktop wraps the whole file loop in a single `catch_unwind`, so one panic aborts all remaining files.
- **Fix:** `RunController` with RAII guard (`Drop` resets), a state machine `Idle → Running → Cancelling → Idle`, run ids on every event, cancellation tokens passed into page workers and checked between phases, per-file isolation, and a watchdog that emits `batch_summary` with `failed`/`cancelled` if a run dies.
- **Test:** forced-panic test (server accepts a new run afterward), double-start returns Busy, stop-then-summary UI test (state shows "Cancelling" then "Cancelled"), cancel-latency test (stops within N seconds on a 50-page fixture).

#### REL-02 · P2 · [V] · Phase 2 — WebSocket event loop ends on lag; no resync
- **Evidence:** `handle_socket` uses `while let Ok(event) = rx.recv().await`. `RecvError::Lagged` ends the loop and silently stops progress for that client. The client reconnects every 2 s with no backoff or state resync.
- **Fix:** Handle `Lagged` by continuing and emitting a `resync` event. Add `GET /api/run-state` snapshot used on (re)connect. Exponential backoff in the client.
- **Test:** flood test with a slow consumer asserting the stream survives and resyncs.

#### REL-03 · P1 · [V] · Phase 2 — Progress events are routed to jobs by fuzzy name matching
- **Evidence:** `useConversionProgress.matchesJob` matches on substring and normalized-alphanumeric containment, and "single job in queue" matches unconditionally. A folder job matches on `filePath.includes(slug)`, so the first sub-document's `doc_complete` marks the whole folder job "completed". `report_1.pdf` vs `report_10.pdf` and slug collisions mis-route.
- **Fix:** The queue assigns `job_id`; `start_conversion` passes ids; every event carries `run_id` and `job_id` (folder children get child ids).
- **Test:** UI reducer tests with colliding names and folder children. Contract test that every backend event type contains the ids.

### 2.5 Configuration (CFG)

#### CFG-01 · P1 · [V] · Phase 4 — UI settings save clobbers config; round-trip bugs
- **Evidence:**
  - The TS `AppConfigData` lacks `models_dir`, `max_depth`, and `layout_model`. `save_config` deserializes the UI payload into `AppConfig` and writes the whole struct, so those keys are erased from `pdfmd-lite.toml`.
  - Rust serializes `save_table_images` (the serde alias `table_crops` works only on input). The UI reads `raw.table_crops`, finds nothing, and always falls back to `true`, so "Save Visual Table Crop Images" cannot be turned off persistently.
  - Desktop `get_config` swallows parse errors via `unwrap_or_default()`, so a malformed config is silently ignored.
  - Config is read from and written to CWD (see BLD-04).
  - `handleOutputDirChange` calls `saveConfig` on every keystroke in the output-dir text box.
- **Fix:** Generate TS types from Rust (`specta` or `ts-rs`). Make `save_config` merge into the existing file (`toml_edit`) and preserve unknown-to-UI keys. Store config in the per-user data dir. Surface parse errors in the UI. Debounce and save on blur.
- **Test:** round-trip test (every `AppConfig` field survives UI→save→load→UI). Test that saving from the UI preserves `models_dir`.

#### CFG-02 · P2 · [V] · Phase 4 — Dead and divergent settings
- **Evidence:**
  - `extract_tables` is read nowhere (only declared in `config.rs`). The UI checkbox "Extract Structured Tables into Markdown Grids" does nothing.
  - `log_level` and `log_file`: only the CLI calls `init_telemetry`. The GUI and server ignore them.
  - Defaults differ: CLI image quality 80 and table crops off; GUI/server 85 and on.
  - `image_quality` is never range-checked (help says 1–100).
  - `--pages abc` or `-p 5` is silently ignored (whole document converted).
  - An unknown `language` (e.g. "hi") silently becomes English in desktop/server/CLI (`parse().ok()` then `unwrap_or(English)`).
- **Fix:** Wire or remove each field. One `ResolvedSettings` struct built once from (defaults → file → CLI/UI) and used by every front end. Validate and reject bad values with a clear message.
- **Test:** table-driven tests for precedence and validation. CLI tests for malformed `--pages`.

### 2.6 UI completeness (UI)

#### UI-01 · P1 · [V] · Phase 4 — Dry-run dialog shows blanks; ignores most of the queue
- **Evidence:** TS `DryRunSummary` does not match Rust `DryRunReport`. The UI reads `total_pages`, `estimated_seconds`, `concurrency_workers`, and `estimated_memory_mb`; Rust sends `total_pages_to_process`, `estimated_time_secs` (a `[min,max]` tuple), `resolved_concurrency`, and `memory_budget_mb`. Only `total_documents` matches. The dialog renders empty values, and the log line prints `undefined`. `handleDryRun` sends only `jobs[0]`. The backend uses the on-disk config, not the unsaved UI config.
- **Fix:** Generated types plus a contract test. Dry-run the whole queue and show a per-file table (status `WOULD_CONVERT/SKIP/OVERWRITE`, pages, target). Pass the current UI config.
- **Test:** contract test that serializes `DryRunReport` from Rust into a fixture and type-checks it against the TS type. Component test with a multi-file report.

#### UI-02 · P1 · [V] · Phase 4 — Actions that report success while doing nothing
- **Evidence:** "Watch Directory" only logs "Directory watch mode active". "Export Markdown" only logs "Exported …". "Fix LaTeX" and "Format Notes" call the same backend routine (`_fix_latex` ignored).
- **Impact:** The UI states things that are untrue.
- **Fix:** Implement them or disable them with an explanatory tooltip. Export should save a copy via a native save dialog. Split "Fix LaTeX" into a real, narrower operation or remove it.
- **Test:** UI test asserting each menu item either invokes the bridge or is disabled. No log line says "active/exported" without a bridge call.

#### UI-03 · P0 · [V] · Phase 4 — Language list promises 11 scripts; the engine routes 2
- **Evidence:** `SettingsModal` offers en, hi, te, kn, ta, ml, mr, bn, gu, pa, or. `IndraScript::from_str` accepts only english/en/latin, telugu/te/world, auto. Everything else fails to parse and falls back to English with no message. There is no "Auto" option in the UI. `ModelsModal` claims Devanagari and Kannada support, which I could not verify from the vocabulary code I read (only `build_telugu_mask`, `has_telugu`).
- **Impact:** A Hindi or Tamil document is OCR'd with the Latin model and produces garbage that looks like a successful conversion.
- **Fix:** Populate the list from a backend `get_capabilities` that returns only scripts whose recognizer is loaded and verified. Add Auto. Reject unknown values with an error. Do not list scripts without a tested model.
- **Test:** UI test (list equals backend capabilities). Rust test (unknown language returns `Err`). A fixture per advertised script.

#### UI-04 · P1 · [V] · Phase 4 — Web-mode paths are broken
- **Evidence (`BatchQueue.tsx`, `api.ts`):**
  - Drag-and-drop and "Add Folder" in the browser push `file.path || file.name`. Browsers expose no `path`, so a bare name is queued and the server can only find it by accident via the `input/` search.
  - On upload failure it silently falls back to the bare name.
  - There is no read-note endpoint, so the inspector shows placeholder content (`readMarkdown` returns `''`).
  - The server's `BatchSummary` has no `documents`, so folder conversions have no per-file "Inspect".
  - In Tauri, if `get_page_preview` fails, the code returns a `/api/page-preview` URL that does not exist there.
  - `formatNotes`, `embedVault`, and `downloadModels` do not check `res.ok`. `saveConfig` (web) does not either, yet the UI logs "Configuration saved".
- **Fix:** Upload every dropped or picked file (and a folder as a batch of uploads), and report failures. Add `GET /api/note?id=` and `GET /api/asset?id=` scoped to run outputs. Unify the desktop and server event shapes. Check response status everywhere and show errors.
- **Test:** Playwright web-mode test: drop two PDFs and a folder, convert, inspect. Bridge unit tests for non-2xx handling.

#### UI-05 · P1 · [V] · Phase 4 — Markdown viewer fidelity
- **Evidence:** The hand-rolled renderer supports a subset: no lists, links, blockquotes, inline code, horizontal rules, or nested emphasis. `\*(.*?)\*` italicizes across asterisks in math and text. `![[dir/img.jpg]]` renders a text placeholder, not the extracted figure. HTML tables inside callouts are inserted raw.
- **Impact:** The inspector cannot show what the note looks like in Obsidian, which defeats its purpose of validating output.
- **Fix:** Part of SEC-01's parser swap. Resolve `![[...]]` through the asset endpoint or command, constrained to the note's folder. Test with real conversion outputs.
- **Test:** snapshot tests on rendered output of fixtures F01, F10, F11.

#### UI-06 · P1 · [V] · Phase 4 — Job states are inaccurate
- **Evidence:** `JobStatus` includes `skipped`, but no event sets it. There is no "completed with warnings" or "cancelled". After a folder batch, child rows get `completed` iff the note file exists, else `error`. A partial note from a cancelled run therefore shows green.
- **Fix:** Drive states from `ConversionReport` (COR-03). Add warning counts per row and a "Warnings" drawer.
- **Test:** reducer tests for every state transition.

#### UI-07 · P2 · [V] · Phase 4 — Concurrency and memory shown in the queue are guesses
- **Evidence:** `concurrencyWorkers={config.concurrency || 4}` is displayed while "auto" actually resolves from hardware. `estimatedMemoryMb = 350 + workers × 200` is recomputed in TS.
- **Fix:** Backend returns resolved values (part of the dry-run and run-start event).
- **Test:** UI test showing backend values.

### 2.7 UX ergonomics (UX)

#### UX-01 · P1 · [V] · Phase 5 — Accessibility is absent
- **Evidence:** Across all components there are zero `aria-*` attributes and zero `role=` attributes. Modals have no focus trap or Escape handling. Icon-only buttons (e.g. the modal close `X`) have no accessible name. Keyboard shortcuts are bound on `window` and fire while a modal is open. Status is conveyed by color and icon alone.
- **Fix:** Use an accessible dialog primitive (Radix or Headless UI) for all modals. Label icon buttons. Add `role="progressbar"` with values, `aria-live` for toasts and the log. Scope shortcuts to the main view. Honor `prefers-reduced-motion`.
- **Test:** `axe-core` in Playwright on every screen (zero serious or critical violations). Keyboard-only walkthrough test (open file, convert, inspect, close) with no mouse.

#### UX-02 · P1 · [V] · Phase 5 — No guardrails on destructive actions
- Covered by SEC-05. UX requirement: a confirm dialog states folder, file count, and what changes, with a "Preview" action and a visible "Backup created at …" result.
- **Test:** UI test that the bridge is not called until confirm.

#### UX-03 · P2 · [V] · Phase 5 — Missing basics
- No persistence of the queue, theme, or window state (no storage use found). No retry-failed, no pause, and no "open note" or "open folder" per row beyond the inspector. Errors are shown as raw `${err}` strings. Output dir defaults to a CWD-relative `./output` with no writability check.
- **Fix:** Persist the queue and theme in the data dir. Add "Retry failed", "Open note", "Reveal in folder". Map errors to plain-language messages with a "Copy details" action. Validate the output directory when chosen.
- **Test:** component tests for each action. Restart test restoring the queue.

#### UX-04 · P2 · [V] · Phase 5 — First-run experience
- **Evidence:** No startup diagnostics. If a required model is missing or an optional one is absent, the user finds out from a raw error, or never (silent degradation).
- **Fix:** First-run "System check" screen driven by `get_model_status` and `get_capabilities` (OFF-02), showing what works now and what is degraded, with the exact folder to place models in.
- **Test:** UI tests for all-ok, optional-missing, and required-missing states.

#### UX-05 · P3 · [V] · Phase 5 — CLI jargon in GUI labels
- Labels such as "Resume (-r)", "Language / Script (-l)", and "(--embed-images-base64)" expose flags to GUI users. Use plain labels with the flag in a tooltip.

#### UX-06 · P2 · [V] · Phase 5 — Logs and warnings are invisible
- **Evidence:** Tracing output is not forwarded to the UI. Only emitter messages appear in the Log Console. Missing-model warnings go only to tracing.
- **Fix:** A `tracing` layer that forwards warn/error into the progress channel, plus the report warnings from COR-03.
- **Test:** UI test that a degraded run shows a warning banner.

### 2.8 Build, release, and test infrastructure (BLD)

#### BLD-01 · P1 · [V] · Phase 1 — A fresh clone cannot build with default features
- **Evidence:** `embed-models` is a default feature and `model_locator.rs` `include_bytes!`s six files under `models/`, which `.gitignore` excludes. The root `build.rs` (which checks model presence) sits at the workspace root, which has no `[package]`, so Cargo never runs it. The `engine` crate has no `build.rs`.
- **Impact:** Cryptic compile errors for any new developer or CI runner.
- **Fix:** Move the check into `crates/engine/build.rs` with a clear message and the path to the provisioning command. Split features: `embed-models` off for dev and on for release. Add `cargo xtask fetch-models --from <dir>` that verifies the manifest.
- **Test:** CI job on a clean checkout: `cargo check --workspace` without models (dev profile) and `cargo build --release --features embed-models` with the provisioned bundle.

#### BLD-02 · P2 · [V]/[L] · Phase 1 — `deny.toml` is outdated
- **Evidence:** It uses keys that current cargo-deny removed (`vulnerability`, `unlicensed`) and the old git index URL in `allow-registry`. The license allow-list omits licenses common in Tauri/ICU dependency trees (e.g. MPL-2.0, Unicode-3.0) [L: confirm with `cargo deny check licenses`].
- **Fix:** Regenerate with `cargo deny init`, then tune. Run `cargo deny check` and `cargo audit` in CI.
- **Test:** CI gate.

#### BLD-03 · P1 · [V] · Phase 1 — Test infrastructure cannot prove correctness
- **Evidence:**
  - There is no `tests/` directory and no committed fixture PDFs. About 110 unit tests, nearly all on string helpers (28 in `markdown.rs`).
  - Several tests pass vacuously by returning early when a file or model is missing: three `pdf.rs` tests (personal `F:\…` PDFs), `test_indra_engine_e2e`, a SLANet test.
  - Other tests need models and fail without them.
  - The UI has no test runner, no lint script, and no typecheck script separate from the build.
  - The zip contains no `.github/` or other CI config, so the gates described in `AGENTS.md` cannot be verified.
- **Fix:** Section 4. Vacuous tests become failing tests (or `#[ignore]` with a reason and a required-in-CI job).
- **Test:** CI report listing tests skipped. Gate: zero silent-skips.

#### BLD-04 · P0 · [V] · Phase 0 — Standard users cannot run the installed app correctly
- **Evidence:** `wix/main.wxs` sets shortcut `WorkingDirectory="INSTALLFOLDER"` (Program Files). The app writes CWD-relative: `pdfmd-lite.toml` (`save_config`), `crash.log`, `temp/uploads`, `models/embeddings` (downloader), and defaults output to `./output`.
- **Impact:** On a locked-down machine, saving settings, crash logging, uploads, and default output all fail or fail silently.
- **Fix:** Resolve a per-user data directory once (Tauri path API or `dirs`) and use it for config, logs, uploads, cache, and models override. Add `--data-dir`. Default output to the user's Documents folder. Decide per-user vs per-machine install explicitly.
- **Test:** Windows CI/VM test: install the MSI, run as a non-admin user, save settings, convert a PDF, trigger a panic, and assert files land under the data dir and nothing is written to Program Files.

#### BLD-05 · P2 · [V] · Phase 6 — Release hygiene
- Version `0.7.2` is duplicated in the workspace manifest, `tauri.conf.json`, `wix/main.wxs`, and `ui/package.json`. `package-msi.ps1` extracts the version with a first-match regex over `Cargo.toml`. The MSI is not signed. No SBOM or checksums are published.
- **Fix:** Single source of truth (workspace version) with a CI check that all four match. Sign the MSI. Publish SHA256SUMS and a CycloneDX SBOM (`cargo cyclonedx`).
- **Test:** CI version-consistency script. A job that verifies the signature and checksum of the built MSI.

### 2.9 Documentation drift (DOC)

#### DOC-01 · P2 · [V] · Phase 6 — Docs contradict code
- `AGENTS.md` links an ADR 0003 file name that does not exist (the file is `0003-resource-aware-dynamic-concurrency.md`).
- ADR 0012 calls Table "Class 1" while ADR 0002 and `yolo.rs` use class 8.
- ADR 0014's model precedence differs from `ModelLocator` (OFF-05).
- ADR 0015 specifies `--device` (OFF-08), which does not exist.
- `KNOWN_MODELS` lists `unimernet.onnx` and `bge_small_int8.onnx`. I found no loader for either (the embedding resolver looks only for `model.onnx` / `all-MiniLM-L6-v2.onnx`; LaTeX loads `encoder.onnx`/`decoder.onnx`).
- README: "pure-Rust" (OFF-07), "100% offline" (OFF-01).
- **Fix:** Reconcile each, and add a CI link-check plus a test that every `KNOWN_MODELS` entry has a loader.

---

## 3. Phased implementation plan with pass gates

Each phase ends with a **gate**. Do not start the next phase until every gate row passes. Phase 0 can be done in parallel by two people (security and engine).

Effort: S ≤ 1 day, M ≈ 2–4 days, L ≈ 1–2 weeks (one developer).

### Phase 0 — Containment (stop silent harm)

**Scope:** SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-07, OFF-01, COR-01, COR-02, COR-04, COR-05 (disable by default), BLD-04.
**Effort:** L total (SEC-01 M, SEC-02 M, SEC-03 M, COR-01 M, BLD-04 M, rest S).
**Deliverables:** parser swap plus CSP; token/Origin/Host checks on the daemon; `PathPolicy`; offline downloader removed from default builds; atomic `.partial` plus fingerprinted resume; render-error propagation; repetition-based header removal; heading regexes opt-in; data-dir migration.

| Gate | Check | Pass condition |
|---|---|---|
| G0.1 | UI XSS corpus (`npm test -- xss`) | 0 payloads yield executable DOM |
| G0.2 | `tauri.conf.json` assertion | CSP non-null, `withGlobalTauri == false` |
| G0.3 | Server security tests (`cargo test -p pdfmd-lite-server security`) | evil Origin 403, no token 401, bad Host 421, path escape 403 |
| G0.4 | Path policy tests | all traversal/UNC/symlink cases denied |
| G0.5 | Offline static gate (`scripts/gate-offline.sh` or `.ps1`) | no `curl`/HTTP client/URL literal outside allow-list |
| G0.6 | Resume tests: kill-mid-run, hashless-note, changed-options | partial notes are never skipped; fingerprint change reconverts |
| G0.7 | Fixtures F13/F14/F15 | each returns failed status and a clear error, never an empty success |
| G0.8 | Fixture F06 | every dated line preserved; numbered list not promoted to headings |
| G0.9 | Non-admin MSI test on a clean VM | nothing written under Program Files; settings persist |
| G0.10 | `cargo test --workspace` plus `npm test` | green, with zero skipped tests |

**Rollback:** each item is behind a small PR. The resume change bumps a frontmatter schema field (`pdfmd_schema: 2`) so old notes are reconverted rather than trusted.

### Phase 1 — Offline guarantee and model integrity

**Scope:** OFF-02, OFF-03, OFF-04, OFF-05, OFF-08, BLD-01, BLD-02, BLD-03 (foundation).
**Effort:** L.
**Deliverables:** `MANIFEST.toml` and verifier; `ModelRegistry`; `get_model_status`/`/api/models`; fail-fast capability errors and `--allow-degraded`; reloadable engines; `device` setting; per-crate `build.rs`; refreshed `deny.toml`; CI skeleton; fixture generator and golden harness skeleton.

| Gate | Check | Pass condition |
|---|---|---|
| G1.1 | Manifest tests | tampered/truncated/wrong-shape model rejected with a specific error |
| G1.2 | `pdfmd-lite --doctor` | exit 0 only if all required models verify; non-zero otherwise (CI test both ways) |
| G1.3 | Locator parity test | doctor-reported source equals loaded source for every model under every precedence layer |
| G1.4 | Telugu capability tests | Telugu without `rec_world` → error by default; with `--allow-degraded` → report warning |
| G1.5 | Network-denied run | full conversion of F01 and doctor succeed with zero outbound connection attempts |
| G1.6 | Models dialog contract test | renders ok/missing/mismatch from backend data; no hard-coded status remains (grep gate) |
| G1.7 | Clean-checkout CI | `cargo check --workspace` passes without models; release build passes with provisioned bundle |
| G1.8 | `cargo deny check` and `cargo audit` | pass |
| G1.9 | Determinism | CPU EP: two runs byte-identical; `-j 1` equals `-j 4` on F01 and F09 |

### Phase 2 — Run integrity, reporting, and accounting

**Scope:** COR-03, COR-10, COR-13, COR-18, COR-19, REL-01, REL-02, REL-03.
**Effort:** L.
**Deliverables:** `ConversionReport`; `OutputPlanner`; `RunController` state machine; run and job ids on all events; reconnect resync; calibrated ETA; accurate batch counts; watch-mode readiness.

| Gate | Check | Pass condition |
|---|---|---|
| G2.1 | Report snapshot tests (F01..F11) | stable JSON; statuses correct including `completed_with_warnings` |
| G2.2 | CLI exit codes | 0 / 2 / 1 as specified |
| G2.3 | OutputPlanner properties | unique, Windows-valid, stable slugs; same-stem F18 no overwrite |
| G2.4 | Run-control tests | forced panic then new run accepted; double-start rejected; cancel within N s on a 50-page fixture |
| G2.5 | UI reducer tests | colliding names and folder children route by id; Stop shows "Cancelling" then "Cancelled" |
| G2.6 | WebSocket flood test | stream survives lag and resyncs |
| G2.7 | ETA accuracy | after warm-up, within the tolerance recorded in the baseline file |
| G2.8 | Batch accounting tests | succeeded/skipped/failed/cancelled correct; CLI summary golden |

### Phase 3 — Content fidelity

**Scope:** COR-06, COR-07, COR-08, COR-09, COR-11, COR-12, OFF-06.
**Effort:** L (COR-08 is the largest).
**Deliverables:** DOM extractor upgrade with completeness check and per-page fallback; positional formula insertion; taxonomy removed or opt-in; safer de-hyphenation; table/figure reconciliation; detector benchmark and tuning; token-based chunking and a decision on a multilingual embedding model.

**Step 3.0 (first, S):** run the full fixture corpus through the current build and commit `tests/baseline.json` (per-fixture CER/WER, table TEDS, heading F1, reading-order score, time, peak RSS). This confirms or kills every [L] item in Section 2 and gives data for thresholds. I have no corpus numbers, so I have not set absolute thresholds here.

| Gate | Check | Pass condition |
|---|---|---|
| G3.1 | Baseline recorded | `tests/baseline.json` committed; every [L] finding marked confirmed or rejected |
| G3.2 | No-regression | every metric ≥ baseline for all fixtures; absolute floors set from baseline and documented |
| G3.3 | DOM fidelity (F01, F02, F03, F07, F08, F09) | ligatures present, word boundaries correct, bbox within tolerance of render |
| G3.4 | F11 | no `?` placeholders; repeated short lines handled by position |
| G3.5 | F05/F06 metadata | no fabricated `domain/*` tags or authors |
| G3.6 | F10 | no duplicate figure for table regions; table structure score ≥ baseline |
| G3.7 | Chunking properties | no chunk exceeds token limit (English, Telugu, mixed) |
| G3.8 | Retrieval smoke set | recall@5 ≥ baseline on the bilingual set |
| G3.9 | Perf | peak RSS and startup time ≤ baseline (+ stated tolerance) |

### Phase 4 — UI contract and completeness

**Scope:** CFG-01, CFG-02, UI-01 … UI-07.
**Effort:** L.
**Deliverables:** generated TS types and contract tests; merged config saves; capability-driven language list; real dry-run dialog; honest menu actions; web-mode parity (upload, note/asset endpoints, events); parity of event shapes between Tauri and web; state model from `ConversionReport`.

| Gate | Check | Pass condition |
|---|---|---|
| G4.1 | Contract tests | generated types match Rust serialization (`git diff --exit-code` on generated files) |
| G4.2 | Config round trip | every field survives UI→save→load→UI; `models_dir` preserved |
| G4.3 | Language list test | equals backend capabilities; unknown → error |
| G4.4 | Dry-run component test | all fields populated; multi-file table shown |
| G4.5 | Menu honesty test | no item logs success without a bridge call |
| G4.6 | Playwright web mode | drop PDFs and a folder, convert, inspect, view figures |
| G4.7 | Dead-setting audit | every UI setting is read by the backend (test enumerates `AppConfig` fields) |

### Phase 5 — UX and accessibility

**Scope:** UX-01 … UX-06 (and the confirmation UI for SEC-05).
**Effort:** M–L.

| Gate | Check | Pass condition |
|---|---|---|
| G5.1 | axe-core on every screen | 0 serious/critical violations |
| G5.2 | Keyboard-only walkthrough | open, convert, inspect, close without a mouse |
| G5.3 | Destructive-op tests | bridge not called before confirm; backup path shown |
| G5.4 | Persistence tests | queue and theme restored after restart |
| G5.5 | First-run check tests | all-ok, optional-missing, required-missing states |
| G5.6 | Warning visibility | degraded run shows banner and per-document warnings |
| G5.7 | Moderated usability check | 3 target users complete convert → inspect → open output unaided in under 5 minutes (record issues) |

### Phase 6 — Performance, resource safety, and release hardening

**Scope:** COR-14, COR-15, COR-16, COR-17, SEC-06, OFF-07, BLD-05, DOC-01.
**Effort:** M–L.

| Gate | Check | Pass condition |
|---|---|---|
| G6.1 | Resource tests (F12 huge page, F16 image bomb, F19 300 pages) | clean completion or failure with report; peak RSS under cap; near-linear time growth |
| G6.2 | Parse-count test | ≤ 2 parses per document |
| G6.3 | `clippy` print gate | no `print_stdout`/`print_stderr` in engine |
| G6.4 | Offline build | `cargo vendor` then `cargo build --offline --workspace --locked` passes; UI builds from vendored npm cache |
| G6.5 | Release checks | versions consistent; MSI signed; SHA256SUMS and SBOM generated |
| G6.6 | Docs gate | link check passes; every `KNOWN_MODELS` entry has a loader; README claims match scorecard |
| G6.7 | Fuzz smoke | each fuzz target runs 10 minutes without crash |

---

## 4. Test strategy

### 4.1 Layers

| Layer | Tooling | What it proves |
|---|---|---|
| Unit | `cargo nextest`, `proptest` | slugs, resume fingerprint, chunking, config precedence, path policy, de-hyphenation, heading rules |
| Golden | `insta` snapshots on fixture PDFs | note output, `ConversionReport`, frontmatter; reviewed diffs only |
| Metrics | custom harness (`cargo xtask eval`) | CER/WER, table TEDS, heading F1, reading-order score, time, peak RSS vs `baseline.json` |
| Fuzz | `cargo-fuzz` | `parse_cmap_str`, `parse_existing_frontmatter`, `normalize_latex_delimiters`, content-stream parser, upload parser |
| Contract | `specta`/`ts-rs` generated types | Rust ⇄ TS shapes (config, events, dry-run, model status) |
| UI unit/component | Vitest + React Testing Library with a mocked bridge | reducers, dialogs, settings, XSS corpus |
| E2E web | Playwright against `pdfmd-lite --serve` (with token) | upload, convert, inspect, cancel, axe checks |
| E2E desktop | `tauri-driver` (WebDriver) on Windows | pick/drop, convert, settings persistence, non-admin install |
| Security | handler tests, XSS corpus, path-policy tables | SEC-01..05 |
| Offline | network-denied run (`unshare -rn`, Windows firewall rule) plus static gate | no network at runtime |
| Static | `clippy -D warnings`, `cargo deny`, `cargo audit`, ESLint (`jsx-a11y`), `tsc --noEmit` | hygiene |
| Coverage | `cargo llvm-cov` | line coverage on non-ML engine modules, with a floor set after baseline |

### 4.2 Fixture corpus (commit small, synthetic or licensed-clean PDFs)

| ID | Content | Used for |
|---|---|---|
| F01 | LaTeX paper, OT1 fonts, ligatures, headers, equations | COR-08, offline run, determinism |
| F02 | Word export | COR-08 |
| F03 | LibreOffice export | COR-08 |
| F04 | Scanned English A4 | OCR, COR-11 |
| F05 | Scanned Telugu government order | OFF-04, UI-03, COR-06 |
| F06 | Government order: numbered list, dated lines, initials in names, letterhead | COR-04, COR-05, COR-06 |
| F07 | Rotated page | COR-08 |
| F08 | CropBox/MediaBox with non-zero origin | COR-08 |
| F09 | Two-column paper | reading order, determinism |
| F10 | Tables: ruled, unruled, merged cells | COR-12 |
| F11 | Inline and display formulas, repeated short lines | COR-09 |
| F12 | Huge MediaBox | COR-17 |
| F13 | Encrypted | COR-02 |
| F14 | Corrupt xref | COR-02 |
| F15 | Zero pages | COR-02 |
| F16 | Image decompression bomb | COR-17 |
| F17 | Text layer containing HTML/script payloads | SEC-01 |
| F18 | Two PDFs with the same stem in different folders | COR-10 |
| F19 | 300-page document | COR-15, perf |

Each fixture ships with `expected.md` and `expected.report.json`. Remove all tests that reference personal paths.

### 4.3 Proposed CI jobs

1. `lint`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `cargo audit`, UI `npm run lint && npm run typecheck`.
2. `test-engine`: `cargo nextest run --workspace --locked` (models provisioned from an internal artifact via `cargo xtask fetch-models`).
3. `test-ui`: `npm ci --ignore-scripts && npm test`.
4. `contract`: regenerate TS types, then `git diff --exit-code`.
5. `e2e-web`: build, start `--serve`, run Playwright and axe.
6. `offline`: network-denied conversion and doctor, plus the static gate.
7. `eval` (nightly or per-PR on engine paths): `cargo xtask eval` compared to `baseline.json`.
8. `fuzz-smoke` (nightly): 10 minutes per target.
9. `windows-installer` (release branches): build MSI, install on a clean VM as non-admin, smoke test, sign, checksum, SBOM.
10. `docs`: link check and the model-loader coverage test.

### 4.4 Rules to keep tests honest

- A test that cannot run (missing file or model) **fails** in CI. No early `return`. Use `#[ignore = "reason"]` only for local-only tests, and run ignored tests in a dedicated required job.
- No absolute paths. No dependence on the current working directory.
- Golden updates require reviewer approval. Metric baselines move only through a dedicated PR.

---

## 5. Quick wins (≤ 1 day each, do these first)

1. Delete root `pdfmd-lite.toml` and the committed `*.log`/`check_output.txt` files. Add them to `.gitignore`.
2. Make `doctor` exit non-zero when a required model is missing.
3. Make an unknown `--lang` / UI language an error instead of English.
4. Stop printing "Directory watch mode active" / "Exported …" from stubs. Disable the menu items.
5. Replace the hard-coded Models dialog statuses with "Unknown — status check not available" until OFF-02 lands.
6. Fix the dry-run dialog field names (`total_pages_to_process`, `estimated_time_secs`, `resolved_concurrency`, `memory_budget_mb`).
7. Join the render thread and propagate its error (COR-02).
8. Delete the month/`198`/`199`/`200` rule in `strip_running_headers_and_footers`.
9. Debounce the output-dir `saveConfig` call and stop it from running on every keystroke.
10. Replace `CorsLayer::permissive()` with same-origin and bind to loopback only.

## 6. Open questions I cannot answer from the repo

- Which PDF generators and languages dominate your client documents? This decides how much COR-08 matters and whether Hindi, Tamil, or Kannada models are needed.
- Is a per-user or per-machine MSI install required by your clients' IT policy? (BLD-04)
- Is the web daemon a supported deployment, or only a developer convenience? If only the latter, SEC-02 can be satisfied by removing it from release builds.
- Which embedding model is acceptable under your licensing and size limits for Telugu retrieval? (OFF-06)
- Do clients require signed binaries and an SBOM? (BLD-05)
