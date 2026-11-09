# 0013: Unified Tauri Desktop and Embedded Axum Web Architecture

## Status
Accepted

## Context
`pdfmd-lite` has operated strictly as a command-line interface (CLI) and Rust library crate. To broaden adoption among non-technical and Obsidian users, desktop and web graphical interfaces are required with complete coverage of all CLI features and an MSI installer for Windows.
However, several hard constraints exist:
1. Pure offline execution with zero external cloud LLM dependencies ([ADR 0001](0001-pure-offline-zero-llm-architecture.md)).
2. Heavy in-process neural model inference (SVTR, YOLOv8n DocLayNet, RapidLaTeXOCR, all-MiniLM-L6-v2) requiring significant memory and native SIMD acceleration, making pure client-side in-browser WASM impractical and unstable.
3. Strict environment variable prohibition.
4. Need for lightweight Windows native desktop deployment (`.msi`) without the memory bloat of Electron (>150MB overhead).

## Decision
1. **Desktop Shell**: Adopt Tauri v2 with native Windows WebView2 for the desktop application, producing official `.msi` installers via the WiX Toolset integration. Tauri invokes `pdfmd-lite` Rust pipeline APIs directly in-process with zero IPC serialization penalty.
2. **Web Application Shell**: Implement an embedded local server daemon (`pdfmd-lite --serve`) powered by `axum` and WebSockets on `localhost`. The web application utilizes the identical single-page application (SPA) frontend as the Tauri desktop app, communicating over local REST and streaming WebSocket endpoints.
3. **Workspace Organization**: Restructure the repository into a Cargo workspace monorepo:
   - `crates/engine`: Core document parsing, layout, OCR, and embedding library (`pdfmd-lite`).
   - `crates/cli`: Command-line executable (`pdfmd-lite`).
   - `crates/desktop`: Tauri v2 application container (`pdfmd-lite-desktop`).
   - `crates/server`: Embedded Axum daemon supporting the web app (`pdfmd-lite --serve`).
   - `ui/`: Shared responsive TypeScript/React/Tailwind frontend compiled for both Tauri assets and Axum static assets.
4. **Installer Distribution**: Provide dual Windows MSI flavors:
   - *Full Offline MSI* (~180MB): Pre-packages all ONNX weights (`models/`) for air-gapped, zero-network first-time installation.
   - *Slim MSI* (~15MB): Lightweight installer that triggers model provisioning on initial launch via the built-in downloader.

## Consequences
- Single frontend codebase powers both Desktop (Tauri) and Web (Browser via Axum), preventing feature drift between platforms.
- Eliminates Electron runtime overhead; desktop executable is native Rust with minimal RAM footprint.
- All CLI parameters are mapped 1-to-1 into structured desktop menus and reactive UI controls.
- Web conversions retain full native CPU SIMD throughput by executing on the host engine rather than emulated browser WASM.
