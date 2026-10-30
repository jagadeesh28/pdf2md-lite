# 0007: Global Worker Pool and Pipelined In-Memory Rasterization

## Status
Accepted

## Context
In previous releases, batch directory conversion iterated through PDF files sequentially, causing CPU starvation when processing directories with short documents (1–3 pages). Furthermore, single-document conversion rendered all pages sequentially, encoded them to PNGs on disk, and then re-read them from disk before starting OCR workers. This introduced significant cold-start latency and hundreds of megabytes of redundant disk I/O.

## Decision
1. **Global Worker Pool**: Establish a unified, process-wide worker pool sized dynamically by the hardware governor:
   $$\text{workers} = \max\left(1, \min\left(\text{logical cores} - 1, 1 + \left\lfloor\frac{\text{free RAM} - 250\text{ MB}}{120\text{ MB}}\right\rfloor\right)\right)$$
   Batch processing dispatches multi-document jobs through this global pool, preventing thread oversubscription and ensuring small documents run concurrently without exceeding host CPU or RAM limits.
2. **Pipelined In-Memory Rasterization**: Stream uncompressed `DynamicImage` page rasters directly from an in-process rendering thread to active workers through a bounded synchronization channel (`bound = min(workers, 4)`), eliminating intermediate PNG encoding, temporary disk spooling, and redundant disk reads.
3. **CLI Quiet Mode**: Add a `--quiet` / `-q` (or `--silent`) CLI flag to suppress individual document and page progress messages, emitting only atomic completion results or fatal errors.

## Consequences
- Eliminates temporary disk churn and cold-start latency for document conversions.
- Maximizes 12-core CPU utilization across directories containing diverse document sizes.
- Keeps peak memory footprint strictly bounded ($\le 60$ MB buffer margin) while preserving full visual color fidelity for cropped figures and tables.
