# 0008: Unified Post-Extraction Note Formatter

## Status
Accepted

## Context
Earlier extractions produced Markdown notes with spurious code blocks (wrapping standard Markdown tables, headings, and paragraphs in ```` ```markdown ```` fences), unnormalized LaTeX equations, and missing YAML frontmatter properties. Re-running complete PDF OCR conversions across entire document collections is computationally expensive and redundant when high-quality OCR text is already extracted.

## Decision
Introduce a unified post-extraction note formatting pipeline (`--format-notes` / `--fix-notes`) with `--dry-run` support that operates directly on existing `.md` files:
1. Unwraps spurious `markdown` and generic code block fences enclosing valid Markdown structures while strictly preserving legitimate code and diagram syntax (`mermaid`, `python`, `rust`, etc.).
2. Cleans legacy extraction artifacts (`# Extracted PDF text`, isolated running headers).
3. Normalizes LaTeX math equations, balances delimiters/braces, and formats table cells with inline math.
4. Executes in-process neural GLiNER entity extraction and transcript heuristics to non-destructively synthesize missing Obsidian YAML frontmatter properties.

## Consequences
Enables rapid, offline batch repair of existing notes in-place without re-extracting PDFs. Preserves user-authored codeblocks and diagrams while restoring complete Obsidian note properties and MathJax rendering.
