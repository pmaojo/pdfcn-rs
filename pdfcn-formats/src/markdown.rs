//! Markdown front-end: CommonMark (+ tables, strikethrough, smart
//! punctuation) rendered to HTML by pulldown-cmark, then lowered by the HTML
//! front-end with default typography. HTML and components inside the
//! Markdown pass through, so `<Card title="x">` blocks and Tailwind `<div>`s
//! mix freely with prose.

use pdfcn_parser::Document;
use pulldown_cmark::{html::push_html, Options, Parser};
use serde_json::Value;

use crate::{err, FormatError};

/// Splits a leading `---` YAML block off `source`: `(front matter, body)`.
pub(crate) fn split_front_matter(source: &str) -> Result<(Option<Value>, &str), FormatError> {
    let s = source.trim_start_matches('\u{feff}');
    let Some(rest) = s.strip_prefix("---") else {
        return Ok((None, s));
    };
    // Opening fence must be alone on its line.
    let Some(rest) = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
    else {
        return Ok((None, s));
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let t = line.trim_end();
        if t == "---" || t == "..." {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            let value: Value = if yaml.trim().is_empty() {
                Value::Object(Default::default())
            } else {
                serde_yaml::from_str(yaml)
                    .map_err(|e| err(format!("front matter is not valid YAML: {e}")))?
            };
            if !value.is_object() {
                return Err(err("front matter must be a YAML mapping"));
            }
            return Ok((Some(value), body));
        }
        offset += line.len();
    }
    // No closing fence: it was a thematic break, not front matter.
    Ok((None, s))
}

pub(crate) fn parse_markdown(source: &str) -> Result<Document, FormatError> {
    let opts =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
    let mut html = String::new();
    push_html(&mut html, Parser::new_ext(source, opts));
    crate::html::parse_html(&html, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcn_parser::Node;

    #[test]
    fn front_matter_is_split_and_parsed() {
        let (fm, body) = split_front_matter("---\ntitle: Hi\nn: 2\n---\n# Body\n").unwrap();
        assert_eq!(fm.unwrap()["title"], "Hi");
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn a_thematic_break_without_closing_fence_is_not_front_matter() {
        let (fm, body) = split_front_matter("---\nno closing fence\n").unwrap();
        assert!(fm.is_none());
        assert!(body.starts_with("---"));
    }

    #[test]
    fn markdown_lowers_to_styled_elements() {
        let doc = parse_markdown("# Title\n\nHello **world** {{ name }}.\n\n- a\n- b\n").unwrap();
        let tags: Vec<_> = doc
            .iter()
            .filter_map(|n| match n {
                Node::Element { tag, .. } => Some(tag.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tags, ["h1", "p", "ul"]);
        let Node::Element { classes, .. } = &doc[0] else {
            panic!()
        };
        assert!(classes.contains(&"text-3xl".to_string()));
    }

    #[test]
    fn components_and_html_blocks_pass_through() {
        let doc = parse_markdown("<Card title=\"Hi\">\n\nInside **card**\n\n</Card>\n").unwrap();
        let Node::Component { name, children, .. } = &doc[0] else {
            panic!("{doc:?}")
        };
        assert_eq!(name, "Card");
        assert!(!children.is_empty());
    }

    #[test]
    fn tables_render() {
        let doc = parse_markdown("| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
        assert!(matches!(&doc[0], Node::Element { tag, .. } if tag == "table"));
    }
}
