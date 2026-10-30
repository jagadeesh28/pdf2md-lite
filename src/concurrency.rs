//! Hardware-Aware Concurrency Governor.
//!
//! Evaluates host machine resources (logical CPU cores and available physical memory)
//! at runtime to determine the optimal number of parallel page workers per ADR 0003.

use sysinfo::System;

/// Resolves page worker concurrency based on machine resources or an explicit user override.
pub fn resolve_concurrency(explicit_override: Option<usize>) -> usize {
    if let Some(val) = explicit_override {
        if val > 0 {
            return val;
        }
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

    // Base footprint for neural models (tract/onnx) + runtime is ~250 MB.
    // Each active worker page holds high-res raster pixmaps and intermediate ONNX tensors (~120 MB safe headroom).
    let base_mb = 250.0;
    let per_worker_mb = 120.0;
    let memory_budget_workers = if ram_mb > base_mb {
        1 + ((ram_mb - base_mb) / per_worker_mb).floor() as usize
    } else {
        1
    };

    // Leave 1 core for OS / main thread if multi-core
    let core_budget_workers = if logical_cores > 1 {
        logical_cores - 1
    } else {
        1
    };

    let optimal_workers = core_budget_workers.min(memory_budget_workers).max(1);

    println!(
        "Hardware Governor: detected {logical_cores} cores, {available_ram_gb:.2} GB available RAM -> resolved {optimal_workers} worker(s)"
    );

    optimal_workers
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
