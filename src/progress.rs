//! Rich progress bar and ETA tracking for batch conversion workflows.
//!
//! Provides dynamic progress display with active document tracking, concurrency telemetry,
//! and rolling average per-page latency ETA calculation using `indicatif`.
//! Strictly suppressed when quiet mode is enabled.

use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

/// Formats a duration in seconds into a human-readable ETA string (e.g. `02m 45s`, `01h 12m 30s`).
pub fn format_eta(secs: f32) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return "--".to_string();
    }
    let total_secs = secs.round() as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}h {minutes:02}m {seconds:02}s")
    } else {
        format!("{minutes:02}m {seconds:02}s")
    }
}

/// Internal shared state for calculating rolling throughput and tracking active workers.
#[derive(Debug)]
struct TrackerState {
    completed_pages: usize,
    total_pages: usize,
    /// Active workers: worker_id -> (document_slug, worker_thread_count)
    active_workers: HashMap<usize, (String, usize)>,
    /// Rolling window of (page_count, elapsed_secs) for the last N completed documents
    rolling_history: VecDeque<(usize, f32)>,
    start_time: Instant,
}

impl TrackerState {
    fn new(total_pages: usize) -> Self {
        Self {
            completed_pages: 0,
            total_pages,
            active_workers: HashMap::new(),
            rolling_history: VecDeque::with_capacity(32),
            start_time: Instant::now(),
        }
    }

    /// Computes rolling page throughput (pages per second) and estimated remaining seconds.
    fn compute_throughput_and_eta(&self) -> (f32, Option<f32>) {
        let (window_pages, window_secs): (usize, f32) = self
            .rolling_history
            .iter()
            .fold((0, 0.0), |(p_acc, s_acc), &(p, s)| (p_acc + p, s_acc + s));

        let throughput = if window_pages > 0 && window_secs > 0.01 {
            window_pages as f32 / window_secs
        } else {
            let total_elapsed = self.start_time.elapsed().as_secs_f32();
            if self.completed_pages > 0 && total_elapsed > 0.01 {
                self.completed_pages as f32 / total_elapsed
            } else {
                0.0
            }
        };

        let remaining_pages = self.total_pages.saturating_sub(self.completed_pages);
        let eta = if throughput > 0.001 && remaining_pages > 0 {
            Some(remaining_pages as f32 / throughput)
        } else {
            None
        };

        (throughput, eta)
    }

    /// Formats active workers summary: e.g. `[doc_1 (4w), doc_2 (4w)]`
    fn format_active_summary(&self) -> String {
        if self.active_workers.is_empty() {
            return "idle".to_string();
        }

        let mut entries: Vec<String> = self
            .active_workers
            .values()
            .map(|(slug, w)| format!("{slug} ({w}w)"))
            .collect();
        entries.sort();

        if entries.len() <= 2 {
            format!("[{}]", entries.join(", "))
        } else {
            format!("[{}, (+{} more)]", entries[0], entries.len() - 1)
        }
    }
}

/// Thread-safe batch progress tracker wrapping `indicatif::ProgressBar`.
pub struct BatchProgressTracker {
    pb: ProgressBar,
    state: Mutex<TrackerState>,
    total_docs: usize,
    quiet: bool,
}

impl BatchProgressTracker {
    /// Creates a new progress tracker for a batch run.
    pub fn new(total_docs: usize, total_pages: usize, quiet: bool) -> Self {
        if quiet {
            return Self {
                pb: ProgressBar::hidden(),
                state: Mutex::new(TrackerState::new(total_pages)),
                total_docs,
                quiet: true,
            };
        }

        let pb = ProgressBar::new(total_docs as u64);
        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} docs ({percent}%) | {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-");

        pb.set_style(style);
        pb.enable_steady_tick(std::time::Duration::from_millis(150));
        pb.set_message("Initializing batch conversion...");

        Self {
            pb,
            state: Mutex::new(TrackerState::new(total_pages)),
            total_docs,
            quiet: false,
        }
    }

    /// Records that an already converted document was skipped via batch resume.
    pub fn on_doc_skipped(&self, index: usize, slug: &str) {
        if !self.quiet {
            self.pb.println(format!(
                "[{}/{}] [SKIPPED] {} (already converted)",
                index, self.total_docs, slug
            ));
            self.pb.inc(1);
        }
    }

    /// Records the start of a document conversion on a specific worker.
    pub fn on_doc_start(&self, worker_id: usize, slug: &str, workers: usize) {
        if self.quiet {
            return;
        }

        let mut state = self.state.lock().unwrap();
        state
            .active_workers
            .insert(worker_id, (slug.to_string(), workers));
        self.update_message_locked(&state);
    }

