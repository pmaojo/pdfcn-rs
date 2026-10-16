//! Evaluates a [`pdfcn_parser::Document`] against a JSON data context:
//! resolves `{{ expr }}` interpolation, `- for` / `- if` control flow and
//! `- include` partials, using `minijinja` purely as an expression engine
//! (no `.jinja` template files, no I/O of its own).

use std::borrow::Cow;
use std::fmt;

use minijinja::Environment;
use pdfcn_parser::{Attr, AttrValue, Document, Node};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAttr {
    pub name: String,
    pub value: String,
}

/// The AST after data resolution: control-flow nodes are gone (`For`/`If`
/// have been expanded/selected), interpolations are resolved to plain
/// strings. Escaping for the HTML/PDF output happens downstream, in
/// whichever renderer consumes this (e.g. `maud`'s auto-escaping), so the
/// strings here are intentionally raw.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Element {
        tag: String,
        id: Option<String>,
        classes: Vec<String>,
        attrs: Vec<ResolvedAttr>,
        children: Vec<Resolved>,
    },
    Component {
        name: String,
        attrs: Vec<ResolvedAttr>,
        children: Vec<Resolved>,
    },
    Text(String),
}

/// Resolves `- include "path"` targets. `pdfcn-template` does no I/O of its
/// own; the host (`pdfcn-core`) supplies partials however it sees fit
/// (filesystem, embedded assets, ...).
pub trait PartialLoader {
    fn load(&self, path: &str) -> Result<Document, EvalError>;
}

/// A loader that rejects every `- include`, for contexts with no partials.
pub struct NoPartials;

impl PartialLoader for NoPartials {
    fn load(&self, path: &str) -> Result<Document, EvalError> {
        Err(EvalError::PartialNotFound(path.to_string()))
    }
}

#[derive(Debug)]
pub enum EvalError {
    Expr {
        expr: String,
        source: minijinja::Error,
    },
    NotIterable {
        binding: String,
        iterable: String,
    },
    PartialNotFound(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Expr { expr, source } => {
                write!(f, "expression error in '{expr}': {source}")
            }
            EvalError::NotIterable { binding, iterable } => write!(
                f,
                "'- for {binding} in {iterable}' did not evaluate to a list"
            ),
            EvalError::PartialNotFound(path) => {
                write!(f, "partial '{path}' could not be loaded")
            }
        }
    }
}

impl std::error::Error for EvalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EvalError::Expr { source, .. } => Some(source),
            EvalError::NotIterable { .. } | EvalError::PartialNotFound(_) => None,
        }
    }
}

fn is_truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        JsonValue::String(s) => !s.is_empty(),
        JsonValue::Array(a) => !a.is_empty(),
        JsonValue::Object(o) => !o.is_empty(),
    }
}

fn display_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The variables an expression can see: the caller's context, never copied,
/// under a stack of frames holding what the template itself bound (`- for`
/// items, `loop`, `- set`). Entering a loop iteration pushes one frame with
/// its two bindings and leaving it pops that frame, so an iteration costs
/// O(1) however large the context is -- it used to clone the whole context
/// object once per iteration. Lookups walk the frames innermost first, so an
/// inner binding shadows an outer one (and the context) exactly as the old
/// merged copy did.
struct Scope<'a> {
    base: &'a JsonValue,
    /// Innermost last. Frame 0 belongs to the top level of the document.
    frames: Vec<Vec<(String, JsonValue)>>,
}

impl<'a> Scope<'a> {
    fn new(base: &'a JsonValue) -> Self {
        Self {
            base,
            frames: vec![Vec::new()],
        }
    }

    fn get(&self, name: &str) -> Option<&JsonValue> {
        for frame in self.frames.iter().rev() {
            if let Some((_, value)) = frame.iter().rev().find(|(k, _)| k == name) {
                return Some(value);
            }
        }
        self.base.get(name)
    }

