# 0009: Per-Note Embedding Sidecars and Frontmatter Hash Idempotency

## Status
Accepted

## Context
When processing large technical vaults with `pdfmd-lite`, users require semantic retrieval (RAG) capabilities for Obsidian plugins (e.g. Smart Connections, Omnisearch) and fast incremental batch conversions. Centralized vector databases or cache stores (e.g. SQLite, Qdrant) introduce mutable single points of failure, break folder portability, and require background coordination across batch processes.

## Decision
1. Persist dense vector embeddings as decentralized per-document JSON Lines sidecars (`<slug>/<slug>.embeddings.jsonl`) placed directly alongside the converted Markdown notes, containing chunked texts, physical page anchors, and float vectors.
2. Implement batch conversion idempotency by computing the input PDF's SHA-256 digest and persisting it as a `pdf_sha256` property in the note's YAML frontmatter. Incremental resume skips re-conversion when the source file hash matches the frontmatter without maintaining external cache files.
3. Retain the `<slug>/<slug>_images/` disk directory layout mandated by ADR 0004 as the default, offering single-file base64 image inlining exclusively as an opt-in CLI flag (`--embed-images-base64`).

## Consequences
Guarantees zero central database dependencies, full file portability across Obsidian vaults and git repositories, and deterministic, stateless incremental batch conversions. Cross-document global search requires scanning sidecars or an external indexing pass.
