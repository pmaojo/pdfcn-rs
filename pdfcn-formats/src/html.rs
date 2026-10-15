//! HTML front-end: parses with html5ever (the HTML5 algorithm, so messy
//! real-world markup behaves like it does in a browser) and lowers the DOM
//! to the HAML AST.
//!
//! - `<Card title="x">` / `<card>` / `<x-card>` / `<pdfcn-card>` are components.
//!   html5ever lowercases tag names, so bare names match case-insensitively
//!   against the component registry, except where the name is also a real
//!   HTML element (`header`, `table`, `input`, ...): those need the `x-` or
//!   `pdfcn-` prefix to mean the component.
//! - `{{ expr }}` in text and attributes is interpolated; a component
//!   attribute that is exactly `{{ expr }}` passes the value (`rows="{{ items }}"`).
//! - Control flow is attributes: `data-for="x in xs"`, `data-if`, `data-elif`,
//!   `data-else`; `<template>` groups children without emitting a wrapper.
//!   `<include src="partials/row.html"/>` and `<set name="n" value="a + b"/>` (self-closed).
//! - `<style>` blocks are kept (the core hoists them into the stylesheet);
//!   `<script>`, `<head>` metadata and comments are dropped.

use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, ParseOpts};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use pdfcn_parser::{Attr, AttrValue, Document, Node};

use crate::{err, FormatError};

/// Components whose lowercase name is also a real HTML element: bare, they
/// stay HTML.
const HTML_COLLISIONS: &[&str] = &[
    "header", "table", "input", "textarea", "select", "label", "progress", "dialog",
];

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Parses an HTML document or fragment. With `prose`, elements that carry no
/// class of their own get default typography classes (used for Markdown).
pub(crate) fn parse_html(source: &str, prose: bool) -> Result<Document, FormatError> {
    let source = expand_self_closing(source);
    let dom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut source.as_bytes())
        .map_err(|e| err(format!("HTML parse error: {e}")))?;
    let mut cx = Cx { prose };
    let mut out = Vec::new();
    let Some(html) = dom
        .document
        .children
        .borrow()
        .iter()
        .find(|n| element_name(n).as_deref() == Some("html"))
        .cloned()
    else {
        return Ok(out);
    };
    let mut body: Option<Handle> = None;
    for child in html.children.borrow().iter() {
        match element_name(child).as_deref() {
            Some("head") => {
                // Only author styles survive from <head>.
                // A leading `<template>` lands in <head> per the HTML5 algorithm.
                let kept: Vec<Handle> = child
                    .children
                    .borrow()
                    .iter()
                    .filter(|h| matches!(element_name(h).as_deref(), Some("style" | "template")))
                    .cloned()
                    .collect();
                out.extend(cx.convert_list(&kept, false)?);
            }
            Some("body") => body = Some(child.clone()),
            _ => {}
        }
    }
    if let Some(body) = body {
        let kids = cx.convert_children(&body)?;
        let attrs = element_attrs(&body);
        let has_wrapper = attrs
            .iter()
            .any(|(k, _)| k == "class" || k == "style" || k == "id");
        if has_wrapper {
            let (classes, id, rest) = split_attrs(&attrs, false);
            out.push(Node::Element {
                tag: "div".into(),
                id,
                classes,
                attrs: rest,
                children: kids,
            });
        } else {
            out.extend(kids);
        }
    }
    Ok(out)
}

struct Cx {
    prose: bool,
}

