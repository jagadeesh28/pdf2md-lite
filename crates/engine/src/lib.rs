#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod backends;
pub mod batch;
pub mod concurrency;
pub mod config;
pub mod controller;
pub mod embeddings;
pub mod figure;
pub mod ir;
pub mod markdown;
pub mod metadata;
pub mod model_locator;
pub mod path_policy;
pub mod pdf;
pub mod pipeline;
pub mod progress;
pub mod report;
pub mod telemetry;
pub mod vault;
pub mod watch;
pub mod xycut;

pub use backends::indra::{IndraEngine, IndraScript, TextLine};
pub use batch::{
    BatchConversionResult, ConversionPlan, DryRunReport, OutputPlanner, PlannedPaths,
    compute_disambiguation_hash, dry_run_pdf_directory, dry_run_single_pdf, process_pdf_directory,
    process_pdf_directory_all, process_pdf_directory_all_with_emitter, process_pdf_directory_full,
    process_pdf_directory_with_image_options, sanitize_short_slug, sanitize_windows_device_name,
};
pub use concurrency::{resolve_concurrency, resolve_session_pool_capacity};
pub use config::{
    AppConfig, CapabilitiesResponse, DeviceTarget, LanguageCapability, get_capabilities,
};
pub use controller::{
    RunConflictError, RunController, RunGuard, RunState, execute_isolated_document,
};
pub use embeddings::{EmbeddingEngine, EmbeddingRecord, SemanticChunk};
pub use figure::{
    CroppedFigure, ImageFormat, ImageOptions, PageLayoutResult, encode_image_to_base64_data_uri,
};
pub use ir::{
    BlockIr, BlockKind, BlockOrderTrace, BoundingBox, DebugLayoutTrace, LayoutBoxTrace, LineIr,
    PageIr, Provenance, RegionIr, RegionKind, SpanIr,
};
pub use markdown::{
    DocumentMetadata, PdfExtractionResult, attach_figures_to_page_text, compute_bytes_sha256,
    compute_file_sha256, convert_figure_links_to_obsidian, format_page_demarcation,
    normalize_latex_delimiters, render_markdown, replace_png_links_with_jpg,
};
pub use model_locator::{
    KNOWN_MODELS, ModelLocator, ModelManifest, ModelStatus, get_global_models_dir,
    get_model_status, invalidate_model_caches, set_global_models_dir,
};
pub use path_policy::{PathPolicy, PathPolicyError};
pub use pdf::{
    DEFAULT_MAX_PIXELS_PER_PAGE, DomExtractionMode, PdfSession, can_use_hybrid_dom,
    clean_extracted_text, extract_baseline_aware_dom_text_lines, extract_pdf_metadata,
    extract_pdf_metadata_from_doc, extract_pdf_text, get_pdf_page_count, render_pages_to_images,
    safe_extract_text_by_pages, stream_rendered_pages_from_session, validate_page_range,
};
pub use pipeline::{
    ConversionOptions, format_warning_callout, get_indra_engine, process_pages_stream,
    process_pages_stream_with_options, process_pdf, process_pdf_full,
    process_pdf_full_with_emitter, process_pdf_with_options, process_pdf_with_pipeline_options,
    process_pdf_with_source, safe_process_single_page,
};
pub use progress::{
    BatchProgressTracker, NullProgressEmitter, ProgressEmitter, TrackerProgressEmitter, format_eta,
};
pub use report::{ConversionReport, ConversionReportFrontmatter, ConversionStatus};
pub use telemetry::{init_telemetry, parse_log_level};
pub use vault::{
    ConvertImagesSummary, EmbedVaultSummary, FormatNotesSummary, VaultTaskProgress,
    convert_vault_png_to_jpeg, convert_vault_png_to_jpeg_with_progress, download_embedding_models,
    embed_vault_in_path, format_notes_in_path, format_notes_in_path_with_progress,
    install_offline_models,
};
pub use watch::{WatchOptions, watch_directory};

/// Helper to resolve paths to model files across locator discovery paths.
pub fn resolve_model_path(subpath: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    let sub = subpath.as_ref();
    let sub_str = sub.to_string_lossy();
    if let Some(located) = ModelLocator::new(None).locate(&sub_str) {
        located
    } else {
        sub.to_path_buf()
    }
}
