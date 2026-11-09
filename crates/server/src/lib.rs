//! In-process Axum Web Server Daemon for pdfmd-lite (`--serve`).

use anyhow::Result;

/// Starts the local web server daemon.
pub async fn start_server(bind_addr: &str) -> Result<()> {
    tracing::info!("Starting pdfmd-lite web daemon on http://{bind_addr}");
    Ok(())
}
