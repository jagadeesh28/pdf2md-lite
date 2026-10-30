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
                "![{}]({}/{})\n*{}*\n\n",
                fig.figure_label, dir_name, fig.filename, caption_text
            ));
        }
    } else if !result.images.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("## Embedded images\n\n");
        for (index, path) in result.images.iter().enumerate() {
            let path_str = path.to_string_lossy().replace('\\', "/");
            output.push_str(&format!("![image-{}]({})\n\n", index + 1, path_str));
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
        let image_link = format!("![{}]({dir_name}/{})", fig.figure_label, fig.filename);
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

    // Common scientific prose words that often appear in false-positive formula boxes
    let sentence_markers = [
        "where", "expressing", "yields", "assuming", "equation", "equations", "gives", "shows",
        "defined", "denotes", "represents", "substituted", "substituting", "conservation",
        "balance", "results", "density", "velocity", "temperature", "pressure", "flux",
    ];
    let has_sentence_marker = words.iter().any(|&w| sentence_markers.contains(&w));
    if has_sentence_marker && words.len() >= 3 {
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

        let is_running_hdr = (upper.starts_with("VOL. ") && upper.contains("NO. "))
            || (upper.contains("J. PROPULSION") && (upper.contains("VOL.") || upper.contains("198") || upper.contains("199")))
            || (upper.starts_with("AIAA JOURNAL") && upper.len() < 40)
            || (upper.starts_with("DOWNLOADED BY ") || upper.starts_with("DOWNLOADED ON "))
            || (upper.starts_with("PAGE ") && upper.contains(" OF ") && upper.len() < 25);

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
    if !line.contains('\\') {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len() + 16);
    let mut chars = line.char_indices().peekable();
    let mut inside_dollar = false;

    while let Some((idx, ch)) = chars.next() {
        if ch == '$' {
            inside_dollar = !inside_dollar;
            result.push(ch);
            continue;
        }

        if !inside_dollar && ch == '\\' {
            let slice = &line[idx..];
            if let Some(&cmd) = LATEX_GREEK_AND_MATH.iter().find(|&&cmd| slice.starts_with(cmd)) {
                let after_cmd_idx = idx + cmd.len();
                let mut end_idx = after_cmd_idx;

                let rem = &line[after_cmd_idx..];
                if rem.starts_with('{') {
                    if let Some(close_brace) = rem.find('}') {
                        end_idx = after_cmd_idx + close_brace + 1;
                    }
                } else if rem.starts_with("_{") {
                    if let Some(close_brace) = rem.find('}') {
                        end_idx = after_cmd_idx + close_brace + 1;
                    }
                } else if rem.starts_with('_') || rem.starts_with('^') {
                    let sub_rem = &rem[1..];
                    if let Some(first_char) = sub_rem.chars().next() {
                        end_idx = after_cmd_idx + 1 + first_char.len_utf8();
                    }
                }

                let math_token = &line[idx..end_idx];
                result.push('$');
                result.push_str(math_token);
                result.push('$');

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

        result.push(ch);
    }

    result
}

fn wrap_unfenced_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut in_display_math = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("$$") {
            in_display_math = !in_display_math || !trimmed.ends_with("$$") || trimmed == "$$";
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if in_display_math {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if is_display_equation_line(line) {
            let cleaned = trimmed.replace('$', "");
            if !out.ends_with("\n\n") && !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str("$$\n");
            out.push_str(&cleaned);
            out.push_str("\n$$\n\n");
        } else {
            out.push_str(&wrap_inline_math(line));
            out.push('\n');
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
        assert!(output.contains("![Figure VII-3](images/Figure-VII-3.png)"));
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
        assert!(output.contains("![Figure 4.7](images/Figure-4-7.png)"));
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
                "![Figure 4.7](images/Figure-4-7.png)"
            )
        );

        // Case 2: Figure label is absent from page text
        let page_text_no_label = "Page with only text paragraphs.";
        let attached2 = attach_figures_to_page_text(page_text_no_label, &figures);
        assert!(attached2.contains("![Figure 4.7](images/Figure-4-7.png)"));
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
        assert!(attached.contains("![Figure 2.3](propulsion_report_images/Figure-2-3.png)"));

        let result = PdfExtractionResult {
            text: String::new(),
            images: vec![],
            figures,
        };
        let rendered = render_markdown(&result);
        assert!(rendered.contains("![Figure 2.3](propulsion_report_images/Figure-2-3.png)"));
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
}
