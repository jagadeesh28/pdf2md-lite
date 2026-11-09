use crate::figure::CroppedFigure;
use std::path::PathBuf;

/// Document properties and frontmatter metadata for Obsidian note reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub source: Option<String>,
    pub author: Option<String>,
    pub organization: Option<String>,
    pub publication_date: Option<String>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creation_date: Option<String>,
    pub doi: Option<String>,
    pub pdf_sha256: Option<String>,
    pub total_pages: Option<usize>,
    pub pages_processed: Option<String>,
    pub tags: Vec<String>,
    pub custom_properties: std::collections::BTreeMap<String, Vec<String>>,
}

impl DocumentMetadata {
    /// Formats metadata into an Obsidian-compatible YAML frontmatter block.
    pub fn to_yaml_frontmatter(&self) -> String {
        let mut yaml = String::new();
        yaml.push_str("---\n");

        if let Some(ref title) = self.title {
            yaml.push_str(&format!("title: \"{}\"\n", escape_yaml(title)));
        }
        if let Some(ref source) = self.source {
            yaml.push_str(&format!("source: \"{}\"\n", escape_yaml(source)));
        }
        if let Some(ref author) = self.author {
            yaml.push_str(&format!("author: \"{}\"\n", escape_yaml(author)));
        }
        if let Some(ref org) = self.organization {
            yaml.push_str(&format!("organization: \"{}\"\n", escape_yaml(org)));
        }
        if let Some(ref pub_date) = self.publication_date {
            yaml.push_str(&format!("date: \"{}\"\n", escape_yaml(pub_date)));
        }
        if let Some(ref journal) = self.journal {
            yaml.push_str(&format!("journal: \"{}\"\n", escape_yaml(journal)));
        }
        if let Some(ref volume) = self.volume {
            yaml.push_str(&format!("volume: \"{}\"\n", escape_yaml(volume)));
        }
        if let Some(ref issue) = self.issue {
            yaml.push_str(&format!("issue: \"{}\"\n", escape_yaml(issue)));
        }
        if let Some(ref subject) = self.subject {
            yaml.push_str(&format!("subject: \"{}\"\n", escape_yaml(subject)));
        }
        if let Some(ref keywords) = self.keywords {
            yaml.push_str(&format!("keywords: \"{}\"\n", escape_yaml(keywords)));
        }
        if let Some(ref date) = self.creation_date {
            yaml.push_str(&format!("created: \"{}\"\n", escape_yaml(date)));
        }
        if let Some(ref doi) = self.doi {
            yaml.push_str(&format!("doi: \"{}\"\n", escape_yaml(doi)));
        }
        if let Some(ref sha) = self.pdf_sha256 {
            yaml.push_str(&format!("pdf_sha256: \"{}\"\n", escape_yaml(sha)));
        }
        if let Some(total) = self.total_pages {
            yaml.push_str(&format!("total_pages: {}\n", total));
        }
        if let Some(ref pages) = self.pages_processed {
            yaml.push_str(&format!("pages_processed: \"{}\"\n", pages));
        }

        for (key, values) in &self.custom_properties {
            if values.len() == 1 {
                yaml.push_str(&format!("{}: \"{}\"\n", escape_yaml(key), escape_yaml(&values[0])));
            } else if !values.is_empty() {
                yaml.push_str(&format!("{}:\n", escape_yaml(key)));
                for val in values {
                    yaml.push_str(&format!("  - \"{}\"\n", escape_yaml(val)));
                }
            }
        }

        if !self.tags.is_empty() {
            yaml.push_str("tags:\n");
            for tag in &self.tags {
                yaml.push_str(&format!("  - \"{}\"\n", escape_yaml(tag)));
            }
        } else {
            yaml.push_str("tags:\n  - pdf\n  - notes\n");
        }
        yaml.push_str("---\n\n");
        yaml
    }
}

/// Escapes double quotes and backslashes in YAML string values.
fn escape_yaml(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "")
        .replace('\n', " ")
}

/// Computes the cryptographic SHA-256 digest of a file on disk.
pub fn compute_file_sha256(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Results of a document conversion or text extraction run.
#[derive(Debug, Default, Clone)]
pub struct PdfExtractionResult {
    pub text: String,
    pub images: Vec<PathBuf>,
    pub figures: Vec<CroppedFigure>,
}

/// Optional configuration for conversion runs.
#[derive(Debug, Default, Clone)]
pub struct ConversionConfig {
    pub temp_dir: Option<PathBuf>,
    pub output_images_dir: Option<PathBuf>,
}

/// Formats the final verbatim Markdown document from extraction results.
pub fn render_markdown(result: &PdfExtractionResult) -> String {
    let mut output = String::new();

    if !result.text.trim().is_empty() {
        output.push_str("# Extracted PDF text\n\n");
        output.push_str(result.text.trim());
        output.push_str("\n\n");
    }

    if !result.figures.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("## Figures\n\n");
        for fig in &result.figures {
            let caption_text = match &fig.caption {
                Some(cap) if !cap.trim().is_empty() => {
                    let trimmed = cap.trim();
                    let lower_label = fig.figure_label.to_lowercase();
                    let lower_cap = trimmed.to_lowercase();
                    if lower_cap.starts_with(&lower_label) {
                        trimmed.to_string()
                    } else {
                        format!("{}. {}", fig.figure_label, trimmed)
                    }
                }
                _ => fig.figure_label.clone(),
            };
            let dir_name = fig.images_dir_name();
            output.push_str(&format!(
                "![[{}/{}]]\n*{}*\n\n",
                dir_name, fig.filename, caption_text
            ));
        }
    } else if !result.images.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("## Embedded images\n\n");
        for (_index, path) in result.images.iter().enumerate() {
            let path_str = path.to_string_lossy().replace('\\', "/");
            output.push_str(&format!("![[{}]]\n\n", path_str));
        }
    }

    normalize_latex_delimiters(&output)
}

