pub mod backends;
pub mod batch;
pub mod cli;
pub mod concurrency;
pub mod config;
pub mod embeddings;
pub mod figure;
pub mod markdown;
pub mod metadata;
pub mod pdf;
pub mod pipeline;
pub mod progress;
pub mod watch;
pub mod xycut;

pub use backends::indra::{IndraEngine, IndraScript, TextLine};
pub use batch::{
    BatchConversionResult, ConversionPlan, DryRunReport, dry_run_pdf_directory, dry_run_single_pdf,
    process_pdf_directory, process_pdf_directory_all, process_pdf_directory_full,
    process_pdf_directory_with_image_options, sanitize_short_slug,
};
pub use cli::{
    ConvertImagesSummary, EmbedVaultSummary, convert_vault_png_to_jpeg, embed_vault_in_path,
};
pub use concurrency::resolve_concurrency;
pub use config::AppConfig;
pub use embeddings::{EmbeddingEngine, EmbeddingRecord, SemanticChunk};
pub use figure::{CroppedFigure, ImageFormat, ImageOptions, PageLayoutResult};
pub use markdown::{
    DocumentMetadata, PdfExtractionResult, attach_figures_to_page_text,
    convert_figure_links_to_obsidian, format_page_demarcation, normalize_latex_delimiters,
    render_markdown, replace_png_links_with_jpg,
};
pub use pdf::{
    clean_extracted_text, extract_pdf_metadata, extract_pdf_text, get_pdf_page_count,
    render_pages_to_images, safe_extract_text_by_pages, validate_page_range,
};
pub use pipeline::{
    get_indra_engine, process_pages_stream, process_pages_stream_with_options, process_pdf,
    process_pdf_full, process_pdf_with_options, process_pdf_with_pipeline_options,
    process_pdf_with_source, safe_process_single_page,
};
pub use progress::{BatchProgressTracker, format_eta};
pub use watch::{WatchOptions, watch_directory};