fn element_name(h: &Handle) -> Option<String> {
    match &h.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

fn element_attrs(h: &Handle) -> Vec<(String, String)> {
    match &h.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

fn template_contents(h: &Handle) -> Option<Handle> {
    match &h.data {
        NodeData::Element {
            template_contents, ..
        } => template_contents.borrow().clone(),
        _ => None,
    }
}

/// `{{ expr }}` and nothing else.
fn whole_expression(value: &str) -> Option<String> {
    let t = value.trim();
    let inner = t.strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains("{{") || inner.contains("}}") {
        return None;
    }
    Some(inner.trim().to_string())
}

/// Splits attributes into (classes, id, other attributes).
fn split_attrs(
    attrs: &[(String, String)],
    component: bool,
) -> (Vec<String>, Option<String>, Vec<Attr>) {
    let mut classes = Vec::new();
    let mut id = None;
    let mut rest = Vec::new();
    for (name, value) in attrs {
        if name.starts_with("data-")
            && matches!(
                name.as_str(),
                "data-for" | "data-if" | "data-elif" | "data-else"
            )
        {
            continue;
        }
        if !component && name == "class" {
            classes = value.split_whitespace().map(str::to_string).collect();
        } else if !component && name == "id" {
            id = Some(value.clone());
        } else {
            let v = match (component, whole_expression(value)) {
                (true, Some(e)) => AttrValue::Expr(e),
                _ => AttrValue::Literal(value.clone()),
            };
            rest.push(Attr {
                name: name.clone(),
                value: v,
            });
        }
    }
    (classes, id, rest)
}

fn component_for(tag: &str) -> Option<&'static str> {
    let (bare, prefixed) = match tag
        .strip_prefix("x-")
        .or_else(|| tag.strip_prefix("pdfcn-"))
    {
        Some(rest) => (rest, true),
        None => (tag, false),
    };
    if !prefixed && HTML_COLLISIONS.contains(&bare) {
        return None;
    }
    let all = pdfcn_components::COMPONENT_NAMES
        .iter()
        .chain(pdfcn_components::interactive_only_names());
    all.copied().find(|n| n.eq_ignore_ascii_case(bare))
}

fn prose_classes(tag: &str, in_pre: bool) -> Option<&'static str> {
    Some(match tag {
        "h1" => "text-3xl font-bold mt-6 mb-3",
        "h2" => "text-2xl font-semibold mt-5 mb-2",
        "h3" => "text-xl font-semibold mt-4 mb-2",
        "h4" => "text-lg font-semibold mt-3 mb-1",
        "h5" | "h6" => "font-semibold mt-3 mb-1",
        "p" => "my-2 leading-relaxed",
        "ul" => "list-disc pl-6 my-2",
        "ol" => "list-decimal pl-6 my-2",
        "li" => "my-1",
        "blockquote" => "border-l-4 pl-4 my-3 text-slate-600 italic",
        "pre" => "bg-slate-100 rounded p-3 my-3 text-sm font-mono",
        "code" if !in_pre => "font-mono text-sm bg-slate-100",
        "table" => "w-full my-3 text-sm",
        "th" => "text-left font-semibold border-b p-2",
        "td" => "border-b p-2",
        "hr" => "my-4 border-t",
        "a" => "text-blue-600 underline",
        "img" => "my-3",
        _ => return None,
    })
}

/// A node plus the directives that wrap it.
struct Directives {
    for_: Option<(String, String)>,
    if_: Option<String>,
    elif: Option<String>,
    else_: bool,
}

fn directives(attrs: &[(String, String)]) -> Result<Directives, FormatError> {
    let get = |k: &str| {
        attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.trim().to_string())
    };
    let for_ = match get("data-for") {
        Some(spec) => {
            let (binding, iterable) = spec
                .split_once(" in ")
                .ok_or_else(|| err(format!("data-for=\"{spec}\": expected \"item in items\"")))?;
            Some((binding.trim().to_string(), iterable.trim().to_string()))
        }
        None => None,
    };
    Ok(Directives {
        for_,
        if_: get("data-if"),
        elif: get("data-elif"),
        else_: attrs.iter().any(|(n, _)| n == "data-else"),
    })
}

impl Cx {
    fn convert_children(&mut self, parent: &Handle) -> Result<Vec<Node>, FormatError> {
        self.convert_list(&parent.children.borrow(), false)
    }

    fn convert_list(&mut self, kids: &[Handle], in_pre: bool) -> Result<Vec<Node>, FormatError> {
        let mut out: Vec<Node> = Vec::new();
        for kid in kids {
            let attrs = element_attrs(kid);
            let d = directives(&attrs)?;
            let nodes = self.convert_one_in(kid, in_pre)?;
            if nodes.is_empty() {
                continue;
            }
            // if / elif / else chain: attach to the preceding `If`.
            if d.elif.is_some() || d.else_ {
                if let Some(Node::If {
                    branches,
                    else_body,
                }) = out.last_mut()
                {
                    if else_body.is_none() {
                        match &d.elif {
                            Some(cond) => branches.push((cond.clone(), nodes)),
                            None => *else_body = Some(nodes),
                        }
                        continue;
                    }
                }
                return Err(err(
                    "data-elif / data-else must follow a sibling with data-if",
                ));
            }
            let mut wrapped = nodes;
            if let Some(cond) = d.if_ {
                wrapped = vec![Node::If {
                    branches: vec![(cond, wrapped)],
                    else_body: None,
                }];
            }
            if let Some((binding, iterable)) = d.for_ {
                wrapped = vec![Node::For {
                    binding,
                    iterable,
                    body: wrapped,
                }];
            }
            out.extend(wrapped);
        }
        Ok(out)
    }

