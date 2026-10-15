//! Declarative block trees (JSON or YAML): the format an API client or an LLM
//! emits when it would rather build a document as data than as markup.
//!
//! ```yaml
//! - type: Header
//!   title: "Invoice {{ number }}"
//! - type: div
//!   class: grid grid-cols-2 gap-4        # or a list of classes
//!   children:
//!     - { type: p, text: "Hello {{ name }}" }
//!     - { type: Card, title: Notes, children: ["Plain strings are text"] }
//! - for: item in items
//!   type: li
//!   text: "{{ item }}"
//! ```
//!
//! A node is a string (text) or an object. `type` is the tag, or a component
//! when capitalised (default `div`). Reserved keys: `type`, `class`, `id`,
//! `text`, `children`, `for`, `if`, `elif`, `else`, `include`, `set`+`value`.
//! Every other key is an attribute; numbers and booleans are stringified,
//! arrays and objects become JSON text (the convention components use for
//! structured props, as in `rows`).

use pdfcn_parser::{Attr, AttrValue, Document, Node};
use serde_json::Value;

use crate::{err, FormatError};

pub(crate) fn from_json(source: &str) -> Result<Document, FormatError> {
    let v: Value = serde_json::from_str(source)
        .map_err(|e| err(format!("block tree is not valid JSON: {e}")))?;
    from_value(&v)
}

pub(crate) fn from_yaml(source: &str) -> Result<Document, FormatError> {
    let v: Value = serde_yaml::from_str(source)
        .map_err(|e| err(format!("block tree is not valid YAML: {e}")))?;
    from_value(&v)
}

fn from_value(v: &Value) -> Result<Document, FormatError> {
    match v {
        // `{ children: [...] }` at the top level is just the document body.
        Value::Object(o) if o.contains_key("children") && !o.contains_key("type") => {
            nodes(o.get("children").unwrap())
        }
        other => nodes(other),
    }
}

fn nodes(v: &Value) -> Result<Vec<Node>, FormatError> {
    match v {
        Value::Array(items) => {
            let mut out: Vec<Node> = Vec::new();
            for item in items {
                push_with_directives(&mut out, item)?;
            }
            Ok(out)
        }
        Value::Null => Ok(Vec::new()),
        other => {
            let mut out = Vec::new();
            push_with_directives(&mut out, other)?;
            Ok(out)
        }
    }
}

fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn push_with_directives(out: &mut Vec<Node>, v: &Value) -> Result<(), FormatError> {
    let Value::Object(o) = v else {
        // Strings (and scalars) are text.
        if !v.is_null() {
            out.push(Node::Text(str_of(v)));
        }
        return Ok(());
    };
    let node = build(o)?;
    let truthy = |k: &str| o.get(k).is_some_and(|x| !x.is_null());
    if truthy("elif") || o.get("else").is_some_and(|x| x == &Value::Bool(true)) {
        let Some(Node::If {
            branches,
            else_body,
        }) = out.last_mut()
        else {
            return Err(err("`elif` / `else` must follow a node with `if`"));
        };
        if else_body.is_some() {
            return Err(err("`elif` / `else` after a final `else`"));
        }
        match o.get("elif") {
            Some(c) if !c.is_null() => branches.push((str_of(c), vec![node])),
            _ => *else_body = Some(vec![node]),
        }
        return Ok(());
    }
    let mut node = node;
    if let Some(c) = o.get("if").filter(|c| !c.is_null()) {
        node = Node::If {
            branches: vec![(str_of(c), vec![node])],
            else_body: None,
        };
    }
    if let Some(spec) = o.get("for").filter(|c| !c.is_null()) {
        let spec = str_of(spec);
        let (binding, iterable) = spec
            .split_once(" in ")
            .ok_or_else(|| err(format!("for: \"{spec}\": expected \"item in items\"")))?;
        node = Node::For {
            binding: binding.trim().to_string(),
            iterable: iterable.trim().to_string(),
            body: vec![node],
        };
    }
    out.push(node);
    Ok(())
}

const RESERVED: &[&str] = &[
    "type", "class", "id", "text", "children", "for", "if", "elif", "else", "include", "set",
    "value",
];

