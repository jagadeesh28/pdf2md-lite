# pdfmd-lite

An autonomous, pure-Rust offline document conversion engine transforming PDFs into Obsidian-flavored Markdown with native math equations and structured tables, operating without external LLM runtimes or cloud APIs.

## Language

**Raw Crop**:
An unenhanced, unbinarized image rectangle extracted directly from a rendered PDF page canvas according to neural layout boundaries.
_Avoid_: Enhanced image, binarized crop, despeckled figure

**YOLO Document Layout Engine**:
An in-process neural layout analysis detector executing YOLOv8n on DocLayNet weights via `tract-onnx` to produce bounding boxes for pictures, tables, and formulas.
_Avoid_: Heuristic segmenter, negative-space clusterer

**Three-Way Layout Router**:
A layout dispatch mechanism that masks pictures from the OCR canvas while routing tables to geometric grid extraction and formulas to display math blocks.
_Avoid_: Uniform canvas processing, blind OCR

**Geometric Table Grid Extraction**:
A zero-weight pure-Rust spatial clustering algorithm that reconstructs 2D tabular rows and columns from OCR and text stream coordinates within detected table boundaries, formatting them as GitHub Flavored Markdown tables.
_Avoid_: Raw text dump, unstructured table text

**Companion Table Callout**:
A collapsible Obsidian Markdown callout (`> [!table]- Original Crop: Table X`) rendered alongside a reconstructed Markdown table linking directly to the table's raw visual image crop.
_Avoid_: Standalone table image, naked table link

**Display Math Block**:
An equation region formatted using Obsidian LaTeX block math delimiters (`$$...$$`), isolated from surrounding prose text.
_Avoid_: Raw LaTeX environment, escaped equation

**Inline Math Span**:
A short formula token, Greek symbol, or variable formatted using single dollar signs (`$...$`) within paragraph text.
_Avoid_: Unfenced LaTeX snippet, backslash Greek word

**Page Demarcation**:
An HTML-style Obsidian comment anchor (`<!-- Page N -->`) inserted at the boundary of each page in the output Markdown document.
_Avoid_: Page break, markdown heading page marker

**Batch Folder Slug**:
A shortened, sanitized filesystem identifier derived from the PDF filename (truncated to prevent Windows MAX_PATH violations) used for the document's output directory and `<slug>_images/` asset subfolder.
_Avoid_: Full-path folder, raw document title folder

**Hardware-Aware Concurrency Governor**:
A runtime resource evaluation routine that dynamically sizes worker threads based on available logical processor cores and free physical RAM.
_Avoid_: Fixed thread pool, static worker count, env-var concurrency

**RapidLaTeXOCR Engine**:
An encoder-decoder vision transformer model executing autoregressive sequence decoding over cropped formula regions to generate exact LaTeX equation markup.
_Avoid_: Formula OCR, equation transcriber, pix2tex wrapper

**Indra World Recognizer**:
An in-process SVTR CTC neural recognizer (`rec_world.onnx`) operating on 32px height line crops across a 2,935-token vocabulary for Indic and non-Latin world scripts, including Telugu (`U+0C00..=U+0C7F`).
_Avoid_: Tesseract Indic, external language pack, secondary OCR pipeline


**SLANet Table Engine**:
A structure-location architecture network that predicts structural HTML tokens and 2D cell bounding boxes from cropped table regions to reconstruct tabular grids.
_Avoid_: Table detector, heuristic table parser

**Recursive XY-Cut++**:
A spatial projection profile partitioning algorithm that recursively isolates vertical column gutters and horizontal section dividers to determine reading order.
_Avoid_: Naive Y-sort, reading line sorter

**GLiNER Metadata Extractor**:
A lightweight bidirectional transformer model that performs zero-shot open-type entity extraction on document text to populate Obsidian YAML frontmatter properties with structured metadata.
_Avoid_: LLM tagger, heuristic metadata extractor, regex parser

**DOM-First Metadata Gating**:
A fallback dispatch strategy that prioritizes native PDF catalog info dictionaries (`/Title`, `/Author`, etc.) and conditionally invokes neural entity extraction only when structural properties are missing or uninformative.
_Avoid_: Unconditional neural extraction, blind metadata tagging

**Constrained CTC Masking**:
A decoding mechanism that dynamically restricts the candidate token set during CTC decoding to the active language script's valid Unicode block, preventing out-of-script character hallucinations.
_Avoid_: Blind CTC argmax, post-OCR regex stripping, unconstrained world decoding

**Hybrid Math Gating**:
A spatial and semantic classification mechanism that preserves inline formulas within running paragraphs as `$...$` while rejecting prose sentences containing English stop words from being fenced inside display math `$$...$$`.
_Avoid_: Blind formula wrapping, isolated inline math, unanchored math block

**Heading Promotion Ensemble**:
A document structuring routine combining neural layout boundaries with typographic and keyword heuristics to format document titles and section dividers with `#` and `##` Markdown markers.
_Avoid_: Flat paragraph dump, unstructured transcript, blind layout tagging

**Two-Stage Progressive Metadata**:
A two-pass metadata extraction architecture that fast-scans PDF catalog info upfront, followed by full-document transcript analysis post-extraction to synthesize titles, authors, and classification/domain tags for YAML frontmatter.
_Avoid_: Upfront-only DOM inspection, premature header slicing

**Global Worker Pool**:
A unified process-wide thread pool dynamically sized by the hardware governor that coordinates concurrent work units across multiple documents and pages without thread oversubscription.
_Avoid_: Per-document thread pool, nested worker spawning, uncoordinated concurrency