/// Injects detected figure image links into the appropriate locations of the page text.
pub fn attach_figures_to_page_text(page_text: &str, figures: &[CroppedFigure]) -> String {
    if figures.is_empty() {
        return page_text.to_string();
    }

    let mut result = page_text.to_string();

    for fig in figures {
        let dir_name = fig.images_dir_name();
        let image_link = format!("![[{dir_name}/{}]]", fig.filename);
        if result.contains(&image_link) || result.contains(&fig.filename) {
            continue;
        }

        let caption_text = match &fig.caption {
            Some(cap) if !cap.trim().is_empty() => {
                let trimmed = cap.trim();
                let lower_label = fig.figure_label.to_lowercase();
                let lower_cap = trimmed.to_lowercase();
                if lower_cap.starts_with(&lower_label) {
                    trimmed.to_string()
                } else {
                    format!("{}. {}", fig.figure_label, trimmed)
                }
            }
            _ => fig.figure_label.clone(),
        };

        let figure_block = format!("{image_link}\n*{caption_text}*\n\n");

        let lower_text = result.to_lowercase();
        let lower_label = fig.figure_label.to_lowercase();

        if let Some(pos) = lower_text.find(&lower_label) {
            let line_start = result[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            result.insert_str(line_start, &figure_block);
        } else {
            if !result.is_empty() && !result.ends_with("\n\n") {
                if result.ends_with('\n') {
                    result.push('\n');
                } else {
                    result.push_str("\n\n");
                }
            }
            result.push_str(&figure_block);
        }
    }

    result
}

/// Emits an Obsidian HTML page demarcation comment anchor for the given page number.
pub fn format_page_demarcation(page_num: usize) -> String {
    format!("<!-- Page {page_num} -->\n\n")
}

/// Normalizes LaTeX math delimiters from standard LaTeX (`\(...\)` and `\[...\]`)
/// to Obsidian-compatible Markdown delimiters (`$...$` for inline, `$$...$$` for block math).
/// Content inside fenced code blocks (``` ... ```) and inline code (`...`) is preserved untouched.
/// Fixes LaTeX formatting, delimiters, and syntax in Markdown content.
pub fn fix_latex_formatting(content: &str) -> String {
    normalize_latex_delimiters(content)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteFormatReport {
    pub codeblocks_unwrapped: usize,
    pub banners_purged: usize,
    pub latex_repaired: bool,
    pub frontmatter_added: bool,
    pub frontmatter_fields_backfilled: usize,
    pub figure_links_converted: usize,
}

impl NoteFormatReport {
    pub fn is_changed(&self) -> bool {
        self.codeblocks_unwrapped > 0
            || self.banners_purged > 0
            || self.latex_repaired
            || self.frontmatter_added
            || self.frontmatter_fields_backfilled > 0
            || self.figure_links_converted > 0
    }
}

const PRESERVED_CODEBLOCK_TAGS: &[&str] = &[
    "mermaid", "python", "py", "rust", "rs", "bash", "sh", "zsh", "c", "cpp", "c++", "csharp",
    "cs", "java", "javascript", "js", "typescript", "ts", "json", "yaml", "yml", "toml", "sql",
    "html", "css", "xml", "diff", "dockerfile", "go", "matlab", "fortran", "r", "julia",
];

/// Unwraps spurious codeblocks (such as ```markdown or generic ```) that contain Markdown elements,
/// tables, callouts, equations, or prose paragraphs, while preserving legitimate code and diagram syntax.
pub fn unwrap_spurious_codeblocks(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    let mut current_tag = String::new();
    let mut block_lines: Vec<&str> = Vec::new();
    let mut unwrapped_count = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if !in_code {
                in_code = true;
                current_tag = trimmed[3..].trim().to_lowercase();
                block_lines.clear();
            } else {
                in_code = false;
                let tag = current_tag.as_str();
                let is_protected = PRESERVED_CODEBLOCK_TAGS.contains(&tag);

                let is_spurious = if is_protected {
                    false
                } else if tag == "markdown" || tag == "md" || tag == "text" || tag == "txt" || tag.is_empty() {
                    let has_markdown_heading = block_lines.iter().any(|l| {
                        let t = l.trim();
                        t.starts_with("# ") || t.starts_with("## ") || t.starts_with("### ") || t.starts_with("#### ")
                    });
                    let has_table_pipe = block_lines.iter().any(|l| {
                        let t = l.trim();
                        t.starts_with('|') && t.ends_with('|') && t.len() >= 2
                    });
                    let has_callout = block_lines.iter().any(|l| l.trim().starts_with("> [!"));
                    let has_image = block_lines.iter().any(|l| {
                        (l.contains("![") && l.contains("](")) || (l.contains("![[") && l.contains("]]"))
                    });
                    let has_math_block = block_lines.iter().any(|l| l.trim().starts_with("$$") || l.contains(r"\begin{"));
                    let has_list_items = block_lines.iter().filter(|l| {
                        let t = l.trim();
                        t.starts_with("* ") || t.starts_with("- ")
                    }).count() >= 2;
                    let has_prose = block_lines.iter().any(|l| {
                        let t = l.trim();
                        t.len() > 60 && is_likely_prose_sentence(t)
                    });

                    has_markdown_heading || has_table_pipe || has_callout || has_image || has_math_block || has_list_items || has_prose
                } else {
                    false
                };

                if is_spurious {
                    unwrapped_count += 1;
                    for bl in &block_lines {
                        out.push_str(bl);
                        out.push('\n');
                    }
                } else {
                    out.push_str("```");
                    out.push_str(&current_tag);
                    out.push('\n');
                    for bl in &block_lines {
                        out.push_str(bl);
                        out.push('\n');
                    }
                    out.push_str("```\n");
                }
                block_lines.clear();
                current_tag.clear();
            }
        } else if in_code {
            block_lines.push(line);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if in_code {
        out.push_str("```");
        out.push_str(&current_tag);
        out.push('\n');
        for bl in &block_lines {
            out.push_str(bl);
            out.push('\n');
        }
    }

    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    (out, unwrapped_count)
}

/// Parses an existing YAML frontmatter block from a Markdown document.
/// Returns `(Option<DocumentMetadata>, body)` where body is the content after the closing `---`.
pub fn parse_existing_frontmatter(content: &str) -> (Option<DocumentMetadata>, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content);
    }

    let after_dashes = &trimmed[3..];
    if !after_dashes.starts_with('\n') && !after_dashes.starts_with("\r\n") {
        return (None, content);
    }

    if let Some(close_idx) = after_dashes.find("\n---") {
        let yaml_text = &after_dashes[..close_idx];
        let after_close = &after_dashes[close_idx + 4..];
        let body = after_close.trim_start_matches('\r').trim_start_matches('\n');

        let mut meta = DocumentMetadata::default();
        let mut in_tags = false;

        for line in yaml_text.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() {
                continue;
            }

            if line_trimmed == "tags:" {
                in_tags = true;
                continue;
            }

            if in_tags {
                if line_trimmed.starts_with("- ") {
                    let tag_val = line_trimmed[2..].trim().trim_matches('"').trim_matches('\'');
                    if !tag_val.is_empty() && !meta.tags.contains(&tag_val.to_string()) {
                        meta.tags.push(tag_val.to_string());
                    }
                    continue;
                } else if !line_trimmed.starts_with(' ') && !line_trimmed.starts_with('\t') {
                    in_tags = false;
                }
            }

            if let Some((key, val)) = line_trimmed.split_once(':') {
                let key_clean = key.trim().to_lowercase();
                let val_clean = val.trim().trim_matches('"').trim_matches('\'').trim().to_string();
                if val_clean.is_empty() {
                    continue;
                }

                match key_clean.as_str() {
                    "title" => meta.title = Some(val_clean),
                    "source" => meta.source = Some(val_clean),
                    "author" => meta.author = Some(val_clean),
                    "organization" => meta.organization = Some(val_clean),
                    "date" | "publication_date" => meta.publication_date = Some(val_clean),
                    "journal" => meta.journal = Some(val_clean),
                    "volume" => meta.volume = Some(val_clean),
                    "issue" => meta.issue = Some(val_clean),
                    "doi" => meta.doi = Some(val_clean),
                    "pdf_sha256" | "sha256" => meta.pdf_sha256 = Some(val_clean),
                    "keywords" => meta.keywords = Some(val_clean),
                    "subject" => meta.subject = Some(val_clean),
                    "total_pages" => meta.total_pages = val_clean.parse().ok(),
                    "pages_processed" => meta.pages_processed = Some(val_clean),
                    _ => {}
                }
            }
        }

        return (Some(meta), body);
    }

    (None, content)
}

/// Formats and repairs a Markdown note:
/// 1. Unwraps spurious code blocks enclosing Markdown structures.
/// 2. Purges legacy extraction banners and headers.
/// 3. Normalizes LaTeX math equations and table cells.
/// 4. Non-destructively synthesizes/backfills Obsidian YAML frontmatter with GLiNER and heuristics.
pub fn format_note_document(content: &str, file_hint: Option<&std::path::Path>) -> (String, NoteFormatReport) {
    let mut report = NoteFormatReport::default();

    let (metadata_opt, raw_body) = parse_existing_frontmatter(content);
    let had_frontmatter = metadata_opt.is_some();
    let mut metadata = metadata_opt.unwrap_or_default();

    // 1. Unwrap spurious codeblocks
    let (unwrapped_body, unwrapped_count) = unwrap_spurious_codeblocks(raw_body);
    report.codeblocks_unwrapped = unwrapped_count;

    // 2. Purge extraction banners & running headers
    let stripped_body = strip_running_headers_and_footers(&unwrapped_body);
    if stripped_body.len() < unwrapped_body.len() {
        report.banners_purged += 1;
    }

    // 3. Promote headings if unpromoted
    let promoted_body = promote_headings(&stripped_body);

    // 4. Normalize LaTeX delimiters and tables
    let normalized_body = normalize_latex_delimiters(&promoted_body);
    if normalized_body != promoted_body {
        report.latex_repaired = true;
    }

    // 5. Convert figure / image links to Obsidian wiki-links (![[<Figure location>]])
    let (converted_body, converted_links) = convert_figure_links_to_obsidian(&normalized_body);
    report.figure_links_converted = converted_links;

    // 6. Synthesize / Backfill frontmatter properties
    if metadata.title.is_none() || metadata.title.as_deref() == Some("Extracted PDF text") {
        for line in converted_body.lines().take(25) {
            let t = line.trim();
            if t.starts_with("# ") || t.starts_with("## ") {
                let clean = t.trim_start_matches('#').trim();
                if clean.len() > 3
                    && clean != "Extracted PDF text"
                    && !clean.to_uppercase().contains("TECHNICAL NOTES")
                    && !clean.to_uppercase().contains("JOURNAL OF")
                {
                    metadata.title = Some(clean.to_string());
                    break;
                }
            }
        }
        if metadata.title.is_none() {
            if let Some(hint) = file_hint {
                if let Some(stem) = hint.file_stem() {
                    metadata.title = Some(stem.to_string_lossy().to_string());
                }
            }
        }
    }

    if metadata.source.is_none() {
        if let Some(hint) = file_hint {
            if let Some(name) = hint.file_name() {
                metadata.source = Some(name.to_string_lossy().to_string());
            }
        }
    }

    let added_fields = crate::metadata::populate_metadata_from_text(raw_body, &mut metadata);
    enrich_document_metadata(&mut metadata, &converted_body);

    if !had_frontmatter {
        report.frontmatter_added = true;
    } else {
        report.frontmatter_fields_backfilled = added_fields;
    }

    let frontmatter_str = metadata.to_yaml_frontmatter();
    let final_body = converted_body.trim_start();
    let result = format!("{frontmatter_str}{final_body}");

    (result, report)
}

/// Converts standard Markdown image links `![alt](location)` to Obsidian wiki-links `![[location]]`.
///
/// Features:
/// - Replaces `![alt](path)` with `![[path]]`
/// - Handles angle-bracket URLs `![alt](<path>)` -> `![[path]]`
/// - Strips optional link titles `![alt](path "title")` -> `![[path]]`
/// - Normalizes backslashes to forward slashes and strips leading `./`
/// - Works on lines within callouts `> ![alt](path)` -> `> ![[path]]`
/// - Preserves fenced codeblocks (``` and ~~~)
/// - Preserves inline code spans (`...`)
/// - Leaves existing Obsidian embeds (`![[...]`) and non-image links (`[...]`) intact
pub fn convert_figure_links_to_obsidian(content: &str) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut in_fence = false;
    let mut fence_char = ' ';
    let mut fence_len = 0;
    let mut total_converted = 0;

    for (line_idx, line) in content.split('\n').enumerate() {
        if line_idx > 0 {
            out.push('\n');
        }

        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            let ch = trimmed_start.chars().next().unwrap();
            let count = trimmed_start.chars().take_while(|&c| c == ch).count();
            if !in_fence {
                in_fence = true;
                fence_char = ch;
                fence_len = count;
                out.push_str(line);
                continue;
            } else if ch == fence_char && count >= fence_len {
                in_fence = false;
                out.push_str(line);
                continue;
            }
        }

        if in_fence {
            out.push_str(line);
        } else {
            let converted_line = convert_figure_links_in_line(line, &mut total_converted);
            out.push_str(&converted_line);
        }
    }

    (out, total_converted)
}

