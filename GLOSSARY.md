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
