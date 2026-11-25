# 0018: Structured Document IR and Explicit Model Manifest Lifecycle

## Status
Accepted (Amends [ADR 0002](0002-three-way-yolo-layout-routing.md), [ADR 0006](0006-constrained-script-decoding-and-progressive-metadata.md), [ADR 0010](0010-inprocess-minilm-sentence-embeddings.md), [ADR 0012](0012-baseline-aware-hybrid-dom-extraction.md), [ADR 0014](0014-cascading-model-locator.md), and [ADR 0015](0015-hybrid-dual-runtime-and-directml-acceleration.md))

## Context
Audits of `v0.7.2` uncovered four structural defects caused by string flattening and unverified model metadata:
1. Early flattening of `TextLine` streams and substring-based `replacen` formula insertion caused cross-column sentence gluing, duplicated table/prose text, and wrong-occurrence replacements.
2. `KNOWN_MODELS` embedded and marked `models/indra/order.onnx` as `required = true` despite zero inference calls (`xycut.rs` handles reading order), listed unimplemented models (`unimernet.onnx`, `bge_small_int8.onnx`), loaded a 183 MB `gliner_small.onnx` session marked `#[allow(dead_code)]`, and hard-coded `"Verified"` badges in `ModelsModal.tsx`.
3. `ModelLocator` checked embedded slices before exe-adjacent weights, omitted the AppData tier promised by ADR 0014, and cached `OnceLock<Option<...>>` failures process-wide.
4. `embeddings/engine.rs` required `ort` despite ADR 0010's `tract-onnx` default, used byte-length chunking across Telugu text, and `IndraScript` fell back silently from Telugu to Latin when `rec_world.onnx` was absent.

## Decision
1. **Structured Page IR**: Replace early string flattening with a hierarchical intermediate representation (`Document -> Page -> Region -> Block -> Line -> Span`) retaining coordinates, reading order, extraction provenance (`dom`, `ocr`, `specialist`), and span-ID addressing so tables and formulas have single exclusive ownership and never duplicate into prose.
2. **Pinned Model Manifest & Honest Registry**: Validate models against a pinned `MANIFEST.toml` (SHA-256, byte size, tensor signature, required/optional status) via a single reloadable (`RwLock`) `ModelRegistry` honoring strict precedence (`--models-dir` > exe-adjacent > AppData > dev workspace > `bundle-weights`). Remove unused `order.onnx`, `unimernet.onnx`, `bge_small_int8.onnx`, and dead `gliner_small.onnx` from active manifests/downloads until real inference loaders are wired; always execute the zero-dependency `Deterministic Metadata Extractor` regardless of GLiNER availability.
3. **Strict Capability & Provider Transparency**: Require explicit `--allow-degraded` before falling back on missing requested models (such as `rec_world.onnx` for Telugu), populate UI script options dynamically from verified runtime capabilities (`en`, `te`, `auto`), wire `--device auto|cpu|directml` end-to-end, and implement pure-Rust `tract-onnx` evaluation plus token-count chunking for MiniLM embeddings.

## Consequences
Eliminates 186+ MB of dead model loading/embedding bloat, resolves silent wrong-script OCR and table duplication, makes `--doctor` and the UI Models modal reflect verifiable runtime truth, and locks reading-order and hybrid DOM/OCR improvements onto a debuggable IR foundation. Re-introducing neural ordering (`order.onnx`) or neural NER (`gliner_small.onnx`) in the future requires adding verified tensor tests and manifest entries first.
