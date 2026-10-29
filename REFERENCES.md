# Model References & Architectural Roadmap

`pdfmd-lite` is designed around specialized, ultra-lightweight, local neural models running in-process via `tract-onnx` and `ort` (ONNX Runtime) without external LLM dependencies, Python interpreters, or cloud APIs.

---

## 🧠 Active Production Models

### 1. Layout Analysis: YOLOv8n-DocLayNet
- **Purpose**: Document layout segmentation into structural classes: `Picture`, `Table`, and `Formula`.
- **Architecture**: YOLOv8 nano visual object detector trained on the DocLayNet dataset.
- **Weight Footprint**: `models/layout/yolov8n-doclaynet.onnx` (~12.2 MB).
- **Runtime**: Pure-Rust in-process execution via `tract-onnx`.
- **Reference**:
  - Puzicha et al., *DocLayNet: A Large Human-Annotated Dataset for Document-Layout Analysis*, KDD 2022.
  - Repository: [DocLayNet](https://github.com/DS4SD/DocLayNet)

### 2. Optical Character Recognition: Indra OCR
- **Purpose**: Multi-script text line detection and recognition with native inline LaTeX tokens.
- **Architecture**:
  - Text Detection: DBNet with 1024×1024 grayscale spatial attention.
  - Text Recognition: SVTR CTC character recognizer with 96px height and 7 dynamic width buckets (160–1536px).
- **Weight Footprint**:
  - `models/indra/det.onnx` (~4.58 MB)
  - `models/indra/rec.onnx` (~12.3 MB)
- **Runtime**: Pure-Rust in-process execution via `tract-onnx`.

### 3. Reading Order Partitioning: Recursive XY-Cut++
- **Purpose**: Multi-column reading order sequencing, column gutter isolation, and banner header preservation.
- **Algorithm**: Spatial projection profile analysis evaluating vertical column gutters ($\ge 1.5 \times H_{\text{avg}}$) and horizontal section dividers ($\ge 1.75 \times H_{\text{avg}}$).
- **Runtime**: Zero-weight pure-Rust deterministic spatial algorithm in [`src/xycut.rs`](src/xycut.rs).
- **Reference**:
  - Ha et al., *Recursive X-Y Cut using Bounding Boxes of Connected Components*, IEEE ICDAR.

### 4. Table Structure Recognition: SLANet (PP-Structure)
- **Purpose**: Table HTML structure skeleton token prediction and 2D cell bounding box localization.
- **Architecture**: Structure-Location Architecture Network with MobileNetV3 backbone and autoregressive GRU decoder.
- **Weight Footprint**: `models/layout/slanet.onnx` (~7.67 MB) + `models/layout/table_structure_dict.txt`.
- **Runtime**: In-process ONNX Runtime via `ort` with automatic geometric fallback.
- **Reference**:
  - PaddlePaddle Authors, *PP-Structure: A Mobile-Grade Document Analysis System*, 2022.
  - Repository: [PaddleOCR / PP-Structure](https://github.com/PaddlePaddle/PaddleOCR)

### 5. Formula Recognition: RapidLaTeXOCR
- **Purpose**: Mathematical formula image crop transcription into clean LaTeX equation syntax.
- **Architecture**: Vision transformer encoder with autoregressive sequence decoder and ByteLevel BPE tokenizer.
- **Weight Footprint**:
  - `models/latex/encoder.onnx` (~84.9 MB)
  - `models/latex/decoder.onnx` (~48.6 MB)
  - `models/latex/tokenizer.json` (~24 KB)
- **Runtime**: In-process ONNX Runtime via `ort` with fallback to text OCR.
- **Reference**:
  - Blecher et al., *LaTeX-OCR (pix2tex)* / RapidAI.
  - Repository: [RapidLaTeXOCR](https://github.com/RapidAI/RapidLaTeXOCR)

---

## 🔮 Roadmap Models & References

### 6. Obsidian Metadata Extraction: GLiNER-base
- **Purpose**: Open-type zero-shot Named Entity Recognition (NER) to extract structured document properties for **Obsidian YAML Frontmatter**.
- **Target Metadata Properties**:
  - `title`: Primary document title.
  - `authors`: Contributing authors and researchers.
  - `date`: Publication date, report release date, or document creation timestamp.
  - `organization`: University, corporation, or publisher affiliation.
  - `keywords`: Subject topics, domain categories, or tags.
  - `doi`: Digital Object Identifier or document reference code.
- **Architecture**: Generalist and Lightweight Named Entity Recognition model based on bidirectional DeBERTa-v3 / ModernBERT encoder with span representation layer.
- **Target Weight Footprint**: ~109M parameters (~100 MB INT8 ONNX).
- **Obsidian Frontmatter Output Schema**:
  ```yaml
  ---
  title: "Attention Is All You Need"
  authors:
    - "Ashish Vaswani"
    - "Noam Shazeer"
    - "Niki Parmar"
  date: "2017-06-12"
  organization: "Google Brain"
  keywords:
    - "Transformer"
    - "Self-Attention"
    - "Neural Machine Translation"
  doi: "10.48550/arXiv.1706.03762"
  ---
  ```
- **Advantages for `pdfmd-lite`**:
  - **Zero-Shot Flexibility**: Extracts user-defined entity labels without task-specific fine-tuning.
  - **No LLM Overhead**: Runs as an encoder-only model without conversational LLM memory overhead (under 300 MB RAM budget).
  - **Pure Local Inference**: Compatible with ONNX Runtime (`ort`) CPU inference.
- **Reference**:
  - Zaratiana et al., *GLiNER: Generalist Model for Named Entity Recognition using Bidirectional Transformer*, NAACL 2024.
  - Repository: [urchade/GLiNER](https://github.com/urchade/GLiNER)
  - ONNX Reference: [GLiNER ONNX Runtime Integration](https://github.com/urchade/gliner)
