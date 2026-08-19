//! Evaluates a [`pdfcn_parser::Document`] against a JSON data context:
//! resolves `{{ expr }}` interpolation, `- for` / `- if` control flow and
//! `- include` partials, using `minijinja` purely as an expression engine
//! (no `.jinja` template files, no I/O of its own).

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

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("expression error in '{expr}': {source}")]
    Expr {
        expr: String,
        #[source]
        source: minijinja::Error,
    },
    #[error("'- for {binding} in {iterable}' did not evaluate to a list")]
    NotIterable { binding: String, iterable: String },
    #[error("partial '{0}' could not be loaded")]
    PartialNotFound(String),
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

fn eval_expr(expr: &str, ctx: &JsonValue, env: &Environment) -> Result<JsonValue, EvalError> {
    let mv_ctx = minijinja::Value::from_serialize(ctx);
    let compiled = env
        .compile_expression(expr)
        .map_err(|source| EvalError::Expr {
            expr: expr.to_string(),
            source,
        })?;
    let result = compiled.eval(mv_ctx).map_err(|source| EvalError::Expr {
        expr: expr.to_string(),
        source,
    })?;
    Ok(serde_json::to_value(result).unwrap_or(JsonValue::Null))
}

/// Replaces every `{{ expr }}` span in `text` with its evaluated,
/// stringified value.
fn interpolate(text: &str, ctx: &JsonValue, env: &Environment) -> Result<String, EvalError> {
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
        let value = eval_expr(expr, ctx, env)?;
        out.push_str(&display_value(&value));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_attrs(
    attrs: &[Attr],
    ctx: &JsonValue,
    env: &Environment,
) -> Result<Vec<ResolvedAttr>, EvalError> {
    attrs
        .iter()
        .map(|a| {
            let value = match &a.value {
                AttrValue::Literal(s) => interpolate(s, ctx, env)?,
                AttrValue::Expr(e) => display_value(&eval_expr(e, ctx, env)?),
            };
            Ok(ResolvedAttr {
                name: a.name.clone(),
                value,
            })
        })
        .collect()
}

fn with_binding(ctx: &JsonValue, binding: &str, item: JsonValue) -> JsonValue {
    match ctx {
        JsonValue::Object(map) => {
            let mut merged = map.clone();
            merged.insert(binding.to_string(), item);
            JsonValue::Object(merged)
        }
        _ => {
            let mut merged = serde_json::Map::new();
            merged.insert(binding.to_string(), item);
            JsonValue::Object(merged)
        }
    }
}

fn evaluate_nodes(
    nodes: &[Node],
    ctx: &JsonValue,
    env: &Environment,
    loader: &dyn PartialLoader,
) -> Result<Vec<Resolved>, EvalError> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Resolved::Text(interpolate(text, ctx, env)?)),
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
                attrs: resolve_attrs(attrs, ctx, env)?,
                children: evaluate_nodes(children, ctx, env, loader)?,
            }),
            Node::Component {
                name,
                attrs,
                children,
            } => out.push(Resolved::Component {
                name: name.clone(),
                attrs: resolve_attrs(attrs, ctx, env)?,
                children: evaluate_nodes(children, ctx, env, loader)?,
            }),
            Node::For {
                binding,
                iterable,
                body,
            } => {
                let seq = eval_expr(iterable, ctx, env)?;
                let JsonValue::Array(items) = seq else {
                    return Err(EvalError::NotIterable {
                        binding: binding.clone(),
                        iterable: iterable.clone(),
                    });
                };
                for item in items {
                    let child_ctx = with_binding(ctx, binding, item);
                    out.extend(evaluate_nodes(body, &child_ctx, env, loader)?);
                }
            }
            Node::If {
                branches,
                else_body,
            } => {
                let mut matched = false;
                for (cond, body) in branches {
                    if is_truthy(&eval_expr(cond, ctx, env)?) {
                        out.extend(evaluate_nodes(body, ctx, env, loader)?);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    if let Some(body) = else_body {
                        out.extend(evaluate_nodes(body, ctx, env, loader)?);
                    }
                }
            }
            Node::Include { path } => {
                let partial = loader.load(path)?;
                out.extend(evaluate_nodes(&partial, ctx, env, loader)?);
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
    evaluate_nodes(document, context, &env, loader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcn_parser::parse_document;
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
