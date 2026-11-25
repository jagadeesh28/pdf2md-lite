# pdfmd-lite — Repository Audit & Phased Implementation Recommendations

**Audit date:** 2026-11-24  
**Scope:** Full supplied repository ZIP plus the nested `ui_src.zip` React/Tauri frontend  
**Primary symptoms reported:** Incorrect reading order and text-parsing errors, including simple single-page PDFs  
**Audit type:** Static source/configuration/test review, with attempted local build checks  
**Status:** Recommendations only; no source files were modified

---

## 1. Executive assessment

The observed failures are consistent with several weaknesses in the current extraction architecture. The issue is not confined to the final line sorter. The current digital-text path reconstructs text from a limited subset of PDF operators, approximates glyph and line geometry, applies a geometry-only XY-cut ordering pass, and then flattens the result into plain text. Errors introduced in the PDF text-state reconstruction therefore become reading-order errors and are difficult for later stages to detect or repair.

There are also release-readiness issues which should be addressed before treating the application as a dependable offline PDF-to-Markdown tool:

1. **Untrusted document text is inserted into HTML without sanitization.** The Markdown viewer transforms PDF-derived content and injects it with `dangerouslySetInnerHTML`; the Tauri Content Security Policy is set to `null`. A PDF containing HTML-like text can therefore reach an active WebView context.
2. **The PDF text parser is incomplete for general PDF text semantics.** Its eligibility checks recognize text-show operators that the parser does not fully implement. It does not reconstruct all text/graphics state needed for reliable placement and uses approximate character-count geometry.
3. **Reading order is geometry-only and loses structure.** The XY-cut result is flattened, and a transition from the bottom of one column to the top of another can be serialized as a space rather than a paragraph/column boundary. The included order model is listed as required but is not used for inference in the inspected path.
4. **DOM-vs-OCR selection is page-wide and insufficiently quality-aware.** Any non-empty DOM extraction can prevent OCR from augmenting the page, even where the DOM text is incomplete or garbled. Table routing also does not match the documented ADR behavior.
5. **Offline-model claims are not backed by a complete, verifiable model lifecycle.** Default features refer to embedded model files absent from the supplied archive; model status in the UI is hard-coded; a separate downloader obtains some assets online without strong integrity validation; and runtime/provider status is not consistently exposed.
6. **The browser UI is not functionally at parity with the desktop UI.** Markdown inspection returns an empty string in browser mode, browser folder/drop handling passes local filenames rather than uploading their bytes, and several menu actions are placeholders or no-ops.
7. **The supplied source bundle is not a reproducible clean-checkout build by itself.** The UI is nested as a ZIP rather than present at the path expected by desktop build scripts, model blobs are absent, and the Tauri build command is Windows-specific.

**Recommendation:** Do not attempt to fix reading order by tuning XY-cut thresholds alone. First establish a reproducible build and a non-skipping PDF regression corpus; then correct PDF text-state reconstruction and the block/reading-order representation; then add quality-aware per-region OCR fallback. In parallel, close the WebView injection and local-server exposure risks, and make model availability/status truthful.

### Priority overview

| Priority | Finding | Why it matters | Required release posture |
|---|---|---|---|
| **P0** | PDF-derived Markdown is inserted as unsanitized HTML | Crafted input can execute content in the app WebView | Fix and security-test before release |
| **P0** | Build inputs are incomplete / not reproducible from supplied source bundle | A clean checkout cannot reliably reproduce the advertised build | Make dependencies/assets and build steps explicit |
| **P1** | Incomplete PDF text-state and CMap handling | Incorrect characters and coordinates are produced before ordering | Fix before claiming general digital-PDF fidelity |
| **P1** | Geometry-only ordering and flattening | Multi-column and mixed-layout pages can be joined in the wrong order | Introduce structured blocks and golden ordering tests |
| **P1** | Page-wide DOM acceptance suppresses OCR fallback | Partial or corrupt text layers can silently win | Add coverage/confidence-based hybrid routing |
| **P1** | Model inventory/status/download guarantees are inaccurate | “Offline”, “installed”, and “verified” can be shown without proof | Make model manifest and status runtime-derived |
| **P1** | Server accepts broad filesystem paths and permissive CORS | Exposure beyond localhost can enable unsafe local file operations | Restrict paths and harden network boundary |
| **P1** | Browser upload, Markdown inspection, and progress parity gaps | Core user workflows fail or report misleading status | Bring browser and desktop behavior to parity |
| **P2** | Feature affordances, accessibility, and ergonomics are incomplete | Users encounter dead controls and fragile workflows | Include in UI completion phase |

---

## 2. Audit scope and evidence

### 2.1 Supplied source inventory

The supplied `pdfmd-lite.zip` contains the Rust workspace, documentation, architecture decision records, historical recommendations/logs, packaging files, and a nested `ui_src.zip`. The nested UI archive was extracted and reviewed separately because the desktop build expects a top-level `ui/` directory.

Reviewed areas include:

- Workspace manifests, lockfile, build script, CLI, server, desktop/Tauri, and engine crate.
- Engine PDF extraction, font/CMap decoding, line-box construction, XY-cut, Indra ordering, pipeline routing, model discovery, model download, embeddings, table/formula/figure integration, and configuration.
- UI application, API bridge, batch queue, Markdown viewer, split pane, model/settings/about/shortcuts/menu modals, conversion-progress hook, styling, and package/build configuration.
- README, `AGENTS.md`, ADRs 0001–0016, phase/recommendation documents, and prior audit/log artifacts.

The archive contains **no PDF regression corpus** and **no model weight files** such as the referenced ONNX assets. The code review covered the complete supplied source/configuration set, not generated binaries or the behavior of real customer PDFs.

### 2.2 Validation that could be performed in this environment

| Check | Result | Interpretation |
|---|---|---|
| Archive extraction and source inventory | Completed | Both outer repository and nested UI source were inspected |
| Rust workspace check | Not runnable | `cargo`/`rustc` are not installed in this environment; the command was attempted and could not start |
| UI production build | Blocked | `npm run build` was attempted, but dependencies were not installed (`node_modules` absent); TypeScript imports therefore could not resolve. This is **not** evidence of an intrinsic source compile failure |
| UI automated tests | None found | The UI package has no test script or test suite in the supplied source |
| Rust unit tests | Source review only | Rust tests exist, but could not be executed here |
| PDF fixture integration tests | Inadequate | The supplied repository has no PDF fixture corpus; several path-based tests return early when their local Windows fixture is absent |
| CI configuration | Not found | No repository CI workflow was present in the supplied tree |

There are old build/test log files in the repository. They are not a substitute for a reproducible current run: some check output reports an older successful build, while test logs are incomplete compilation output. Do not use these logs as current release-gate evidence.

### 2.3 Important limits

This is a source-level audit. It identifies concrete code-path risks and test gaps, but does not claim to have measured character error rate (CER), word error rate (WER), reading-order accuracy, model latency, or real PDF output quality. Those measurements must be established in Phase 0 using a curated corpus and reproducible toolchain.

