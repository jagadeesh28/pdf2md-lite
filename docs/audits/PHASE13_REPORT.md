# Phase 13 Sign-Off Report: UI/Web Parity, Contract Schema Synchronization & Accessibility

> **Target Environment:** Antigravity IDE + Gemini 3.8 Flash (Medium)  
> **Workspace:** `pdfmd-lite` Rust Workspace (`crates/engine`, `crates/cli`, `crates/server`, `crates/desktop`) + React 19 Frontend (`ui/`)  
> **Status:** COMPLETED & VERIFIED  

---

## 1. Executive Summary

Phase 13 establishes seamless parity between the native Tauri v2 desktop shell and the Axum web daemon, locks frontend-backend contract schemas with robust configuration patch preservation, adds full keyboard and dialog accessibility semantics, delivers two-way synchronized reading-order scrolling in the split-pane viewer, and implements secure browser-mode drag-and-drop file/folder streaming uploads with path containment.

---

## 2. Key Architectural Deliverables

### 2.1 Configuration Round-Trip & Patch Preservation (`crates/engine/src/config.rs`)
- Implemented `AppConfig::merge_patch(&mut self, incoming: AppConfig)` preserving unmanaged backend fields (`models_dir`, `max_depth`, `layout_model`, etc.) during frontend settings updates.
- Hardened `AppConfig::save` to create parent directories automatically with `fs::create_dir_all`.
- Added unit test `test_config_merge_patch_preserves_unmanaged_fields`.
- Added `save_table_images` and `device` (`auto | cpu | directml`) fields with complete TypeScript parity.

### 2.2 Capability Discovery & Hardware Target Detection (`crates/engine/src/config.rs`, `crates/engine/src/lib.rs`)
- Implemented `LanguageCapability`, `CapabilitiesResponse`, and `get_capabilities(is_tauri: bool)` re-exported at engine root.
- Dynamically reflects installed OCR language models (`rec.onnx`, `rec_world.onnx` for Telugu/Hindi), active execution device (`auto`, `cpu`, `directml`), and supported DOM extraction modes (`auto`, `always`, `never`).

### 2.3 Server Daemon Endpoints & Browser Parity (`crates/server/src/lib.rs`)
- Added `GET /api/note?path=...`: Validates path containment via `PathPolicy::check_read` and streams markdown content with `text/markdown; charset=utf-8`.
- Added `GET /api/asset?path=...`: Validates path containment via `PathPolicy::check_read`, resolves MIME types (`image/png`, `image/jpeg`, `image/webp`, `image/svg+xml`, `application/pdf`), and streams binary asset bytes.
- Added `GET /api/capabilities`: Exposes system capabilities, supported languages, and execution device to the web frontend.
- Hardened `update_config` handler to load existing configuration, apply `merge_patch`, and persist to the user AppData config directory.
- Added integration tests covering note access, asset streaming with MIME resolution, capabilities, and forbidden path rejection outside granted roots.

### 2.4 Desktop Shell Alignment (`crates/desktop/src/main.rs`)
- Updated `save_config` Tauri command to merge with existing config using `merge_patch` and write to the OS AppData config location.
- Added `get_capabilities` Tauri command and registered in `tauri::generate_handler![...]`.

### 2.5 Bridge & Contract Normalization (`ui/src/types/index.ts`, `ui/src/bridge/api.ts`)
- Added `save_table_images` and `device` properties to `AppConfigData`.
- Added `LanguageCapability` and `CapabilitiesResponse` interfaces.
- Updated `defaultConfig` and `normalizeConfig` ensuring clean defaults for all properties.
- Enhanced `readMarkdown(path)`: Automatically falls back to `/api/note?path=...` in browser/web daemon mode.
- Added `getAssetUrl(path)`: Generates `/api/asset?path=...` URLs for the web daemon and direct local paths for Tauri desktop.
- Added `fetchCapabilities()` bridge call.

### 2.6 Modal Accessibility & Focus Management (`ui/src/components/`)
- Hardened all modals (`SettingsModal`, `AboutModal`, `ShortcutsModal`, `ModelsModal`):
  - Added `role="dialog"`, `aria-modal="true"`, and appropriate `aria-labelledby` identifiers.
  - Added global window `Escape` key listeners for keyboard dismissal.
  - Updated `AboutModal` to display current version `0.7.2`.
  - Added hardware execution device selector (`auto`, `cpu`, `directml`) to `SettingsModal`.

### 2.7 Two-Way Synchronized Split-Pane (`ui/src/components/SplitPane.tsx`)
- Connected Markdown scroll events with page anchor demarcations (`<!-- Page N -->` / `#page-anchor-N`) to automatically update the active page preview.
- Connected page preview thumbnail changes to scroll corresponding Markdown anchor demarcations into view.
- Added complete document export support (`resolvedMarkdown || activePage.markdown`).

### 2.8 Honest UI Actions & Browser Byte Uploads (`ui/src/App.tsx`, `ui/src/components/BatchQueue.tsx`)
- Replaced stubbed export and directory watching with honest UI behaviors and contextual user notifications.
- Added confirmation dialogs before destructive/modifying operations (`handleFormatNotes`, `handleFixLatex`, `handleConvertPngToJpeg`, `handleEmbedVault`).
- Replaced truncated dry-run summary with a full scrollable document table detailing conversion status and page counts per file.
- Wired browser file drag-and-drop and folder picking to stream PDF bytes via `uploadFile` to the server upload sandbox with error reporting.
- Displayed pipeline tags (`HYBRID DOM` / `OCR`) and degradation warning badges in queue item cards.

---

## 3. Verification & Passing Gates

All Phase 13 verification commands completed with exit code `0`:

| Gate | Command | Result |
| :--- | :--- | :--- |
| **Workspace Compilation** | `cargo check --workspace --all-targets` | **PASS (0 warnings/errors)** |
| **Benches Compilation** | `cargo check --benches` | **PASS (0 warnings/errors)** |
| **Workspace Test Suite** | `cargo test --workspace --all-targets` | **PASS (127/127 tests passed)** |
| **Clippy Linter** | `cargo clippy --workspace --all-targets -- -D warnings` | **PASS (0 warnings)** |
| **Code Formatting** | `cargo fmt --all -- --check` | **PASS (Clean)** |
| **UI Typecheck** | `cmd.exe /c "npm --prefix ui run typecheck"` | **PASS (0 errors)** |
| **UI Production Build** | `cmd.exe /c "npm --prefix ui run build"` | **PASS (Bundle built in 6.8s)** |
| **UI Test Suite** | `cmd.exe /c "npm --prefix ui run test"` | **PASS (14/14 tests passed)** |

---

## 4. Architectural Invariants Maintained

- **Zero-Unwrap Policy:** No `.unwrap()` or `.expect()` calls added in production code paths.
- **Zero Environment Variables:** Configuration and state strictly managed through typed AppConfig, OS AppData paths, and CLI/API arguments.
- **Path Containment:** All file reads in `crates/server` (`/api/note`, `/api/asset`) strictly validated through `PathPolicy::check_read`.
- **Single-Phase Discipline:** Phase 13 fully certified and committed before proceeding to Phase 14.