    /// Binds `name` in the innermost frame (`- set`), replacing an earlier
    /// binding of the same name there. A top-level `- set` over a context that
    /// is not an object has nowhere to go and is dropped -- like an unknown
    /// utility class this degrades rather than fails the render (and is what
    /// binding into a non-object context always did).
    fn set(&mut self, name: &str, value: JsonValue) {
        if self.frames.len() == 1 && !self.base.is_object() {
            return;
        }
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        match frame.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value,
            None => frame.push((name.to_string(), value)),
        }
    }

    /// Only the root variables `names` mentions, as a minijinja context: what
    /// an expression handed to minijinja can reach, without serializing the
    /// whole context for each one.
    fn minijinja_context<'n>(
        &self,
        names: impl IntoIterator<Item = &'n String>,
    ) -> minijinja::Value {
        let map: serde_json::Map<String, JsonValue> = names
            .into_iter()
            .filter_map(|name| Some((name.clone(), self.get(name)?.clone())))
            .collect();
        minijinja::Value::from_serialize(&map)
    }
}

/// Resolves an expression that is a plain variable path -- `title`,
/// `p.name`, `rows[0].total` -- straight against the scope. Most template
/// expressions are exactly that, and handing each one to minijinja meant
/// serializing the context into minijinja values first: a 100-product catalog
/// paid for 100 products once per `{{ p.name }}`. Returns `None` for anything
/// else, and for any path that does not resolve, so minijinja keeps the final
/// word on every case this does not cover.
fn eval_path<'s>(expr: &str, scope: &'s Scope<'_>) -> Option<&'s JsonValue> {
    let bytes = expr.as_bytes();
    let ident_end = |from: usize| {
        let mut i = from;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        (i > from && !bytes[from].is_ascii_digit()).then_some(i)
    };
    let end = ident_end(0)?;
    let root = &expr[..end];
    // Literals minijinja reads as constants, never as variables.
    if matches!(root, "true" | "false" | "none" | "True" | "False" | "None") {
        return None;
    }
    let mut value = scope.get(root)?;
    let mut i = end;
    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                let end = ident_end(i + 1)?;
                value = value.as_object()?.get(&expr[i + 1..end])?;
                i = end;
            }
            b'[' => {
                let close = i + 1 + expr[i + 1..].find(']')?;
                let index: usize = expr[i + 1..close].parse().ok()?;
                value = value.as_array()?.get(index)?;
                i = close + 1;
            }
            _ => return None,
        }
    }
    Some(value)
}

fn eval_expr<'s>(
    expr: &str,
    scope: &'s Scope<'_>,
    env: &Environment,
) -> Result<Cow<'s, JsonValue>, EvalError> {
    if let Some(value) = eval_path(expr, scope) {
        return Ok(Cow::Borrowed(value));
    }
    let compiled = env
        .compile_expression(expr)
        .map_err(|source| EvalError::Expr {
            expr: expr.to_string(),
            source,
        })?;
    let ctx = scope.minijinja_context(&compiled.undeclared_variables(false));
    let result = compiled.eval(ctx).map_err(|source| EvalError::Expr {
        expr: expr.to_string(),
        source,
    })?;
    Ok(Cow::Owned(
        serde_json::to_value(result).unwrap_or(JsonValue::Null),
    ))
}

