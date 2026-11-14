# 0014: Cascading Model Locator and Zero-Copy Provisioning

## Status
Accepted

## Context
Offline neural models (Indra OCR, DocLayNet YOLO, RapidLaTeXOCR, SLANet, GLiNER, MiniLM) were previously resolved using ad-hoc mechanisms across different engines, including compile-time `CARGO_MANIFEST_DIR` paths, hardcoded relative `./models` lookups in current working directories, or eager `.to_vec()` heap allocations of embedded slices. In distributed standalone binaries, portable archives, and MSI packages, ambient CWD lookups silently shadowed embedded weights or failed without diagnostic information.

## Decision
Introduce a unified `ModelLocator` managing deterministic resolution across all engines in strict descending precedence:
1. Explicit `--models-dir` CLI flag or `AppConfig.models_dir` setting.
2. Binary-adjacent directory (`std::env::current_exe().parent()/models`).
3. Platform user/application data directory (`AppData/Local/pdfmd-lite/models` on Windows, `~/.local/share/pdfmd-lite/models` on Unix).
4. Embedded static binaries via `Cow::Borrowed(&'static [u8])` when compiled with `embed-models`.

Expose a `--doctor` inspection command displaying resolution source, file presence, size, and SHA-256 for all required model weights. Forbid runtime ambient `CARGO_MANIFEST_DIR` lookups in release targets.

## Consequences
Guarantees reliable model discovery regardless of working directory. Eliminates heap allocations for statically embedded model bytes. Provides auditable diagnostic verification for client deployments.
