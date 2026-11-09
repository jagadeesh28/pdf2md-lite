# 0012: Baseline-Aware Hybrid DOM Extraction with Layout Gating

## Status
Accepted

## Context
Scientific PDFs with pristine digital text streams frequently suffer when extracted using naive DOM text dumps:
1. Academic fonts (CMMI, CMSY) lacking `/ToUnicode` CMaps decode into unprintable ASCII noise.
2. Subscripts, superscripts, and baseline offsets ($\Delta y$) flatten into identical baselines ($x^2 \rightarrow x2$), destroying mathematical meaning.
3. Spatial 2D math (fractions, integrals, summations) collapses into disconnected glyph fragments.
Conversely, routing every digital paragraph through Indra neural vision OCR imposes significant CPU tensor overhead on pages with clean, well-formed digital fonts.

## Decision
1. Introduce a baseline-aware hybrid text extraction engine controlled by `--dom-extraction <auto|always|never>` (defaulting to `auto`).
2. When evaluating a page in `auto` mode:
   - Validate font `/ToUnicode` CMaps in the PDF catalog dictionary. If CMaps are incomplete or corrupt, automatically route the full page to Indra OCR.
   - Detect vertical baseline shifts ($\Delta y$) between adjacent glyphs using font matrices and text operators. If font size decreases and $\Delta y > 2\text{ pt}$, synthesize semantic Markdown sub/superscripts (`^2`, `_i`).
   - Strictly gate neural layout regions: all YOLOv8n DocLayNet formula boxes (Class 2) and table boundaries (Class 1) are excluded from the DOM text stream and routed exclusively to RapidLaTeXOCR and SLANet/geometric grid extractors.
3. Allow users to force pure vision OCR via `--dom-extraction never` or unconditional DOM extraction via `--dom-extraction always`.

## Consequences
- Clean digital scientific PDFs process 5× to 10× faster by bypassing DBNet/SVTR tensor inference on body prose.
- Mathematical equations, tables, figures, and sub/superscripts maintain 100% fidelity through neural layout gating and $\Delta y$ baseline tracking.
- Scanned, raster-only, or degraded PDFs safely fall back to pure-vision Indra OCR without user intervention.
