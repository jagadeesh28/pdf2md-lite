//! Section-Aware Semantic Chunking for technical Markdown documents.
//!
//! Partitions converted Markdown notes along structural heading boundaries (`#`, `##`, `###`)
//! and physical page demarcation anchors (`<!-- Page N -->`) into coherent 256–512 token
//! (~1,200 character) chunks while strictly preserving display math blocks and tables.

use serde::{Deserialize, Serialize};

/// A semantically bounded chunk of document content keyed to physical page and structural heading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticChunk {
    /// Unique identifier for this chunk (e.g. `<slug>#p1-c0`).
    pub chunk_id: String,
    /// Physical 1-based page number where this chunk appears or begins.
    pub page_num: usize,
    /// Nearest enclosing Markdown heading (e.g. "2. Governing Equations").
    pub heading: Option<String>,
    /// Cleaned chunk text formatted in Markdown.
    pub text: String,
}

/// Target character bounds for section-aware chunks (~3-4 chars per token).
const MIN_CHUNK_CHARS: usize = 200;
const TARGET_CHUNK_CHARS: usize = 1200;
const MAX_CHUNK_CHARS: usize = 2000;

/// Partitions a Markdown note into section-aware semantic chunks.
pub fn chunk_markdown_document(content: &str, slug: &str) -> Vec<SemanticChunk> {
    let mut chunks = Vec::new();
    let mut current_page: usize = 1;
    let mut current_heading: Option<String> = None;
    let mut chunk_index: usize = 0;

    let mut buffer = String::new();
    let mut chunk_page = current_page;
    let mut chunk_heading = current_heading.clone();

    let blocks = split_into_structural_blocks(content);

    for block in blocks {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check for page demarcation comment: <!-- Page N -->
        if let Some(page) = parse_page_demarcation(trimmed) {
            current_page = page;
            continue;
        }

        // Check for Markdown heading: # Title, ## Section, etc.
        if is_heading(trimmed) {
            let heading_text = extract_heading_text(trimmed);

            // Flush previous accumulated buffer before beginning a new section
            if !buffer.trim().is_empty() {
                chunks.push(SemanticChunk {
                    chunk_id: format!("{slug}#p{chunk_page}-c{chunk_index}"),
                    page_num: chunk_page,
                    heading: chunk_heading.clone(),
                    text: buffer.trim().to_string(),
                });
                chunk_index += 1;
                buffer.clear();
            }

            current_heading = Some(heading_text);
            chunk_page = current_page;
            chunk_heading = current_heading.clone();
            continue;
        }

        // If buffer is empty, initialize chunk metadata
        if buffer.is_empty() {
            chunk_page = current_page;
            chunk_heading = current_heading.clone();
        }

        // If adding this block would exceed MAX_CHUNK_CHARS and we already have sufficient content
        if buffer.len() >= MIN_CHUNK_CHARS && (buffer.len() + block.len() > TARGET_CHUNK_CHARS) {
            chunks.push(SemanticChunk {
                chunk_id: format!("{slug}#p{chunk_page}-c{chunk_index}"),
                page_num: chunk_page,
                heading: chunk_heading.clone(),
                text: buffer.trim().to_string(),
            });
            chunk_index += 1;
            buffer.clear();

            chunk_page = current_page;
            chunk_heading = current_heading.clone();
        }

        // Handle oversized individual prose blocks (> MAX_CHUNK_CHARS) with sentence-level sliding overlap
        if block.len() > MAX_CHUNK_CHARS && !is_atomic_block(trimmed) {
            let sub_paragraphs = split_oversized_prose(&block, TARGET_CHUNK_CHARS, 150);
            for (sub_idx, sub_text) in sub_paragraphs.into_iter().enumerate() {
                if sub_idx == 0 && buffer.is_empty() {
                    chunks.push(SemanticChunk {
                        chunk_id: format!("{slug}#p{chunk_page}-c{chunk_index}"),
                        page_num: chunk_page,
                        heading: chunk_heading.clone(),
                        text: sub_text,
                    });
                    chunk_index += 1;
                } else if !buffer.is_empty() {
                    chunks.push(SemanticChunk {
                        chunk_id: format!("{slug}#p{chunk_page}-c{chunk_index}"),
                        page_num: chunk_page,
                        heading: chunk_heading.clone(),
                        text: buffer.trim().to_string(),
                    });
                    chunk_index += 1;
                    buffer.clear();

                    chunks.push(SemanticChunk {
                        chunk_id: format!("{slug}#p{current_page}-c{chunk_index}"),
                        page_num: current_page,
                        heading: current_heading.clone(),
                        text: sub_text,
                    });
                    chunk_index += 1;
                } else {
                    chunks.push(SemanticChunk {
                        chunk_id: format!("{slug}#p{current_page}-c{chunk_index}"),
                        page_num: current_page,
                        heading: current_heading.clone(),
                        text: sub_text,
                    });
                    chunk_index += 1;
                }
            }
            continue;
        }

        if !buffer.is_empty() {
            buffer.push_str("\n\n");
        }
        buffer.push_str(trimmed);
    }

    // Flush any remaining text in buffer
    if !buffer.trim().is_empty() {
        chunks.push(SemanticChunk {
            chunk_id: format!("{slug}#p{chunk_page}-c{chunk_index}"),
            page_num: chunk_page,
            heading: chunk_heading,
            text: buffer.trim().to_string(),
        });
    }

    chunks
}

