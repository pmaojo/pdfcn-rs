use serde::{Deserialize, Serialize};

/// An attribute value: either a string literal or a `{{ expr }}` binding
/// evaluated against the template context at render time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttrValue {
    Literal(String),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    pub name: String,
    pub value: AttrValue,
}

/// A parsed node of the document tree. Text interpolation (`{{ ... }}`)
/// inside `Text` nodes is left as raw source; `pdfcn-template` resolves it
/// against the data context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Node {
    /// A plain HTML element, e.g. `%table.w-full#invoice(role="table")`.
    Element {
        tag: String,
        id: Option<String>,
        classes: Vec<String>,
        attrs: Vec<Attr>,
        children: Vec<Node>,
    },
    /// A first-class UI component, e.g. `%InvoiceTable(rows={{ items }})`.
    /// Distinguished from `Element` by an uppercase first letter after `%`.
    Component {
        name: String,
        attrs: Vec<Attr>,
        children: Vec<Node>,
    },
    /// Literal text content, which may embed `{{ expr }}` interpolations.
    Text(String),
    /// `- for item in items`
    For {
        binding: String,
        iterable: String,
        body: Vec<Node>,
    },
    /// `- if cond` / `- elif cond` / `- else`
    If {
        branches: Vec<(String, Vec<Node>)>,
        else_body: Option<Vec<Node>>,
    },
    /// `- include "partials/footer.haml"`
    Include { path: String },
}

pub type Document = Vec<Node>;
