# 0002: Three-Way YOLO Layout Routing for Pictures, Tables, and Formulas

## Status
Accepted

## Context
Standard document OCR struggles with mixed media: non-text diagrams hallucinate corrupted text lines, tabular rows collapse into unformatted single lines, and multi-line mathematical equations get intermixed with prose sentences.

## Decision
Implement a three-way layout routing strategy driven by YOLOv8n DocLayNet detections:
1. `Picture` (class 6): Masked on the page image before text OCR to eliminate line hallucinations; saved as a raw image crop and linked in Markdown.
2. `Table` (class 8): Unmasked; internal text line coordinates are grouped via pure-Rust spatial clustering into a GitHub Flavored Markdown table, accompanied by a collapsible Obsidian image callout (`> [!table]-`).
3. `Formula` (class 2): Unmasked; text lines falling within formula bounds are isolated and guaranteed to be rendered inside Obsidian display math blocks (`$$...$$`).

## Consequences
Eliminates OCR hallucination on diagrams while preserving full OCR and text stream precision for tabular and mathematical structures without requiring separate neural table or formula recognition models.
