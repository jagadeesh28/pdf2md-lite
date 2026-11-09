# 0011: Fine-Grained Session Mutexes and In-Place Grayscale Masking

## Status
Accepted

## Context
High-concurrency document conversion profiling identified two primary bottlenecks:
1. Autoregressive formula decoding in `RapidLatexEngine` locked `Mutex<ort::session::Session>` for the entire 128-token sequence generation loop, stalling all concurrent page worker threads across the process. Because `ort::Session::run(&mut self)` requires mutable references, sessions cannot be evaluated simultaneously without synchronization. Duplicating sessions per worker thread incurs ~134 MB of resident ONNX weights per thread, threatening the 350 MB memory budget on constrained hosts.
2. When layout analysis detected zero picture regions, `src/pipeline.rs` executed `page_img.clone()` on the 2× supersampled RGBA raster (15–30 MB) before converting to grayscale, wasting heap allocation bandwidth and triggering unnecessary memory pressure.

## Decision
1. Retain a single resident `Session` instance for the encoder and decoder to preserve the conservative memory budget ($350\text{ MB} + W \times 200\text{ MB}$), but narrow the `Mutex` lock scope in the decoder loop to the single inference step (`decoder.run(...)`, ~1–2ms). Token sampling, argmax, and repetition gating execute outside the lock, allowing concurrent worker threads to interleave decoding steps with minimal contention.
2. Eliminate intermediate 32-bit RGBA clones by converting the rendered page canvas to an 8-bit `GrayImage` once upfront. Apply figure masking via `mask_gray_regions_mut(&mut GrayImage, &[FigureRect])`, writing white pixels (`255u8`) directly into the grayscale byte buffer in-place.

## Consequences
- Multi-worker page pipelines process formulas concurrently without multi-hundred-millisecond thread stalls while adding zero extra memory overhead for duplicate ONNX sessions.
- Eliminates 15–30 MB of transient heap allocations per page, significantly lowering memory churn and reducing garbage collection / allocator fragmentation during large batch runs.
