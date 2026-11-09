# 0010: In-Process MiniLM Sentence Embeddings and Standard RAG Schema

## Status
Accepted

## Context
Adding semantic search and retrieval-augmented generation (RAG) capabilities to `pdfmd-lite` requires an in-process, offline text embedding pipeline that operates without external network services or heavy memory footprints. Converted notes span technical prose, equations, and tables, necessitating coherent chunking and standard vector persistence for Obsidian RAG plugins (e.g. Smart Connections, Omnisearch).

## Decision
1. Standardize on `all-MiniLM-L6-v2` (quantized INT8 ONNX, ~22 MB footprint) generating 384-dimensional dense vectors evaluated via pure-Rust `tract-onnx` by default, with optional `ort` acceleration.
2. Employ the pure-Rust `tokenizers` crate to parse Hugging Face `tokenizer.json` for faithful WordPiece sequence encoding and attention masking.
3. Partition notes via section-aware chunking (keyed to Markdown headings `#`/`##` and `<!-- Page N -->` anchors, bounded to 256–512 tokens).
4. Persist vector representations in `<slug>/<slug>.embeddings.jsonl` using the standard RAG schema: `{"chunk_id", "page_num", "heading", "text", "embedding"}`.
5. Provide automatic local resolution in `models/embeddings/` backed by a `--download-models` provisioning command to pull certified INT8 ONNX models and tokenizers from Hugging Face.

## Consequences
Guarantees lightweight (~22 MB), 100% offline embedding generation within the 350 MB base concurrency budget. The 384-dimensional vectors are fixed across all sidecars; changing embedding models in the future requires re-embedding vault notes.