fn convert_figure_links_in_line(line: &str, count: &mut usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        // 1. Preserve inline code spans: `...` or ``...``
        if ch == '`' {
            let mut backtick_len = 1;
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c == '`' {
                    chars.next();
                    backtick_len += 1;
                } else {
                    break;
                }
            }
            let fence = "`".repeat(backtick_len);
            out.push_str(&fence);

            let rest_start = chars.peek().map(|(idx, _)| *idx).unwrap_or(line.len());
            let rest = &line[rest_start..];
            if let Some(close_idx) = rest.find(&fence) {
                let code_content = &rest[..close_idx];
                out.push_str(code_content);
                out.push_str(&fence);
                let advance_to = rest_start + close_idx + backtick_len;
                while let Some(&(idx, _)) = chars.peek() {
                    if idx < advance_to {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            continue;
        }

        // 2. Detect and convert `![alt](url)`
        if ch == '!' {
            if let Some(&(_, next_c)) = chars.peek() {
                if next_c == '[' {
                    // Check if already an obsidian embed: `![[`
                    let after_bracket = &line[i + 2..];
                    if after_bracket.starts_with('[') {
                        out.push('!');
                        continue;
                    }

                    // Attempt to parse standard Markdown image link: `![alt](url)`
                    chars.next(); // Consume '['

                    let mut bracket_depth = 1;
                    let mut escaped = false;
                    let mut alt_end = None;

                    while let Some((alt_idx, alt_c)) = chars.next() {
                        if escaped {
                            escaped = false;
                            continue;
                        }
                        if alt_c == '\\' {
                            escaped = true;
                            continue;
                        }
                        if alt_c == '[' {
                            bracket_depth += 1;
                        } else if alt_c == ']' {
                            bracket_depth -= 1;
                            if bracket_depth == 0 {
                                alt_end = Some(alt_idx);
                                break;
                            }
                        }
                    }

                    if alt_end.is_some() {
                        // Check if immediate next character is '('
                        if let Some(&(_, paren_c)) = chars.peek() {
                            if paren_c == '(' {
                                chars.next(); // Consume '('
                                let mut paren_depth = 1;
                                let mut paren_escaped = false;
                                let paren_start = chars.peek().map(|(idx, _)| *idx).unwrap_or(line.len());
                                let mut paren_end = None;

                                while let Some((p_idx, p_c)) = chars.next() {
                                    if paren_escaped {
                                        paren_escaped = false;
                                        continue;
                                    }
                                    if p_c == '\\' {
                                        paren_escaped = true;
                                        continue;
                                    }
                                    if p_c == '(' {
                                        paren_depth += 1;
                                    } else if p_c == ')' {
                                        paren_depth -= 1;
                                        if paren_depth == 0 {
                                            paren_end = Some(p_idx);
                                            break;
                                        }
                                    }
                                }

                                if let Some(p_end) = paren_end {
                                    let raw_inner = &line[paren_start..p_end];
                                    let trimmed = raw_inner.trim();

                                    let location = if trimmed.starts_with('<') {
                                        if let Some(end_angle) = trimmed.find('>') {
                                            trimmed[1..end_angle].trim()
                                        } else {
                                            trimmed.trim_start_matches('<').trim()
                                        }
                                    } else {
                                        let mut parts = trimmed.split_whitespace();
                                        parts.next().unwrap_or(trimmed).trim_matches('"').trim_matches('\'')
                                    };

                                    if !location.is_empty() {
                                        let clean_location = location.replace('\\', "/");
                                        let final_location = clean_location.strip_prefix("./").unwrap_or(&clean_location);
                                        out.push_str("![[");
                                        out.push_str(final_location);
                                        out.push_str("]]");
                                        *count += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Fallback: parsing did not yield an image link, re-emit consumed text
                    let current_idx = chars.peek().map(|(idx, _)| *idx).unwrap_or(line.len());
                    out.push_str(&line[i..current_idx]);
                    continue;
                }
            }
        }

        out.push(ch);
    }

    out
}

/// Converts image links in Markdown documents pointing to `.png` files to `.jpg`.
///
/// Modifies both Obsidian wiki-links (`![[...png]]` / `![[...png|size]]`) and standard
/// Markdown links (`![alt](...png)`), while preserving code blocks and inline code spans.
pub fn replace_png_links_with_jpg(content: &str) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut in_fence = false;
    let mut fence_char = ' ';
    let mut fence_len = 0;
    let mut total_converted = 0;

    for (line_idx, line) in content.split('\n').enumerate() {
        if line_idx > 0 {
            out.push('\n');
        }

        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            let ch = trimmed_start.chars().next().unwrap();
            let count = trimmed_start.chars().take_while(|&c| c == ch).count();
            if !in_fence {
                in_fence = true;
                fence_char = ch;
                fence_len = count;
                out.push_str(line);
                continue;
            } else if ch == fence_char && count >= fence_len {
                in_fence = false;
                out.push_str(line);
                continue;
            }
        }

        if in_fence {
            out.push_str(line);
        } else {
            let converted_line = replace_png_links_in_line(line, &mut total_converted);
            out.push_str(&converted_line);
        }
    }

    (out, total_converted)
}

fn replace_png_links_in_line(line: &str, count: &mut usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        // 1. Preserve inline code spans: `...`
        if ch == '`' {
            let mut backtick_len = 1;
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c == '`' {
                    chars.next();
                    backtick_len += 1;
                } else {
                    break;
                }
            }
            let fence = "`".repeat(backtick_len);
            out.push_str(&fence);

            let rest_start = chars.peek().map(|(idx, _)| *idx).unwrap_or(line.len());
            let rest = &line[rest_start..];
            if let Some(close_idx) = rest.find(&fence) {
                let code_content = &rest[..close_idx];
                out.push_str(code_content);
                out.push_str(&fence);
                let advance_to = rest_start + close_idx + backtick_len;
                while let Some(&(idx, _)) = chars.peek() {
                    if idx < advance_to {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            continue;
        }

        // 2. Obsidian embed: ![[...]]
        if ch == '!' {
            let rest = &line[i..];
            if rest.starts_with("![[") {
                if let Some(close_pos) = rest.find("]]") {
                    let inner = &rest[3..close_pos];
                    let (target, extra) = match inner.split_once('|') {
                        Some((t, e)) => (t, Some(e)),
                        None => (inner, None),
                    };

                    let trimmed_target = target.trim();
                    if trimmed_target.to_ascii_lowercase().ends_with(".png") {
                        let new_target = format!("{}.jpg", &trimmed_target[..trimmed_target.len() - 4]);
                        out.push_str("![[");
                        out.push_str(&new_target);
                        if let Some(e) = extra {
                            out.push('|');
                            out.push_str(e);
                        }
                        out.push_str("]]");
                        *count += 1;

                        let advance_to = i + close_pos + 2;
                        while let Some(&(idx, _)) = chars.peek() {
                            if idx < advance_to {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                }
            }
        }

        // 3. Standard markdown: ![alt](url)
        if ch == '!' {
            let rest = &line[i..];
            if rest.starts_with("![") && !rest.starts_with("![[") {
                if let Some(bracket_end) = rest.find(']') {
                    let after_bracket = &rest[bracket_end + 1..];
                    if after_bracket.starts_with('(') {
                        if let Some(paren_end) = after_bracket.find(')') {
                            let alt = &rest[2..bracket_end];
                            let raw_url = &after_bracket[1..paren_end];
                            let trimmed_url = raw_url.trim();

                            let (url_part, suffix) = if trimmed_url.starts_with('<') {
                                if let Some(end_angle) = trimmed_url.find('>') {
                                    (&trimmed_url[1..end_angle], &trimmed_url[end_angle..])
                                } else {
                                    (trimmed_url, "")
                                }
                            } else if let Some(space_idx) = trimmed_url.find(|c: char| c.is_whitespace()) {
                                (&trimmed_url[..space_idx], &trimmed_url[space_idx..])
                            } else {
                                (trimmed_url, "")
                            };

                            if url_part.to_ascii_lowercase().ends_with(".png") {
                                let new_url = format!("{}.jpg", &url_part[..url_part.len() - 4]);
                                out.push_str("![");
                                out.push_str(alt);
                                out.push_str("](");
                                if trimmed_url.starts_with('<') {
                                    out.push('<');
                                    out.push_str(&new_url);
                                    out.push_str(suffix);
                                } else {
                                    out.push_str(&new_url);
                                    out.push_str(suffix);
                                }
                                out.push(')');
                                *count += 1;

                                let advance_to = i + bracket_end + 1 + paren_end + 1;
                                while let Some(&(idx, _)) = chars.peek() {
                                    if idx < advance_to {
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                }
                                continue;
                            }
                        }
                    }
                }
            }
        }

        out.push(ch);
    }

    out
}

pub fn normalize_latex_delimiters(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;
    let mut remaining = content;

    while !remaining.is_empty() {
        if in_code_block {
            if let Some(pos) = remaining.find("```") {
                let end = pos + 3;
                result.push_str(&remaining[..end]);
                remaining = &remaining[end..];
                in_code_block = false;
            } else {
                result.push_str(remaining);
                break;
            }
        } else {
            let next_code = remaining.find("```");
            let (text_to_process, rest) = match next_code {
                Some(pos) => (&remaining[..pos], &remaining[pos..]),
                None => (remaining, ""),
            };

            result.push_str(&convert_delimiters_outside_code_blocks(text_to_process));
            remaining = rest;
            if !remaining.is_empty() {
                in_code_block = true;
                result.push_str("```");
                remaining = &remaining[3..];
            }
        }
    }

    result
}

fn convert_delimiters_outside_code_blocks(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find('`') {
            let before = &remaining[..pos];
            result.push_str(&convert_math_spans(before));

            let after_tick = &remaining[pos + 1..];
            if let Some(close_pos) = after_tick.find('`') {
                let code_span = &remaining[pos..pos + 1 + close_pos + 1];
                result.push_str(code_span);
                remaining = &after_tick[close_pos + 1..];
            } else {
                // Unclosed backtick: push literal backtick and continue processing after_tick
                result.push('`');
                remaining = after_tick;
            }
        } else {
            result.push_str(&convert_math_spans(remaining));
            break;
        }
    }

    result
}

fn convert_math_spans(text: &str) -> String {
    let with_blocks = convert_block_math(text);
    let with_inline = convert_inline_math(&with_blocks);
    wrap_unfenced_math(&with_inline)
}

const LATEX_GREEK_AND_MATH: &[&str] = &[
    r"\alpha", r"\beta", r"\gamma", r"\delta", r"\epsilon", r"\varepsilon", r"\zeta", r"\eta",
    r"\theta", r"\vartheta", r"\iota", r"\kappa", r"\lambda", r"\mu", r"\nu", r"\xi",
    r"\pi", r"\varpi", r"\rho", r"\varrho", r"\sigma", r"\varsigma", r"\tau", r"\upsilon",
    r"\phi", r"\varphi", r"\chi", r"\psi", r"\omega",
    r"\Gamma", r"\Delta", r"\Theta", r"\Lambda", r"\Xi", r"\Pi", r"\Sigma", r"\Upsilon",
    r"\Phi", r"\Psi", r"\Omega",
    r"\frac", r"\dfrac", r"\tfrac", r"\sqrt", r"\sum", r"\prod", r"\int", r"\iint", r"\oint",
    r"\partial", r"\nabla", r"\infty", r"\times", r"\pm", r"\mp", r"\cdot", r"\leq", r"\geq",
    r"\neq", r"\approx", r"\equiv", r"\propto", r"\to", r"\implies",
    r"\dot", r"\ddot", r"\hat", r"\bar", r"\tilde", r"\vec", r"\left", r"\right", r"\exp", r"\ln", r"\log",
];

const PROSE_STOP_WORDS: &[&str] = &[
    "the", "is", "are", "was", "were", "with", "from", "that", "this", "these", "those",
    "have", "has", "had", "will", "would", "shall", "should", "must", "can", "could",
    "following", "equations", "calculated", "using", "when", "where", "which", "between",
    "typical", "coefficient", "friction", "interface", "requirements", "paragraph",
    "positive", "negative", "thermal", "maximum", "minimum", "preloads", "factor",
    "method", "experimental", "uncertainty", "avoided", "specific", "testing", "determine",
];

fn is_display_equation_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Don't wrap lines already in $$
    if trimmed.starts_with("$$") || trimmed.ends_with("$$") {
        return false;
    }
    // Skip markdown headers, callouts, and table pipes
    if trimmed.starts_with('#') || trimmed.starts_with('>') || trimmed.starts_with('|') {
        return false;
    }
    // Conjunction words between equations like "OR" or "AND" are not equations
    if trimmed.eq_ignore_ascii_case("or") || trimmed.eq_ignore_ascii_case("and") {
        return false;
    }

    let has_eq = trimmed.contains('=')
        || trimmed.contains(r"\approx")
        || trimmed.contains(r"\le")
        || trimmed.contains(r"\ge");
    let has_latex_math = LATEX_GREEK_AND_MATH.iter().any(|&cmd| trimmed.contains(cmd));

    let lower = trimmed.to_lowercase();
    let prose_word_count = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty() && PROSE_STOP_WORDS.contains(w))
        .count();

    if prose_word_count >= 2 {
        return false;
    }

    if has_eq {
        let math_chars = ['+', '-', '*', '/', '(', ')', '[', ']', '^', '_'];
        let has_math_ops = trimmed.chars().any(|c| math_chars.contains(&c));
        if (has_latex_math || has_math_ops) && prose_word_count <= 1 {
            return true;
        }
    }

    if has_latex_math
        && (trimmed.contains(r"\frac")
            || trimmed.contains(r"\sum")
            || trimmed.contains(r"\int")
            || trimmed.contains(r"\sqrt"))
        && prose_word_count == 0
    {
        return true;
    }

    false
}

/// Returns true if the text looks like an ordinary prose sentence rather than a mathematical formula.
pub fn is_likely_prose_sentence(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return false;
    }

    let prose_count = words
        .iter()
        .filter(|&&w| PROSE_STOP_WORDS.contains(&w))
        .count();

    // If text contains 2 or more stop words, or > 25% of words are standard prose words, reject math wrapping
    if prose_count >= 2 || (words.len() >= 4 && (prose_count as f32 / words.len() as f32) > 0.25) {
        return true;
    }

    // Common scientific prose words that often appear in false-positive formula boxes or table descriptions
    let sentence_markers = [
        "where", "expressing", "yields", "assuming", "equation", "equations", "gives", "shows",
        "defined", "denotes", "represents", "substituted", "substituting", "conservation",
        "balance", "results", "density", "velocity", "temperature", "pressure", "flux",
        "number", "constant", "ratio", "rate", "factor", "fraction", "diameter", "coefficient",
        "mass", "volume", "radius", "area", "energy", "enthalpy", "dimensionless",
    ];
    let has_sentence_marker = words.iter().any(|&w| sentence_markers.contains(&w));
    if has_sentence_marker {
        return true;
    }

    false
}

/// Returns true if a formula candidate represents an inline formula or short token rather than a display equation block.
pub fn is_inline_formula(text: &str, line_share_count: usize, box_h: u32, avg_h: f32) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    // If the formula shares a baseline with other text on the same line, it is inline
    if line_share_count > 0 {
        return true;
    }
    // If height is close to single-line text and expression is short without matrices or multi-line structures
    if (box_h as f32) <= 1.35 * avg_h
        && trimmed.len() <= 35
        && !trimmed.contains('\n')
        && !trimmed.contains(r"\begin")
        && !trimmed.contains(r"\frac")
        && !trimmed.contains(r"\sum")
        && !trimmed.contains(r"\int")
    {
        return true;
    }
    false
}

/// De-hyphenates words broken across line breaks (e.g. "combus-\ntor" -> "combustor").
/// Compound prefixes (e.g. "three-", "two-", "one-", "non-", "quasi-", "cross-", "self-") retain their hyphen ("three-dimensional").
pub fn dehyphenate_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '-' {
            let mut ahead = chars.clone();
            let mut newline_found = false;
            if ahead.peek() == Some(&'\r') {
                ahead.next();
            }
            if ahead.peek() == Some(&'\n') {
                ahead.next();
                newline_found = true;
            }

            if newline_found {
                if let Some(&next_c) = ahead.peek() {
                    if next_c.is_ascii_lowercase() {
                        // Check previous word in result
                        let prev_word: String = result
                            .chars()
                            .rev()
                            .take_while(|ch| ch.is_ascii_alphabetic())
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        let lower_prefix = prev_word.to_ascii_lowercase();
                        let compound_prefixes = [
                            "three", "two", "one", "non", "quasi", "cross", "self", "multi", "sub",
                            "half", "all", "well", "high", "low",
                        ];

                        if compound_prefixes.contains(&lower_prefix.as_str()) {
                            // Retain hyphen, skip only the newline
                            result.push('-');
                            if chars.peek() == Some(&'\r') {
                                chars.next();
                            }
                            if chars.peek() == Some(&'\n') {
                                chars.next();
                            }
                            continue;
                        } else if !prev_word.is_empty() {
                            // Merge split word (drop both hyphen and newline)
                            if chars.peek() == Some(&'\r') {
                                chars.next();
                            }
                            if chars.peek() == Some(&'\n') {
                                chars.next();
                            }
                            continue;
                        }
                    }
                }
            }
        }
        result.push(c);
    }
    result
}

/// Strips repetitive running headers and footers (e.g. "VOL. 1, NO. 1", "AIAA JOURNAL", "Page X of Y") from document body.
pub fn strip_running_headers_and_footers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_uppercase();
        let clean = upper.replace('$', "").replace('#', "").trim().to_string();

        let is_running_hdr = (clean.starts_with("VOL. ") && clean.contains("NO. "))
            || (clean.contains("J. PROPULSION") && (clean.contains("VOL.") || clean.contains("198") || clean.contains("199")))
            || (clean.starts_with("AIAA JOURNAL") && clean.len() < 40)
            || (clean.starts_with("DOWNLOADED BY ") || clean.starts_with("DOWNLOADED ON "))
            || (clean.starts_with("PAGE ") && clean.contains(" OF ") && clean.len() < 25)
            || clean == "EXTRACTED PDF TEXT"
            || ((clean.starts_with("MARCH") || clean.starts_with("JAN") || clean.starts_with("FEB") || clean.starts_with("APR") || clean.starts_with("MAY") || clean.starts_with("JUNE") || clean.starts_with("JULY") || clean.starts_with("AUG") || clean.starts_with("SEPT") || clean.starts_with("OCT") || clean.starts_with("NOV") || clean.starts_with("DEC"))
                && (clean.contains("TECHNICAL NOTES") || clean.contains("198") || clean.contains("199") || clean.contains("200"))
                && clean.len() < 80);

        if !is_running_hdr {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Promotes academic section headings and document titles to Markdown headings (# Title, ## Section).
pub fn promote_headings(text: &str) -> String {
    let standard_sections = [
        "ABSTRACT",
        "INTRODUCTION",
        "BACKGROUND",
        "THEORY",
        "THEORETICAL FORMULATION",
        "GOVERNING EQUATIONS",
        "NUMERICAL METHOD",
        "NUMERICAL METHODS",
        "METHODOLOGY",
        "METHODS",
        "EXPERIMENTAL SETUP",
        "EXPERIMENTAL APPARATUS",
        "RESULTS AND DISCUSSION",
        "RESULTS",
        "DISCUSSION",
        "CONCLUSIONS",
        "CONCLUSION",
        "NOMENCLATURE",
        "ACKNOWLEDGEMENTS",
        "ACKNOWLEDGMENT",
        "REFERENCES",
    ];

    let mut out = String::with_capacity(text.len() + 256);
    let mut in_code = false;
    let mut title_promoted = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            in_code = !in_code;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if in_code || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('>') || trimmed.starts_with('|') || trimmed.starts_with("<!--") {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let upper = trimmed.to_uppercase();

        // 1. Check for standard academic section names
        let is_section = standard_sections.iter().any(|&sec| upper == sec || upper == format!("{}:", sec));

        // 2. Check for numbered section headers: "I. INTRODUCTION", "1.0 INTRODUCTION", "1. Introduction"
        let is_numbered_section = (trimmed.len() < 60) && (
            (trimmed.starts_with("I. ") || trimmed.starts_with("II. ") || trimmed.starts_with("III. ")
                || trimmed.starts_with("IV. ") || trimmed.starts_with("V. ") || trimmed.starts_with("VI. ")
                || trimmed.starts_with("VII. ") || trimmed.starts_with("VIII. ") || trimmed.starts_with("IX. ")
                || trimmed.starts_with("X. "))
            || (trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                && (trimmed.contains(".0 ") || (trimmed.chars().nth(1) == Some('.') && trimmed.chars().nth(2) == Some(' '))))
        );

        if is_section || is_numbered_section {
            out.push_str(&format!("## {}\n", trimmed));
            continue;
        }

        // 3. Promote primary title on first page if not already promoted
        if !title_promoted && trimmed.len() > 10 && trimmed.len() < 120 && !trimmed.ends_with('.') && !upper.starts_with("VOL") {
            if !trimmed.contains("University") && !trimmed.contains("Company") && !trimmed.contains("Institute") && !trimmed.contains("Laboratory") && !trimmed.contains('@') {
                out.push_str(&format!("# {}\n", trimmed));
                title_promoted = true;
                continue;
            }
        }

        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Enriches document metadata post-extraction by analyzing the full transcribed document text.
/// Extracts missing titles, authors, and dynamically generates taxonomy tags.
pub fn enrich_document_metadata(metadata: &mut DocumentMetadata, full_doc_text: &str) {
    if full_doc_text.trim().is_empty() {
        return;
    }

    // 1. Recover Document Title if missing or generic
    if metadata.title.as_ref().map(|t| t.len() < 4 || t.chars().all(|c| c.is_ascii_digit()) || t.contains('_')).unwrap_or(true) {
        for line in full_doc_text.lines().take(40) {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
                let candidate = trimmed[2..].trim();
                if candidate.len() > 4 {
                    metadata.title = Some(candidate.to_string());
                    break;
                }
            }
        }
    }

    // 2. Recover Authors if missing
    if metadata.author.is_none() {
        let lines: Vec<&str> = full_doc_text.lines().take(30).map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("University") || line.contains("Company") || line.contains("Institute") || line.contains("Laboratory") {
                if i > 0 {
                    let prev = lines[i - 1];
                    let clean: String = prev.chars().filter(|c| c.is_alphabetic() || *c == ' ' || *c == ',' || *c == '&' || *c == '.').collect();
                    let trimmed = clean.trim();
                    if trimmed.len() > 5 && trimmed.len() < 80 && !trimmed.starts_with('#') {
                        metadata.author = Some(trimmed.to_string());
                        break;
                    }
                }
            }
        }
    }

    // 3. Generate Classification & Categorization Tags
    let mut tags = Vec::new();
    let upper = full_doc_text.to_uppercase();

    // A. Document Classification (Type)
    if upper.contains("JOURNAL OF") || upper.contains("AIAA") || upper.contains("TRANSACTIONS") || (upper.contains("ABSTRACT") && upper.contains("REFERENCES")) {
        tags.push("type/journal-article".to_string());
    } else if upper.contains("REPORT") || upper.contains("NASA CR") || upper.contains("TECHNICAL REPORT") {
        tags.push("type/technical-report".to_string());
    } else if upper.contains("CONFERENCE") || upper.contains("SYMPOSIUM") || upper.contains("PROCEEDINGS") || upper.contains("PAPER 8") {
        tags.push("type/conference-paper".to_string());
    } else if full_doc_text.contains("పత్రిక") || full_doc_text.contains("వార పత్రిక") || full_doc_text.contains("సంచిక") {
        tags.push("type/periodical-magazine".to_string());
    } else {
        tags.push("type/document".to_string());
    }

    // B. Domain Categorization
    let mut domains = Vec::new();
    if upper.contains("PROPULSION") || upper.contains("COMBUSTOR") || upper.contains("SLURRY") || upper.contains("NOZZLE") || upper.contains("SCRAMJET") || upper.contains("RAMJET") || upper.contains("ROCKET") {
        domains.push("domain/propulsion");
        domains.push("domain/aerospace");
    }
    if upper.contains("TRANSONIC") || upper.contains("SUPERSONIC") || upper.contains("FLOWFIELD") || upper.contains("AERODYNAMIC") || upper.contains("BOUNDARY-LAYER") || upper.contains("AIRFOIL") || upper.contains("MACH") {
        domains.push("domain/aerodynamics");
        domains.push("domain/fluid-dynamics");
    }
    if upper.contains("FACTORIZATION") || upper.contains("FINITE-DIFFERENCE") || upper.contains("MULTIGRID") || upper.contains("NAVIER-STOKES") || upper.contains("EULER") {
        domains.push("domain/computational-methods");
    }
    if upper.contains("COMBUSTION") || upper.contains("DROPLET") || upper.contains("FLAME") || upper.contains("IGNITION") {
        domains.push("domain/combustion");
    }
    if crate::backends::indra::vocab::has_telugu(full_doc_text) {
        domains.push("domain/telugu-literature");
        domains.push("domain/history");
        tags.push("lang/telugu".to_string());
    } else {
        tags.push("lang/english".to_string());
    }

    for d in domains {
        if !tags.contains(&d.to_string()) {
            tags.push(d.to_string());
        }
    }

    // C. Topical entities from title
    if let Some(ref title) = metadata.title {
        let lower_t = title.to_lowercase();
        for kw in &["scramjet", "combustor", "nacelle", "inlet", "slurry", "droplet", "supersonic", "transonic", "factorization", "turbulent", "nozzle"] {
            if lower_t.contains(kw) {
                let tag = format!("topic/{}", kw);
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
    }

    metadata.tags = tags;
}

fn wrap_inline_math(line: &str) -> String {
    if !line.contains('\\') && !line.contains('$') && !line.contains('_') {
        return line.to_string();
    }

    // Step 1: Repair any existing $...$ spans with broken LaTeX syntax
    let mut intermediate = String::with_capacity(line.len() + 16);
    let mut rem = line;
    let mut has_repaired_dollars = false;

    while let Some(start_d) = rem.find('$') {
        intermediate.push_str(&rem[..start_d]);
        let after_start = &rem[start_d + 1..];
        if let Some(end_d) = after_start.find('$') {
            let formula = &after_start[..end_d];
            if !formula.contains('\n') && !formula.trim().is_empty() {
                if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(formula) {
                    intermediate.push('$');
                    intermediate.push_str(&repaired);
                    intermediate.push('$');
                    has_repaired_dollars = true;
                    rem = &after_start[end_d + 1..];
                    continue;
                }
            }
            intermediate.push('$');
            intermediate.push_str(formula);
            intermediate.push('$');
            rem = &after_start[end_d + 1..];
        } else {
            // Stray unclosed dollar at end of line: check if it's an inline formula candidate
            let trailing = after_start.trim();
            if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(trailing) {
                intermediate.push('$');
                intermediate.push_str(&repaired);
                intermediate.push('$');
                has_repaired_dollars = true;
            } else {
                intermediate.push_str(after_start);
            }
            rem = "";
            break;
        }
    }
    intermediate.push_str(rem);

    let active_line = if has_repaired_dollars { intermediate } else { line.to_string() };

    if !active_line.contains('\\') && !active_line.contains('_') && !active_line.contains('^') {
        return active_line;
    }

    let mut result = String::with_capacity(active_line.len() + 16);
    let mut chars = active_line.char_indices().peekable();
    let mut inside_dollar = false;

    while let Some((idx, ch)) = chars.next() {
        if ch == '$' {
            inside_dollar = !inside_dollar;
            result.push(ch);
            continue;
        }

        if !inside_dollar {
            if ch == '\\' {
                let slice = &active_line[idx..];
                if let Some(&cmd) = LATEX_GREEK_AND_MATH.iter().find(|&&cmd| slice.starts_with(cmd)) {
                    let after_cmd_idx = idx + cmd.len();
                    let rem_str = &active_line[after_cmd_idx..];
                    let mut cur_offset = 0;

                    // Capture argument in braces e.g. \dot{m}
                    if rem_str.starts_with('{') {
                        if let Some(close_brace) = rem_str.find('}') {
                            cur_offset = close_brace + 1;
                        }
                    }

                    // Capture chained subscripts/superscripts e.g. \dot{m}_{nn} or \rho_{i0}
                    let rest_after_arg = &rem_str[cur_offset..];
                    if rest_after_arg.starts_with("_{") || rest_after_arg.starts_with("^{") {
                        if let Some(close_brace) = rest_after_arg.find('}') {
                            cur_offset += close_brace + 1;
                        }
                    } else if rest_after_arg.starts_with('_') || rest_after_arg.starts_with('^') {
                        let sub_rem = &rest_after_arg[1..];
                        if let Some(first_char) = sub_rem.chars().next() {
                            cur_offset += 1 + first_char.len_utf8();
                        }
                    }

                    let end_idx = after_cmd_idx + cur_offset;
                    let math_token = &active_line[idx..end_idx];

                    if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(math_token) {
                        result.push('$');
                        result.push_str(&repaired);
                        result.push('$');
                    } else {
                        result.push('$');
                        result.push_str(math_token);
                        result.push('$');
                    }

                    while let Some(&(next_idx, _)) = chars.peek() {
                        if next_idx < end_idx {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    continue;
                }
            } else if ch.is_ascii_alphabetic() {
                // Check for single variable with subscript/superscript e.g. T_{n}, p_{p}, R_i, R_i}$
                let prev_ok = if idx == 0 {
                    true
                } else {
                    let prev_char = active_line[..idx].chars().last().unwrap();
                    !prev_char.is_alphanumeric() && prev_char != '_' && prev_char != '^' && prev_char != '$'
                };

                if prev_ok {
                    let slice = &active_line[idx..];
                    let base_len = slice.chars().take_while(|c| c.is_ascii_alphabetic()).count();
                    if base_len > 0 && base_len <= 3 {
                        let after_base = &slice[base_len..];
                        if after_base.starts_with('_') || after_base.starts_with('^') {
                            let cur_offset = base_len;
                            let sub_slice = &slice[cur_offset..];
                            let mut captured = 0;

                            if sub_slice.starts_with("_{") || sub_slice.starts_with("^{") {
                                if let Some(cb) = sub_slice.find('}') {
                                    captured = cb + 1;
                                }
                            } else if sub_slice.starts_with('_') || sub_slice.starts_with('^') {
                                let sub_rem = &sub_slice[1..];
                                let sub_word_len = sub_rem.chars().take_while(|c| c.is_ascii_alphanumeric()).count();
                                if sub_word_len > 0 {
                                    captured = 1 + sub_word_len;
                                    let rem_after_sub = &sub_rem[sub_word_len..];
                                    if rem_after_sub.starts_with("}$") {
                                        captured += 2;
                                    } else if rem_after_sub.starts_with('}') {
                                        captured += 1;
                                    }
                                }
                            }

                            if captured > 0 {
                                let end_idx = idx + base_len + captured;
                                let next_ok = if end_idx >= active_line.len() {
                                    true
                                } else {
                                    let next_c = active_line[end_idx..].chars().next().unwrap();
                                    !next_c.is_alphanumeric() && next_c != '_' && next_c != '^'
                                };

                                if next_ok {
                                    let raw_token = &active_line[idx..end_idx];
                                    let clean_token = raw_token.replace('$', "");
                                    let token_to_repair = if clean_token.ends_with('}') && !clean_token.contains('{') {
                                        clean_token.trim_end_matches('}').to_string()
                                    } else {
                                        clean_token
                                    };

                                    if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(&token_to_repair) {
                                        result.push('$');
                                        result.push_str(&repaired);
                                        result.push('$');
                                    } else {
                                        result.push('$');
                                        result.push_str(&token_to_repair);
                                        result.push('$');
                                    }

                                    while let Some(&(next_idx, _)) = chars.peek() {
                                        if next_idx < end_idx {
                                            chars.next();
                                        } else {
                                            break;
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }

        result.push(ch);
    }

    result
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !is_table_row(trimmed) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner.chars().all(|c| c == '-' || c == ':' || c == '|' || c.is_whitespace())
}

fn split_table_row(row: &str) -> Vec<String> {
    let trimmed = row.trim();
    let inner = if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = inner.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == '|' {
                    current.push('|');
                    chars.next();
                    continue;
                }
            }
            current.push(c);
        } else if c == '|' {
            cells.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    cells.push(current.trim().to_string());
    cells
}

fn format_table_row(row: &str) -> String {
    if is_table_separator(row) {
        return row.to_string();
    }
    let cells = split_table_row(row);
    let mut out = String::with_capacity(row.len() + 16);
    out.push('|');
    for cell in cells {
        out.push(' ');
        out.push_str(&crate::figure::table::format_table_cell_with_latex(&cell));
        out.push_str(" |");
    }
    out
}

fn wrap_unfenced_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut in_display_math = false;
    let mut display_buf = String::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // An empty line NEVER stays in display math mode: close hanging display math immediately
        // so that subsequent paragraphs are never rendered as italicized math font.
        if trimmed.is_empty() {
            if in_display_math {
                let trimmed_buf = display_buf.trim();
                if !trimmed_buf.is_empty() {
                    if is_likely_prose_sentence(trimmed_buf) {
                        out.push_str(trimmed_buf);
                        out.push_str("\n\n");
                    } else if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(trimmed_buf) {
                        out.push_str("$$\n");
                        out.push_str(&repaired);
                        out.push_str("\n$$\n");
                    } else {
                        out.push_str(trimmed_buf);
                        out.push('\n');
                    }
                }
                display_buf.clear();
                in_display_math = false;
            }
            out.push('\n');
            continue;
        }

        if trimmed == "$$" {
            if in_display_math {
                let trimmed_buf = display_buf.trim();
                if !trimmed_buf.is_empty() {
                    if is_likely_prose_sentence(trimmed_buf) {
                        out.push_str(trimmed_buf);
                        out.push_str("\n\n");
                    } else if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(trimmed_buf) {
                        out.push_str("$$\n");
                        out.push_str(&repaired);
                        out.push_str("\n$$\n");
                    } else {
                        out.push_str(trimmed_buf);
                        out.push('\n');
                    }
                }
                display_buf.clear();
                in_display_math = false;
            } else {
                in_display_math = true;
                display_buf.clear();
            }
            continue;
        }

        // Self-contained or single-line display math: e.g. "$$formula$$" or "$$formula$$ (9)"
        if !in_display_math && trimmed.starts_with("$$") {
            let after_open = &trimmed[2..];
            if let Some(close_pos) = after_open.find("$$") {
                let inside_math = &after_open[..close_pos];
                let tag_suffix = after_open[close_pos + 2..].trim();
                let repaired = crate::figure::latex::validate_and_repair_latex(inside_math)
                    .unwrap_or_else(|| inside_math.to_string());

                if tag_suffix.is_empty() {
                    out.push_str(&format!("$${repaired}$$\n"));
                } else {
                    out.push_str(&format!("$${repaired}$$ {tag_suffix}\n"));
                }
                continue;
            } else {
                // Line starts with $$ but has no closing $$ on this line
                let candidate = &trimmed[2..];
                if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(candidate) {
                    out.push_str(&format!("$${repaired}$$\n"));
                    continue;
                } else {
                    in_display_math = true;
                    display_buf.clear();
                    display_buf.push_str(candidate);
                    continue;
                }
            }
        }

        if in_display_math {
            if trimmed.ends_with("$$") {
                let before_close = trimmed.trim_end_matches('$').trim();
                if !before_close.is_empty() {
                    if !display_buf.is_empty() {
                        display_buf.push(' ');
                    }
                    display_buf.push_str(before_close);
                }
                let trimmed_buf = display_buf.trim();
                let repaired = crate::figure::latex::validate_and_repair_latex(trimmed_buf)
                    .unwrap_or_else(|| trimmed_buf.to_string());
                out.push_str("$$\n");
                out.push_str(&repaired);
                out.push_str("\n$$\n");
                display_buf.clear();
                in_display_math = false;
            } else {
                if !display_buf.is_empty() {
                    display_buf.push('\n');
                }
                display_buf.push_str(line);
            }
            continue;
        }

        if is_table_row(line) {
            out.push_str(&format_table_row(line));
            out.push('\n');
            continue;
        }

        if is_display_equation_line(line) {
            let cleaned = trimmed.replace('$', "");
            if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(&cleaned) {
                if !out.ends_with("\n\n") && !out.is_empty() {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                }
                out.push_str("$$\n");
                out.push_str(&repaired);
                out.push_str("\n$$\n\n");
            } else {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(&wrap_inline_math(line));
            out.push('\n');
        }
    }

    if in_display_math {
        let trimmed_buf = display_buf.trim();
        if !trimmed_buf.is_empty() {
            if is_likely_prose_sentence(trimmed_buf) {
                out.push_str(trimmed_buf);
                out.push_str("\n\n");
            } else if let Some(repaired) = crate::figure::latex::validate_and_repair_latex(trimmed_buf) {
                out.push_str("$$\n");
                out.push_str(&repaired);
                out.push_str("\n$$\n");
            } else {
                out.push_str(trimmed_buf);
                out.push('\n');
            }
        }
    }

    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn convert_block_math(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(start) = remaining.find("\\[") {
        result.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        if let Some(end) = after_start.find("\\]") {
            let formula = &after_start[..end];
            if formula.contains('\n') {
                let trimmed = formula.trim_matches(|c| c == '\r' || c == '\n');
                result.push_str("$$\n");
                result.push_str(trimmed);
                result.push_str("\n$$");
            } else {
                let trimmed = formula.trim();
                result.push_str("$$");
                result.push_str(trimmed);
                result.push_str("$$");
            }
            remaining = &after_start[end + 2..];
        } else {
            result.push_str("\\[");
            remaining = after_start;
        }
    }
    result.push_str(remaining);
    result
}

fn convert_inline_math(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(start) = remaining.find("\\(") {
        result.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        if let Some(end) = after_start.find("\\)") {
            let formula = &after_start[..end];
            // Inline math shouldn't cross empty paragraph lines
            if !formula.contains("\n\n") {
                let trimmed = formula.trim();
                if !trimmed.is_empty() {
                    result.push('$');
                    result.push_str(trimmed);
                    result.push('$');
                } else {
                    result.push_str("\\(\\)");
                }
                remaining = &after_start[end + 2..];
                continue;
            }
        }
        result.push_str("\\(");
        remaining = after_start;
    }
    result.push_str(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_markdown_with_text_content() {
        let result = PdfExtractionResult {
            text: "Hello world\nThis is a test PDF\n".to_string(),
            images: vec![],
            figures: vec![],
        };

        let output = render_markdown(&result);
        assert!(output.contains("# Extracted PDF text"));
        assert!(output.contains("Hello world"));
        assert!(output.contains("This is a test PDF"));
    }

    #[test]
    fn renders_images_section_when_present() {
        let result = PdfExtractionResult {
            text: "Page one\n".to_string(),
            images: vec![PathBuf::from("assets/page-1.png")],
            figures: vec![],
        };

        let output = render_markdown(&result);
        assert!(output.contains("## Embedded images"));
        assert!(output.contains("page-1.png"));
    }

    #[test]
    fn renders_images_without_text() {
        let result = PdfExtractionResult {
            text: String::new(),
            images: vec![PathBuf::from("assets/page-1.png")],
            figures: vec![],
        };

        let output = render_markdown(&result);
        assert!(!output.contains("# Extracted PDF text"));
        assert!(output.contains("## Embedded images"));
        assert!(output.contains("page-1.png"));
    }

    #[test]
    fn renders_empty_result_without_sections() {
        let output = render_markdown(&PdfExtractionResult::default());
        assert!(output.is_empty());
    }

    #[test]
    fn renders_figures_with_exact_labels() {
        let result = PdfExtractionResult {
            text: "# Section\nSome text".to_string(),
            images: vec![],
            figures: vec![CroppedFigure {
                figure_label: "Figure VII-3".to_string(),
                caption: None,
                filename: "Figure-VII-3.png".to_string(),
                file_path: PathBuf::from("output/images/Figure-VII-3.png"),
                page_num: 1,
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                is_table: false,
            }],
        };

        let output = render_markdown(&result);
        assert!(output.contains("## Figures"));
        assert!(output.contains("![[images/Figure-VII-3.png]]"));
        assert!(output.contains("*Figure VII-3*"));
    }

    #[test]
    fn renders_figures_with_caption() {
        let result = PdfExtractionResult {
            text: "# Section\nSome text".to_string(),
            images: vec![],
            figures: vec![CroppedFigure {
                figure_label: "Figure 4.7".to_string(),
                caption: Some("Variation of species mole fractions with distance.".to_string()),
                filename: "Figure-4-7.png".to_string(),
                file_path: PathBuf::from("output/images/Figure-4-7.png"),
                page_num: 4,
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                is_table: false,
            }],
        };

        let output = render_markdown(&result);
        assert!(output.contains("## Figures"));
        assert!(output.contains("![[images/Figure-4-7.png]]"));
        assert!(
            output.contains("*Figure 4.7. Variation of species mole fractions with distance.*")
        );
    }

    #[test]
    fn test_attach_figures_to_page_text() {
        let figures = vec![CroppedFigure {
            figure_label: "Figure 4.7".to_string(),
            caption: Some("Variation of species mole fractions with distance.".to_string()),
            filename: "Figure-4-7.png".to_string(),
            file_path: PathBuf::from("output/images/Figure-4-7.png"),
            page_num: 4,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            is_table: false,
        }];

        // Case 1: Figure label is present in page text
        let page_text =
            "Introductory paragraph.\n\nFigure 4.7.—Variation of species.\n\nNext paragraph.";
        let attached = attach_figures_to_page_text(page_text, &figures);
        assert!(
            attached.contains(
                "![[images/Figure-4-7.png]]"
            )
        );

        // Case 2: Figure label is absent from page text
        let page_text_no_label = "Page with only text paragraphs.";
        let attached2 = attach_figures_to_page_text(page_text_no_label, &figures);
        assert!(attached2.contains("![[images/Figure-4-7.png]]"));
        assert!(
            attached2.contains("*Figure 4.7. Variation of species mole fractions with distance.*")
        );
    }

    #[test]
    fn test_figures_with_pdfname_images_folder() {
        let figures = vec![CroppedFigure {
            figure_label: "Figure 2.3".to_string(),
            caption: Some("Nozzle Geometry".to_string()),
            filename: "Figure-2-3.png".to_string(),
            file_path: PathBuf::from("output/propulsion_report_images/Figure-2-3.png"),
            page_num: 2,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            is_table: false,
        }];

        assert_eq!(figures[0].images_dir_name(), "propulsion_report_images");

        let page_text = "Refer to Figure 2.3 for contour details.";
        let attached = attach_figures_to_page_text(page_text, &figures);
        assert!(attached.contains("![[propulsion_report_images/Figure-2-3.png]]"));

        let result = PdfExtractionResult {
            text: String::new(),
            images: vec![],
            figures,
        };
        let rendered = render_markdown(&result);
        assert!(rendered.contains("![[propulsion_report_images/Figure-2-3.png]]"));
    }

    #[test]
    fn test_normalize_latex_delimiters() {
        // Inline math with extra whitespace trimmed
        let input =
            "Surface temperature of \\( T_S \\) rises to \\(T_1\\) where \\( u_g \\) is velocity.";
        let normalized = normalize_latex_delimiters(input);
        assert_eq!(
            normalized,
            "Surface temperature of $T_S$ rises to $T_1$ where $u_g$ is velocity."
        );

        // Multi-line block math
        let block_input = "Equation:\n\\[\n\\rho_p r = \\rho_g u_g\n\\]\nFollowed by text.";
        let normalized_block = normalize_latex_delimiters(block_input);
        assert_eq!(
            normalized_block,
            "Equation:\n$$\n\\rho_p r = \\rho_g u_g\n$$\nFollowed by text."
        );

        // Single-line block math
        let single_block = "\\[ E = mc^2 \\]";
        assert_eq!(normalize_latex_delimiters(single_block), "$$E = mc^2$$");

        // Code blocks are untouched
        let code_input = "```latex\n\\(x\\)\n\\[y\\]\n```\nAnd outside: \\(z\\)";
        assert_eq!(
            normalize_latex_delimiters(code_input),
            "```latex\n\\(x\\)\n\\[y\\]\n```\nAnd outside: $z$"
        );

        // Inline code spans are untouched
        let inline_code = "Use `\\(x\\)` function and \\(y\\) variable.";
        assert_eq!(
            normalize_latex_delimiters(inline_code),
            "Use `\\(x\\)` function and $y$ variable."
        );

        // Lone unclosed backtick does not duplicate content
        let unclosed = "Prefix before backtick `iplot' with math \\(z = 1\\).";
        assert_eq!(
            normalize_latex_delimiters(unclosed),
            "Prefix before backtick `iplot' with math $z = 1$."
        );

        // Auto-detection of unadorned equations and inline Greek math
        let doc_sample = r#"The maximum and minimum preloads must be calculated using the following equations:
PLDmax = (1 + \Gamma)Tmaw/[Rt (tana + u1 VP/coss) + Roub Vp] + Ptn p0s
PLDmin = (1 - \Gamma) (Tmin - Tp)/[Rt (tana + u P/cosp) + Ro ub P] + Ptn n9 - Pioss
OR
PLDmax = (1 + \Gamma)Tmav/KYPD + Pmn Pp0s

avoided if application specific testing is done to determine \mu{t, _tb} and K."#;

        let formatted = normalize_latex_delimiters(doc_sample);
        assert!(formatted.contains("$$\nPLDmax = (1 + \\Gamma)Tmaw/[Rt (tana + u1 VP/coss) + Roub Vp] + Ptn p0s\n$$"));
        assert!(formatted.contains("$$\nPLDmin = (1 - \\Gamma) (Tmin - Tp)/[Rt (tana + u P/cosp) + Ro ub P] + Ptn n9 - Pioss\n$$"));
        assert!(formatted.contains("$$\nPLDmax = (1 + \\Gamma)Tmav/KYPD + Pmn Pp0s\n$$"));
        assert!(formatted.contains("determine $\\mu{t, _tb}$ and K."));
        // "OR" and prose headers must not be wrapped inside a math block
        assert!(!formatted.contains("$$\nOR\n$$"));
        assert!(!formatted.contains("$$\nThe maximum"));
    }

    #[test]
    fn test_document_metadata_yaml_frontmatter() {
        let meta = DocumentMetadata {
            title: Some("Thermodynamic Analysis of Rocket Engines".to_string()),
            source: Some("propulsion/reports/engine_1.pdf".to_string()),
            author: Some("Dr. John Doe".to_string()),
            subject: Some("Liquid Rocket Propulsion".to_string()),
            keywords: Some("propulsion, thermodynamics".to_string()),
            creation_date: Some("2026-10-29".to_string()),
            total_pages: Some(42),
            pages_processed: Some("1-10".to_string()),
            organization: Some("Aerospace Labs".to_string()),
            doi: Some("10.1000/182".to_string()),
            ..Default::default()
        };

        let yaml = meta.to_yaml_frontmatter();
        assert!(yaml.starts_with("---\n"));
        assert!(yaml.ends_with("---\n\n"));
        assert!(yaml.contains("title: \"Thermodynamic Analysis of Rocket Engines\"\n"));
        assert!(yaml.contains("source: \"propulsion/reports/engine_1.pdf\"\n"));
        assert!(yaml.contains("author: \"Dr. John Doe\"\n"));
        assert!(yaml.contains("created: \"2026-10-29\"\n"));
        assert!(yaml.contains("total_pages: 42\n"));
        assert!(yaml.contains("pages_processed: \"1-10\"\n"));
        assert!(yaml.contains("tags:\n  - pdf\n  - notes\n"));
    }

    #[test]
    fn test_dehyphenate_text() {
        let input = "The combus-\ntor performance in this ex-\nperiment demonstrated three-\ndimensional flow.";
        let res = dehyphenate_text(input);
        assert_eq!(
            res,
            "The combustor performance in this experiment demonstrated three-dimensional flow."
        );
    }

    #[test]
    fn test_strip_running_headers() {
        let input = "58 J. PROPULSION VOL. 1, NO. 1\n\nIntroduction\nAccurate flowfield predictions.\n\nDownloaded by Lockheed on Jan 1 1985\n\nNext paragraph.";
        let res = strip_running_headers_and_footers(input);
        assert!(!res.contains("58 J. PROPULSION"));
        assert!(!res.contains("Downloaded by"));
        assert!(res.contains("Introduction"));
        assert!(res.contains("Accurate flowfield"));
    }

    #[test]
    fn test_promote_headings() {
        let input = "Transonic Inlet Flowfield Computations\n\nJoseph Vadyak\n\nAbstract\nA computational method.\n\nIntroduction\nPredictions are required.\n\n1.0 Governing Equations\nThe equations are solved.";
        let res = promote_headings(input);
        assert!(res.contains("# Transonic Inlet Flowfield Computations\n"));
        assert!(res.contains("## Abstract\n"));
        assert!(res.contains("## Introduction\n"));
        assert!(res.contains("## 1.0 Governing Equations\n"));
    }

    #[test]
    fn test_math_gating_and_prose_rejection() {
        // Stop words in prose should be rejected from math wrapping
        let prose_candidate = "Expressing the particle mass in Eq. (2) in terms of the particle density and volume";
        assert!(is_likely_prose_sentence(prose_candidate));

        // Legitimate formula should not be rejected
        let formula_candidate = r"\rho_p r = \rho_g u_g";
        assert!(!is_likely_prose_sentence(formula_candidate));

        // Short inline variable sharing a line
        assert!(is_inline_formula(r"m = \dot{m}", 1, 20, 24.0));
        // Tall display equation block
        assert!(!is_inline_formula(r"\frac{\partial \rho}{\partial t} + \nabla \cdot (\rho \mathbf{u}) = 0", 0, 60, 24.0));
    }

    #[test]
    fn test_enrich_document_metadata_with_tags() {
        let mut meta = DocumentMetadata {
            title: Some("1".to_string()),
            ..Default::default()
        };
        let text = "# Supersonic Scramjet Propulsion and Combustion Analysis\n\nBy Dr. Jane Doe\nUniversity of Michigan\n\n## Abstract\nExperimental investigation of scramjet propulsion.\n\n## References\n1. Chen et al.";
        enrich_document_metadata(&mut meta, text);

        assert_eq!(meta.title.as_deref(), Some("Supersonic Scramjet Propulsion and Combustion Analysis"));
        assert_eq!(meta.author.as_deref(), Some("By Dr. Jane Doe"));
        assert!(meta.tags.contains(&"type/journal-article".to_string()));
        assert!(meta.tags.contains(&"domain/propulsion".to_string()));
        assert!(meta.tags.contains(&"domain/aerospace".to_string()));
        assert!(meta.tags.contains(&"domain/combustion".to_string()));
        assert!(meta.tags.contains(&"topic/scramjet".to_string()));
    }

    #[test]
    fn test_fix_latex_formatting_multi_line_display_math() {
        let input = "Intro\n\n$$\nq=4\\pi{m_{t}e\\epsilon{\\sigma_r}{(T_{n^{}-T{}}{}\\left(m_{j}V{n})$ $ (9)\n$$\n\nProse text after equation.";
        let res = fix_latex_formatting(input);
        assert!(res.contains("Intro\n\n$$\n"));
        assert!(res.contains(r"\tag{9}"));
        assert!(res.contains(r"\right)"));
        assert!(res.contains("$$\n\nProse text after equation."));
        assert!(!res.contains("$$\nProse text"));
    }

    #[test]
    fn test_fix_latex_formatting_table_math() {
        let input = "| Variable | Description |\n| :--- | :--- |\n| Re | = Reynolds number |\n| Nu | = Nusselt number |\n| \\dot{m} | = mass flow rate |\n| V_{p0}/V_{ref} | 1.0 |";
        let res = fix_latex_formatting(input);
        assert!(res.contains("| $Re$ | = Reynolds number |"));
        assert!(res.contains("| $Nu$ | = Nusselt number |"));
        assert!(res.contains(r"| $\dot{m}$ | = mass flow rate |"));
        assert!(res.contains(r"| $V_{p0}/V_{ref}$ | 1.0 |"));
        assert!(res.contains("| :--- | :--- |"));
    }

    #[test]
    fn test_unwrap_spurious_codeblocks() {
        let input = r#"Normal text.

```markdown
| Col 1 | Col 2 |
| :--- | :--- |
| Val 1 | Val 2 |
```

```mermaid
graph TD;
    A-->B;
```

```python
def foo():
    return 42
```

```
# Section In Untagged Fence

This is an unadorned prose paragraph that was trapped inside backticks.
```
"#;
        let (unwrapped, count) = unwrap_spurious_codeblocks(input);
        assert_eq!(count, 2);
        // Table should be unwrapped
        assert!(unwrapped.contains("| Col 1 | Col 2 |"));
        assert!(!unwrapped.contains("```markdown\n| Col 1"));
        // Mermaid should be preserved
        assert!(unwrapped.contains("```mermaid\ngraph TD;\n    A-->B;\n```"));
        // Python should be preserved
        assert!(unwrapped.contains("```python\ndef foo():\n    return 42\n```"));
        // Section in untagged fence should be unwrapped
        assert!(unwrapped.contains("# Section In Untagged Fence\n\nThis is an unadorned prose paragraph"));
    }

    #[test]
    fn test_parse_existing_frontmatter() {
        let input = r#"---
title: "Existing Title"
author: "Existing Author"
date: "1985"
tags:
  - type/journal-article
---

# Body Content
Hello world.
"#;
        let (meta_opt, body) = parse_existing_frontmatter(input);
        assert!(meta_opt.is_some());
        let meta = meta_opt.unwrap();
        assert_eq!(meta.title.as_deref(), Some("Existing Title"));
        assert_eq!(meta.author.as_deref(), Some("Existing Author"));
        assert_eq!(meta.publication_date.as_deref(), Some("1985"));
        assert!(meta.tags.contains(&"type/journal-article".to_string()));
        assert_eq!(body, "# Body Content\nHello world.\n");
    }

    #[test]
    fn test_format_note_document_end_to_end() {
        let input = r#"# Extracted PDF text

# MARCH-APRIL 1985 TECHNICAL NOTES

## Scramjet Combustor Analysis

**G. A. Sullins and P. J. Waltrupt**
The Johns Hopkins University Applied Physics Laboratory, Laurel, Maryland

```markdown
| Fuel | Air capture ratio |
| :--- | :--- |
| RJ-5 | \( A_0/A_i = 0.65 \) |
```

Under these conditions, \( M = 1 \) is reached.
"#;
        let hint = std::path::Path::new("Scramjet Combustor Analysis.md");
        let (output, report) = format_note_document(input, Some(hint));

        assert!(report.is_changed());
        assert_eq!(report.codeblocks_unwrapped, 1);
        assert!(report.frontmatter_added);

        // Banners must be purged
        assert!(!output.contains("# Extracted PDF text"));
        assert!(!output.contains("# MARCH-APRIL 1985 TECHNICAL NOTES"));

        // Table must be unwrapped and math normalized
        assert!(!output.contains("```markdown"));
        assert!(output.contains("| Fuel | Air capture ratio |"));
        assert!(output.contains("$A_0/A_i = 0.65$"));
        assert!(output.contains("$M = 1$"));

        // Frontmatter must be synthesized with properties
        assert!(output.starts_with("---\n"));
        assert!(output.contains("title: \"Scramjet Combustor Analysis\"\n"));
        assert!(output.contains("author: \"G. A. Sullins and P. J. Waltrupt\"\n"));
        assert!(output.contains("date: \"MARCH-APRIL 1985\"\n"));
        assert!(output.contains("tags:"));
    }

    #[test]
    fn test_convert_figure_links_to_obsidian_basic() {
        let input = "Here is ![Figure 1](images/Figure-1.png)\n*Figure 1*";
        let (output, count) = convert_figure_links_to_obsidian(input);
        assert_eq!(count, 1);
        assert_eq!(output, "Here is ![[images/Figure-1.png]]\n*Figure 1*");
    }

    #[test]
    fn test_convert_figure_links_to_obsidian_varieties() {
        let input = r#"
![Fig 2](Approximate_factorization_algori_images/page_2_fig_2.png)
*Fig 2. Computational domain.*

![](images/empty_alt.png)
![With Title](images/with_title.png "Figure title")
![Angle Brackets](<images/path with spaces/fig.png>)
![Backslashes](images\subfolder\fig.png)
![Dot Slash](./images/dot_slash.png)
> [!table]- Original Crop: Table 1
> ![Table 1](images/Table-1.png)
> *Table 1*
"#;
        let (output, count) = convert_figure_links_to_obsidian(input);
        assert_eq!(count, 7);
        assert!(output.contains("![[Approximate_factorization_algori_images/page_2_fig_2.png]]"));
        assert!(output.contains("![[images/empty_alt.png]]"));
        assert!(output.contains("![[images/with_title.png]]"));
        assert!(output.contains("![[images/path with spaces/fig.png]]"));
        assert!(output.contains("![[images/subfolder/fig.png]]"));
        assert!(output.contains("![[images/dot_slash.png]]"));
        assert!(output.contains("> ![[images/Table-1.png]]"));
    }

    #[test]
    fn test_convert_figure_links_to_obsidian_preserves_code_and_existing() {
        let input = r#"
Already obsidian: ![[images/already.png]]
Regular link: [Click Here](https://example.com)
Inline code: `![Ignore](images/code.png)`

```markdown
![Do Not Touch](images/codeblock.png)
```

~~~
![Tilde Fence](images/tilde.png)
~~~
"#;
        let (output, count) = convert_figure_links_to_obsidian(input);
        assert_eq!(count, 0);
        assert_eq!(output, input);
    }

    #[test]
    fn test_format_note_document_converts_figure_links() {
        let input = r#"# Extracted PDF text

## Propulsion Overview

Refer to the system schematic below.

![Figure 2.1](propulsion_images/page_2_fig_1.png)
*Figure 2.1. Schematic of the hypersonic inlet.*
"#;
        let (output, report) = format_note_document(input, None);
        assert!(report.is_changed());
        assert_eq!(report.figure_links_converted, 1);
        assert!(output.contains("![[propulsion_images/page_2_fig_1.png]]"));
        assert!(!output.contains("![Figure 2.1](propulsion_images/page_2_fig_1.png)"));
    }

    #[test]
    fn test_replace_png_links_with_jpg() {
        let input = r#"
![[propulsion_report_images/page_2_fig_1.png]]
![[page_1_fig_2.png|400]]
![[uppercase.PNG]]
![Figure 1](images/page_1.png)
![Figure 2](<images/page_2.png>)
![Figure 3](images/page_3.png "Title")
> ![[quote/page_4.png]]
`![[code_span.png]]`
```markdown
![[fenced_block.png]]
```
"#;
        let (output, count) = replace_png_links_with_jpg(input);
        assert_eq!(count, 7);
        assert!(output.contains("![[propulsion_report_images/page_2_fig_1.jpg]]"));
        assert!(output.contains("![[page_1_fig_2.jpg|400]]"));
        assert!(output.contains("![[uppercase.jpg]]"));
        assert!(output.contains("![Figure 1](images/page_1.jpg)"));
        assert!(output.contains("![Figure 2](<images/page_2.jpg>)"));
        assert!(output.contains("![Figure 3](images/page_3.jpg \"Title\")"));
        assert!(output.contains("> ![[quote/page_4.jpg]]"));
        // Code spans and fenced blocks preserved
        assert!(output.contains("`![[code_span.png]]`"));
        assert!(output.contains("![[fenced_block.png]]"));
    }

    #[test]
    fn test_compute_file_sha256_and_frontmatter() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dummy_path = temp_dir.path().join("dummy.pdf");
        std::fs::write(&dummy_path, b"hello world pdf content").unwrap();

        let sha = compute_file_sha256(&dummy_path).unwrap();
        assert_eq!(sha.len(), 64);

        let mut meta = DocumentMetadata::default();
        meta.title = Some("Test Title".to_string());
        meta.pdf_sha256 = Some(sha.clone());

        let yaml = meta.to_yaml_frontmatter();
        assert!(yaml.contains(&format!("pdf_sha256: \"{sha}\"")));

        let (parsed_meta, _) = parse_existing_frontmatter(&yaml);
        assert_eq!(parsed_meta.unwrap().pdf_sha256.as_deref(), Some(sha.as_str()));
    }
}