/// Identifies if a block must remain atomic (tables, display math, code fences, callouts).
fn is_atomic_block(s: &str) -> bool {
    s.starts_with("$$") || s.starts_with("```") || s.starts_with('|') || s.starts_with('>')
}

/// Parses page comment `<!-- Page N -->` and returns the 1-based page number.
fn parse_page_demarcation(s: &str) -> Option<usize> {
    if s.starts_with("<!--") && s.ends_with("-->") {
        let inside = s[4..s.len() - 3].trim();
        if inside.to_lowercase().starts_with("page") {
            let parts: Vec<&str> = inside.split_whitespace().collect();
            if parts.len() >= 2 {
                return parts[1].parse::<usize>().ok();
            }
        }
    }
    None
}

/// Tests if a line or block represents a Markdown heading.
fn is_heading(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with("# ")
        || trimmed.starts_with("## ")
        || trimmed.starts_with("### ")
        || trimmed.starts_with("#### ")
        || trimmed.starts_with("##### ")
        || trimmed.starts_with("###### ")
}

/// Extracts clean heading text without the leading `#` symbols.
fn extract_heading_text(s: &str) -> String {
    let trimmed = s.trim_start();
    let without_hash = trimmed.trim_start_matches('#').trim();
    // In case heading contains trailing markers or tags
    without_hash.to_string()
}

/// Splits raw Markdown into structural blocks (preserving code blocks, display math, and tables).
fn split_into_structural_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut in_code_fence = false;
    let mut in_display_math = false;

    for line in content.lines() {
        let trimmed_line = line.trim();

        // Check for YAML frontmatter delimiters
        if trimmed_line == "---" {
            // Skip frontmatter lines if within header
            continue;
        }

        // Toggle code fence
        if trimmed_line.starts_with("```") {
            in_code_fence = !in_code_fence;
            current_block.push_str(line);
            current_block.push('\n');
            if !in_code_fence {
                blocks.push(current_block.trim().to_string());
                current_block.clear();
            }
            continue;
        }

        if in_code_fence {
            current_block.push_str(line);
            current_block.push('\n');
            continue;
        }

        // Toggle display math block
        if trimmed_line.starts_with("$$") {
            if trimmed_line.ends_with("$$") && trimmed_line.len() > 2 {
                // Single line display math block
                if !current_block.trim().is_empty() {
                    blocks.push(current_block.trim().to_string());
                    current_block.clear();
                }
                blocks.push(trimmed_line.to_string());
                continue;
            }
            in_display_math = !in_display_math;
            current_block.push_str(line);
            current_block.push('\n');
            if !in_display_math {
                blocks.push(current_block.trim().to_string());
                current_block.clear();
            }
            continue;
        }

        if in_display_math {
            current_block.push_str(line);
            current_block.push('\n');
            continue;
        }

        // Blank line marks standard paragraph boundary
        if trimmed_line.is_empty() {
            if !current_block.trim().is_empty() {
                blocks.push(current_block.trim().to_string());
                current_block.clear();
            }
            continue;
        }

        // Heading marks immediate block boundary
        if is_heading(trimmed_line) {
            if !current_block.trim().is_empty() {
                blocks.push(current_block.trim().to_string());
                current_block.clear();
            }
            blocks.push(trimmed_line.to_string());
            continue;
        }

        // Demarcation comments <!-- Page N -->
        if trimmed_line.starts_with("<!--") && trimmed_line.ends_with("-->") {
            if !current_block.trim().is_empty() {
                blocks.push(current_block.trim().to_string());
                current_block.clear();
            }
            blocks.push(trimmed_line.to_string());
            continue;
        }

        current_block.push_str(line);
        current_block.push('\n');
    }

    if !current_block.trim().is_empty() {
        blocks.push(current_block.trim().to_string());
    }

    blocks
}

