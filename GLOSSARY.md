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
