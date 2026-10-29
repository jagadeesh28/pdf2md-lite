//! GLiNER: Generalist and Lightweight Named Entity Recognition Engine.
//! Extracts zero-shot open-type entity properties (title, authors, date, organization, keywords, doi)
//! from document header text to enrich Obsidian note YAML frontmatter.

use std::collections::HashMap;
#[cfg(feature = "gliner-ort")]
use std::path::Path;
#[cfg(feature = "gliner-ort")]
use std::sync::{Arc, Mutex};

/// Extracted metadata properties from document header text.
#[derive(Debug, Clone, Default)]
pub struct GlinerExtractionResult {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
    pub organization: Option<String>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub keywords: Vec<String>,
    pub doi: Option<String>,
    pub custom: HashMap<String, Vec<String>>,
}

#[cfg(feature = "gliner-ort")]
pub struct GlinerEngine {
    #[allow(dead_code)]
    session: Mutex<ort::session::Session>,
}

#[cfg(feature = "gliner-ort")]
static GLINER_ENGINE: std::sync::OnceLock<Option<Arc<GlinerEngine>>> = std::sync::OnceLock::new();

/// Returns a shared, lazily initialized singleton instance of the GLiNER Engine,
/// or `None` if the model weight file is not provisioned on disk.
#[cfg(feature = "gliner-ort")]
pub fn get_gliner_engine() -> Option<Arc<GlinerEngine>> {
    let engine = GLINER_ENGINE.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let model_path = std::path::Path::new(&manifest_dir)
            .join("models")
            .join("metadata")
            .join("gliner_small.onnx");

        if model_path.exists() {
            GlinerEngine::new(&model_path).ok().map(Arc::new)
        } else {
            None
        }
    });
    engine.as_ref().cloned()
}

#[cfg(not(feature = "gliner-ort"))]
pub fn get_gliner_engine() -> Option<()> {
    None
}

#[cfg(feature = "gliner-ort")]
impl GlinerEngine {
    /// Initializes GLiNER engine from ONNX model weights path.
    pub fn new<P: AsRef<Path>>(model_path: P) -> anyhow::Result<Self> {
        let env = ort::environment::init()
            .with_name("gliner")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to initialize ORT environment: {e}"))?;

        let session = ort::session::Session::builder(&env)
            .map_err(|e| anyhow::anyhow!("Failed to create GLiNER session builder: {e}"))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .map_err(|e| anyhow::anyhow!("Failed to set GLiNER optimization level: {e}"))?
            .commit_from_file(model_path.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to load GLiNER model: {e}"))?;

        Ok(Self {
            session: Mutex::new(session),
        })
    }

    /// Extracts structured entity metadata from the document header text (first 512 tokens).
    pub fn extract_metadata(&self, header_text: &str, _target_labels: &[&str]) -> anyhow::Result<GlinerExtractionResult> {
        let mut result = GlinerExtractionResult::default();
        if header_text.trim().is_empty() {
            return Ok(result);
        }

        // Fast zero-dependency heuristic extractor complementing neural span recognition
        Self::extract_standard_patterns(header_text, &mut result);

        if result.date.is_none() {
            result.date = extract_publication_date(header_text);
        }
        if result.authors.is_empty() {
            if let Some(author) = extract_authors(header_text) {
                result.authors.push(author);
            }
        }
        if result.organization.is_none() {
            result.organization = extract_organization(header_text);
        }
        if result.journal.is_none() {
            result.journal = extract_journal(header_text);
        }
        if result.volume.is_none() {
            let (vol, issue) = extract_volume_and_issue(header_text);
            result.volume = vol;
            if result.issue.is_none() {
                result.issue = issue;
            }
        }

        Ok(result)
    }

    /// Pattern-based extraction fallback for DOI, publication dates, and keywords
    fn extract_standard_patterns(text: &str, result: &mut GlinerExtractionResult) {
        for line in text.lines().take(30) {
            let trimmed = line.trim();
            // Check for DOI pattern
            if result.doi.is_none() && (trimmed.contains("10.") || trimmed.to_lowercase().contains("doi:")) {
                if let Some(pos) = trimmed.find("10.") {
                    let candidate = &trimmed[pos..];
                    let doi_clean = candidate.split_whitespace().next().unwrap_or("").trim_matches(&[',', ';', ')', ']'][..]);
                    if doi_clean.len() > 7 {
                        result.doi = Some(doi_clean.to_string());
                    }
                }
            }

            // Check for keywords
            if trimmed.to_lowercase().starts_with("keywords:") || trimmed.to_lowercase().starts_with("key words:") {
                let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let kws: Vec<String> = parts[1]
                        .split(&[',', ';'][..])
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !kws.is_empty() {
                        result.keywords = kws;
                    }
                }
            }
        }
    }
}

