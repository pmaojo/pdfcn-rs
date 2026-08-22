use std::fmt;

use crate::ast::{Attr, AttrValue, Document, Node};
use crate::lexer::{self, LineHead};

#[derive(Debug)]
pub enum ParseError {
    UnexpectedIndent {
        line: usize,
        expected: usize,
        found: usize,
    },
    UnknownDirective {
        line: usize,
        keyword: String,
    },
    MalformedFor {
        line: usize,
    },
    MalformedSet {
        line: usize,
    },
    TabIndent {
        line: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedIndent {
                line,
                expected,
                found,
            } => write!(
                f,
                "line {line}: unexpected indentation (expected {expected}, found {found})"
            ),
            ParseError::UnknownDirective { line, keyword } => {
                write!(f, "line {line}: unknown directive '- {keyword}'")
            }
            ParseError::MalformedFor { line } => write!(
                f,
                "line {line}: malformed '- for' directive, expected 'for <var> in <expr>'"
            ),
            ParseError::MalformedSet { line } => write!(
                f,
                "line {line}: malformed '- set' directive, expected 'set <name> = <expr>'"
            ),
            ParseError::TabIndent { line } => {
                write!(
                    f,
                    "indentation must use spaces only (tabs found at line {line})"
                )
            }
        }
    }
}

impl std::error::Error for ParseError {}

struct Cursor<'a> {
    lines: &'a [(usize, usize, &'a str)], // (indent, source_line_no, content)
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek_indent(&self) -> Option<usize> {
        self.lines.get(self.pos).map(|(i, _, _)| *i)
    }

    fn parse_siblings(&mut self, indent: usize) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        while let Some(cur_indent) = self.peek_indent() {
            if cur_indent < indent {
                break;
            }
            let (line_indent, line_no, content) = self.lines[self.pos];
            if line_indent > indent {
                return Err(ParseError::UnexpectedIndent {
                    line: line_no,
                    expected: indent,
                    found: line_indent,
                });
            }
            self.pos += 1;
            let head = lexer::parse_line(content).unwrap_or(LineHead::Text(content.to_string()));
            nodes.push(self.build_node(head, indent, line_no)?);
        }
        Ok(nodes)
    }

    fn parse_children(&mut self, parent_indent: usize) -> Result<Vec<Node>, ParseError> {
        match self.peek_indent() {
            Some(ci) if ci > parent_indent => self.parse_siblings(ci),
            _ => Ok(Vec::new()),
        }
    }

    fn build_node(
        &mut self,
        head: LineHead,
        indent: usize,
        line_no: usize,
    ) -> Result<Node, ParseError> {
        match head {
            LineHead::Directive { keyword, rest } if keyword == "if" => {
                let mut branches = vec![(rest, self.parse_children(indent)?)];
                let mut else_body = None;
                loop {
                    let Some((ci, next_line_no, content)) = self.lines.get(self.pos).copied()
                    else {
                        break;
                    };
                    if ci != indent {
                        break;
                    }
                    match lexer::parse_line(content) {
                        Ok(LineHead::Directive {
                            keyword: k2,
                            rest: r2,
                        }) if k2 == "elif" => {
                            self.pos += 1;
                            branches.push((r2, self.parse_children(indent)?));
                        }
                        Ok(LineHead::Directive { keyword: k2, .. }) if k2 == "else" => {
                            self.pos += 1;
                            let _ = next_line_no;
                            else_body = Some(self.parse_children(indent)?);
                            break;
                        }
                        _ => break,
                    }
                }
                Ok(Node::If {
                    branches,
                    else_body,
                })
            }
            LineHead::Directive { keyword, rest } if keyword == "for" => {
                let (binding, iterable) = rest
                    .split_once(" in ")
                    .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                    .ok_or(ParseError::MalformedFor { line: line_no })?;
                let body = self.parse_children(indent)?;
                Ok(Node::For {
                    binding,
                    iterable,
                    body,
                })
            }
            LineHead::Directive { keyword, rest } if keyword == "include" => {
                let path = rest.trim_matches('"').to_string();
                Ok(Node::Include { path })
            }
            LineHead::Directive { keyword, rest } if keyword == "set" => {
                let (name, expr) = rest
                    .split_once('=')
                    .ok_or(ParseError::MalformedSet { line: line_no })?;
                Ok(Node::Set {
                    name: name.trim().to_string(),
                    expr: expr.trim().to_string(),
                })
            }
            LineHead::Directive { keyword, .. } => Err(ParseError::UnknownDirective {
                line: line_no,
                keyword,
            }),
            LineHead::Tag {
                name,
                id,
                classes,
                mut attrs,
                inline,
            } => {
                let mut children = self.parse_children(indent)?;
                if let Some(text) = inline {
                    children.insert(0, Node::Text(text));
                }
                let is_component = name.chars().next().is_some_and(|c| c.is_uppercase());
                if is_component {
                    if let Some(id) = id {
                        attrs.push(Attr {
                            name: "id".into(),
                            value: AttrValue::Literal(id),
                        });
                    }
                    if !classes.is_empty() {
                        attrs.push(Attr {
                            name: "class".into(),
                            value: AttrValue::Literal(classes.join(" ")),
                        });
                    }
                    Ok(Node::Component {
                        name,
                        attrs,
                        children,
                    })
                } else {
                    Ok(Node::Element {
                        tag: name,
                        id,
                        classes,
                        attrs,
                        children,
                    })
                }
            }
            LineHead::ImplicitDiv {
                id,
                classes,
                attrs,
                inline,
            } => {
                let mut children = self.parse_children(indent)?;
                if let Some(text) = inline {
                    children.insert(0, Node::Text(text));
                }
                Ok(Node::Element {
                    tag: "div".to_string(),
                    id,
                    classes,
                    attrs,
                    children,
                })
            }
            LineHead::Text(t) => {
                let _ = self.parse_children(indent)?; // text nodes don't nest; drop stray children
                Ok(Node::Text(t))
            }
        }
    }
}

/// Parses a full `.haml`-style source document into a [`Document`] AST.
/// Indentation must use spaces (tabs are rejected); nesting depth is
/// derived purely from leading-space counts, no fixed step size required.
pub fn parse_document(source: &str) -> Result<Document, ParseError> {
    let mut lines = Vec::new();
    for (idx, raw) in source.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let leading_ws_len = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        if raw[..leading_ws_len].contains('\t') {
            return Err(ParseError::TabIndent { line: idx + 1 });
        }
        let stripped = raw.trim_start_matches(' ');
        let indent = raw.len() - stripped.len();
        lines.push((indent, idx + 1, stripped.trim_end()));
    }
    let mut cursor = Cursor {
        lines: &lines,
        pos: 0,
    };
    cursor.parse_siblings(0)
}
