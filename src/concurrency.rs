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

    let mut sys = System::new_all();
    sys.refresh_memory();

    let available_ram_bytes = sys.available_memory();
    let available_ram_gb = available_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    // Each active worker page holds high-res raster pixmaps and ONNX tensors (~1.5 GB budget)
    let memory_budget_workers = (available_ram_gb / 1.5).floor() as usize;

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
