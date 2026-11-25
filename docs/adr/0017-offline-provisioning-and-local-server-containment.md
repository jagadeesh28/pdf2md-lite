# 0017: Offline Provisioning Isolation, AppData Paths, and Local Server Containment

## Status
Accepted (Clarifies and amends [ADR 0001](0001-pure-offline-zero-llm-architecture.md), [ADR 0013](0013-tauri-axum-unified-desktop-web-architecture.md), and [ADR 0014](0014-cascading-model-locator.md))

## Context
Audits of `v0.7.2` identified contradictions between `pdfmd-lite`'s offline, locked-down-host posture and shipped implementation:
1. Default Cargo features enabled `embed-models` (triggering `include_bytes!` compile-time weight lookups that fail clean checkouts without `models/`) while `vault.rs` invoked Windows-only `curl.exe` against unpinned remote URLs without SHA-256 verification.
2. Mutable state (`pdfmd-lite.toml`, `crash.log`, `temp/uploads`, default `./output`) was written relative to CWD, failing when installed under `Program Files`.
3. The Axum daemon (`--serve`) used `CorsLayer::permissive()`, accepted arbitrary filesystem paths, and allowed non-loopback binds without authentication, while the Markdown viewer injected unescaped HTML under `"csp": null`.

## Decision
1. **Strict Conversion-Time Offline Guarantee & Feature Split**: Decouple tokenizer support (`tokenizers-support`, default) from static weight embedding (`bundle-weights`, opt-in release-only). Remove network downloaders from default and MSI builds behind an opt-in `online-provisioning` feature, and introduce offline package import (`pdfmd-lite --install-models <dir|zip>`). Clarify documentation to state 100% offline conversion inference (`tract-onnx` pure-Rust core plus optional native `ort`/DirectML).
2. **Per-User AppData State & Locked-Down Host Containment**: Route all mutable configuration, logs, uploads, caches, and user-installed models to OS-native user AppData directories (overridable via `--data-dir`, still zero `std::env::var` reads) and default conversion output to the user's Documents directory rather than CWD or `Program Files`.
3. **Web Daemon & WebView Containment**: Bind `--serve` strictly to loopback by default (refusing remote binds without explicit opt-in and authentication), require a per-run random bearer token plus `Host`/`Origin` validation, remove `CorsLayer::permissive()`, enforce canonicalized `PathPolicy` root checks across server and Tauri IPC commands, replace regex HTML rendering with a sanitized Markdown parser (`DOMPurify`), enforce a restrictive Tauri CSP, and disable `withGlobalTauri`.

## Consequences
Guarantees zero outbound network capability during standard document conversion, enables reproducible clean-checkout builds without pre-bundled binaries, allows standard non-admin users to run installed Windows MSI packages cleanly, and blocks CSRF/XSS/arbitrary-file-read escalation from hostile PDFs or browser origins. Requires explicit `--install-models` or opt-in `online-provisioning` when provisioning Slim installs.