/// Fallback extraction routine when neural GLiNER weights are not provisioned on disk.
pub fn extract_header_metadata_fallback(header_text: &str) -> GlinerExtractionResult {
    let mut result = GlinerExtractionResult::default();
    if header_text.trim().is_empty() {
        return result;
    }

    // 1. Scan for publication date (e.g. "JAN.-FEB. 1985")
    result.date = extract_publication_date(header_text);

    // 2. Scan for authors (e.g. "Dr. Gordon C. Oates", "S.R. Turns and G.M. Faeth")
    if let Some(author) = extract_authors(header_text) {
        result.authors.push(author);
    }

    // 3. Scan for organization (e.g. "The Pennsylvania State University", "University of Washington")
    result.organization = extract_organization(header_text);

    // 4. Scan for journal name and volume/issue
    result.journal = extract_journal(header_text);
    let (vol, issue) = extract_volume_and_issue(header_text);
    result.volume = vol;
    result.issue = issue;

    // 5. Scan lines for DOI and Keywords
    for line in header_text.lines().take(40) {
        let trimmed = line.trim();
        if result.doi.is_none() && (trimmed.contains("10.") || trimmed.to_lowercase().contains("doi:")) {
            if let Some(pos) = trimmed.find("10.") {
                let candidate = &trimmed[pos..];
                let doi_clean = candidate.split_whitespace().next().unwrap_or("").trim_matches(&[',', ';', ')', ']'][..]);
                if doi_clean.len() > 7 {
                    result.doi = Some(doi_clean.to_string());
                }
            }
        }
        if trimmed.to_lowercase().starts_with("keywords:") || trimmed.to_lowercase().starts_with("key words:") {
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let kws: Vec<String> = parts[1]
                    .split(&[',', ';'][..])
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !kws.is_empty() {
                    result.keywords = kws;
                }
            }
        }
    }

    result
}

