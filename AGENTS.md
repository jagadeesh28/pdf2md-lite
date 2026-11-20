# AGENTS.md — Agent Engineering Guide

This document serves as the canonical operating manual and technical roadmap for AI coding agents working on `pdfmd-lite`.

---

## 1. Project Mission & Foundational Constraints

`pdfmd-lite` is an autonomous, high-performance, pure-Rust offline document conversion engine transforming scientific and technical PDFs into Obsidian-flavored Markdown with native LaTeX mathematics and structured tables.

### Non-Negotiable Architectural Rules

1. **100% Pure Offline & Zero External LLM**:
   - Never introduce dependencies on external cloud APIs, local LLM runtimes, or network clients during standard document conversion.
   - All models execute locally in-process via `tract-onnx` (pure-Rust SIMD) with optional `ort` (ONNX Runtime) feature acceleration.
   - *Reference: See [ADR 0001](./docs/adr/0001-pure-offline-zero-llm-architecture.md) when evaluating dependency or runtime architecture decisions.*

2. **Strict Environment Variable Prohibition**:
   - **Never use system environment variables** for application settings, paths, directory locations, or runtime flags.
   - Rely strictly on typed CLI arguments and configuration files (`pdfmd-lite.toml` via `crates/engine/src/config.rs`).

3. **Domain Vocabulary Integrity**:
   - Enforce canonical domain terms documented in [`GLOSSARY.md`](./GLOSSARY.md) (*Raw Crop*, *Three-Way Layout Router*, *Geometric Table Grid Extraction*, *Embedding Sidecar*, *Section-Aware Semantic Chunking*, *Document Content-Hash Idempotency*). Avoid banned synonyms.

4. **Architectural Decision Records (ADRs)**:
   - When introducing hard-to-reverse architectural changes, record a numbered ADR in [`docs/adr/`](./docs/adr/) adhering to the 3-gate rule (*Hard to reverse*, *Surprising without context*, *Real trade-off*).
   - Only consult specific ADR documents when working directly within their architectural domain.

---

## 2. Workspace Monorepo Topology

```
pdfmd-lite/
├── Cargo.toml                  # Workspace manifest and root configurations
├── GLOSSARY.md                 # Ubiquitous domain language and terminology
├── IMPROVEMENTS.md             # Benchmark baselines and changelog
├── FUTURE_IMPROVEMENTS.md      # Multi-core profiling and optimization telemetry
├── implementedphases.md        # Historical record of completed Phases 0–5
├── docs/adr/                   # Architectural Decision Records (0001 – 0014)
├── benches/                    # Statistical Criterion benchmarks
├── ui/                         # React 19 + Vite + Tailwind CSS shared web frontend
├── wix/                        # WiX Toolset v5 installer source manifests
├── scripts/                    # Packaging and build automation scripts
└── crates/
    ├── engine/                 # Core headless library (PDF parsing, OCR, layout, pipeline)
    ├── cli/                    # CLI binary and batch processing runner
    ├── server/                 # Axum localhost HTTP/WebSocket daemon (`pdfmd-lite --serve`)
    └── desktop/                # Tauri v2 native desktop application shell
```

---

## 3. Contextual Architectural References

Consult individual ADRs only when working within that specific subsystem:

- **Concurrency & Memory Budgeting**: [ADR 0003](./docs/adr/0003-hardware-aware-concurrency-and-memory-budget.md)
- **Batch Output & Path Sanitization**: [ADR 0004](./docs/adr/0004-short-slug-batch-directory-layout.md)
- **Semantic Embeddings & Sidecars**: [ADR 0009](./docs/adr/0009-per-note-embedding-sidecars-and-frontmatter-hash.md) & [ADR 0010](./docs/adr/0010-inprocess-minilm-sentence-embeddings.md)
- **Fine-Grained Session Mutexes & Masking**: [ADR 0011](./docs/adr/0011-fine-grained-session-mutexes-and-inplace-grayscale-masking.md)
- **Baseline-Aware Hybrid DOM Extraction**: [ADR 0012](./docs/adr/0012-baseline-aware-hybrid-dom-extraction.md)
- **Unified Desktop & Web Server Shell**: [ADR 0013](./docs/adr/0013-tauri-axum-unified-desktop-web-architecture.md)
- **Cascading Model Locator & Zero-Copy Provisioning**: [ADR 0014](./docs/adr/0014-cascading-model-locator.md)
- **Hybrid Dual-Runtime & DirectML Acceleration**: [ADR 0015](./docs/adr/0015-hybrid-dual-runtime-and-directml-acceleration.md)
- **Hybrid Table Serialization & Span Reconstruction**: [ADR 0016](./docs/adr/0016-hybrid-table-serialization-and-span-reconstruction.md)

*For historical implementation milestones, tasks, and verifications across Phases 0–5, refer to [`implementedphases.md`](./implementedphases.md).*

---

## 4. Verification & Testing Standards

Before completing any implementation step, agents **must** execute and pass the standard quality gates:

```powershell
# 1. Compilation and syntax verification
cargo check --all-targets

# 2. Benchmarks compilation check
cargo check --benches

# 3. Comprehensive unit and integration test suite
cargo test --workspace

# 4. Strict clippy linting
cargo clippy --all-targets -- -D warnings
```

### Git Commit & PR Protocol
- Use Conventional Commits (`feat:`, `fix:`, `perf:`, `docs:`, `test:`, `chore:`).
- Document performance delta or benchmark impact in commit bodies.
- Never commit large binaries, model weights (`models/`), or `.zip` archives.