    /// Records the completion of a document conversion.
    pub fn on_doc_complete(
        &self,
        worker_id: usize,
        index: usize,
        slug: &str,
        pages: usize,
        elapsed: f32,
        is_ok: bool,
    ) {
        if self.quiet {
            return;
        }

        let mut state = self.state.lock().unwrap();
        state.active_workers.remove(&worker_id);
        state.completed_pages += pages;

        if state.rolling_history.len() >= 20 {
            state.rolling_history.pop_front();
        }
        state.rolling_history.push_back((pages, elapsed));

        let rate_str = if elapsed > 0.01 {
            format!("{:.1} pgs/s", pages as f32 / elapsed)
        } else {
            "-".to_string()
        };

        let status = if is_ok { "OK" } else { "FAILED" };
        self.pb.println(format!(
            "[{}/{}] [{status}] {} ({} page(s), {:.2}s, {rate_str})",
            index, self.total_docs, slug, pages, elapsed
        ));

        self.update_message_locked(&state);
        self.pb.inc(1);
    }

    /// Records that a document conversion failed.
    pub fn on_doc_failed(&self, worker_id: usize, index: usize, slug: &str, err: &str) {
        if self.quiet {
            return;
        }

        {
            let mut state = self.state.lock().unwrap();
            state.active_workers.remove(&worker_id);
            self.update_message_locked(&state);
        }

        self.pb.println(format!(
            "[{}/{}] [FAILED] {slug}: {err}",
            index, self.total_docs
        ));
        self.pb.inc(1);
    }

    /// Updates the status message on the progress bar.
    fn update_message_locked(&self, state: &TrackerState) {
        let (throughput, eta) = state.compute_throughput_and_eta();
        let eta_str = eta.map(format_eta).unwrap_or_else(|| "--".to_string());
        let active = state.format_active_summary();

        let throughput_str = if throughput > 0.0 {
            format!("{throughput:.1} p/s")
        } else {
            "-- p/s".to_string()
        };

        self.pb.set_message(format!(
            "{active} | {throughput_str} | ETA: {eta_str}"
        ));
    }

    /// Finishes the progress bar and clears the transient line cleanly.
    pub fn finish(&self) {
        if !self.quiet {
            self.pb.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_eta_formatting() {
        assert_eq!(format_eta(0.0), "--");
        assert_eq!(format_eta(-5.0), "--");
        assert_eq!(format_eta(f32::NAN), "--");
        assert_eq!(format_eta(45.0), "00m 45s");
        assert_eq!(format_eta(125.0), "02m 05s");
        assert_eq!(format_eta(3665.0), "01h 01m 05s");
    }

    #[test]
    fn test_tracker_throughput_and_eta() {
        let mut state = TrackerState::new(100);
        state.completed_pages = 20;
        state.rolling_history.push_back((20, 10.0)); // 2 pages / sec

        let (throughput, eta) = state.compute_throughput_and_eta();
        assert!((throughput - 2.0).abs() < 1e-4);
        // 80 pages remaining at 2 pages/sec = 40 seconds
        assert!(eta.is_some());
        let eta_val = eta.unwrap();
        assert!((eta_val - 40.0).abs() < 1e-2);
    }

    #[test]
    fn test_tracker_quiet_mode_does_not_panic() {
        let tracker = BatchProgressTracker::new(5, 50, true);
        tracker.on_doc_start(0, "test_doc", 2);
        tracker.on_doc_complete(0, 1, "test_doc", 10, 5.0, true);
        tracker.on_doc_failed(1, 2, "failed_doc", "error reading page");
        tracker.on_doc_skipped(3, "skipped_doc");
        tracker.finish();
    }

    #[test]
    fn test_tracker_active_summary_formatting() {
        let mut state = TrackerState::new(50);
        assert_eq!(state.format_active_summary(), "idle");

        state.active_workers.insert(0, ("paper_a".to_string(), 4));
        assert_eq!(state.format_active_summary(), "[paper_a (4w)]");

        state.active_workers.insert(1, ("paper_b".to_string(), 4));
        assert!(state.format_active_summary().contains("paper_a (4w)"));
        assert!(state.format_active_summary().contains("paper_b (4w)"));

        state.active_workers.insert(2, ("paper_c".to_string(), 2));
        assert!(state.format_active_summary().contains("(+2 more)"));
    }
}
