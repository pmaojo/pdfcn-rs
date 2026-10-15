use maud::{html, Markup};
use pdfcn_template::Resolved;

use crate::CoreError;

/// Renders a resolved document tree to `maud` markup, expanding component
/// nodes through the `pdfcn-components` registry (FR-2). An unknown
/// component name is rendered as an inert, visibly-marked placeholder
/// rather than silently dropped; a deliberately-unsupported, interactive-only
/// component (Dialog, Tooltip, ...) is rejected with an explicit error
/// instead.
pub fn render_body(nodes: &[Resolved]) -> Result<Markup, CoreError> {
    let mut rendered = Vec::with_capacity(nodes.len());
    for node in nodes {
        rendered.push(render_node(node)?);
    }
    Ok(html! {
        @for node in &rendered {
            (node)
        }
    })
}

/// Tags that flow inside a line of text.
const INLINE_TAGS: [&str; 14] = [
    "a", "span", "b", "strong", "i", "em", "u", "s", "small", "sub", "sup", "code", "mark", "abbr",
];

fn is_inline_flow(node: &Resolved) -> bool {
    match node {
        Resolved::Text(_) => true,
        Resolved::Element { tag, .. } => INLINE_TAGS.contains(&tag.as_str()),
        Resolved::Component { .. } => false,
    }
}

/// Whether two adjacent siblings share a line and so need a space between
/// them (text and inline elements in any combination). No space is added
/// where the text already has one, or where punctuation hugs the previous
/// word (`<a>link</a>.`), which HTML sources rely on.
fn needs_space(prev: &Resolved, next: &Resolved) -> bool {
    if !(is_inline_flow(prev) && is_inline_flow(next)) {
        return false;
    }
    if let Resolved::Text(t) = prev {
        if t.ends_with(char::is_whitespace) || t.ends_with(['(', '[', '\u{a0}']) {
            return false;
        }
    }
    if let Resolved::Text(t) = next {
        if t.starts_with(char::is_whitespace)
            || t.starts_with(['.', ',', ';', ':', '!', '?', ')', ']', '%'])
        {
            return false;
        }
    }
    true
}

/// Moves every `<style>...</style>` block out of rendered body markup:
/// returns the body without them and their concatenated CSS, to be placed in
/// the document stylesheet. Author CSS may sit anywhere in a template (HTML
/// and Markdown sources put it where they like).
pub fn hoist_styles(body: &str) -> (String, String) {
    let mut out = String::with_capacity(body.len());
    let mut css = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("<style>") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "<style>".len()..];
        match after.find("</style>") {
            Some(end) => {
                css.push_str(&unescape_css(&after[..end]));
                rest = &after[end + "</style>".len()..];
            }
            None => {
                rest = &rest[start..];
                break;
            }
        }
    }
    out.push_str(rest);
    (out, css)
}

/// The renderer HTML-escapes text, which would turn a `>` combinator or a
/// quoted `content` string in author CSS into entities.
fn unescape_css(s: &str) -> String {
    s.replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn render_node(node: &Resolved) -> Result<Markup, CoreError> {
    Ok(match node {
        Resolved::Text(text) => html! { (text) },
        Resolved::Element {
            tag,
            id,
            classes,
            attrs,
            children,
        } => {
            let class_attr = classes.join(" ");
            let mut rendered_children = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                // HAML separates sibling lines with a newline, which HTML
                // reads as one space: keep "Visit" and `%a link` apart.
                if i > 0 && needs_space(&children[i - 1], child) {
                    rendered_children.push(html! { " " });
                }
                rendered_children.push(render_node(child)?);
            }
            html! {
                (maud::PreEscaped(format!("<{tag}")))
                @if let Some(id) = id {
                    (maud::PreEscaped(" id=\""))(id)(maud::PreEscaped("\""))
                }
                @if !classes.is_empty() {
                    (maud::PreEscaped(" class=\""))(class_attr)(maud::PreEscaped("\""))
                }
                @for a in attrs {
                    (maud::PreEscaped(format!(" {}=\"", a.name)))(a.value)(maud::PreEscaped("\""))
                }
                (maud::PreEscaped(">"))
                @for child in &rendered_children {
                    (child)
                }
                (maud::PreEscaped(format!("</{tag}>")))
            }
        }
        Resolved::Component {
            name,
            attrs,
            children,
        } => {
            let inner = render_body(children)?;
            match pdfcn_components::render(name, attrs, inner) {
                Ok(Some(markup)) => markup,
                Ok(None) => html! {
                    div class="pdfcn-unknown-component" data-component=(name) {
                        "Unknown component: " (name)
                    }
                },
                Err(rejected) => return Err(CoreError::Render(rejected.to_string())),
            }
        }
    })
}

/// Wraps a rendered body (as an already-rendered HTML string — the shape
/// [`crate::render_html`] works in once the gap-rewrite pass has
/// post-processed the markup) and a stylesheet into a complete HTML document.
pub fn wrap_document_str(body: &str, stylesheet: &str) -> String {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                // Embedded fonts (see pdfcn_core::BUILTIN_FONTS) are registered
                // under this exact family name; the generic `sans-serif`
                // fallback has nothing to resolve to on a host with no system
                // fonts installed (e.g. a serverless Lambda container).
                style { (maud::PreEscaped(format!("body{{font-family:'{}',sans-serif}}", crate::DEFAULT_FONT_FAMILY))) (maud::PreEscaped(stylesheet)) }
            }
            body {
                (maud::PreEscaped(body.to_string()))
            }
        }
    }
    .into_string()
}
