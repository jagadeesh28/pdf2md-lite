# ADR 0006: Constrained Script Decoding and Progressive Document Metadata

## Context
Analysis of 119 converted documents revealed two critical failure modes:
1. Multilingual OCR (`rec_world.onnx`) running on Telugu archives produced severe cross-script character hallucinations (Georgian, Ethiopic, Sinhala) due to unconstrained argmax across a 2,935-token vocabulary where only 100 characters are Telugu.
2. Metadata extraction executed upfront on Page 1 before document transcription; on scanned PDFs, DOM text extraction produced empty strings, yielding generic titles (`title: "1"`), null authors, and hardcoded dummy tags (`[pdf, notes]`). Additionally, inline math formulas were aggressively split into standalone display blocks (`\n\n$$\n...\n$$\n\n`), fragmenting paragraph prose.

## Decision
1. **Constrained CTC Masking**: Dynamically mask candidate token indices during CTC decoding to the active language script (e.g. Telugu `U+0C00..=U+0C7F`, basic Latin, ASCII numerals, and punctuation), leveraging `topi`/`topv` rankings to select the highest-scoring in-script candidate.
2. **Hybrid Math Gating**: Differentiate inline math spans (`$...$`) from display math blocks (`$$...$$`) using spatial baseline continuity with surrounding text and semantic stop-word rejection.
3. **Two-Stage Progressive Metadata**: Fast-scan PDF catalog properties upfront, followed by full-document transcript analysis post-extraction to synthesize missing titles, authors, and dynamic Obsidian categorization and classification tags (`#type/...`, `#domain/...`).

## Consequences
- **Positive**: Eliminates out-of-script hallucinations in Indic documents without requiring retraining or secondary external OCR engines.
- **Positive**: Keeps inline math variables embedded inside continuous sentences while reserving display blocks for true standalone equations.
- **Positive**: Guarantees rich, high-quality YAML frontmatter with accurate document types and topic tags even for scanned documents.
- **Negative**: Adds a negligible post-extraction metadata analysis pass after page transcription completes.
