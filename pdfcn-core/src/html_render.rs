use maud::{html, Markup};
use pdfcn_template::Resolved;

/// Renders a resolved document tree to `maud` markup, expanding component
/// nodes through the `pdfcn-components` registry (FR-2). An unknown
/// component name is rendered as an inert, visibly-marked placeholder
/// rather than silently dropped.
pub fn render_body(nodes: &[Resolved]) -> Markup {
    html! {
        @for node in nodes {
            (render_node(node))
        }
    }
}

fn render_node(node: &Resolved) -> Markup {
    match node {
        Resolved::Text(text) => html! { (text) },
        Resolved::Element {
            tag,
            id,
            classes,
            attrs,
            children,
        } => {
            let class_attr = classes.join(" ");
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
                @for child in children {
                    (render_node(child))
                }
                (maud::PreEscaped(format!("</{tag}>")))
            }
        }
        Resolved::Component {
            name,
            attrs,
            children,
        } => {
            let inner = render_body(children);
            pdfcn_components::render(name, attrs, inner).unwrap_or_else(|| {
                html! {
                    div class="pdfcn-unknown-component" data-component=(name) {
                        "Unknown component: " (name)
                    }
                }
            })
        }
    }
}

/// Wraps a rendered body and a stylesheet into a complete HTML document.
pub fn wrap_document(body: &Markup, stylesheet: &str) -> String {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                // Embedded fonts (see pdfcn_core::DEFAULT_FONT) are registered under
                // this exact family name; the generic `sans-serif` fallback has
                // nothing to resolve to on a host with no system fonts.
                style { "body{font-family:'DejaVu Sans',sans-serif}" (maud::PreEscaped(stylesheet)) }
            }
            body {
                (body)
            }
        }
    }
    .into_string()
}