/// Replaces every `{{ expr }}` span in `text` with its evaluated,
/// stringified value.
fn interpolate(text: &str, scope: &Scope<'_>, env: &Environment) -> Result<String, EvalError> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let expr = after[..end].trim();
        let value = eval_expr(expr, scope, env)?;
        out.push_str(&display_value(&value));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_attrs(
    attrs: &[Attr],
    scope: &Scope<'_>,
    env: &Environment,
) -> Result<Vec<ResolvedAttr>, EvalError> {
    attrs
        .iter()
        .map(|a| {
            let value = match &a.value {
                AttrValue::Literal(s) => interpolate(s, scope, env)?,
                AttrValue::Expr(e) => display_value(&*eval_expr(e, scope, env)?),
            };
            Ok(ResolvedAttr {
                name: a.name.clone(),
                value,
            })
        })
        .collect()
}

/// The per-iteration `loop` binding exposed to `- for` bodies, mirroring
/// Jinja's `loop` variable: `{{ loop.index1 }}` (1-based), `{{ loop.index0 }}`,
/// `{{ loop.first }}`, `{{ loop.last }}` and `{{ loop.length }}`.
fn loop_binding(index: usize, length: usize) -> JsonValue {
    serde_json::json!({
        "index0": index,
        "index1": index + 1,
        "first": index == 0,
        "last": index + 1 == length,
        "length": length,
    })
}

/// Evaluates a node list in `scope`. Takes `&mut` because `- set` binds a
/// value for every node after it; a loop iteration gets a frame of its own,
/// so a `- set` inside it is scoped to that iteration -- matching Jinja's
/// scoping. An included partial shares its includer's frame.
fn evaluate_nodes(
    nodes: &[Node],
    scope: &mut Scope<'_>,
    env: &Environment,
    loader: &dyn PartialLoader,
) -> Result<Vec<Resolved>, EvalError> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Resolved::Text(interpolate(text, scope, env)?)),
            Node::Element {
                tag,
                id,
                classes,
                attrs,
                children,
            } => out.push(Resolved::Element {
                tag: tag.clone(),
                id: id.clone(),
                classes: classes.clone(),
                attrs: resolve_attrs(attrs, scope, env)?,
                children: evaluate_nodes(children, scope, env, loader)?,
            }),
            Node::Component {
                name,
                attrs,
                children,
            } => out.push(Resolved::Component {
                name: name.clone(),
                attrs: resolve_attrs(attrs, scope, env)?,
                children: evaluate_nodes(children, scope, env, loader)?,
            }),
            Node::For {
                binding,
                iterable,
                body,
            } => {
                // One copy of the list per loop (the items then move into
                // their frames); never one copy of the context per item.
                let seq = eval_expr(iterable, scope, env)?.into_owned();
                let JsonValue::Array(items) = seq else {
                    return Err(EvalError::NotIterable {
                        binding: binding.clone(),
                        iterable: iterable.clone(),
                    });
                };
                let length = items.len();
                for (index, item) in items.into_iter().enumerate() {
                    scope.frames.push(vec![
                        (binding.clone(), item),
                        ("loop".to_string(), loop_binding(index, length)),
                    ]);
                    let body = evaluate_nodes(body, scope, env, loader);
                    scope.frames.pop();
                    out.extend(body?);
                }
            }
            Node::If {
                branches,
                else_body,
            } => {
                let mut matched = false;
                for (cond, body) in branches {
                    if is_truthy(&*eval_expr(cond, scope, env)?) {
                        out.extend(evaluate_nodes(body, scope, env, loader)?);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    if let Some(body) = else_body {
                        out.extend(evaluate_nodes(body, scope, env, loader)?);
                    }
                }
            }
            Node::Include { path } => {
                let partial = loader.load(path)?;
                out.extend(evaluate_nodes(&partial, scope, env, loader)?);
            }
            Node::Set { name, expr } => {
                let value = eval_expr(expr, scope, env)?.into_owned();
                scope.set(name, value);
            }
        }
    }
    Ok(out)
}

