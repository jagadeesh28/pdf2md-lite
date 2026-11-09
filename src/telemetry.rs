//! Structured telemetry and logging for pdfmd-lite.
//!
//! Provides unified `tracing` subscriber initialization with non-interfering console output,
//! optional file logging (`--log-file`), and configurable log levels (`--log-level`).
//! Strictly zero environment variable configuration per project architectural constraints.

use anyhow::Result;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Once;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static INIT_ONCE: Once = Once::new();

/// Parses a string into a tracing `LevelFilter`.
pub fn parse_log_level(level_str: Option<&str>) -> LevelFilter {
    match level_str.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("trace") => LevelFilter::TRACE,
        Some("debug") => LevelFilter::DEBUG,
        Some("info") => LevelFilter::INFO,
        Some("warn") | Some("warning") => LevelFilter::WARN,
        Some("error") => LevelFilter::ERROR,
        Some("off") => LevelFilter::OFF,
        _ => LevelFilter::INFO,
    }
}

/// Initializes process-wide structured tracing subscriber.
///
/// - Console layer: outputs formatted events to stderr with ANSI colors (muted if `quiet` is true).
/// - File layer: if `log_file` is specified, appends timestamped plain-text events to disk.
pub fn init_telemetry(level_str: Option<&str>, log_file: Option<&Path>, quiet: bool) -> Result<()> {
    let mut init_err: Option<anyhow::Error> = None;

    INIT_ONCE.call_once(|| {
        let level = parse_log_level(level_str);

        let console_filter = if quiet { LevelFilter::OFF } else { level };

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(!quiet)
            .with_target(false)
            .with_filter(console_filter);

        let file_layer = if let Some(path) = log_file {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = std::fs::create_dir_all(parent);
            }
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => {
                    let layer = tracing_subscriber::fmt::layer()
                        .with_writer(file)
                        .with_ansi(false)
                        .with_target(true)
                        .with_filter(level);
                    Some(layer)
                }
                Err(e) => {
                    init_err = Some(anyhow::anyhow!(
                        "Failed to open log file at {}: {e}",
                        path.display()
                    ));
                    None
                }
            }
        } else {
            None
        };

        let registry = tracing_subscriber::registry()
            .with(console_layer)
            .with(file_layer);

        if let Err(e) = registry.try_init() {
            // Already initialized (e.g. during test suites), ignore
            let _ = e;
        }
    });

    if let Some(err) = init_err {
        return Err(err);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_level() {
        assert_eq!(parse_log_level(Some("trace")), LevelFilter::TRACE);
        assert_eq!(parse_log_level(Some("debug")), LevelFilter::DEBUG);
        assert_eq!(parse_log_level(Some("INFO")), LevelFilter::INFO);
        assert_eq!(parse_log_level(Some("warn")), LevelFilter::WARN);
        assert_eq!(parse_log_level(Some("warning")), LevelFilter::WARN);
        assert_eq!(parse_log_level(Some("ERROR")), LevelFilter::ERROR);
        assert_eq!(parse_log_level(Some("off")), LevelFilter::OFF);
        assert_eq!(parse_log_level(Some("unknown")), LevelFilter::INFO);
        assert_eq!(parse_log_level(None), LevelFilter::INFO);
    }

    #[test]
    fn test_init_telemetry_with_log_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("test_run.log");

        let res = init_telemetry(Some("debug"), Some(&log_path), true);
        assert!(res.is_ok());

        tracing::info!("Telemetry test event: info");
        tracing::debug!("Telemetry test event: debug");

        // The file should have been created
        assert!(log_path.exists());
    }
}
