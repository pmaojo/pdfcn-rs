//! Parses the *head* of a single dedented line: `%tag.class#id(attr="v")`,
//! `.class#id`, or a `- directive`. Indentation and tree structure are
//! handled by [`crate::tree`]; this module only understands one line at a
//! time, via `winnow` combinators.

use winnow::ascii::multispace0;
use winnow::combinator::{alt, delimited, opt, repeat};
use winnow::prelude::*;
use winnow::token::{take_till, take_while};

use crate::ast::{Attr, AttrValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineHead {
    /// `%tag` or `%Component` plus selectors/attrs, with optional inline
    /// text/expr trailing on the same line.
    Tag {
        name: String,
        id: Option<String>,
        classes: Vec<String>,
        attrs: Vec<Attr>,
        inline: Option<String>,
    },
    /// `.class` / `#id` shorthand for an implicit `%div`.
    ImplicitDiv {
        id: Option<String>,
        classes: Vec<String>,
        attrs: Vec<Attr>,
        inline: Option<String>,
    },
    /// `- for x in items`, `- if cond`, `- elif cond`, `- else`,
    /// `- include "path"`, or any other `- <keyword> <rest>` directive.
    Directive { keyword: String, rest: String },
    /// A plain text line (may contain `{{ expr }}` interpolation).
    Text(String),
}

fn ident<'a>(input: &mut &'a str) -> winnow::ModalResult<&'a str> {
    take_while(1.., |c: char| c.is_alphanumeric() || c == '_' || c == '-').parse_next(input)
}

fn class_or_id<'a>(input: &mut &'a str) -> winnow::ModalResult<(char, &'a str)> {
    let sigil: char = alt(('.', '#')).parse_next(input)?;
    let name = ident.parse_next(input)?;
    Ok((sigil, name))
}

fn quoted_string(input: &mut &str) -> winnow::ModalResult<String> {
    delimited('"', take_till(0.., '"'), '"')
        .map(|s: &str| s.to_string())
        .parse_next(input)
}

fn expr_braces(input: &mut &str) -> winnow::ModalResult<String> {
    delimited("{{", take_till(0.., |c| c == '}'), "}}")
        .map(|s: &str| s.trim().to_string())
        .parse_next(input)
}

fn attr_value(input: &mut &str) -> winnow::ModalResult<AttrValue> {
    alt((
        expr_braces.map(AttrValue::Expr),
        quoted_string.map(AttrValue::Literal),
        ident.map(|s: &str| AttrValue::Literal(s.to_string())),
    ))
    .parse_next(input)
}

fn single_attr(input: &mut &str) -> winnow::ModalResult<Attr> {
    multispace0.parse_next(input)?;
    let name = ident.parse_next(input)?;
    multispace0.parse_next(input)?;
    '='.parse_next(input)?;
    multispace0.parse_next(input)?;
    let value = attr_value.parse_next(input)?;
    Ok(Attr {
        name: name.to_string(),
        value,
    })
}

fn attr_list(input: &mut &str) -> winnow::ModalResult<Vec<Attr>> {
    delimited(
        '(',
        repeat(0.., single_attr).fold(Vec::new, |mut acc: Vec<Attr>, a| {
            acc.push(a);
            acc
        }),
        ')',
    )
    .parse_next(input)
}

struct Selectors {
    id: Option<String>,
    classes: Vec<String>,
}

fn selectors(input: &mut &str) -> winnow::ModalResult<Selectors> {
    let parts: Vec<(char, &str)> = repeat(0.., class_or_id).parse_next(input)?;
    let mut id = None;
    let mut classes = Vec::new();
    for (sigil, name) in parts {
        match sigil {
            '#' => id = Some(name.to_string()),
            '.' => classes.push(name.to_string()),
            _ => unreachable!(),
        }
    }
    Ok(Selectors { id, classes })
}

fn inline_trailer(input: &mut &str) -> winnow::ModalResult<Option<String>> {
    multispace0.parse_next(input)?;
    let rest = input.trim();
    if rest.is_empty() {
        return Ok(None);
    }
    let rest = rest.strip_prefix('=').map(str::trim).unwrap_or(rest);
    *input = "";
    Ok(Some(rest.to_string()))
}

fn tag_line(input: &mut &str) -> winnow::ModalResult<LineHead> {
    '%'.parse_next(input)?;
    let name = ident.parse_next(input)?.to_string();
    let sel = selectors.parse_next(input)?;
    let attrs = opt(attr_list).parse_next(input)?.unwrap_or_default();
    let inline = inline_trailer.parse_next(input)?;
    Ok(LineHead::Tag {
        name,
        id: sel.id,
        classes: sel.classes,
        attrs,
        inline,
    })
}

fn implicit_div_line(input: &mut &str) -> winnow::ModalResult<LineHead> {
    let sel = selectors.parse_next(input)?;
    if sel.id.is_none() && sel.classes.is_empty() {
        return Err(winnow::error::ErrMode::Backtrack(
            winnow::error::ContextError::new(),
        ));
    }
    let attrs = opt(attr_list).parse_next(input)?.unwrap_or_default();
    let inline = inline_trailer.parse_next(input)?;
    Ok(LineHead::ImplicitDiv {
        id: sel.id,
        classes: sel.classes,
        attrs,
        inline,
    })
}

fn directive_line(input: &mut &str) -> winnow::ModalResult<LineHead> {
    '-'.parse_next(input)?;
    multispace0.parse_next(input)?;
    let keyword = ident.parse_next(input)?.to_string();
    multispace0.parse_next(input)?;
    let rest = input.trim().to_string();
    *input = "";
    Ok(LineHead::Directive { keyword, rest })
}

/// Parses one already-dedented, trimmed line (no leading whitespace, no
/// trailing newline) into a [`LineHead`].
pub fn parse_line(raw: &str) -> Result<LineHead, String> {
    let mut input = raw;
    let result = alt((tag_line, implicit_div_line, directive_line)).parse_next(&mut input);
    match result {
        Ok(head) => Ok(head),
        Err(_) => Ok(LineHead::Text(raw.to_string())),
    }
}