---

## 3. Detailed findings and recommendations

## 3.1 P0 — Sanitize PDF-derived content before rendering in the WebView

**Affected file:** `ui/src/components/MarkdownViewer.tsx` (notably the content-to-HTML conversion and `dangerouslySetInnerHTML` rendering path)  
**Related configuration:** `crates/desktop/tauri.conf.json` (`security.csp: null`)

### Finding

The viewer transforms the Markdown string using regular expressions and inserts the resulting string into the DOM with `dangerouslySetInnerHTML`. Code-block text is escaped in a helper, but ordinary text, headings, tables, callout title/body content, and URL attributes do not receive equivalent comprehensive sanitization. The document text originates from user-selected PDFs and must be treated as hostile input. The Tauri configuration disables CSP by setting it to `null`.

This creates an HTML/script injection path from a crafted PDF into the application renderer. It is not sufficient to assume extracted content is Markdown or that it cannot contain HTML-significant characters.

### Required implementation

1. Replace the home-grown Markdown-to-HTML regex pipeline with a maintained Markdown parser configured to disable raw HTML unless there is a deliberate, safe product requirement for it.
2. Sanitize the final HTML with a maintained sanitizer (for example, DOMPurify) using a narrow allowlist of elements and attributes required by the viewer.
3. Allow only safe URL protocols for links and images (`https:`, `http:` where appropriate, and carefully scoped local asset URLs). Reject `javascript:`, `data:` where not necessary, `file:`, and unexpected SVG/data payloads.
4. Keep KaTeX/math rendering, callouts, tables, and code blocks working through explicit safe renderers rather than regex-inserting raw source fragments.
5. Configure a restrictive Tauri CSP. Do not regard CSP as a replacement for sanitization; use both.
6. Add a clear error/fallback state if parsing or sanitization fails. Never silently fall back to raw HTML.

### Required tests

- A Markdown body containing `<img src=x onerror=...>` renders as inert text or is removed; no event executes.
- Raw `<script>`, `<iframe>`, SVG with event handlers, and HTML in table cells/callout bodies are inert.
- Links using `javascript:`, `data:`, and `file:` are rejected or rendered as non-clickable text.
- Benign headings, nested lists, tables, code blocks, callouts, and supported math still render correctly.
- Add a WebView/Tauri smoke test where supported, not only a pure string-unit test.

**Passing gate:** No user-controlled PDF-derived string can create executable DOM nodes, event attributes, script URLs, or unapproved navigation targets in desktop or browser rendering.

---

## 3.2 P0 — Make a clean-checkout build reproducible

**Affected files:** `Cargo.toml`, `crates/engine/Cargo.toml`, `crates/engine/src/model_locator.rs`, root `build.rs`, `crates/desktop/build.rs`, `crates/desktop/tauri.conf.json`, `ui_src.zip`

### Finding

The default engine feature set enables embedded-model functionality. `model_locator.rs` uses compile-time `include_bytes!` for multiple model assets, but the supplied archive contains no corresponding model directory or model weights. The root build script can emit warnings, but it cannot make a missing compile-time `include_bytes!` asset available.

The UI source is included as `ui_src.zip`, not as the top-level `ui/` directory expected by the desktop build script and Tauri `frontendDist`. The desktop Tauri build command also invokes `npm.cmd`, which is Windows-specific. Therefore, the supplied bundle is not self-contained as a reproducible clean-checkout build without undocumented/manual preparation.

### Required implementation

1. Decide whether model files are committed, downloaded as a separate versioned artifact, or supplied through a documented provisioning command. Do not leave required build assets implicit.
2. Add a model asset manifest containing exact filename, model role, version/revision, byte size, SHA-256, license/source URL, runtime compatibility, architecture, and required/optional status.
3. Separate **compile-time embedded** assets from **runtime-provisioned** assets. A no-default-features build must not unexpectedly require absent embedded blobs.
4. Either commit the canonical UI source under `ui/`, or add a deterministic, documented extraction step that is invoked by all build paths. Do not rely on a developer manually unzipping the nested archive.
5. Use a platform-neutral package script (`npm run build` through the package manager/platform shell) rather than hardcoding `npm.cmd`, unless Windows-only distribution is an explicit and documented constraint.
6. Add CI from a clean checkout that provisions exactly the required assets and records checksums.

### Required tests

- Clean checkout with no model assets: no-default-features compile succeeds, and the error for a deliberately requested unavailable inference feature is explicit and actionable.
- Provisioned model checkout: default-feature compile succeeds and verifies all expected model hashes.
- Clean checkout builds the UI without manual archive extraction.
- Windows build and at least one non-Windows CI build succeed if cross-platform support is claimed. If Windows-only is intended, state that explicitly and test only the supported target.

**Passing gate:** A new machine can follow one documented command sequence from the supplied source bundle to a reproducible app build; all required assets are either in the bundle or retrieved through an explicit, verifiable provisioning step.

---

## 3.3 P1 — Rebuild the PDF text parser around actual PDF text state

**Affected file:** `crates/engine/src/pdf.rs` (DOM gate, font decoder, CMap parsing, content-stream parsing, line-box construction)

### Findings

The current parser implements a limited subset of PDF text operators and does not reconstruct all state that controls visible glyph placement. Specifically:

- The DOM eligibility gate recognizes `Tj`, `TJ`, `'`, and `"` as text operations, but the inspected content parser does not implement the single-quote and double-quote text-show operators equivalently. A page can therefore be considered viable for DOM extraction even when some of its text-show commands are not actually parsed.
- Text positioning is reconstructed from a limited set of `BT`, `ET`, `Tf`, `Ts`, `TL`, `Tm`, `Td`, `TD`, `T*`, `Tj`, and `TJ` behavior. The parser does not comprehensively track graphics-state transforms (`q`, `Q`, `cm`), text character/word spacing (`Tc`, `Tw`), horizontal scaling (`Tz`), rise/leading interactions, and rotation/coordinate transforms.
- `Tj`/`TJ` text matrix advancement is approximated rather than calculated using glyph widths, character spacing, word spacing, font size, horizontal scale, and TJ adjustments. Numeric TJ adjustments are reduced to a heuristic space decision.
- Line geometry is estimated using character count multiplied by a fixed font-size factor, with a synthetic height. This is not a reliable substitute for glyph metrics or text rendering matrices.
- Type0/CID decoding uses a fixed two-byte assumption in the inspected decoder path. General CMap source-code spaces can use variable-length codes; simple ASCII fallback guesses can conceal unsupported font encodings.
- CMap parsing covers only a subset of `bfchar`/`bfrange` cases and does not fully handle codespace ranges, inherited CMaps, `usecmap`, array-form `bfrange` mappings, or all Unicode/surrogate mapping cases.
- When a CMap has some mappings but not others, unmapped character codes can be silently omitted. A partial map can thus yield plausible-looking but incomplete text.
- Some decompression failures fall back to treating raw compressed bytes as content and converting lossily. A malformed or unsupported stream can become garbage text rather than a structured parse error.
- The page coordinate path obtains page bounds but the line parser primarily uses page height; crop-box origin offsets, page rotation, and transforms are not consistently represented.
- The emitted line score is a fixed high value rather than a measured confidence/coverage score. Downstream code is therefore unable to distinguish approximate geometry from a verified text span.

