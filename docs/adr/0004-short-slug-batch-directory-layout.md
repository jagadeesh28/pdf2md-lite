# 0004: Short-Slug Batch Directory Layout and Page Indexing

## Status
Accepted

## Context
Batch converting nested hierarchies of PDFs with long filenames on Windows risks exceeding the legacy 260-character path limit (`MAX_PATH`) when nested image asset directories are created. Additionally, consumers of converted Markdown notes need clear page boundaries and flexible 1-based page range slices.

## Decision
Derive a sanitized short slug (capped at 32 characters) for each document to namespace its output directory and `<slug>_images/` asset subfolder. Insert explicit Obsidian page demarcation comments (`<!-- Page N -->`) at every page boundary, and support arbitrary 1-based page slicing (`[begin-page] [end-page]`) across single and batch modes.

## Consequences
Guarantees robust cross-platform path safety on Windows while ensuring Obsidian readers can trace text, tables, and raw crops directly back to specific physical document pages.