fn build(o: &serde_json::Map<String, Value>) -> Result<Node, FormatError> {
    if let Some(path) = o.get("include") {
        return Ok(Node::Include { path: str_of(path) });
    }
    if let Some(name) = o.get("set") {
        let expr = o
            .get("value")
            .map(str_of)
            .ok_or_else(|| err("`set` needs a `value` expression"))?;
        return Ok(Node::Set {
            name: str_of(name),
            expr,
        });
    }
    let ty = o.get("type").map(str_of).unwrap_or_else(|| "div".into());
    let component = ty.chars().next().is_some_and(char::is_uppercase);

    let mut children = Vec::new();
    if let Some(t) = o.get("text").filter(|t| !t.is_null()) {
        children.push(Node::Text(str_of(t)));
    }
    if let Some(c) = o.get("children") {
        children.extend(nodes(c)?);
    }

    let class_value = o.get("class").map(|c| match c {
        Value::Array(items) => items.iter().map(str_of).collect::<Vec<_>>().join(" "),
        other => str_of(other),
    });
    let mut attrs: Vec<Attr> = Vec::new();
    for (k, v) in o {
        if RESERVED.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        let value = match v {
            Value::String(s) => match whole_expression(s) {
                Some(e) if component => AttrValue::Expr(e),
                _ => AttrValue::Literal(s.clone()),
            },
            Value::Array(_) | Value::Object(_) => AttrValue::Literal(v.to_string()),
            other => AttrValue::Literal(other.to_string()),
        };
        attrs.push(Attr {
            name: k.clone(),
            value,
        });
    }

    if component {
        if let Some(c) = class_value {
            attrs.push(Attr {
                name: "class".into(),
                value: AttrValue::Literal(c),
            });
        }
        if let Some(id) = o.get("id") {
            attrs.push(Attr {
                name: "id".into(),
                value: AttrValue::Literal(str_of(id)),
            });
        }
        Ok(Node::Component {
            name: ty,
            attrs,
            children,
        })
    } else {
        Ok(Node::Element {
            tag: ty,
            id: o.get("id").map(str_of),
            classes: class_value
                .map(|c| c.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            attrs,
            children,
        })
    }
}

fn whole_expression(value: &str) -> Option<String> {
    let t = value.trim();
    let inner = t.strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains("{{") || inner.contains("}}") {
        return None;
    }
    Some(inner.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_tree_lowers_to_elements_and_components() {
        let doc = from_json(
            r#"[{"type":"Header","title":"Hi {{ n }}"},
                {"class":["grid","gap-4"],"children":[{"type":"p","text":"x"},"plain"]}]"#,
        )
        .unwrap();
        assert!(matches!(&doc[0], Node::Component { name, .. } if name == "Header"));
        let Node::Element {
            tag,
            classes,
            children,
            ..
        } = &doc[1]
        else {
            panic!()
        };
        assert_eq!(tag, "div");
        assert_eq!(classes, &["grid", "gap-4"]);
        assert!(matches!(&children[1], Node::Text(t) if t == "plain"));
    }

    #[test]
    fn yaml_and_json_agree() {
        let y = from_yaml("- type: p\n  text: hi\n  style: 'color:red'\n").unwrap();
        let j = from_json(r#"[{"type":"p","text":"hi","style":"color:red"}]"#).unwrap();
        assert_eq!(y, j);
    }

    #[test]
    fn for_if_else_and_expression_props() {
        let doc = from_yaml(
            "- {type: InvoiceTable, rows: '{{ items }}', n: 3}\n\
             - {type: li, for: 'x in xs', if: x.ok, text: '{{ x.n }}'}\n\
             - {type: p, if: a, text: A}\n\
             - {type: p, elif: b, text: B}\n\
             - {type: p, else: true, text: C}\n",
        )
        .unwrap();
        let Node::Component { attrs, .. } = &doc[0] else {
            panic!()
        };
        assert_eq!(
            attrs.iter().find(|a| a.name == "rows").unwrap().value,
            AttrValue::Expr("items".into())
        );
        assert_eq!(
            attrs.iter().find(|a| a.name == "n").unwrap().value,
            AttrValue::Literal("3".into())
        );
        assert!(matches!(&doc[1], Node::For { .. }));
        let Node::If {
            branches,
            else_body,
        } = &doc[2]
        else {
            panic!()
        };
        assert_eq!(branches.len(), 2);
        assert!(else_body.is_some());
    }

    #[test]
    fn top_level_children_object_is_the_body() {
        let doc = from_json(r#"{"children":[{"type":"p","text":"x"}]}"#).unwrap();
        assert_eq!(doc.len(), 1);
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(from_json("{").is_err());
        assert!(from_json(r#"[{"type":"p","else":true}]"#).is_err());
        assert!(from_json(r#"[{"for":"nonsense"}]"#).is_err());
    }
}