### Required implementation

1. **Choose the parser boundary deliberately.** Prefer a mature PDF text extraction/parser library for font decoding, text operators, and text rendering matrices if it can be integrated without compromising the offline/packaging requirements. If retaining the custom parser, implement and document the PDF operators and font encodings supported, and return explicit `Unsupported`/`Partial` outcomes for the rest.
2. Track the PDF text state and graphics state separately. At minimum, model text and line matrices, CTM, font and font size, character spacing, word spacing, horizontal scaling, leading, rise, rendering mode, and page rotation/crop-box offsets.
3. Calculate text advance using actual font metrics and TJ adjustments. Preserve per-span source codes, decoded Unicode, baseline, bbox, font, and render state.
4. Implement CMap codespace lengths and proper source-code tokenization; support the relevant `bfchar`, `bfrange` (including array form), inherited/usecmap behavior, and UTF-16BE destination decoding. Do not fall back to printable-ASCII guesses when the font encoding is unknown.
5. Distinguish `Decoded`, `PartiallyDecoded`, `UnsupportedEncoding`, `MalformedStream`, and `NoText` results. Preserve replacement markers and diagnostics rather than silently dropping glyphs.
6. Remove synthetic confidence values. Derive confidence/coverage from decoded glyph fraction, font map coverage, valid geometry, text operator support, and consistency checks.
7. Treat malformed streams as explicit parse failures or partial results; never reinterpret compressed bytes as plain text without a confirmed filter/decompression path.

### Required tests

Generate small synthetic PDFs (rather than relying on local user-specific PDFs) that cover:

- `Tj`, `TJ`, single-quote, and double-quote text operators.
- Multiple text objects, `Tm`, `Td`, `TD`, `T*`, leading, rise, and text matrix movement.
- Character/word spacing, horizontal scaling, and non-default font sizes.
- `q/Q/cm` transforms, rotated pages, non-zero CropBox/MediaBox origins, and mixed transforms.
- Type1/simple fonts with WinAnsi/StandardEncoding/Differences where supported.
- Type0/CID Identity-H and variable-length CMaps, partial CMaps, `bfchar`, scalar/range/array `bfrange`, ligatures, surrogate pairs, and missing glyph mappings.
- Compressed and uncompressed content streams, unsupported filters, malformed streams, and truncated data.
- Exact expected Unicode text and expected bounding boxes with tolerance defined per fixture.

**Passing gate:** All supported operators and encodings have deterministic fixture tests. Unsupported content is reported as partial/unsupported and routed for fallback; it is not silently accepted as high-confidence text.

---

## 3.4 P1 — Stop flattening reading order to a single string

**Affected files:** `crates/engine/src/xycut.rs`, `crates/engine/src/backends/indra/order.rs`, and the pipeline/Markdown formatter

### Findings

The current ordering stage uses a recursive geometry-based XY-cut. This can be a useful fallback, but the implementation is not sufficient as the only ordering representation:

- Cut thresholds are based on geometric gaps and average line heights. Mixed font sizes, marginal notes, spanning headings, tables, footnotes, and sidebars can distort the gap statistics.
- The algorithm has no semantic or script-aware reading-order handling (for example, RTL text, vertical text, or explicit heading/list/footnote relationships).
- The Indra order backend sorts boxes and then flattens them into a string. The output loses block, region, paragraph, table, and provenance boundaries.
- The formatter detects line gaps using a saturating subtraction between current y and prior bottom. When the order transitions from a lower line in one column to a higher line in the next, the gap becomes zero and a space can be emitted. This can join the end of one column directly to the start of another.
- The included `order.onnx` model is listed in model metadata as required, but a functional inference/load path for that model was not found in the inspected ordering path. Current ordering is therefore a hand-coded XY-cut pass, not a neural reading-order model.
- Existing tests primarily exercise XY-cut sorting itself; they do not assert the final serialized Markdown across two columns or mixed-layout blocks.

### Required implementation

Introduce a structured intermediate representation (IR) that survives until serialization. A suitable starting point is:

```text
Document
  Page
    Region (prose | table | formula | figure | header | footer | marginalia)
      Block (paragraph | heading | list | table | formula | caption | footnote)
        Line
          Span (text, font, bbox, source, confidence)
```

Each node should retain:

- Stable page-local ID and source bounding box.
- Original reading-order candidate(s), chosen order, and ordering rationale/confidence.
- Extraction source (`pdf_text`, `ocr`, `model`, `merged`) and decoding diagnostics.
- Region/type label, paragraph/block grouping, and whether text is owned by another specialist region.
- Language/script direction metadata where supported.

Then:

1. Use region segmentation before line ordering; identify full-width blocks, columns, tables, sidebars, headers/footers, figures, and footnotes.
2. Order regions and blocks hierarchically. Use XY-cut as a deterministic fallback, not as the sole semantic truth.
3. Preserve paragraph boundaries and column breaks through Markdown generation. Never infer a paragraph break solely from a negative/saturated y delta.
4. Make model-assisted ordering explicit: load and run the order model with documented inputs/outputs and confidence if it is genuinely supported, or remove it from the required model list and UI claims until it is integrated.
5. Keep source spans attached to blocks so that errors can be diagnosed and a user can inspect a suspect region.

### Required tests

- Two-column page with a full-width title and spanning subheading.
- Two columns with different line counts and uneven bottoms.
- Sidebar/marginal note beside a main column.
- Header/footer repeated across pages, footnote markers and footnote body.
- Paragraphs with a large gap, wrapped lines, indentation, bullets, numbered lists, and hanging indents.
- Tables and formulas interleaved with prose.
- RTL and vertical-writing fixtures only for scripts the product explicitly claims to support.
- Golden serialized Markdown snapshots, not only sorted coordinate arrays.

**Passing gate:** All synthetic reading-order fixtures serialize to the expected block sequence and boundaries. No cross-column sentence concatenation or duplicated block occurs. On the approved real-PDF gold corpus, report pairwise reading-order accuracy by layout stratum; set a release threshold after Phase 0 baseline, with 100% required on deterministic synthetic fixtures.

---

## 3.5 P1 — Make hybrid DOM/OCR routing confidence- and region-aware

**Affected file:** `crates/engine/src/pipeline.rs` (DOM viability, OCR selection, table/formula routing, text fallback)

### Findings