    /// Converts one DOM node, ignoring directive attributes (the caller wraps).
    fn convert_one_in(&mut self, h: &Handle, in_pre: bool) -> Result<Vec<Node>, FormatError> {
        match &h.data {
            NodeData::Text { contents } => {
                let raw: String = contents.borrow().to_string();
                let text = if in_pre {
                    raw
                } else {
                    collapse_whitespace(&raw)
                };
                if text.trim().is_empty() && !in_pre {
                    Ok(Vec::new())
                } else {
                    Ok(vec![Node::Text(text)])
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.to_string();
                let attrs = element_attrs(h);
                match tag.as_str() {
                    "script" | "head" | "meta" | "link" | "title" | "noscript" | "base" => {
                        Ok(Vec::new())
                    }
                    "style" => {
                        let css: String = h
                            .children
                            .borrow()
                            .iter()
                            .filter_map(|c| match &c.data {
                                NodeData::Text { contents } => Some(contents.borrow().to_string()),
                                _ => None,
                            })
                            .collect();
                        Ok(vec![Node::Element {
                            tag: "style".into(),
                            id: None,
                            classes: vec![],
                            attrs: vec![],
                            children: vec![Node::Text(css)],
                        }])
                    }
                    "include" => {
                        let src = attrs
                            .iter()
                            .find(|(k, _)| k == "src")
                            .map(|(_, v)| v.clone())
                            .ok_or_else(|| err("<include> needs a src attribute"))?;
                        Ok(vec![Node::Include { path: src }])
                    }
                    "set" => {
                        let get =
                            |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                        let (Some(n), Some(v)) = (get("name"), get("value")) else {
                            return Err(err("<set> needs name and value attributes"));
                        };
                        Ok(vec![Node::Set { name: n, expr: v }])
                    }
                    "template" => {
                        let kids = match template_contents(h) {
                            Some(c) => self.convert_list(&c.children.borrow(), in_pre)?,
                            None => self.convert_children(h)?,
                        };
                        Ok(kids)
                    }
                    _ => {
                        let inner_pre = in_pre || tag == "pre";
                        let kids = self.convert_list(&h.children.borrow(), inner_pre)?;
                        if let Some(component) = component_for(&tag) {
                            let (_, _, rest) = split_attrs(&attrs, true);
                            return Ok(vec![Node::Component {
                                name: component.to_string(),
                                attrs: rest,
                                children: kids,
                            }]);
                        }
                        let (mut classes, id, rest) = split_attrs(&attrs, false);
                        if self.prose && classes.is_empty() {
                            if let Some(c) = prose_classes(&tag, in_pre) {
                                classes = c.split_whitespace().map(str::to_string).collect();
                            }
                        }
                        let kids = if VOID.contains(&tag.as_str()) {
                            Vec::new()
                        } else {
                            kids
                        };
                        Ok(vec![Node::Element {
                            tag,
                            id,
                            classes,
                            attrs: rest,
                            children: kids,
                        }])
                    }
                }
            }
            _ => Ok(Vec::new()),
        }
    }
}

/// Collapses runs of whitespace to one space, like an HTML renderer, but
/// keeps one leading/trailing space so inline neighbours stay separated.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out
}

