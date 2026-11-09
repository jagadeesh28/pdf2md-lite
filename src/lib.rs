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
pub mod xycut;

pub use backends::indra::{IndraEngine, IndraScript, TextLine};
pub use batch::{
    process_pdf_directory, process_pdf_directory_all, process_pdf_directory_full,
    process_pdf_directory_with_image_options, BatchConversionResult, sanitize_short_slug,
};
pub use cli::{convert_vault_png_to_jpeg, embed_vault_in_path, ConvertImagesSummary, EmbedVaultSummary};
pub use concurrency::resolve_concurrency;
pub use config::AppConfig;
pub use embeddings::{EmbeddingEngine, EmbeddingRecord, SemanticChunk};
pub use figure::{CroppedFigure, ImageFormat, ImageOptions, PageLayoutResult};
pub use progress::{BatchProgressTracker, format_eta};
pub use markdown::{
    attach_figures_to_page_text, convert_figure_links_to_obsidian, format_page_demarcation,
    normalize_latex_delimiters, render_markdown, replace_png_links_with_jpg, DocumentMetadata,
    PdfExtractionResult,
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