/// Evaluates `document` against `context`, resolving all interpolation and
/// control flow. `loader` supplies `- include` partials.
pub fn evaluate(
    document: &Document,
    context: &JsonValue,
    loader: &dyn PartialLoader,
) -> Result<Vec<Resolved>, EvalError> {
    let env = Environment::new();
    // `- set` binds into the scope's own frames; the caller's value is only
    // ever read.
    let mut scope = Scope::new(context);
    evaluate_nodes(document, &mut scope, &env, loader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcn_parser::parse_document;

    /// The path fast path must agree with minijinja on everything it
    /// answers, and decline (None) everything else.
    #[test]
    fn plain_paths_resolve_like_minijinja() {
        let ctx = serde_json::json!({
            "title": "T",
            "n": 1.5,
            "big": 12345678901234u64,
            "p": {"name": "Ana", "tags": ["a", "b"], "nested": {"x": null}},
            "rows": [[{"total": 3}]],
        });
        let env = Environment::new();
        let via_minijinja = |expr: &str| {
            let compiled = env.compile_expression(expr).unwrap();
            serde_json::to_value(
                compiled
                    .eval(minijinja::Value::from_serialize(&ctx))
                    .unwrap(),
            )
            .unwrap_or(JsonValue::Null)
        };
        for expr in [
            "title",
            "n",
            "big",
            "p",
            "p.name",
            "p.tags",
            "p.tags[1]",
            "p.nested.x",
            "rows[0][0].total",
        ] {
            assert_eq!(
                eval_path(expr, &Scope::new(&ctx)).cloned(),
                Some(via_minijinja(expr)),
                "{expr}"
            );
        }
        for expr in [
            "missing",
            "p.missing",
            "p.tags[9]",
            "title | upper",
            "n + 1",
            "true",
            "none",
            "p['name']",
            "1abc",
            "",
        ] {
            assert_eq!(eval_path(expr, &Scope::new(&ctx)), None, "{expr}");
        }
    }
    use serde_json::json;

    #[test]
    fn interpolates_nested_field() {
        let doc = parse_document("%p Hello {{ user.name }}!").unwrap();
        let ctx = json!({ "user": { "name": "Ada Lovelace" } });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        assert_eq!(
            resolved,
            vec![Resolved::Element {
                tag: "p".into(),
                id: None,
                classes: vec![],
                attrs: vec![],
                children: vec![Resolved::Text("Hello Ada Lovelace!".into())],
            }]
        );
    }

    #[test]
    fn for_loop_expands_once_per_item() {
        let doc = parse_document("- for item in items\n  %li {{ item }}").unwrap();
        let ctx = json!({ "items": ["a", "b", "c"] });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        assert_eq!(resolved.len(), 3);
    }

    /// `- for` exposes Jinja's `loop` variable so bodies can render row
    /// numbers, zebra decisions and last-item special cases without the
    /// caller precomputing indexes into the data.
    #[test]
    fn for_loop_exposes_loop_metadata() {
        let doc = parse_document(concat!(
            "- for item in items\n",
            "  %li(class=\"row\") #{{ loop.index1 }}/{{ loop.length }} {{ item }}",
        ))
        .unwrap();
        let ctx = json!({ "items": ["a", "b"] });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        let text_of = |r: &Resolved| match r {
            Resolved::Element { children, .. } => match &children[0] {
                Resolved::Text(t) => t.clone(),
                other => panic!("expected Text, got {other:?}"),
            },
            other => panic!("expected Element, got {other:?}"),
        };
        assert_eq!(text_of(&resolved[0]), "#1/2 a");
        assert_eq!(text_of(&resolved[1]), "#2/2 b");
    }

    #[test]
    fn for_loop_first_and_last_flags() {
        let doc =
            parse_document("- for x in xs\n  %li\n    - if loop.first\n      %span FIRST").unwrap();
        let ctx = json!({ "xs": [1, 2] });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        // Only the first iteration emits the FIRST span.
        let span_count = resolved
            .iter()
            .filter(|r| {
                matches!(r, Resolved::Element { children, .. }
                if children.iter().any(|c| matches!(c,
                    Resolved::Element { tag, .. } if tag == "span")))
            })
            .count();
        assert_eq!(span_count, 1);
    }

    /// `- set` binds an expression into the context for subsequent siblings.
    #[test]
    fn set_binds_a_value_for_later_nodes() {
        let doc = parse_document(concat!(
            "- set total = price * qty\n",
            "%p Total: {{ total }}",
        ))
        .unwrap();
        let ctx = json!({ "price": 12, "qty": 4 });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        match &resolved[0] {
            Resolved::Element { children, .. } => {
                assert_eq!(children[0], Resolved::Text("Total: 48".into()));
            }
            other => panic!("expected Element, got {other:?}"),
        }
    }

    /// minijinja filters were always reachable through the expression engine;
    /// pin that they work through both interpolation and attributes.
    #[test]
    fn minijinja_filters_work_in_interpolations() {
        let doc = parse_document("%p {{ name | upper }} has {{ items | length }} items").unwrap();
        let ctx = json!({ "name": "ada", "items": [1, 2, 3] });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        match &resolved[0] {
            Resolved::Element { children, .. } => {
                assert_eq!(children[0], Resolved::Text("ADA has 3 items".into()));
            }
            other => panic!("expected Element, got {other:?}"),
        }
    }

    fn texts(resolved: &[Resolved]) -> Vec<String> {
        resolved
            .iter()
            .map(|r| match r {
                Resolved::Element { children, .. } => match &children[0] {
                    Resolved::Text(t) => t.clone(),
                    other => panic!("expected Text, got {other:?}"),
                },
                other => panic!("expected Element, got {other:?}"),
            })
            .collect()
    }

    /// A loop binding shadows a context key of the same name inside the body
    /// only; the context value is back after the loop.
    #[test]
    fn a_loop_binding_shadows_the_context_only_inside_the_loop() {
        let doc = parse_document(
            "- for x in xs
  %p {{ x }}
%p {{ x }}",
        )
        .unwrap();
        let ctx = json!({ "x": "outer", "xs": ["a", "b"] });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        assert_eq!(texts(&resolved), ["a", "b", "outer"]);
    }

    /// Nested loops see the outer item, the inner `loop` shadows the outer
    /// one, and expressions minijinja evaluates (filters, arithmetic) see the
    /// same bindings as plain paths do.
    #[test]
    fn nested_loops_and_minijinja_expressions_see_every_frame() {
        let doc = parse_document(concat!(
            "- for row in rows
",
            "  - for cell in row.cells
",
            "    %p {{ row.name | upper }}-{{ cell * 10 }}-{{ loop.index1 }}/{{ loop.length }}-{{ title }}",
        ))
        .unwrap();
        let ctx = json!({ "title": "T", "rows": [
            { "name": "a", "cells": [1, 2] },
            { "name": "b", "cells": [3] },
        ]});
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        assert_eq!(texts(&resolved), ["A-10-1/2-T", "A-20-2/2-T", "B-30-1/1-T"]);
    }

    /// `- set` inside an iteration is scoped to that iteration; a top-level
    /// `- set` shadows the context for the rest of the document, and the
    /// caller's context is never modified.
    #[test]
    fn set_is_scoped_like_jinja_and_never_touches_the_callers_context() {
        let doc = parse_document(concat!(
            "- set label = \"top\"\n",
            "- for x in xs\n",
            "  - set label = x ~ \"!\"\n",
            "  %p {{ label }}\n",
            "%p {{ label }}",
        ))
        .unwrap();
        let ctx = json!({ "label": "ctx", "xs": ["a", "b"] });
        let before = ctx.clone();
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        assert_eq!(texts(&resolved), ["a!", "b!", "top"]);
        assert_eq!(ctx, before);
    }

    /// A context that is not an object still renders: loops bind their item,
    /// and a top-level `- set` is dropped as it always was.
    #[test]
    fn a_non_object_context_degrades_like_before() {
        let doc = parse_document("- set y = 1\n%p [{{ y }}]").unwrap();
        let resolved = evaluate(&doc, &json!([1, 2]), &NoPartials).unwrap();
        assert_eq!(texts(&resolved), ["[]"]);
    }

    #[test]
    fn if_else_picks_the_right_branch() {
        let doc = parse_document("- if active\n  %span Active\n- else\n  %span Inactive").unwrap();
        let ctx = json!({ "active": false });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        match &resolved[0] {
            Resolved::Element { children, .. } => {
                assert_eq!(children[0], Resolved::Text("Inactive".into()));
            }
            other => panic!("expected Element, got {other:?}"),
        }
    }

    #[test]
    fn component_attrs_resolve_expressions() {
        let doc = parse_document("%Badge(variant=\"destructive\" count={{ n }})").unwrap();
        let ctx = json!({ "n": 3 });
        let resolved = evaluate(&doc, &ctx, &NoPartials).unwrap();
        match &resolved[0] {
            Resolved::Component { name, attrs, .. } => {
                assert_eq!(name, "Badge");
                assert_eq!(attrs[0].value, "destructive");
                assert_eq!(attrs[1].value, "3");
            }
            other => panic!("expected Component, got {other:?}"),
        }
    }
}
