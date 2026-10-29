pub mod backends;
pub mod batch;
pub mod cli;
pub mod concurrency;
pub mod config;
pub mod figure;
pub mod markdown;
pub mod pdf;
pub mod pipeline;

pub use backends::indra::{IndraEngine, TextLine};
pub use batch::{process_pdf_directory, BatchConversionResult, sanitize_short_slug};
pub use concurrency::resolve_concurrency;
pub use config::AppConfig;
pub use figure::{CroppedFigure, PageLayoutResult};
pub use markdown::{
    PdfExtractionResult, attach_figures_to_page_text, format_page_demarcation,
    normalize_latex_delimiters, render_markdown,
};
pub use pdf::{
    clean_extracted_text, extract_pdf_text, get_pdf_page_count, render_pages_to_images,
    safe_extract_text_by_pages, validate_page_range,
};
pub use pipeline::{get_indra_engine, process_pages_stream, process_pdf};