- DOM viability is currently decided at page level. When the DOM path produces any non-empty set of lines, the pipeline can use it and skip OCR for the entire page. There is no robust check that the extracted text covers the visible page, that all text-show operators were handled, or that decoded text is coherent.
- This can allow a sparse, partial, or corrupted digital text layer to suppress OCR for regions where OCR would have recovered text.
- The page pipeline excludes picture and formula regions from some ordinary-text handling but does not consistently exclude table regions, despite ADR 0012 stating tables and formulas are routed away from ordinary prose and handled by specialized extractors.
- Table extraction input is built from the same text-line collection, which creates a risk that table text enters both the prose output and table serialization.
- Formula replacement uses string replacement/substrings against the formatted page text. Repeated identical text or similar snippets can cause the wrong occurrence to be replaced or removed because the operation is not tied to a page-local span/block ID.
- The raw-text fallback uses a sparse-text heuristic rather than a measured comparison of coverage, decode quality, and OCR output.

### Required implementation

1. Produce DOM and OCR candidates per region (or per bounded text/visual region), not just one page-level winner.
2. Score DOM candidates using visible-text coverage, glyph decode coverage, parser support completeness, geometry validity, reading-order plausibility, and language/script confidence.
3. Run OCR for uncertain/missing regions while retaining trusted DOM spans. Merge by geometry and source provenance, with duplicate suppression based on overlap and text similarity.
4. Give each region exclusive serialization ownership. Table/formula content must not be emitted as ordinary prose and again as a table/formula block.
5. Replace global string substitution with structured edits referencing stable block/span IDs and bounding boxes.
6. Store provenance in the IR so that a user can determine whether a block came from embedded PDF text, OCR, or model-based extraction.
7. Provide explicit route outcomes (`dom_only`, `ocr_only`, `dom_plus_ocr`, `specialist_region`, `unsupported`) and diagnostics per page/region.

### Required tests

- Clean born-digital page: DOM selected and no unnecessary OCR.
- Scanned page: OCR selected.
- Hybrid scan with a small digital header and image-based body: OCR must still process the body.
- PDF with a hidden/bad text layer over a visible scan: bad DOM text must not suppress OCR.
- Sparse/partial CMap page: unmapped text region is sent to OCR or marked incomplete.
- Page with a table, formula, and prose: no table/formula duplication in prose; specialist blocks preserve content.
- Repeated words/equations in two regions: replacement must target the correct spatial span only.

**Passing gate:** A partial DOM extraction cannot cause the rest of the page to be skipped. Controlled mixed-content fixtures recover all expected text once, with region provenance and no duplicate table/formula output.

---

## 3.6 P1 — Correct language capability reporting and selection

**Affected files:** `crates/engine/src/backends/indra/engine.rs`, `crates/server/src/lib.rs`, `crates/desktop/src/main.rs`, `ui/src/components/SettingsModal.tsx`, `ui/src/components/ModelsModal.tsx`

### Finding

The UI offers language choices beyond the scripts accepted by the OCR engine. The inspected engine parser accepts English, Telugu, and Auto (with aliases), while the Settings UI exposes Hindi, Kannada, Tamil, Malayalam, Marathi, Bengali, Gujarati, Punjabi, and Odia among others. Both server and desktop request handling parse the requested language and fall back to English if parsing fails. Unsupported choices can therefore silently run as English.

The model modal also describes wider script coverage than is established by the actual model/runtime configuration.

### Required implementation

1. Create one shared capability manifest for supported languages/scripts by operation: PDF text decoding, OCR detection, OCR recognition, layout, and UI display.
2. Reject unsupported language selections with a visible, actionable error. Do not coerce them to English silently.
3. If Auto is selected, report the detected script and confidence; distinguish “language selection supported” from “model trained for this language”.
4. Render UI options dynamically from runtime capabilities and model installation state.
5. Add per-script quality fixtures before adding a script to the supported list.

### Required tests

- Each supported language invokes the expected model/vocabulary.
- Each unsupported language returns a clear validation error in both desktop and server API paths.
- UI dropdown only enables capabilities advertised by the runtime manifest.
- Auto detection on the approved script corpus returns expected script or an explicit low-confidence result.

**Passing gate:** No unsupported language silently becomes English; UI labels, API validation, model manifest, and actual runtime capability agree.

---

## 3.7 P1 — Make offline model provisioning and runtime status truthful

**Affected files:** `crates/engine/src/model_locator.rs`, `crates/engine/src/vault.rs`, `crates/engine/src/embeddings/engine.rs`, `crates/engine/src/figure/models.rs`, `crates/engine/src/config.rs`, server/desktop model handlers, `ui/src/components/ModelsModal.tsx`

### Findings

- The UI `ModelsModal` presents a hard-coded model list with `installed`/“Verified” status rather than querying actual files or runtime load status.
- The model doctor report checks file presence and calculates a hash, but the inspected implementation does not compare it to an allowlisted expected digest from a signed/pinned manifest. A checksum calculated from the file itself is not verification of authenticity/integrity.
- The model locator's search paths do not fully match ADR 0014's app-data fallback promise. It searches custom/executable/current/relative paths, but platform application-data handling is not fully implemented in the inspected path.
- Model resolution is not fully centralized: embeddings use their own path resolution and some model families use feature-specific paths.
- The order model is listed as required without a confirmed inference path in the ordering backend.
- The downloader uses `curl.exe`, making it Windows-specific. It does not robustly validate HTTP status/content, expected byte size, and pinned SHA before accepting an asset; it downloads directly to the final path rather than a staging path followed by atomic replacement.
- The success message says required models/tokenizers have been downloaded and verified, but only a subset of model assets is provisioned through that route. The message can overstate what has been installed or verified.
- Default features include ONNX Runtime and DirectML paths alongside tract. The repo's documentation also describes the product as pure-Rust/tract-only. These statements are not aligned. The actual providers enabled vary by platform and model path; for example, SLANet is invoked with DirectML disabled in the inspected path.
- ADR 0015 describes device choice (`auto`, `cpu`, `directml`) and runtime probing, but the inspected `AppConfig` does not expose a corresponding device setting or a complete runtime provider diagnostic.
- Embedding `embed_batch` processes individual inputs serially under the session mutex rather than forming a true batch tensor. Treat any claim of batched inference as unproven until benchmarked.

### Required implementation

1. Define an authoritative model manifest with: ID, purpose, required/optional, supported scripts, version/revision, SHA-256, byte count, source, license, runtime, provider compatibility, minimum app version, and whether the model is actually called by production code.
2. Use one model service for discovery, app-data paths, custom paths, provisioning, hash validation, compatibility checks, and load/health status. Remove independent path rules or route them through the service.
3. Separate statuses clearly:
   - `not_installed`
   - `present_unverified`
   - `integrity_verified`
   - `runtime_load_verified`
   - `incompatible`
   - `download_failed`
   - `optional_not_installed`
4. Compare actual downloaded/embedded hashes and sizes to pinned manifest values. Download to a temporary file; validate HTTP status, size, checksum, and file format; atomically rename only after validation.
5. Use a cross-platform HTTP client or a platform-neutral downloader. Network access must occur only in an explicit provisioning action, never silently during PDF conversion.
6. Add an offline conversion test which denies network access and uses pre-provisioned assets. Instrument or mock network calls to prove conversion itself does not attempt access.
7. Make UI model inventory call the runtime and display true status, not green “Verified” badges generated from constants.
8. Decide explicitly whether each model is tract-only, ORT, or dual-runtime. Update README, AGENTS, ADRs, build features, packaging, and model setup instructions to match the shipped implementation.
9. Add the device/provider setting described by ADR 0015 only if the product is ready to support it end-to-end; expose actual selected provider per model session. Otherwise revise the ADR and avoid implying universal acceleration.
10. Either implement order-model inference or mark that model optional/not-used and remove the “required” claim.

