//! Alternative template front-ends for pdfcn.
//!
//! HAML is pdfcn's native syntax, but the pipeline after parsing only ever
//! sees the [`pdfcn_parser::Document`] AST. Every format here lowers to that
//! same AST, so `{{ interpolation }}`, `for`/`if`, components, includes and
//! every later pass (utilities, QR, images, icons...) work identically no
//! matter how the document was written:
//!
//! | Format | Source |
//! |---|---|
//! | [`Format::Haml`] | the native indentation syntax |
//! | [`Format::Html`] | HTML with Tailwind classes, `<Card>` components, `data-for`/`data-if` |
//! | [`Format::Markdown`] | CommonMark + tables, optional YAML front matter, HTML/components inline |
//! | [`Format::Json`] / [`Format::Yaml`] | a declarative tree of blocks (what an API or an LLM emits) |
//! | [`Format::Typst`] / [`Format::Latex`] | small, documented subsets for simple documents |

mod blocks;
mod html;
mod latex;
mod markdown;
mod typst;

use std::fmt;

use pdfcn_parser::Document;
use serde_json::Value;

/// Error from any front-end.
#[derive(Debug)]
pub enum FormatError {
    /// The HAML parser's own error.
    Haml(pdfcn_parser::ParseError),
    /// Any other front-end: a message naming the problem.
    Message(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Haml(e) => write!(f, "{e}"),
            FormatError::Message(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<pdfcn_parser::ParseError> for FormatError {
    fn from(e: pdfcn_parser::ParseError) -> Self {
        FormatError::Haml(e)
    }
}

pub(crate) fn err(message: impl Into<String>) -> FormatError {
    FormatError::Message(message.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Haml,
    Html,
    Markdown,
    Json,
    Yaml,
    Typst,
    Latex,
}

impl Format {
    /// From a file extension (without the dot), case-insensitive.
    pub fn from_extension(ext: &str) -> Option<Format> {
        Self::from_name(ext)
    }

    /// From a format name or file extension, case-insensitive.
    pub fn from_name(name: &str) -> Option<Format> {
        Some(match name.to_ascii_lowercase().as_str() {
            "haml" => Format::Haml,
            "html" | "htm" | "xhtml" => Format::Html,
            "md" | "markdown" => Format::Markdown,
            "json" => Format::Json,
            "yaml" | "yml" => Format::Yaml,
            "typ" | "typst" => Format::Typst,
            "tex" | "latex" => Format::Latex,
            _ => return None,
        })
    }

    /// Guesses the format from the source text alone, for callers that have
    /// no file name (the HTTP API, an editor buffer).
    ///
    /// Deliberately conservative: only unambiguous openings are recognised,
    /// everything else is HAML, the historical default. HAML lines start with
    /// `%`, `.`, `#id`, `-` or text; none of the following can begin a valid
    /// HAML document.
    ///
    /// - `<` → HTML
    /// - `{` or `[` → JSON block tree
    /// - `---` front matter, or a `# Heading` / `## Heading` first line → Markdown
    /// - `\documentclass` / `\begin{document}` → LaTeX
    ///
    /// YAML block trees and Typst documents have no unambiguous opening and
    /// need an explicit format or a `.yaml` / `.typ` extension.
    pub fn detect(source: &str) -> Format {
        let t = source.trim_start_matches('\u{feff}').trim_start();
        if t.starts_with('<') {
            return Format::Html;
        }
        if t.starts_with('{') || t.starts_with('[') {
            return Format::Json;
        }
        if t.starts_with("---") || is_atx_heading(t.lines().next().unwrap_or("")) {
            return Format::Markdown;
        }
        if t.starts_with("\\documentclass") || t.starts_with("\\begin{document}") {
            return Format::Latex;
        }
        Format::Haml
    }
}

fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

/// A parsed document plus whatever the format carries besides markup.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub document: Document,
    /// Markdown front matter (YAML), as default template data: values the
    /// caller supplies at render time win over it.
    pub front_matter: Option<Value>,
}

/// Parses `source`. With `format == None` the format is detected from the
/// text ([`Format::detect`]).
pub fn parse(source: &str, format: Option<Format>) -> Result<Parsed, FormatError> {
    let format = format.unwrap_or_else(|| Format::detect(source));
    let (document, front_matter) = match format {
        Format::Haml => (pdfcn_parser::parse_document(source)?, None),
        Format::Html => (html::parse_html(source, false)?, None),
        Format::Markdown => {
            let (front, body) = markdown::split_front_matter(source)?;
            (markdown::parse_markdown(body)?, front)
        }
        Format::Json => (blocks::from_json(source)?, None),
        Format::Yaml => (blocks::from_yaml(source)?, None),
        Format::Typst => (typst::parse_typst(source)?, None),
        Format::Latex => (latex::parse_latex(source)?, None),
    };
    Ok(Parsed {
        document,
        front_matter,
    })
}

/// [`parse`], keeping only the document.
pub fn parse_document(source: &str, format: Option<Format>) -> Result<Document, FormatError> {
    parse(source, format).map(|p| p.document)
}

/// Merges `defaults` (front matter) under `data`: keys already in `data`
/// win, and objects merge recursively.
pub fn merge_defaults(defaults: &Value, data: &Value) -> Value {
    match (defaults, data) {
        (Value::Object(d), Value::Object(v)) => {
            let mut out = d.clone();
            for (k, val) in v {
                let merged = match d.get(k) {
                    Some(base) => merge_defaults(base, val),
                    None => val.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (_, Value::Null) => defaults.clone(),
        (_, v) => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unambiguous_openings_and_defaults_to_haml() {
        assert_eq!(Format::detect("<div>x</div>"), Format::Html);
        assert_eq!(Format::detect("\n  <!doctype html>"), Format::Html);
        assert_eq!(Format::detect("{\"type\":\"p\"}"), Format::Json);
        assert_eq!(Format::detect("[\"a\"]"), Format::Json);
        assert_eq!(Format::detect("---\ntitle: x\n---\nhi"), Format::Markdown);
        assert_eq!(Format::detect("# Title\n\ntext"), Format::Markdown);
        assert_eq!(Format::detect("\\documentclass{article}"), Format::Latex);
        // Valid HAML openings stay HAML.
        assert_eq!(Format::detect("%h1 Hi"), Format::Haml);
        assert_eq!(Format::detect(".card Hi"), Format::Haml);
        assert_eq!(Format::detect("#main Hi"), Format::Haml);
        assert_eq!(Format::detect("- for x in xs\n  %p"), Format::Haml);
        assert_eq!(Format::detect("Just text"), Format::Haml);
    }

    #[test]
    fn names_and_extensions_map_to_formats() {
        assert_eq!(Format::from_extension("MD"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("yml"), Some(Format::Yaml));
        assert_eq!(Format::from_extension("typ"), Some(Format::Typst));
        assert_eq!(Format::from_extension("tex"), Some(Format::Latex));
        assert_eq!(Format::from_extension("docx"), None);
    }

    #[test]
    fn defaults_merge_under_data() {
        let d = serde_json::json!({"a": 1, "o": {"x": 1, "y": 2}});
        let v = serde_json::json!({"a": 9, "o": {"y": 3}});
        assert_eq!(
            merge_defaults(&d, &v),
            serde_json::json!({"a": 9, "o": {"x": 1, "y": 3}})
        );
        assert_eq!(merge_defaults(&d, &Value::Null), d);
    }
}

#[cfg(test)]
mod node_components {
    /// `pdfcn-node/components.js` lists the component tags for JSX users; it
    /// must name exactly the registry's components.
    #[test]
    fn jsx_component_tags_match_the_registry() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../pdfcn-node/components.js");
        let js = std::fs::read_to_string(path).expect("components.js");
        let mut listed: Vec<&str> = js
            .lines()
            .filter_map(|l| l.strip_prefix("exports."))
            .filter_map(|l| l.split(" = ").next())
            .collect();
        let mut expected: Vec<&str> = pdfcn_components::COMPONENT_NAMES.to_vec();
        listed.sort_unstable();
        expected.sort_unstable();
        assert_eq!(listed, expected);
    }
}
