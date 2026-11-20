# 0015: Hybrid Dual-Runtime and DirectML Hardware Acceleration

## Status
Accepted

## Context
Document conversion profiling showed ~95.5% user CPU time in tensor math (~21 s/doc) because all ONNX Runtime (`ort`) vision models (RapidLaTeXOCR, SLANet, GLiNER, MiniLM) executed on CPU under `GraphOptimizationLevel::Level1` without execution providers. Furthermore, the repository maintained `tract-onnx` for pure-Rust SIMD execution of Indra OCR and YOLOv8n layout analysis (ADRs 0001 and 0005) while running other models in `ort`, creating dual-runtime memory overhead (~350 MB base session budget) and unaccelerated layout analysis.

## Decision
Establish a **Hybrid Dual-Runtime Architecture** with dynamic **Accelerated Execution Provider (EP)** selection:
1. **Preserve `tract-onnx` Pure-Rust Baseline**: Retain `tract-onnx` as the default, zero-dependency pure-Rust fallback engine for baseline CPU document conversion (Indra OCR + YOLO layout) across all platforms.
2. **DirectML Acceleration for ORT**: Enable DirectX 12 DirectML hardware acceleration on Windows for `ort` vision sessions, targeting GPUs and NPUs across Intel, AMD, NVIDIA, and Qualcomm hardware.
3. **Dynamic Auto-Detection with Graceful Fallback**: Probe DirectML device creation at runtime; if DirectML fails, probe CPU execution provider with `GraphOptimizationLevel::Level3` and explicit intra/inter-op thread bounds. Expose typed `--device auto|cpu|directml` CLI/config controls.
4. **Quantized Next-Gen Models**: Target INT8/FP16 ONNX models for heavy neural tasks (UniMERNet, PP-DocLayout-L) to maintain lean MSI packaging (<350 MB Full MSI) and bounded RAM resident budgets.

## Consequences
Unlocks 10–50× tensor execution speedups on GPU-equipped Windows systems while preserving 100% pure-Rust offline baseline portability without requiring external GPU driver SDKs or CUDA toolchains. Prevents thread oversubscription across parallel page workers.
