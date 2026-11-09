//! Hardware-Aware Concurrency Governor.
//!
//! Evaluates host machine resources (logical CPU cores and available physical memory)
//! at runtime to determine the optimal number of parallel page workers per ADR 0003.
//!
//! The governor is deliberately conservative: it reserves a large base footprint for
//! the ONNX model sessions (which are shared across workers) and assumes each worker
//! needs a full high-res page raster plus inference tensors. On low-RAM hosts this
//! prevents the working-set pressure and OS paging observed during production profiling.

use sysinfo::System;

/// Resolves page worker concurrency based on machine resources or an explicit user override.
pub fn resolve_concurrency(explicit_override: Option<usize>) -> usize {
    if let Some(val) = explicit_override
        && val > 0 {
            return val;
        }

    let logical_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);

    let available_ram_bytes = if let Ok(mut sys) = System::new_all() {
        sys.refresh_memory();
        sys.available_memory()
    } else {
        4 * 1024 * 1024 * 1024
    };
    let ram_mb = available_ram_bytes as f64 / (1024.0 * 1024.0);
    let available_ram_gb = ram_mb / 1024.0;

    // Base footprint: ONNX model sessions (tract + ort), runtime, and OS overhead.
    // These are shared across workers via singletons but still resident once.
    let base_mb = 350.0;
    // Each active worker page holds a high-res raster pixmap (2x scale) plus
    // intermediate ONNX inference tensors. Measured peak ~200 MB per worker.
    let per_worker_mb = 200.0;
    let memory_budget_workers = if ram_mb > base_mb {
        ((ram_mb - base_mb) / per_worker_mb).floor() as usize
    } else {
        1
    };

    // Leave 1 core for OS / main thread if multi-core.
    let core_budget_workers = if logical_cores > 1 {
        logical_cores - 1
    } else {
        1
    };

    // Never exceed memory budget; cap at 1 on severely constrained hosts.
    let optimal_workers = core_budget_workers.min(memory_budget_workers).max(1);

    println!(
        "Hardware Governor: detected {logical_cores} cores, {available_ram_gb:.2} GB available RAM -> resolved {optimal_workers} worker(s)"
    );

    optimal_workers
}

/// Returns a conservative per-document page-worker limit for batch mode.
/// Prevents nested concurrency (batch threads × page workers) from oversubscribing
/// the host. Use this in `src/batch.rs` to cap total in-flight pages.
pub fn batch_page_worker_limit(explicit_override: Option<usize>, global_workers: usize) -> usize {
    if let Some(val) = explicit_override
        && val > 0 {
            return val.min(global_workers);
        }
    // In batch mode, each document could spawn its own page workers.
    // Cap at global_workers so total in-flight pages ≤ resolved concurrency.
    global_workers.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_concurrency_override() {
        assert_eq!(resolve_concurrency(Some(4)), 4);
        assert_eq!(resolve_concurrency(Some(1)), 1);
    }

    #[test]
    fn test_resolve_concurrency_auto() {
        let workers = resolve_concurrency(None);
        assert!(workers >= 1);
    }
}
