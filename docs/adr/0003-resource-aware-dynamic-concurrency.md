# 0003: Resource-Aware Dynamic Concurrency

## Status
Accepted

## Context
Running multiple concurrent ONNX model instances (`det.onnx`, `rec.onnx`, `yolov8n-doclaynet.onnx`) and high-resolution PDF page rasters can exhaust host memory and cause thrashing or out-of-memory panics if concurrency is static or uncalibrated. Furthermore, application settings must not rely on environment variables.

## Decision
Dynamically calculate page worker concurrency at startup using available machine resources:
$$\text{workers} = \max\left(1, \min\left(\text{logical cores} - 1, \left\lfloor\frac{\text{free RAM (MB)} - 350\text{ MB}}{200\text{ MB}}\right\rfloor\right)\right)$$
Accounting for a shared 350 MB base resident footprint for ONNX model sessions and 200 MB per active high-resolution worker canvas. Support explicit overrides via command-line flags (`-j`, `--concurrency`) and `pdfmd-lite.toml`, with zero dependency on environment variables. Maintain strict ascending page order using an in-memory BTreeMap reordering buffer.

## Consequences
Prevents system memory exhaustion on memory-constrained workstations while maximizing multi-core throughput on high-spec servers, always streaming pages to disk in correct page order.