### Required tests

- Manifest hash match, mismatch, wrong size, corrupted model, partial download, HTTP error response, and interrupted download.
- Platform app-data path resolution, custom model directory, relocation, and missing-model diagnostics.
- Runtime load test per required model and expected provider.
- Offline conversion with network disabled and all required model files pre-provisioned.
- UI status reflects each lifecycle state, including absent, unverified, incompatible, and load failure.
- Embedding benchmark distinguishes serial loop from true tensor batching and records throughput/latency.

**Passing gate:** The app can truthfully identify exactly which models are present, integrity-verified, runtime-loadable, and used. After provisioning, conversion completes with network access disabled. Model claims in docs/UI match actual runtime code.

---

## 3.8 P1 — Restrict server filesystem and network boundaries

**Affected file:** `crates/server/src/lib.rs` (router/CORS, document path resolution, upload handling, and filesystem-backed handlers)

### Findings

- The server applies permissive CORS. The default CLI bind is loopback, but the server can be explicitly bound to another address.
- There is no corresponding authentication layer in the inspected router for a non-loopback deployment.
- Document resolution accepts an existing caller-supplied path and searches input directories. Other operations accept vault/output-related filesystem paths.
- Uploads use a sanitized filename and file-signature check, but the inspected handler does not provide a robust maximum upload-size policy, random collision-resistant storage identity, full parser validation, or atomic write lifecycle.

This is especially important because a local conversion UI may look safe while the same server binary can be configured to listen on a network interface.

### Required implementation

1. Keep loopback binding as the default and refuse non-loopback binds unless the operator explicitly enables remote mode.
2. Require authentication for remote mode (per-install random secret/token or a stronger deployment auth layer). Do not use permissive CORS as an access-control mechanism.
3. Validate `Host` and `Origin` against an explicit allowlist. Apply appropriate CORS only to trusted UI origins.
4. Resolve every input/output/vault path relative to an explicitly configured base directory. Canonicalize paths and verify they remain inside the allowed root after symlink resolution.
5. Do not allow browser request bodies to select arbitrary server-side paths. Use server-issued upload IDs and opaque document IDs instead.
6. Enforce upload byte limits, request timeouts, concurrent upload limits, quota/retention policy, parser-level validation, unique storage names, and atomic temporary-file writes.
7. Return structured errors and avoid disclosing sensitive absolute paths to untrusted clients.

### Required tests

- Non-loopback bind fails without remote-mode opt-in/authentication.
- Requests with hostile/unapproved Origin and Host are rejected.
- Path traversal, symlink escape, arbitrary absolute path, and out-of-root output/vault paths fail.
- Oversized upload, wrong magic bytes, malformed-but-header-valid PDF, and duplicate filename do not overwrite or exhaust storage.
- Only an authorized opaque document ID can be previewed/converted.

**Passing gate:** A remote or hostile web origin cannot use the server as an unrestricted filesystem conversion endpoint; all file operations stay within configured roots and limits.

---

## 3.9 P1 — Complete browser/desktop UI workflow parity

**Affected files:** `ui/src/bridge/api.ts`, `ui/src/components/BatchQueue.tsx`, `ui/src/components/SplitPane.tsx`, `ui/src/hooks/useConversionProgress.ts`, `crates/server/src/lib.rs`, `crates/desktop/src/main.rs`

### Findings

- `readMarkdown` returns an empty string in browser mode. The server exposes page preview, but the inspected router has no matching full-Markdown read endpoint. The SplitPane therefore falls back to a sample/empty-inspector state in the browser instead of showing the generated note.
- Browser folder selection and drag/drop paths use `file.path || file.name` rather than uploading the file bytes. Browser `File` objects generally do not expose a usable filesystem path, so the server receives only a filename. Single-file upload exists, but its error path queues the bare filename after the upload fails, masking the actual upload failure as a later path-resolution error.
- Progress correlation uses slug/filename heuristics and a special single-job assumption. Similar names or repeated files can be matched to the wrong queue item.
- The web `BatchSummary` server event does not contain the `documents` field which the UI progress hook expects and the desktop event sends, so batch completion behavior is not equivalent across surfaces.
- Several API bridge methods do not consistently check `response.ok` before returning success or parsing a response. The UI can miss server-side errors.
- Event listener registration uses asynchronous `listen().then(...)` patterns where cleanup can run before the promise resolves, potentially leaking listeners.
- Export uses `job.markdownContent || activePage.markdown` rather than the loaded whole-document Markdown state. A document loaded from disk without `job.markdownContent` can therefore export only the active page.
- The copy/export flow does not always provide clear clipboard/download failure feedback. Download object URL cleanup is immediate and should be tested for browser compatibility.

### Required implementation

1. Implement a real `/api/documents/{id}/markdown` (or equivalent) endpoint and a shared bridge contract for desktop/web.
2. Upload bytes for all browser workflows: single file, drag/drop, and directory picker. Store the upload under a server-generated opaque ID and retain the original display filename as metadata.
3. Remove the “queue filename after upload failure” fallback. Preserve the original upload error and provide retry/remove actions.
4. Assign stable `job_id`/`document_id` values when a queue item is created; all progress events must include the same ID. Do not infer identity from slug substrings.
5. Make desktop and server progress event schemas identical and versioned, including batch child results and failure details.
6. Check HTTP status consistently; decode structured error payloads; show actionable messages and retry where safe.
7. Make event subscription cleanup safe when asynchronous registration resolves after component unmount.
8. Export the complete loaded Markdown document, not the active page preview. Confirm copy/download behavior and only revoke object URLs after the download has been initiated safely.
9. Add explicit loading, empty, error, and success states for page preview and Markdown inspection.

### Required tests

- Browser upload of one PDF, multiple dropped PDFs, and a directory of PDFs; verify bytes arrive at the server and each job has a stable ID.
- Upload failure does not create a queue item with a bare local filename.
- Similar filenames (e.g. `report.pdf`, `report-final.pdf`, duplicate basename from different directories) receive only their own progress events.
- Server and desktop progress events validate against the same schema fixture.
- Browser can load and inspect full generated Markdown; page selection changes only the page preview.
- Export contains the whole document after reading from the server, including pages not currently selected.
- API 4xx/5xx responses surface as visible errors; listener lifecycle does not leak after unmount.

**Passing gate:** The same core workflow—upload, batch convert, track progress, inspect page and full note, copy/export—works in browser and desktop with consistent IDs and error behavior.

---

## 3.10 P2 — Wire every advertised UI action and improve ergonomics/accessibility

**Affected files:** `ui/src/App.tsx`, `ui/src/components/MenuBar.tsx`, `ui/src/components/SettingsModal.tsx`, `ui/src/components/ShortcutsModal.tsx`, `ui/src/index.css`, and modal components

