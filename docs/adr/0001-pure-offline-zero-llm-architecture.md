# 0001: Pure-Offline Zero-LLM Architecture

## Status
Accepted

## Context
The previous `pdf2md` pipeline depended on external cloud APIs (Gemini, Gemma) and local LLM runtimes (Ollama/Qwen) for figure analysis, text transcription, and Mermaid synthesis. This created operational friction, latency, and network dependencies that prevent execution in air-gapped, offline environments.

## Decision
Strip all external LLM backends and network clients (`reqwest`, `openssh`, Gemini, Gemma, Qwen). Rely exclusively on in-process neural models (`Indra` OCR and `YOLOv8n` DocLayNet) executed locally via `tract-onnx` and architecture-tuned SIMD kernels via `oxide-matrix`.

## Consequences
Guarantees 100% offline, deterministic execution with zero API keys or external server processes. Complex visual diagram understanding and Mermaid synthesis are intentionally omitted in favor of raw image crops, LaTeX math formatting, and structured tabular extraction.