/// `<Icon name="x"/>` is a start tag in HTML5 (the slash is ignored for
/// non-void elements), which would swallow every following sibling. Rewrite
/// `<Name .../>` to `<Name ...></Name>` for non-void tags, leaving quoted
/// attribute values, comments and raw-text elements alone.
pub(crate) fn expand_self_closing(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 16);
    let mut i = 0;
    let mut raw_until: Option<&'static str> = None;
    while i < b.len() {
        if let Some(end) = raw_until {
            let rest = &src[i..];
            match rest.to_ascii_lowercase().find(end) {
                Some(p) => {
                    out.push_str(&rest[..p]);
                    i += p;
                    raw_until = None;
                }
                None => {
                    out.push_str(rest);
                    break;
                }
            }
            continue;
        }
        if b[i] == b'<' {
            if src[i..].starts_with("<!--") {
                let end = src[i..].find("-->").map(|p| i + p + 3).unwrap_or(src.len());
                out.push_str(&src[i..end]);
                i = end;
                continue;
            }
            // `</Header>`: a capitalised name that is also an HTML element means
            // the component (html5ever would lowercase it into the element).
            if b.get(i + 1) == Some(&b'/') {
                let ns = i + 2;
                let mut j = ns;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-') {
                    j += 1;
                }
                if j > ns && is_capitalised_collision(&src[ns..j]) {
                    out.push_str("</x-");
                    out.push_str(&src[ns..j].to_ascii_lowercase());
                    i = j;
                    continue;
                }
            }
            // Start tag?
            let name_start = i + 1;
            let mut j = name_start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b':') {
                j += 1;
            }
            if j > name_start && b[name_start].is_ascii_alphabetic() {
                let name = &src[name_start..j];
                // Scan to the closing `>` honouring quotes.
                let mut k = j;
                let mut quote: Option<u8> = None;
                while k < b.len() {
                    match (quote, b[k]) {
                        (Some(q), c) if c == q => quote = None,
                        (None, b'"') | (None, b'\'') => quote = Some(b[k]),
                        (None, b'>') => break,
                        _ => {}
                    }
                    k += 1;
                }
                if k >= b.len() {
                    out.push_str(&src[i..]);
                    break;
                }
                let renamed;
                let tag_text = if is_capitalised_collision(name) {
                    renamed = format!("<x-{}{}", name.to_ascii_lowercase(), &src[j..=k]);
                    renamed.as_str()
                } else {
                    &src[i..=k]
                };
                let name = if is_capitalised_collision(name) {
                    &tag_text[1..tag_text
                        .find([' ', '>', '/', '\n', '\t'])
                        .unwrap_or(tag_text.len())]
                } else {
                    name
                };
                let lower = name.to_ascii_lowercase();
                if tag_text.ends_with("/>")
                    && !VOID.contains(&lower.as_str())
                    && !is_foreign_inline(&lower)
                {
                    out.push_str(tag_text[..tag_text.len() - 2].trim_end_matches(' '));
                    out.push_str("></");
                    out.push_str(name);
                    out.push('>');
                } else {
                    out.push_str(tag_text);
                }
                if lower == "style" && !tag_text.ends_with("/>") {
                    raw_until = Some("</style");
                } else if lower == "script" && !tag_text.ends_with("/>") {
                    raw_until = Some("</script");
                }
                i = k + 1;
                continue;
            }
        }
        // Copy one char (UTF-8 safe).
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `Header`, `Table`, `Input`...: written with a capital, a name that is also
/// an HTML element means the component, not the element.
fn is_capitalised_collision(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && HTML_COLLISIONS.contains(&name.to_ascii_lowercase().as_str())
}