/// Splits an oversized continuous prose block with sentence-level sliding overlap.
fn split_oversized_prose(text: &str, target_size: usize, overlap_chars: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let sentences = split_sentences(text);

    let mut current = String::new();
    let mut last_sentence = String::new();

    for sent in sentences {
        if current.len() + sent.len() > target_size && !current.is_empty() {
            parts.push(current.trim().to_string());
            current.clear();
            if overlap_chars > 0 && !last_sentence.is_empty() {
                current.push_str(&last_sentence);
                current.push(' ');
            }
        }
        current.push_str(&sent);
        current.push(' ');
        last_sentence = sent;
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

/// Basic sentence boundary splitter respecting abbreviations.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        current.push_str(word);
        current.push(' ');

        let ends_sentence = word.ends_with('.') || word.ends_with('?') || word.ends_with('!');
        let is_short_abbreviation = word.len() <= 3 && word.chars().all(|c| c.is_ascii_uppercase() || c == '.');

        if ends_sentence && !is_short_abbreviation && current.len() > 40 {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }

    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking_with_headings_and_page_anchors() {
        let md = r#"
# 1. Introduction

<!-- Page 1 -->
This is the introductory paragraph of the paper. It describes the aerodynamic principles of axial compressors.

## 2. Governing Equations

<!-- Page 2 -->
The flow field is modeled using the Navier-Stokes equations in compressible form:

$$
\frac{\partial \rho}{\partial t} + \nabla \cdot (\rho \mathbf{u}) = 0
$$

The conservation of momentum follows with viscous tensor $\tau_{ij}$.

| Variable | Definition |
| --- | --- |
| $\rho$ | Fluid density |
| $\mathbf{u}$ | Velocity vector |

<!-- Page 3 -->
## 3. Results & Discussion

Experimental validation demonstrated a 4.2% increase in stage efficiency.
"#;

        let chunks = chunk_markdown_document(md, "test-doc");
        assert!(chunks.len() >= 3, "Expected at least 3 chunks, got {}", chunks.len());

        // Check chunk 0
        assert_eq!(chunks[0].chunk_id, "test-doc#p1-c0");
        assert_eq!(chunks[0].page_num, 1);
        assert_eq!(chunks[0].heading.as_deref(), Some("1. Introduction"));
        assert!(chunks[0].text.contains("aerodynamic principles"));

        // Check math block preservation
        let math_chunk = chunks.iter().find(|c| c.text.contains(r"\frac{\partial \rho}"));
        assert!(math_chunk.is_some(), "Display math equation should be preserved inside a chunk");
        let mc = math_chunk.unwrap();
        assert_eq!(mc.heading.as_deref(), Some("2. Governing Equations"));
        assert!(mc.text.contains("$$"));
    }

    #[test]
    fn test_parse_page_demarcation() {
        assert_eq!(parse_page_demarcation("<!-- Page 1 -->"), Some(1));
        assert_eq!(parse_page_demarcation("<!-- Page 42 -->"), Some(42));
        assert_eq!(parse_page_demarcation("<!-- page 12 -->"), Some(12));
        assert_eq!(parse_page_demarcation("<!-- Other Comment -->"), None);
    }
}
