pub mod ast;
mod lexer;
mod tree;

pub use ast::{Attr, AttrValue, Document, Node};
pub use tree::{parse_document, ParseError};