/// SVG shape tags are self-closing by convention and html5ever handles
/// `<path/>` natively inside `<svg>`; leave them untouched.
fn is_foreign_inline(tag: &str) -> bool {
    matches!(
        tag,
        "path" | "circle" | "rect" | "line" | "polyline" | "polygon" | "ellipse" | "stop" | "use"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Document {
        parse_html(src, false).unwrap()
    }

    #[test]
    fn plain_elements_keep_classes_ids_and_attrs() {
        let doc = parse(
            r#"<div id="a" class="p-4 text-lg" style="color:red"><p>Hi <b>there</b></p></div>"#,
        );
        let Node::Element {
            tag,
            id,
            classes,
            attrs,
            children,
        } = &doc[0]
        else {
            panic!()
        };
        assert_eq!(tag, "div");
        assert_eq!(id.as_deref(), Some("a"));
        assert_eq!(classes, &["p-4", "text-lg"]);
        assert_eq!(attrs[0].name, "style");
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn components_are_matched_case_insensitively_and_self_close_safely() {
        let doc = parse(r#"<Icon name="camera"/><p>after</p><x-header title="T"></x-header>"#);
        assert!(matches!(&doc[0], Node::Component { name, .. } if name == "Icon"));
        // The self-closing component must not swallow the paragraph.
        assert!(matches!(&doc[1], Node::Element { tag, .. } if tag == "p"));
        assert!(matches!(&doc[2], Node::Component { name, .. } if name == "Header"));
    }

    #[test]
    fn a_capitalised_colliding_tag_is_the_component() {
        let doc = parse("<Header title=\"T\"></Header><header>h</header>");
        assert!(matches!(&doc[0], Node::Component { name, .. } if name == "Header"));
        // Same name, lowercase in the source: still the HTML element.
        assert!(matches!(&doc[1], Node::Element { tag, .. } if tag == "header"));
    }

    #[test]
    fn bare_header_is_html_but_prefixed_is_component() {
        let doc = parse("<header>x</header>");
        assert!(matches!(&doc[0], Node::Element { tag, .. } if tag == "header"));
        let doc = parse("<pdfcn-header>x</pdfcn-header>");
        assert!(matches!(&doc[0], Node::Component { name, .. } if name == "Header"));
    }

    #[test]
    fn whole_expression_attribute_on_a_component_is_an_expr() {
        let doc = parse(r#"<InvoiceTable rows="{{ items }}" title="Hi {{ n }}"></InvoiceTable>"#);
        let Node::Component { attrs, .. } = &doc[0] else {
            panic!()
        };
        assert_eq!(attrs[0].value, AttrValue::Expr("items".into()));
        assert_eq!(attrs[1].value, AttrValue::Literal("Hi {{ n }}".into()));
    }

    #[test]
    fn directives_lower_to_for_and_if_chains() {
        let doc = parse(
            r#"<ul><li data-for="x in xs" data-if="x.ok">{{ x.n }}</li></ul>
               <p data-if="a">A</p><p data-elif="b">B</p><p data-else>C</p>"#,
        );
        let Node::Element { children, .. } = &doc[0] else {
            panic!()
        };
        assert!(
            matches!(&children[0], Node::For { binding, iterable, .. } if binding == "x" && iterable == "xs")
        );
        let Node::If {
            branches,
            else_body,
        } = &doc[1]
        else {
            panic!()
        };
        assert_eq!(branches.len(), 2);
        assert!(else_body.is_some());
        assert_eq!(doc.len(), 2);
    }

    #[test]
    fn dangling_else_is_an_error() {
        assert!(parse_html("<p data-else>x</p>", false).is_err());
    }

    #[test]
    fn full_documents_keep_styles_and_body_wrapper_classes() {
        let doc = parse(
            "<!doctype html><html><head><title>t</title><style>.a{color:red}</style></head>\
             <body class=\"bg-slate-50\"><p>x</p></body></html>",
        );
        assert!(matches!(&doc[0], Node::Element { tag, .. } if tag == "style"));
        let Node::Element { tag, classes, .. } = &doc[1] else {
            panic!()
        };
        assert_eq!(tag, "div");
        assert_eq!(classes, &["bg-slate-50"]);
    }

    #[test]
    fn scripts_and_comments_are_dropped() {
        let doc = parse("<!-- c --><script>alert(1)</script><p>x</p>");
        assert_eq!(doc.len(), 1);
    }

    #[test]
    fn template_groups_without_a_wrapper_and_include_and_set_work() {
        let doc = parse(
            r#"<template data-for="x in xs"><p>{{ x }}</p><hr></template><include src="a.html"/><set name="n" value="1 + 2"/>"#,
        );
        assert!(matches!(&doc[0], Node::For { body, .. } if body.len() == 2));
        assert!(matches!(&doc[1], Node::Include { path } if path == "a.html"));
        assert!(matches!(&doc[2], Node::Set { name, .. } if name == "n"));
    }

    #[test]
    fn expand_leaves_quoted_slashes_and_void_tags_alone() {
        let out = expand_self_closing(r#"<a href="x/>y"/><img src="a.png"/><br/><Icon n="1"/>"#);
        assert!(out.contains(r#"<a href="x/>y"></a>"#), "{out}");
        assert!(out.contains(r#"<img src="a.png"/>"#), "{out}");
        assert!(out.contains("<Icon n=\"1\"></Icon>"), "{out}");
    }

    #[test]
    fn prose_defaults_apply_only_without_a_class() {
        let doc = parse_html("<h1>A</h1><h2 class=\"x\">B</h2>", true).unwrap();
        let Node::Element { classes, .. } = &doc[0] else {
            panic!()
        };
        assert!(classes.contains(&"font-bold".to_string()));
        let Node::Element { classes, .. } = &doc[1] else {
            panic!()
        };
        assert_eq!(classes, &["x"]);
    }
}
