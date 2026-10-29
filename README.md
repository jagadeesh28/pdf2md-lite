# pdfmd-lite

An autonomous, pure-Rust offline document conversion engine that transforms PDF documents into **Obsidian-flavored Markdown** with native LaTeX math notation and structured tables—operating entirely without external LLMs (no Qwen, Gemini, Gemma, or cloud APIs).

---

## 🚀 Key Features

- **100% Offline & Zero External LLM**: Runs completely in-process using pure-Rust `tract-onnx` neural models (`Indra` OCR + `YOLOv8n` DocLayNet). Requires zero Python, zero Ollama, zero API keys, and no network access.
- **Three-Way YOLO Layout Routing**:
  - **`Picture` (Class 6)**: Masked out (whited out) on the canvas before text OCR to eliminate character hallucination from drawing lines; saved as raw image crops to `<slug>_images/`.
  - **`Table` (Class 8)**: Unmasked on canvas. Tabular text lines are grouped via **Geometric Table Grid Extraction** into clean GitHub Flavored Markdown tables (`| col1 | col2 |`), accompanied by an Obsidian collapsible callout (`> [!table]- Original Crop: Table X`).
  - **`Formula` (Class 2)**: Unmasked on canvas. Text lines bounded by formula boxes are isolated and rendered cleanly inside Obsidian **Display Math Blocks** (`$$...$$`).
- **Obsidian Math Normalization**: Automatically normalizes standard LaTeX delimiters (`\(...\)` to `$...$`, `\[...\]` to `$$...$$`) and detects inline symbols and variables.
- **Hardware-Aware Concurrency Governor**: Dynamically sizes parallel worker threads at runtime by evaluating logical processor cores and free physical RAM:
  $$\text{workers} = \max\left(1, \min\left(\text{logical cores} - 1, \frac{\text{free RAM (GB)}}{1.5\text{ GB}}\right)\right)$$
- **Page Indexing & Strict Ordering**:
  - Emits Obsidian page demarcation comments (`<!-- Page N -->`) at every page boundary.
  - Supports 1-based page slicing (`[begin-page] [end-page]` or `--pages <start>-<end>`).
  - In-memory `BTreeMap` reordering buffer guarantees strict ascending page ordering during concurrent processing.
- **Batch Processing with Short Slugs**: Recursively discovers PDFs in nested folders (up to 3 levels deep) and generates sanitized, shortened directory names (capped at 32 characters) to prevent Windows `MAX_PATH` length overflows.
- **Zero Environment Variables**: All settings are configured strictly through command-line arguments and optional `pdfmd-lite.toml` configuration files.

---

## 📦 Installation & Build

Build the optimized release executable:

```bash
cargo build --release
```

The compiled binary will be placed at:
- `target/release/pdfmd-lite.exe` (Windows)
- `target/release/pdfmd-lite` (Linux/macOS)

---

## 🛠️ Usage

### 1. Single PDF Conversion

Convert a PDF document into Obsidian Markdown:

```bash
# Basic conversion:
pdfmd-lite input/document.pdf output/document.md

# Convert specific page range (e.g., pages 1 to 10):
pdfmd-lite input/document.pdf output/document.md 1 10

# Using explicit flags:
pdfmd-lite -i input/document.pdf -o output/ -p 1-10
```

### 2. Batch Directory Conversion

Convert an entire directory of PDFs recursively:

```bash
# Recursively process all PDFs in input/ to output/:
pdfmd-lite input/ output/

# Batch process with custom search depth and concurrency:
pdfmd-lite -i input/ -o output/ -d 4 -j 4
```

Each document generates a dedicated subfolder with its Markdown file and image assets:
```
output/
└── thermodynamic_report/
    ├── thermodynamic_report.md
    └── thermodyn_images/
        ├── page_1_fig_1.png
        └── page_3_table_1.png
```

### 3. Normalize Math Delimiters in Existing Markdown Notes

Convert existing LaTeX math delimiters (`\(...\)` and `\[...\]`) to Obsidian format (`$...$` and `$$...$$`):

```bash
pdfmd-lite --fix-math path/to/notes/
```

---

## ⚙️ Configuration (`pdfmd-lite.toml`)

You can optionally place a `pdfmd-lite.toml` file in your workspace or pass `-c path/to/config.toml`:

```toml
concurrency = 4
max_depth = 3
# models_dir = "models/"
# layout_model = "models/layout/yolov8n-doclaynet.onnx"
```

---

## 📚 Documentation & References

- [GLOSSARY.md](GLOSSARY.md) — Canonical Ubiquitous Language definitions.
- [REFERENCES.md](REFERENCES.md) — Neural model architecture details, citations, and metadata roadmap.

---

## 📄 License

MIT OR Apache-2.0
