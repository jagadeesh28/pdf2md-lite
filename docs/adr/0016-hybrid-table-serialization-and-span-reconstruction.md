# 0016: Hybrid Table Serialization and Span Reconstruction

## Status
Accepted

## Context
Standard GitHub Flavored Markdown (GFM) tables lack native syntax for multi-row and multi-column merged cells (`rowspan` and `colspan`). Previously, `pdfmd-lite` flattened detected tables into naive 2D grids, discarding cell spans. While next-generation table models (PP-StructureV2 SLANet_Plus) accurately predict cell span tokens, replacing Markdown tables with raw HTML `<table>` markup in the primary document flow degrades readability in Obsidian and impairs plaintext grep/search.

## Decision
Adopt a **Hybrid Table Serialization** strategy across the pipeline:
1. **Primary Document Flow (Clean GFM Table)**: Emit standard GitHub Flavored Markdown tables with merged cell text replicated across covered cells, ensuring portable, readable Markdown and clean semantic chunking.
2. **Companion Callout Flow (Exact HTML Structure)**: Inside the collapsible Obsidian Companion Table Callout (`> [!table]- Original Table & Structure`), emit an exact HTML `<table>` preserving `rowspan` and `colspan` attributes adjacent to the high-resolution visual crop.

## Consequences
Preserves 100% tabular geometry and cell hierarchy for complex technical tables without compromising Obsidian markdown readability or plaintext search indexing.