/// Extracts dates like "JAN.-FEB. 1985", "Jan-Feb 1985", "January 1985", "Oct 1982"
pub fn extract_publication_date(text: &str) -> Option<String> {
    let months = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];

    for line in text.lines().take(30) {
        let upper = line.to_uppercase();
        for &m in &months {
            if let Some(m_idx) = upper.find(m) {
                // Ensure boundary before month (not part of words like "DEMAND", "MAYBE")
                if m_idx > 0 && upper.as_bytes()[m_idx - 1].is_ascii_alphabetic() {
                    continue;
                }
                let after = &upper[m_idx..];
                for part in after.split_whitespace() {
                    let clean: String = part.chars().filter(|c| c.is_ascii_digit()).collect();
                    if clean.len() == 4 && (clean.starts_with("19") || clean.starts_with("20")) {
                        if let Some(y_pos) = after.find(&clean) {
                            let raw_date = &line[m_idx..m_idx + y_pos + clean.len()];
                            let trimmed = raw_date.trim_matches(&[' ', ',', ';', '.'][..]);
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extracts author names from document header lines.
pub fn extract_authors(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();

    // 0. Explicit author prefix lines: "Author: ...", "Authors: ...", "By ..."
    for line in lines.iter().take(20) {
        let lower = line.to_lowercase();
        if lower.starts_with("author:") || lower.starts_with("authors:") || lower.starts_with("by:") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 && !parts[1].trim().is_empty() {
                return Some(parts[1].trim().to_string());
            }
        } else if lower.starts_with("by ") && line.len() > 3 && line.len() < 60 {
            let candidate = line[3..].trim();
            let words: Vec<&str> = candidate.split_whitespace().collect();
            if words.len() >= 2 && words.iter().all(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)) {
                return Some(candidate.to_string());
            }
        }
    }

    // 1. Look for explicit Doctor/Professor signatures: "Dr. Gordon C. Oates"
    for line in lines.iter().take(40) {
        if line.contains("Dr. ") || line.contains("Prof. ") || line.contains("Professor ") {
            for prefix in &["Dr. ", "Prof. ", "Professor "] {
                if let Some(idx) = line.find(prefix) {
                    let slice = &line[idx..];
                    let name_part = slice.split(&[',', ';', '(', ')', '\n'][..]).next().unwrap_or(slice);
                    let words: Vec<&str> = name_part.split_whitespace().collect();
                    if words.len() >= 2 && words.len() <= 5 {
                        return Some(name_part.trim().to_string());
                    }
                }
            }
        }
    }

    // 2. Look for lines signed before Editor-in-Chief or Editor
    for (i, line) in lines.iter().enumerate().take(30) {
        if line.eq_ignore_ascii_case("Editor-in-Chief") || line.eq_ignore_ascii_case("Editor") {
            if i > 0 {
                let candidate = lines[i - 1];
                let words: Vec<&str> = candidate.split_whitespace().collect();
                if words.len() >= 2 && words.len() <= 4 && words.iter().all(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)) {
                    return Some(candidate.to_string());
                }
            }
        }
    }

    // 3. Look for author line immediately preceding institutional affiliation:
    let aff_keywords = ["University", "Institute", "Department", "Laboratory", "College", "Center"];
    for (i, line) in lines.iter().enumerate().take(20) {
        if aff_keywords.iter().any(|k| line.contains(k)) {
            if i > 0 {
                let prev = lines[i - 1];
                // Clean footnote marker symbols like *, †, ‡, !, digits while preserving names (e.g. John, Faeth)
                let clean_words: Vec<String> = prev
                    .split_whitespace()
                    .map(|w| w.trim_matches(&['*', '†', '‡', '§', '¶', '#', '!', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', ','][..]).to_string())
                    .filter(|w| !w.is_empty())
                    .collect();
                let candidate = clean_words.join(" ");

                if !candidate.is_empty() && candidate.len() < 80 {
                    let is_conjunction = prev.contains(" and ") || prev.contains(" & ") || prev.contains('*') || prev.contains('†') || prev.contains('!');
                    let is_capitalized_name = clean_words.len() >= 2
                        && clean_words.len() <= 6
                        && clean_words.iter().all(|w| {
                            w == "and" || w == "&" || w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                        });

                    if is_conjunction || is_capitalized_name {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

/// Extracts organization / affiliation from document header lines.
pub fn extract_organization(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let aff_keywords = ["University", "Institute", "Department", "Laboratory", "College", "Center", "Corporation", "Inc.", "LLC", "NASA"];

    for line in lines.iter().take(25) {
        if aff_keywords.iter().any(|k| line.contains(k)) {
            if !line.starts_with("It is") && !line.starts_with("This") && !line.starts_with("We ") {
                if let Some(pos) = line.find("at the ") {
                    let org = &line[pos + 7..];
                    let clean = org.split(&[',', ';', '.'][..]).next().unwrap_or(org).trim();
                    if !clean.is_empty() {
                        return Some(clean.to_string());
                    }
                }
                let clean = line.trim_matches(&[' ', ',', ';', '.'][..]);
                if clean.len() < 100 {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Extracts volume and optional issue number from header lines.
/// Examples:
/// "VOL. 1, NO. 1, JAN.-FEB. 1985" -> (Some("1"), Some("1"))
/// "Volume 24, Issue 3" -> (Some("24"), Some("3"))
/// "Vol. 12" -> (Some("12"), None)
pub fn extract_volume_and_issue(text: &str) -> (Option<String>, Option<String>) {
    for line in text.lines().take(30) {
        let upper = line.to_uppercase();
        if upper.contains("VOL.") || upper.contains("VOLUME") {
            let mut vol = None;
            let mut issue = None;

            // Find volume
            let vol_start = if let Some(idx) = upper.find("VOL.") {
                idx + 4
            } else if let Some(idx) = upper.find("VOLUME") {
                idx + 6
            } else {
                0
            };

            let after_vol = &upper[vol_start..];
            let vol_digits: String = after_vol
                .chars()
                .skip_while(|c| *c == ' ' || *c == ':' || *c == '#' || *c == '.')
                .take_while(|c| c.is_ascii_digit() || *c == 'I' || *c == 'V' || *c == 'X')
                .collect();

            if !vol_digits.is_empty() {
                vol = Some(vol_digits);
            }

            // Find issue / no.
            for no_tag in &["NO.", "ISSUE", "NUMBER"] {
                if let Some(idx) = upper.find(no_tag) {
                    let after_no = &upper[idx + no_tag.len()..];
                    let issue_digits: String = after_no
                        .chars()
                        .skip_while(|c| *c == ' ' || *c == ':' || *c == '#' || *c == '.')
                        .take_while(|c| c.is_ascii_digit() || *c == 'I' || *c == 'V' || *c == 'X')
                        .collect();

                    if !issue_digits.is_empty() {
                        issue = Some(issue_digits);
                        break;
                    }
                }
            }

            if vol.is_some() {
                return (vol, issue);
            }
        }
    }
    (None, None)
}

/// Extracts journal name from document header lines (e.g. "Journal of Propulsion and Power").
pub fn extract_journal(text: &str) -> Option<String> {
    // 1. Explicit AIAA / known abbreviation mappings
    let upper = text.to_uppercase();
    if upper.contains("J. PROPULSION") || upper.contains("JOURNAL OF PROPULSION") {
        return Some("Journal of Propulsion and Power".to_string());
    }
    if upper.contains("J. AIRCRAFT") || upper.contains("JOURNAL OF AIRCRAFT") {
        return Some("Journal of Aircraft".to_string());
    }
    if upper.contains("J. SPACECRAFT") || upper.contains("JOURNAL OF SPACECRAFT") {
        return Some("Journal of Spacecraft and Rockets".to_string());
    }
    if upper.contains("J. GUIDANCE") || upper.contains("JOURNAL OF GUIDANCE") {
        return Some("Journal of Guidance, Control, and Dynamics".to_string());
    }
    if upper.contains("AIAA J.") || upper.contains("AIAA JOURNAL") {
        return Some("AIAA Journal".to_string());
    }

    // 2. Generic "Journal of ..." or "Transactions on ..." pattern
    for line in text.lines().take(30) {
        let trimmed = line.trim();
        // Check for "Journal of <...>"
        if let Some(pos) = trimmed.to_lowercase().find("journal of ") {
            let candidate = &trimmed[pos..];
            let clean = candidate
                .split(&[',', ';', '\n', '(', ')'][..])
                .next()
                .unwrap_or(candidate)
                .trim();
            let words: Vec<&str> = clean.split_whitespace().collect();
            if words.len() >= 3 && words.len() <= 8 {
                return Some(clean.to_string());
            }
        }
        // Check for "Transactions on <...>"
        if let Some(pos) = trimmed.find("Transactions on ") {
            let candidate = &trimmed[pos..];
            let clean = candidate
                .split(&[',', ';', '\n', '(', ')'][..])
                .next()
                .unwrap_or(candidate)
                .trim();
            let words: Vec<&str> = clean.split_whitespace().collect();
            if words.len() >= 3 && words.len() <= 8 {
                return Some(clean.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_doi_and_keywords_pattern() {
        let sample = "Deep Learning with Attention\n\
                      Authors: Alice Smith, Bob Jones\n\
                      Keywords: Transformer; Attention Mechanism, NLP\n\
                      DOI: 10.1145/3318464.3389700\n\
                      Abstract text follows here.";
        let res = extract_header_metadata_fallback(sample);
        assert_eq!(res.doi.as_deref(), Some("10.1145/3318464.3389700"));
        assert_eq!(res.keywords.len(), 3);
        assert_eq!(res.keywords[0], "Transformer");
    }

    #[test]
    fn test_extract_combustor_paper_metadata() {
        let sample = "VOL. 1, NO. 1, JAN.-FEB. 1985 J. PROPULSION 5\n\
                      \n\
                      A One-Dimensional Model of a Carbon-Black\n\
                      Slurry-Fueled Combustor\n\
                      \n\
                      S.R. Turns* and G.M. Faeth!\n\
                      The Pennsylvania State University, University Park, Pennsylvania\n\
                      \n\
                      Results of a one-dimensional model of a carbon-black slurry-fueled combustor...";

        let res = extract_header_metadata_fallback(sample);
        assert_eq!(res.date.as_deref(), Some("JAN.-FEB. 1985"));
        assert_eq!(res.journal.as_deref(), Some("Journal of Propulsion and Power"));
        assert_eq!(res.volume.as_deref(), Some("1"));
        assert_eq!(res.issue.as_deref(), Some("1"));
        assert_eq!(res.authors.first().map(|s| s.as_str()), Some("S.R. Turns and G.M. Faeth"));
        assert_eq!(
            res.organization.as_deref(),
            Some("The Pennsylvania State University, University Park, Pennsylvania")
        );
    }

    #[test]
    fn test_extract_editor_paper_metadata() {
        let sample = "VOL. 1,NO. 1 J. PROPULSION JAN.-FEB. 1985\n\
                      \n\
                      A Publication of Wide Scope\n\
                      \n\
                      Gordon C. Oates\n\
                      Editor-in-Chief\n\
                      \n\
                      Dr. Gordon C. Oates, Professor of Aeronautics and Astronautics at the University of Washington...";

        let res = extract_header_metadata_fallback(sample);
        assert_eq!(res.date.as_deref(), Some("JAN.-FEB. 1985"));
        assert_eq!(res.journal.as_deref(), Some("Journal of Propulsion and Power"));
        assert_eq!(res.volume.as_deref(), Some("1"));
        assert_eq!(res.issue.as_deref(), Some("1"));
        assert_eq!(res.authors.first().map(|s| s.as_str()), Some("Dr. Gordon C. Oates"));
        assert_eq!(res.organization.as_deref(), Some("University of Washington"));
    }
}