### Findings

- The menu/shortcut UI advertises actions such as opening a vault and exporting Markdown with shortcuts, but the inspected key handlers do not implement every advertised binding.
- `onExportMarkdown` currently reports/logs that export occurred instead of performing a real export. The watch-directory action similarly reports an active state without invoking the available backend watcher.
- Global `user-select: none` styling impairs selecting/copying extracted document text in the viewer.
- Several UI elements use very small text and muted contrast. Modal components need a consistent focus, keyboard, and screen-reader interaction model.
- There are no UI component tests, end-to-end tests, or accessibility checks in the supplied package.

### Required implementation

1. Inventory every menu item, shortcut, button, and settings action. Either wire it to a working command or remove/disable it with an explicit “not supported” state.
2. Add tests mapping every displayed shortcut to the same handler used by the menu item; prevent duplicate/conflicting shortcuts.
3. Implement real Markdown export and watch-directory behavior, or clearly mark these capabilities as unavailable until implemented.
4. Allow text selection in the Markdown/document viewer and code blocks; reserve `user-select: none` for decorative chrome.
5. Add accessible dialog semantics (`role="dialog"`, `aria-modal`, labelled title), focus trap/restore, Escape handling, keyboard navigation, and visible focus states.
6. Review contrast and minimum readable text sizing, especially status labels, progress details, and secondary controls.
7. Add UI unit/component tests and browser/desktop E2E tests.

### Required tests

- Every visible menu/shortcut action invokes the expected behavior exactly once.
- Unsupported actions are not presented as functioning shortcuts.
- Text in document preview can be selected and copied with mouse and keyboard.
- Modals open/close with keyboard, contain focus while open, restore focus on close, and expose labels to assistive technology.
- Automated accessibility scan has no critical or serious violations in the primary workflow.

**Passing gate:** No dead/false-success UI affordance remains in the main workflow; keyboard and screen-reader users can complete upload, conversion, inspection, and export.

---

## 3.11 P2 — Make stream and batch failures explicit

**Affected files:** `crates/engine/src/pipeline.rs`, batch/watch modules, desktop/server progress emitters

### Findings

- Some stream setup reads suppress filesystem errors with `.ok()` and continue with an absent optional value.
- Worker/thread join results are ignored in the inspected stream path. A worker panic can therefore be hidden behind partial results.
- Final output is drained from an ordered map without a strong assertion that every expected page has completed; a missing page can be silently omitted while later pages are returned.
- Cancellation is checked at coarse page boundaries, and desktop cancellation state can race with a background operation that resets the flag during startup.
- Watch startup can discard the result of initial directory processing and continue watching, which makes initial failures hard to diagnose.

### Required implementation

1. Return typed page outcomes (`Completed`, `SkippedByPolicy`, `Failed`, `Cancelled`) keyed by explicit page number.
2. Propagate read errors, worker panics, and join failures to the top-level conversion result and progress event stream.
3. Verify the output page set exactly matches the requested page range, allowing only explicitly represented failures/skips.
4. Define cancellation state ownership and reset it before work is spawned; do not reset a cancellation request after it could have been received.
5. Report initial watcher scan errors and distinguish them from ongoing watch errors.

### Required tests

- Missing/unreadable input, worker panic, failed rasterization, cancellation during a page, and cancellation before worker start.
- Requested page range with one forced page failure must report the missing page, never return a misleading complete result.
- Out-of-order worker completion still emits stable page order.
- Initial watch scan failure is visible and recoverable.

**Passing gate:** No worker or page failure can be converted into an apparently successful, incomplete document.

---

## 4. Recommended target architecture

The central change should be to retain structured extraction data until the final Markdown serializer. Avoid a pipeline in which each stage converts its results into a flat string and the next stage has to infer structure again.

### 4.1 Target data flow

```text
PDF input
  │
  ├── page geometry + metadata
  │
  ├── digital text extraction ──► decoded spans + transforms + metrics + diagnostics
  │
  ├── raster/layout analysis ──► regions + region types + geometry
  │
  ├── region-level DOM/OCR routing
  │       ├── trusted digital spans
  │       ├── OCR for scans/uncertain regions
  │       └── specialist table/formula/figure extraction
  │
  ├── deduplication + provenance-preserving merge
  │
  ├── hierarchical reading-order graph
  │       └── regions → blocks → paragraphs/lines → spans
  │
  ├── structured document IR + diagnostics
  │
  └── Markdown serialization + safe viewer rendering
```

### 4.2 Design principles

- **Geometry is evidence, not the document model.** A bounding box cannot by itself express paragraph ownership, list nesting, table cell structure, or footnote relationships.
- **Ordering must be hierarchical.** Establish page regions, then blocks, then lines/spans. Never flatten columns before their order and boundaries have been decided.
- **Every output should have provenance.** Retain whether text came from embedded PDF text, OCR, or a specialist model, with confidence and decoding diagnostics.
- **Partial is a first-class result.** A parser should return what it knows and clearly report what it did not decode. It must not convert uncertainty into a fake high confidence.
- **Specialist regions have one owner.** A table/formula should be represented once in the document IR, not duplicated as ordinary prose plus a second specialist representation.
- **Offline means no network during conversion.** Explicit model provisioning may use the network; conversion must be testable with network access denied.
- **UI status must be derived from runtime truth.** Do not hard-code installation, verification, capability, progress, or success states.

---

## 5. Phased implementation plan and passing gates

The phases are ordered to prevent downstream tuning from masking upstream defects. A phase is complete only when its tests and exit criteria pass; code merged without the gate is not phase completion.

## Phase 0 — Establish a reproducible baseline and golden PDF corpus

**Goal:** Make the repository buildable and measurable before changing extraction behavior.

### Work items

- Add CI/toolchain pinning and a documented clean-checkout build sequence.
- Resolve the nested UI ZIP/build path and define how model assets are supplied.
- Add a no-default-features compile path that does not require absent embedded weights.
- Create a versioned PDF fixture corpus with expected extracted text, page geometry, block structure, and Markdown snapshots.
- Replace environment-specific test paths and silent early returns with repository fixtures or explicit ignored tests that report why they are ignored.
- Record current output and baseline CER/WER, reading-order, table/formula, latency, and memory metrics by document stratum.

### Required fixture strata

1. One-page born-digital, single-column PDF.
2. Two-column page with a spanning heading.
3. Text with ligatures, accented characters, punctuation, and superscripts/subscripts.
4. PDF with Type0/CID font and ToUnicode map.
5. Scanned page with no text layer.
6. Hybrid page with digital header and scanned body.
7. Table mixed with prose.
8. Formula mixed with prose.
9. Rotated page and non-zero crop-box origin.
10. Malformed/partial font map and malformed content stream.
11. Hidden or intentionally incorrect text layer over a visible scan.
12. A malicious PDF-derived text fixture for viewer security tests.

### Passing gate

