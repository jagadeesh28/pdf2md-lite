# ADR 0005: Retention of YOLOv8n DocLayNet for Pure-Rust Execution

## Context
YOLO11 introduces architectural updates (such as C2PSA attention blocks) offering marginal (~1–2% mAP) improvements on small objects over YOLOv8. However, YOLO11's attention subgraphs and dynamic slice operations are currently unsupported by `tract-onnx`, which would force layout analysis onto external C++ runtimes (`ort`) and forfeit zero-dependency pure-Rust compilation.

## Decision
Retain `yolov8n-doclaynet.onnx` as the standard in-process document layout detector.

## Consequences
- **Positive**: Preserves 100% pure-Rust standalone compilation via `tract-onnx` for all supported platforms (Windows, Linux, x86-64, x86-32, aarch64) without requiring external ONNX Runtime shared libraries.
- **Positive**: Maintains verified >93% mAP50 layout segmentation precision on DocLayNet classes (`Picture`, `Table`, `Formula`).
- **Negative**: Foregoes minor small-object segmentation improvements present in YOLO11.
