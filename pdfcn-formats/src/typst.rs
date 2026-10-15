//! A small subset of Typst for simple documents — **not** a Typst compiler.
//! Supported: `= Headings` (1–6 `=`), paragraphs, `- bullets`, `+ numbered`,
//! `*bold*`, `_italic_`, `` `code` ``, `#link("url")[text]`,
//! `#image("path", width: 50%)`, `#table(columns: N, [a], [b], ...)`,
//! `#line()`, `#pagebreak()`, `//` and `/* */` comments, `\` escapes.
//! `#set`, `#show`, `#let`, math and scripting are not supported: lines
//! starting with them are skipped.

use pdfcn_parser::Document;

use crate::{err, FormatError};

pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn parse_typst(source: &str) -> Result<Document, FormatError> {
    let src = strip_comments(source);
    let mut html = String::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    let mut para: Vec<String> = Vec::new();
    let flush = |para: &mut Vec<String>, html: &mut String| {
        if !para.is_empty() {
            html.push_str(&format!("<p>{}</p>", inline(&para.join(" "))));
            para.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            flush(&mut para, &mut html);
            i += 1;
            continue;
        }
        if line.starts_with("#set ")
            || line.starts_with("#show ")
            || line.starts_with("#let ")
            || line.starts_with("#import ")
        {
            i += 1;
            continue;
        }
        let eqs = line.chars().take_while(|c| *c == '=').count();
        if (1..=6).contains(&eqs) && line[eqs..].starts_with(' ') {
            flush(&mut para, &mut html);
            html.push_str(&format!("<h{eqs}>{}</h{eqs}>", inline(line[eqs..].trim())));
            i += 1;
            continue;
        }
        if line.starts_with("- ") || line.starts_with("+ ") {
            flush(&mut para, &mut html);
            let ordered = line.starts_with('+');
            let tag = if ordered { "ol" } else { "ul" };
            html.push_str(&format!("<{tag}>"));
            while i < lines.len() {
                let l = lines[i].trim();
                let marker = if ordered { "+ " } else { "- " };
                match l.strip_prefix(marker) {
                    Some(item) => html.push_str(&format!("<li>{}</li>", inline(item.trim()))),
                    None => break,
                }
                i += 1;
            }
            html.push_str(&format!("</{tag}>"));
            continue;
        }
        if line.starts_with("#table(") {
            flush(&mut para, &mut html);
            // A table call may span lines: gather until parentheses balance.
            let mut call = String::new();
            let mut depth = 0i32;
            while i < lines.len() {
                for ch in lines[i].chars() {
                    match ch {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                call.push_str(lines[i]);
                call.push(' ');
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
            html.push_str(&table(&call)?);
            continue;
        }
        if line == "#pagebreak()" {
            flush(&mut para, &mut html);
            html.push_str("<div class=\"break-after-page\"></div>");
            i += 1;
            continue;
        }
        if line == "#line()" || line.starts_with("#line(") {
            flush(&mut para, &mut html);
            html.push_str("<hr>");
            i += 1;
            continue;
        }
        if line.starts_with("#image(") && line.ends_with(')') {
            flush(&mut para, &mut html);
            html.push_str(&image(line)?);
            i += 1;
            continue;
        }
        para.push(line.to_string());
        i += 1;
    }
    flush(&mut para, &mut html);
    crate::html::parse_html(&html, true)
}

fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        match rest[p..].find("*/") {
            Some(e) => rest = &rest[p + e + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|l| match l.find("//") {
            // Keep `https://`.
            Some(p) if !l[..p].ends_with(':') => &l[..p],
            _ => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first quoted string inside `#name("...", ...)`.
fn first_string(call: &str) -> Option<String> {
    let start = call.find('"')? + 1;
    let end = call[start..].find('"')? + start;
    Some(call[start..end].to_string())
}

fn named(call: &str, key: &str) -> Option<String> {
    let p = call.find(&format!("{key}:"))? + key.len() + 1;
    let rest = call[p..].trim_start();
    let end = rest.find([',', ')']).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

fn image(call: &str) -> Result<String, FormatError> {
    let path = first_string(call).ok_or_else(|| err("#image needs a quoted path"))?;
    let style = match named(call, "width") {
        Some(w) => format!(" style=\"width:{w}\""),
        None => String::new(),
    };
    Ok(format!("<img src=\"{}\"{style}>", esc(&path)))
}

fn table(call: &str) -> Result<String, FormatError> {
    // `columns: 3` or `columns: (1fr, 2fr)` (a track per entry).
    let cols: usize = named(call, "columns")
        .map(|c| {
            c.parse::<usize>().unwrap_or_else(|_| {
                // `named` stops at the first comma, so for a tuple read the
                // balanced parentheses directly.
                let p = call
                    .find("columns:")
                    .map(|p| p + "columns:".len())
                    .unwrap_or(0);
                let rest = call[p..].trim_start();
                rest.strip_prefix('(')
                    .and_then(|r| {
                        r.find(')')
                            .map(|e| r[..e].split(',').filter(|t| !t.trim().is_empty()).count())
                    })
                    .unwrap_or(1)
            })
        })
        .filter(|n| *n > 0)
        .unwrap_or(1);
    // Cells are the top-level `[...]` groups.
    let mut cells = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in call.chars() {
        match ch {
            '[' => {
                if depth > 0 {
                    cur.push(ch);
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    cells.push(std::mem::take(&mut cur));
                } else {
                    cur.push(ch);
                }
            }
            _ if depth > 0 => cur.push(ch),
            _ => {}
        }
    }
    let mut out = String::from("<table>");
    for (r, row) in cells.chunks(cols).enumerate() {
        out.push_str("<tr>");
        for c in row {
            let tag = if r == 0 { "th" } else { "td" };
            out.push_str(&format!("<{tag}>{}</{tag}>", inline(c.trim())));
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    Ok(out)
}

/// Inline markup: `*bold*`, `_italic_`, `` `code` ``, `#link("u")[t]`, escapes.
fn inline(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                out.push_str(&esc(&chars[i + 1].to_string()));
                i += 2;
            }
            '*' | '_' => {
                if let Some(end) = chars[i + 1..].iter().position(|x| *x == c) {
                    let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                    let tag = if c == '*' { "strong" } else { "em" };
                    // `snake_case` words keep their underscores.
                    let word_inner = c == '_' && i > 0 && chars[i - 1].is_alphanumeric();
                    if !inner.is_empty() && !word_inner {
                        out.push_str(&format!("<{tag}>{}</{tag}>", inline(&inner)));
                        i += end + 2;
                        continue;
                    }
                }
                out.push(c);
                i += 1;
            }
            '`' => {
                if let Some(end) = chars[i + 1..].iter().position(|x| *x == '`') {
                    let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                    out.push_str(&format!("<code>{}</code>", esc(&inner)));
                    i += end + 2;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            '#' if chars[i..].iter().take(6).collect::<String>() == "#link(" => {
                let rest: String = chars[i..].iter().collect();
                if let (Some(url), Some(open)) = (first_string(&rest), rest.find("[")) {
                    if let Some(close) = rest[open..].find(']') {
                        let text = &rest[open + 1..open + close];
                        out.push_str(&format!("<a href=\"{}\">{}</a>", esc(&url), inline(text)));
                        i += rest[..open + close + 1].chars().count();
                        continue;
                    }
                }
                out.push(c);
                i += 1;
            }
            '<' | '>' | '&' => {
                out.push_str(&esc(&c.to_string()));
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcn_parser::Node;

    fn tags(doc: &Document) -> Vec<String> {
        doc.iter()
            .filter_map(|n| match n {
                Node::Element { tag, .. } => Some(tag.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn headings_paragraphs_lists_and_comments() {
        let doc = parse_typst("= Title // trailing\n\nSome *bold* and _it_ text.\n\n- a\n- b\n\n+ one\n+ two\n/* gone */\n").unwrap();
        assert_eq!(tags(&doc), ["h1", "p", "ul", "ol"]);
    }

    #[test]
    fn links_images_tables_and_breaks() {
        let doc = parse_typst(
            "See #link(\"https://x.test/a\")[the site].\n\n#image(\"a.png\", width: 50%)\n\n#table(\n  columns: 2,\n  [H1], [H2],\n  [a], [b],\n)\n\n#pagebreak()\n",
        )
        .unwrap();
        assert_eq!(tags(&doc), ["p", "img", "table", "div"]);
    }

    #[test]
    fn table_column_counts_from_a_number_or_a_tuple() {
        let t = table("#table(columns: 2, [a], [b], [c], [d])").unwrap();
        assert_eq!(t.matches("<tr>").count(), 2);
        let t = table("#table(columns: (1fr, 2fr, 1fr), [a], [b], [c])").unwrap();
        assert_eq!(t.matches("<tr>").count(), 1);
    }

    #[test]
    fn unsupported_directives_are_skipped_not_fatal() {
        let doc = parse_typst("#set page(margin: 1cm)\n#let x = 1\n= Hi\n").unwrap();
        assert_eq!(tags(&doc), ["h1"]);
    }

    #[test]
    fn snake_case_keeps_underscores_and_urls_keep_slashes() {
        let out = inline("use snake_case_name here");
        assert_eq!(out, "use snake_case_name here");
        assert!(strip_comments("go https://a.b/c").contains("https://a.b/c"));
    }
}