**Pipelined In-Memory Rasterization**:
A zero-disk streaming architecture where a dedicated rendering thread streams decoded in-memory page canvases through a bounded channel directly to active OCR workers, eliminating intermediate PNG disk writes.
_Avoid_: Disk spooling, batch PNG caching, sequential pre-rendering

**Spurious Codeblock Stripping**:
The automated unwrapping of accidental fence boundaries (` ```markdown ` or generic ` ``` `) enclosing standard Markdown elements (tables, callouts, headings, prose) while strictly preserving legitimate programming and diagram syntax blocks.
_Avoid_: Naive codeblock deletion, universal backtick stripping

**Post-Extraction Note Formatter**:
A unified offline document sanitization routine that unwraps spurious codeblocks, cleans legacy extraction banners, normalizes LaTeX equations/tables, and synthesizes missing YAML frontmatter properties using GLiNER neural entity extraction and transcript heuristics.
_Avoid_: Separate cleaning scripts, external linter, blind overwriting

**Embedding Sidecar**:
A per-document JSON Lines metadata file (`<slug>.embeddings.jsonl`) placed alongside the converted Markdown note containing semantically chunked text segments, physical page coordinates, and dense vector embeddings for offline vector search and Obsidian RAG plugins.
_Avoid_: Central vector database, frontmatter vector dump, embedded vector block

**Document Content-Hash Idempotency**:
An incremental batch skip mechanism that records the cryptographic SHA-256 hash of the input PDF directly into the output Markdown YAML frontmatter (`pdf_sha256`), bypassing re-conversion only when the source hash matches without relying on external cache databases.
_Avoid_: External cache SQLite, mtime heuristic, filesize-only check

**Embedded Base64 Image Mode**:
An opt-in single-file output serialization mode (`--embed-images-base64`) that inlines visual figure and table raster assets directly into Markdown image tags as base64 data URIs instead of creating `<slug>_images/` asset subdirectories.
_Avoid_: Default inline base64, raw data URL spam

**Section-Aware Semantic Chunking**:
A text partitioning strategy that segments Markdown documents along heading boundaries (`#`, `##`, `###`) and physical page demarcation anchors into coherent 256–512 token chunks while preserving paragraph integrity.
_Avoid_: Fixed-length sliding window, arbitrary character cut, blind page chunking

**Dual-Backend Embedding Engine**:
An in-process neural sentence transformer pipeline evaluating `all-MiniLM-L6-v2` (384-dimensional dense vectors) using pure-Rust `tract-onnx` by default, with optional acceleration via `ort` (ONNX Runtime) feature gating.
_Avoid_: External embedding service, API vector generator, cloud embedding endpoint

**Retroactive Vault Embedding Pass**:
An offline CLI execution mode (`pdfmd-lite --embed <path>`) that traverses an existing Obsidian Markdown vault, parses structural headers, generates semantic chunks, and creates or updates `<slug>.embeddings.jsonl` sidecars without re-rasterizing source PDFs.
_Avoid_: PDF-coupled re-embedding, full conversion re-run

**In-Place Grayscale Masking**:
A zero-copy memory optimization that directly converts a rendered page raster into an 8-bit grayscale canvas once and mutates masked visual regions directly on raw byte slices in-place, eliminating intermediate 32-bit RGBA buffer clones.
_Avoid_: RGBA clone masking, intermediate canvas copy, multi-pass rasterization

**Baseline-Aware Hybrid DOM Extraction**:
A selective document text extraction pipeline that extracts digital characters directly from PDF content streams by tracking glyph vertical offset shifts ($\Delta y$) to reconstruct sub/superscripts, while routing YOLO layout regions (formulas, tables, figures) to neural vision models.
_Avoid_: Naive stream dump, uncalibrated text extraction, blind layout OCR

**Concurrent Neural Session Dispatch**:
An in-process neural model execution architecture utilizing session pooling or thread-isolated model arenas to evaluate neural vision models across parallel page workers without serializing the pipeline behind a single global mutex.
_Avoid_: Global session lock, coarse-grained mutex lock, multi-worker pipeline serialization

**Tauri Application Shell**:
A lightweight native desktop container executing Tauri v2 over the Windows WebView2 runtime that invokes the core document pipeline via in-process Rust commands with zero Node.js runtime footprint.
_Avoid_: Electron wrapper, webview container, desktop browser bridge

**Unified Web/Desktop Shell**:
A single responsive user interface frontend built with modern web technologies that executes interchangeably inside the Tauri desktop shell (via direct IPC) or inside standard web browsers (via local Axum WebSocket and REST endpoints).
_Avoid_: Split frontend, dual UI codebases, web-only client

**Embedded Web Server Daemon**:
A lightweight in-process HTTP and WebSocket server powered by `axum` (`pdfmd-lite --serve`) that exposes document conversion, file monitoring, and streaming progress telemetry to browser clients on `localhost`.
_Avoid_: Cloud API backend, external microservice, node proxy server

**Dual-Flavor MSI Packaging**:
A Windows installer generation strategy utilizing WiX via Tauri to produce both a fully offline bundle (pre-packaging all ONNX model weights) and a slim installer (provisioning model weights on initial launch).
_Avoid_: Monolithic MSI only, web-dependent installer only

**Split-Pane Inspector**:
A side-by-side interactive preview interface displaying the source rendered PDF page canvas adjacent to synthesized Obsidian Markdown with live KaTeX mathematics and structured tables.
_Avoid_: Markdown-only preview, external PDF reader, detached viewer

**Unified Event Bridge**:
A polymorphic frontend communication layer that consumes native Tauri IPC window events on the desktop and Axum WebSocket event streams in the browser through a unified reactive telemetry hook.
_Avoid_: Polling status checker, fragmented event handler, dual telemetry clients