- A clean environment can reproduce the documented Rust and UI build path.
- Every normal test runs without relying on developer-local absolute paths.
- Every skipped fixture is marked as explicitly ignored with a reason; “file absent, return success” is prohibited.
- The golden corpus and baseline metrics are checked into version control or fetched through a pinned, documented test-fixture mechanism.
- All benchmark measurements include the exact commit, toolchain, model manifest, and hardware/software environment.

**Do not proceed to reading-order tuning until this phase is complete.**

## Phase 1 — Security and model integrity contracts

**Goal:** Close P0 security/build hazards and make model guarantees observable.

### Work items

- Replace/sanitize Markdown rendering and add restrictive CSP.
- Restrict server origins, remote binding, filesystem roots, upload limits, and document identifiers.
- Implement model manifest, pinned hashes, atomic downloader, platform-aware paths, and runtime status API.
- Ensure the UI reports actual model status and supported language capability.
- Align docs with the real inference runtimes and offline boundaries.

### Passing gate

- Security injection tests pass in browser and Tauri/WebView paths.
- Traversal, symlink escape, arbitrary path, unauthorized remote access, and upload quota tests pass.
- Every required model reports a pinned hash/size and a real integrity/runtime state.
- Offline smoke test converts the fixture corpus with network disabled and pre-provisioned assets.
- A network trace or test seam proves no conversion-stage network calls occur.
- Clean-checkout build and package tests pass.

## Phase 2 — PDF text correctness

**Goal:** Make digital PDF text and geometry reliable before improving its order.

### Work items

- Adopt a mature PDF extraction parser or complete the custom text-state implementation.
- Implement supported text-show operators and font decoding/CMap semantics.
- Handle transform matrices, page rotations, crop/media box origins, glyph widths, spacing, and scaling.
- Produce span-level geometry and explicit decode/coverage diagnostics.
- Remove synthetic 0.99 confidence and compressed-byte fallback behavior.

### Passing gate

- Exact expected text on every supported synthetic font/operator fixture.
- Bounding boxes match fixture truth within a documented tolerance.
- Partial/unsupported encodings are reported as partial/unsupported, not passed as complete.
- Malformed streams produce typed errors/diagnostics and do not become plausible but corrupt output.
- Existing golden corpus has no unexplained regression; any intentional behavior change includes an updated reviewed expected output.

## Phase 3 — Structured layout and reading order

**Goal:** Correctly order and preserve page structure.

### Work items

- Introduce the document/page/region/block/line/span IR.
- Segment regions before ordering and preserve block/paragraph/list/table/formula boundaries.
- Replace flatten-first XY-cut output with hierarchical reading order.
- Decide whether `order.onnx` is implemented and tested or removed from the required inventory.
- Add deterministic fallback behavior and debug output for ordering decisions.

### Passing gate

- 100% of deterministic synthetic layout fixtures produce the exact expected block order and boundaries.
- No cross-column paragraph concatenation, missing block, or duplicated block in synthetic cases.
- Pairwise ordering accuracy is reported by layout stratum on the approved real-PDF corpus. Set the production threshold from Phase 0 results and require no material regression in any stratum; proposed initial target is at least 99% on the curated gold corpus once the annotation protocol is stable.
- Golden Markdown snapshots are reviewed for columns, tables, formulas, headings, lists, captions, and footnotes.

## Phase 4 — Region-aware hybrid DOM/OCR and specialist routing

**Goal:** Use OCR to fill extraction gaps without duplicating trusted digital text.

### Work items

- Route DOM/OCR per region using coverage, decode quality, layout confidence, and language confidence.
- Add OCR for suspect/missing regions even when other parts of the page contain valid digital text.
- Merge with geometry-aware deduplication and preserve source provenance.
- Route tables/formulas/figures exclusively to specialist blocks and serialize each exactly once.
- Replace global text replacement with span/block-addressed edits.

### Passing gate

- All controlled digital, scanned, and hybrid fixtures contain every expected text span exactly once.
- A partial DOM region never suppresses OCR for unrelated low-confidence regions.
- Table and formula content is never duplicated into prose.
- CER/WER are reported by digital/scanned/hybrid stratum; proposed guardrail is no regression greater than 0.5 percentage points in any established stratum versus the Phase 0 baseline, with improvement expected in known failure strata.
- Every merged block has source provenance and diagnostics.

## Phase 5 — Offline runtime and model lifecycle hardening

**Goal:** Make offline operation and accelerator selection deterministic and verifiable.

### Work items

- Centralize all model path resolution and install state.
- Validate models against pinned checksums and runtime loading.
- Make model provisioning platform-neutral and atomic.
- Expose actual runtime/provider status per model.
- Add a real `auto`/`cpu`/`directml` setting only where supported, or revise the ADR and product claims.
- Verify that all embeddings and OCR/layout model calls honor the common manifest and configured model directory.
- Benchmark whether embedding inference is truly batched; implement real batching only if it improves measured throughput and memory use.

### Passing gate

- Required models are runtime-load verified on every supported build target.
- Download corruption and hash mismatch are rejected and never replace a valid installed model.
- Conversion succeeds with the network blocked after provisioning.
- Provider reporting reflects the provider actually used by each session.
- No UI “Verified” or “Installed” state can be shown without runtime evidence.

## Phase 6 — UI feature completion, parity, and usability

**Goal:** Make all visible core workflows functional and consistent across desktop and browser.

### Work items

- Add browser full-Markdown read endpoint.
- Implement byte-based upload for drag/drop and directory selection.
- Add stable job/document IDs and align progress schemas.
- Fix whole-document export, page inspector state, and API error handling.
- Wire all menu/shortcut/watch/export actions or remove unavailable affordances.
- Add text-selection, focus management, accessibility semantics, and readable status contrast.
- Add component, browser E2E, and desktop smoke tests.

### Passing gate

- Browser and desktop both pass upload → convert → progress → preview → inspect Markdown → copy/export.
- Multi-file and directory workflows work without relying on browser filesystem paths.
- Progress for similarly named documents is correctly isolated.
- Every visible command does what its label says; failed actions show errors.
- Keyboard-only workflow passes; no critical/serious accessibility issue remains in the primary workflow.

## Phase 7 — Release certification and regression protection

**Goal:** Prevent regressions and certify advertised capabilities.

### Required release commands

