use crate::figure::CroppedFigure;
use std::path::PathBuf;

/// Document properties and frontmatter metadata for Obsidian note reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub source: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creation_date: Option<String>,
    pub total_pages: Option<usize>,
    pub pages_processed: Option<String>,
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
        if let Some(ref subject) = self.subject {
            yaml.push_str(&format!("subject: \"{}\"\n", escape_yaml(subject)));
        }
        if let Some(ref keywords) = self.keywords {
            yaml.push_str(&format!("keywords: \"{}\"\n", escape_yaml(keywords)));
        }
        if let Some(ref date) = self.creation_date {
            yaml.push_str(&format!("created: \"{}\"\n", escape_yaml(date)));
        }
        if let Some(total) = self.total_pages {
            yaml.push_str(&format!("total_pages: {}\n", total));
        }
        if let Some(ref pages) = self.pages_processed {
            yaml.push_str(&format!("pages_processed: \"{}\"\n", pages));
        }

        yaml.push_str("tags:\n  - pdf\n  - notes\n");
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
}