Run in CI on a clean checkout, with the exact model fixture/provisioning procedure documented:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo check --benches
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cd ui
npm ci
npm run build
npm test
```

If some workspace targets or optional features require models/platform SDKs, add explicit CI jobs for each supported configuration rather than quietly omitting the target.

### Additional release gates

- Golden PDF corpus test suite passes.
- Security tests pass for viewer and server filesystem/origin boundary.
- Offline conversion smoke passes with network disabled.
- Model checksums and runtime provider report are attached to release artifacts.
- Browser and desktop E2E smoke passes on every claimed platform.
- Packaging test starts the built application from a clean install and converts a one-page fixture.
- README, ADRs, model dialog, settings, and release notes agree on supported scripts, runtimes, models, offline behavior, and hardware acceleration.

**Passing gate:** No release is certified based solely on `cargo check`; the complete PDF corpus, security, offline, UI, and packaging gates must all pass.

---

## 6. Regression test matrix

| Area | Fixture / scenario | Assertions |
|---|---|---|
| Digital text | Simple one-page single-column PDF | Exact text, stable line spans, no missing punctuation |
| Text operators | `Tj`, `TJ`, `'`, `"` | All visible strings extracted in correct order |
| Text state | `Tm`, `Td`, `TD`, `T*`, spacing, scaling, rise | Correct glyph advances and line positions |
| Page coordinates | Rotation, CropBox offset, MediaBox offset, `cm` transforms | Bounding boxes map to visible page coordinates |
| Font decoding | WinAnsi/StandardEncoding/Type0/CID/Identity-H | Exact Unicode mappings for claimed support |
| CMaps | `codespace`, `bfchar`, `bfrange` scalar/array, inherited maps, partial maps | No silent dropped glyphs; explicit partial diagnostics |
| Reading order | Two columns, spanning title, sidebar, uneven columns | Expected block order and paragraph boundaries |
| Lists and paragraphs | Bullets, numbering, indentation, wrapped lines | Correct list and paragraph serialization |
| Table | Table interleaved with prose | Table emitted once; prose not polluted by table cells |
| Formula | Inline/block formula among prose | Formula block preserved once; surrounding prose intact |
| Scan | Image-only PDF | OCR text and geometry meet measured baseline |
| Hybrid | Digital header + scanned body | DOM header plus OCR body, no skipped body |
| Bad text layer | Hidden/corrupt text layer over scan | Bad DOM text rejected or marked low-confidence; OCR used |
| Language | Every advertised script and unsupported script | Supported script route is correct; unsupported route errors visibly |
| Model health | Missing, hash mismatch, runtime load failure | Accurate status and actionable diagnostics |
| Offline | Network disabled after provisioning | Conversion succeeds without network attempts |
| Viewer security | HTML/event handler/SVG/script URL in PDF text | No executable content or unsafe navigation |
| Server boundary | Traversal, symlink, remote origin, oversized upload | Request rejected and data remains inside configured roots |
| Browser upload | Drag/drop, folder upload, repeated basenames | Bytes uploaded; stable IDs; correct progress association |
| UI parity | Desktop and browser inspect/export | Same whole-document content and error states |
| Failure handling | Page worker panic, missing page, cancellation | Explicit incomplete/failed state; never false success |

Every fixture should store the source PDF, fixture metadata, expected normalized text, expected region/block order, and expected Markdown. Expected output changes must be reviewed like code changes.

---

## 7. Documentation and product-contract corrections

The supplied documentation has meaningful contradictions. These should be resolved as part of implementation rather than left as future cleanup.

| Document / contract | Inconsistency to resolve |
|---|---|
| `README.md`, `AGENTS.md`, ADR 0001 | “Pure Rust/tract” and “100% offline” language needs to distinguish conversion-time offline behavior from explicit model provisioning, and align with ORT/DirectML features actually enabled by default |
| ADR 0012 | Describes tables and formulas being routed away from normal prose; current page routing does not consistently exclude tables |
| ADR 0014 | Promises cascading model resolution including app-data behavior not fully present in the inspected locator |
| ADR 0015 | Describes device selection/probing not represented end-to-end in app configuration/UI/provider reporting |
| ADR 0016 | Table structure/serialization promises need golden fixture evidence and exclusive table ownership |
| Model dialog | Must not claim “Verified”, “Installed”, or script compatibility from hard-coded values |
| `implementedphases.md` and historical reports | Phase completion must point to reproducible test evidence and identify skipped/unsupported tests |
| `Audit_2026_11_19.md` | Revalidate before carrying findings forward; at least some previously reported concerns are stale (for example, page preview is now wired, and inspected ORT session configuration uses optimization level 3) |

Maintain a single capability/model manifest and generate or validate the README/model UI tables from it where practical. ADRs should describe implemented behavior or clearly state that a decision is pending—not imply functionality that code does not provide.

---

## 8. Suggested issue breakdown

Create individual tickets/PRs so each phase can be reviewed and gated independently:

1. `SEC-01` Sanitize Markdown rendering and enforce CSP.
2. `BUILD-01` Make UI/model source assets reproducible from a clean checkout.
3. `PDF-01` Add generated one-page fixture corpus and remove silently skipped path tests.
4. `PDF-02` Correct text-show operators, matrices, glyph advances, font decoding, and CMaps.
5. `LAYOUT-01` Introduce structured document IR and region/block ownership.
6. `LAYOUT-02` Hierarchical reading order and golden Markdown tests.
7. `HYBRID-01` Region-aware DOM/OCR confidence and fallback.
8. `ROUTE-01` Exclusive table/formula routing with span-addressed replacement.
9. `MODEL-01` Manifest, checksums, provisioning, runtime health, and app-data resolution.
10. `LANG-01` Capability-driven language choices and explicit unsupported-language errors.
11. `SERVER-01` Authentication/origin/path/upload boundary hardening.
12. `UI-01` Browser uploads and full-Markdown inspector endpoint.
13. `UI-02` Stable job IDs and shared progress event schema.
14. `UI-03` Functional export/watch/menu actions and keyboard/accessibility improvements.
15. `PIPE-01` Propagate read, worker, missing-page, and cancellation failures.
16. `RELEASE-01` CI, full fixture gates, offline test, and packaging certification.

Each ticket should include the matching tests and acceptance gate from the phase above. Do not close a ticket because code compiles if its acceptance tests have not run.

---

## 9. Immediate recommended order of work

For the next implementation cycle, use this order:

1. **Close the Markdown injection and server exposure risks.** These do not depend on the parser redesign.
2. **Make build inputs reproducible and create the PDF corpus.** This enables reliable review of every subsequent parser change.
3. **Correct text extraction and font/coordinate semantics.** Reading-order tuning before this will optimize incorrect input geometry.
4. **Introduce the structured IR and fix hierarchical reading order.** Keep block boundaries until final serialization.
5. **Add region-aware OCR fallback and exclusive specialist routing.** Use provenance to debug mixed-content PDFs.
6. **Make model status/offline/provider reporting truthful.** Align model assets, feature flags, docs, and UI.
7. **Complete browser/desktop workflows and UX/accessibility gates.**
8. **Run release certification on a clean machine and archive the evidence.**

### Final acceptance criterion

The project should not claim general-purpose PDF reading-order or offline-model correctness until it can demonstrate, from a clean checkout and with network access disabled during conversion, that:

- supported PDFs decode to expected text and geometry;
- multi-column and mixed-layout pages serialize in verified reading order;
- incomplete digital text is supplemented by OCR at the region level;
- tables/formulas are represented exactly once;
- every used model is integrity- and runtime-verified;
- unsupported scripts and formats fail transparently;
- untrusted PDF text cannot execute in the UI;
- browser and desktop workflows pass the same core E2E tests; and
- all of the above are protected by a checked-in regression corpus and CI gates.

