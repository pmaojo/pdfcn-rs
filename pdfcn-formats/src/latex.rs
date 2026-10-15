//! A small subset of LaTeX for simple documents — **not** a TeX engine.
//! Supported: `\title/\author/\date` + `\maketitle`, `\section` /
//! `\subsection` / `\subsubsection`, paragraphs, `itemize` / `enumerate`
//! with `\item`, `\textbf`, `\textit`, `\emph`, `\texttt`, `\underline`,
//! `\href{url}{text}`, `\includegraphics[width=..]{path}`, `tabular`
//! (`&`, `\\`, `\hline`), `\newpage`, `\\`, `%` comments, the usual
//! escapes (`\& \% \$ \_ \# \{ \}`) and `--` / `---` dashes. Preamble
//! packages, math (`$...$`, `equation`), floats and macros are not
//! supported: unknown commands are dropped, keeping their braced text.

use pdfcn_parser::Document;

use crate::typst::esc;
use crate::FormatError;

pub(crate) fn parse_latex(source: &str) -> Result<Document, FormatError> {
    let body = match source.find("\\begin{document}") {
        Some(p) => {
            let after = &source[p + "\\begin{document}".len()..];
            let preamble = &source[..p];
            let end = after.find("\\end{document}").unwrap_or(after.len());
            (preamble.to_string(), after[..end].to_string())
        }
        None => (String::new(), source.to_string()),
    };
    let meta = |cmd: &str| -> Option<String> {
        let p = body.0.find(&format!("\\{cmd}{{"))? + cmd.len() + 2;
        let rest = &body.0[p..];
        Some(rest[..matching_brace(rest)?].to_string())
    };
    let (title, author, date) = (meta("title"), meta("author"), meta("date"));
    let text = strip_comments(&body.1);
    let mut html = String::new();
    let mut para = String::new();
    let mut rest = text.as_str();
    let flush = |para: &mut String, html: &mut String| {
        let t = para.trim();
        if !t.is_empty() {
            html.push_str(&format!("<p>{}</p>", inline(t)));
        }
        para.clear();
    };
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("\\maketitle") {
            flush(&mut para, &mut html);
            if let Some(t) = &title {
                html.push_str(&format!("<h1>{}</h1>", inline(t)));
            }
            let sub: Vec<String> = [author.clone(), date.clone()]
                .into_iter()
                .flatten()
                .collect();
            if !sub.is_empty() {
                html.push_str(&format!("<p>{}</p>", inline(&sub.join(" · "))));
            }
            rest = stripped;
            continue;
        }
        let mut handled = false;
        for (cmd, tag) in [
            ("section", "h2"),
            ("subsection", "h3"),
            ("subsubsection", "h4"),
            ("paragraph", "h5"),
        ] {
            let key = format!("\\{cmd}");
            if let Some(after) = rest.strip_prefix(&key) {
                let after = after.strip_prefix('*').unwrap_or(after);
                if let Some(arg) = after.strip_prefix('{') {
                    if let Some(end) = matching_brace(arg) {
                        flush(&mut para, &mut html);
                        html.push_str(&format!("<{tag}>{}</{tag}>", inline(&arg[..end])));
                        rest = &arg[end + 1..];
                        handled = true;
                        break;
                    }
                }
            }
        }
        if handled {
            continue;
        }
        if let Some(after) = rest.strip_prefix("\\begin{") {
            if let Some(close) = after.find('}') {
                let env = &after[..close];
                let tail = &after[close + 1..];
                match env {
                    "itemize" | "enumerate" => {
                        flush(&mut para, &mut html);
                        let tag = if env == "itemize" { "ul" } else { "ol" };
                        let end_tag = format!("\\end{{{env}}}");
                        let end = tail.find(&end_tag).unwrap_or(tail.len());
                        html.push_str(&format!("<{tag}>"));
                        for item in tail[..end].split("\\item").skip(1) {
                            html.push_str(&format!("<li>{}</li>", inline(item.trim())));
                        }
                        html.push_str(&format!("</{tag}>"));
                        rest = tail.get(end + end_tag.len()..).unwrap_or("");
                        continue;
                    }
                    "tabular" => {
                        flush(&mut para, &mut html);
                        let spec = tail
                            .strip_prefix('{')
                            .and_then(|t| matching_brace(t).map(|e| (e, t)));
                        let (skip, tail2) = match spec {
                            Some((e, t)) => (e + 1, t),
                            None => (0, tail),
                        };
                        let body = &tail2[skip..];
                        let end = body.find("\\end{tabular}").unwrap_or(body.len());
                        html.push_str("<table>");
                        let mut first = true;
                        for row in body[..end].split("\\\\") {
                            let row = row.replace("\\hline", "");
                            if row.trim().is_empty() {
                                continue;
                            }
                            let tag = if first { "th" } else { "td" };
                            html.push_str("<tr>");
                            for cell in row.split('&') {
                                html.push_str(&format!("<{tag}>{}</{tag}>", inline(cell.trim())));
                            }
                            html.push_str("</tr>");
                            first = false;
                        }
                        html.push_str("</table>");
                        rest = body.get(end + "\\end{tabular}".len()..).unwrap_or("");
                        continue;
                    }
                    _ => {
                        // Unknown environment: keep its text.
                        rest = tail;
                        continue;
                    }
                }
            }
        }
        if let Some(after) = rest.strip_prefix("\\includegraphics") {
            flush(&mut para, &mut html);
            let (opts, after) = match after.strip_prefix('[') {
                Some(a) => {
                    let e = a.find(']').unwrap_or(0);
                    (a[..e].to_string(), &a[(e + 1).min(a.len())..])
                }
                None => (String::new(), after),
            };
            if let Some(arg) = after.strip_prefix('{') {
                if let Some(end) = matching_brace(arg) {
                    let style = latex_width(&opts)
                        .map(|w| format!(" style=\"width:{w}\""))
                        .unwrap_or_default();
                    html.push_str(&format!("<img src=\"{}\"{style}>", esc(&arg[..end])));
                    rest = &arg[end + 1..];
                    continue;
                }
            }
        }
        if let Some(after) = rest
            .strip_prefix("\\newpage")
            .or_else(|| rest.strip_prefix("\\clearpage"))
        {
            flush(&mut para, &mut html);
            html.push_str("<div class=\"break-after-page\"></div>");
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix("\\end{") {
            rest = after.find('}').map(|p| &after[p + 1..]).unwrap_or("");
            continue;
        }
        // Blank line = paragraph break.
        if rest.starts_with("\n\n") || rest.starts_with("\r\n\r\n") {
            flush(&mut para, &mut html);
            rest = rest.trim_start();
            continue;
        }
        // Copy up to the next backslash or blank line, keeping a leading `\`.
        let ch = rest.chars().next().unwrap();
        let next = rest[ch.len_utf8()..]
            .find(['\\', '\n'])
            .map(|p| p + ch.len_utf8())
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(next);
        para.push_str(chunk);
        rest = tail;
        if rest.starts_with('\n') && !rest.starts_with("\n\n") && !rest.starts_with("\r\n\r\n") {
            para.push(' ');
            rest = &rest[1..];
        }
    }
    flush(&mut para, &mut html);
    crate::html::parse_html(&html, true)
}

fn latex_width(opts: &str) -> Option<String> {
    let v = opts
        .split(',')
        .find_map(|o| o.trim().strip_prefix("width="))?;
    if let Some(f) = v
        .strip_suffix("\\textwidth")
        .or_else(|| v.strip_suffix("\\linewidth"))
    {
        let f: f64 = if f.is_empty() { 1.0 } else { f.parse().ok()? };
        return Some(format!("{}%", (f * 100.0).round()));
    }
    Some(v.to_string())
}

fn strip_comments(s: &str) -> String {
    s.lines()
        .map(|l| {
            let mut prev = ' ';
            for (i, c) in l.char_indices() {
                if c == '%' && prev != '\\' {
                    return &l[..i];
                }
                prev = c;
            }
            l
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Index of the `}` that closes an already-opened `{` at the start of `s`.
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if prev != '\\' {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// Inline commands, escapes and dashes.
fn inline(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('\\') {
            // Escaped specials and `\\`.
            let mut it = after.chars();
            match it.next() {
                Some(c @ ('&' | '%' | '$' | '_' | '#' | '{' | '}')) => {
                    out.push(c);
                    rest = &after[1..];
                    continue;
                }
                Some('\\') => {
                    out.push_str("<br>");
                    rest = &after[1..];
                    continue;
                }
                _ => {}
            }
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let after_name = &after[name.len()..];
            // Argument-less symbols (`\texteuro{}` keeps its empty braces).
            let symbol = match name.as_str() {
                "texteuro" => Some("€"),
                "ldots" | "dots" => Some("…"),
                "LaTeX" => Some("LaTeX"),
                "TeX" => Some("TeX"),
                "textbackslash" => Some("\\"),
                _ => None,
            };
            if let Some(sym) = symbol {
                out.push_str(sym);
                rest = after_name.strip_prefix("{}").unwrap_or(after_name);
                continue;
            }
            let wrap = match name.as_str() {
                "textbf" => Some("strong"),
                "textit" | "emph" => Some("em"),
                "texttt" => Some("code"),
                "underline" => Some("u"),
                _ => None,
            };
            if let Some(arg) = after_name.strip_prefix('{') {
                if let Some(end) = matching_brace(arg) {
                    let inner = &arg[..end];
                    let tail = &arg[end + 1..];
                    if let Some(tag) = wrap {
                        out.push_str(&format!("<{tag}>{}</{tag}>", inline(inner)));
                        rest = tail;
                        continue;
                    }
                    if name == "href" {
                        if let Some(t) = tail.strip_prefix('{') {
                            if let Some(e2) = matching_brace(t) {
                                out.push_str(&format!(
                                    "<a href=\"{}\">{}</a>",
                                    esc(inner),
                                    inline(&t[..e2])
                                ));
                                rest = &t[e2 + 1..];
                                continue;
                            }
                        }
                    }
                    // Unknown command: keep its braced text.
                    out.push_str(&inline(inner));
                    rest = tail;
                    continue;
                }
            }
            // Bare unknown command (`\noindent`, `\centering`): drop it.
            rest = after_name.trim_start_matches(' ');
            if name.is_empty() {
                rest = &after[after.chars().next().map(char::len_utf8).unwrap_or(0)..];
            }
            continue;
        }
        if let Some(after) = rest.strip_prefix("---") {
            out.push('—');
            rest = after;
        } else if let Some(after) = rest.strip_prefix("--") {
            out.push('–');
            rest = after;
        } else {
            let ch = rest.chars().next().unwrap();
            match ch {
                '~' => out.push('\u{a0}'),
                '<' | '>' | '&' => out.push_str(&esc(&ch.to_string())),
                '{' | '}' => {}
                c => out.push(c),
            }
            rest = &rest[ch.len_utf8()..];
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
    fn article_with_title_sections_lists_and_tables() {
        let doc = parse_latex(
            "\\documentclass{article}\n\\usepackage{x}\n\\title{My Report}\n\\author{Ada}\n\\begin{document}\n\\maketitle\n\\section{Intro}\nHello \\textbf{world} \\& co.\n\n\\begin{itemize}\n\\item one\n\\item two\n\\end{itemize}\n\\begin{tabular}{ll}\nA & B \\\\ \\hline\n1 & 2 \\\\\n\\end{tabular}\n\\end{document}",
        )
        .unwrap();
        assert_eq!(tags(&doc), ["h1", "p", "h2", "p", "ul", "table"]);
    }

    #[test]
    fn links_images_dashes_and_comments() {
        let doc = parse_latex(
            "See \\href{https://x.test}{site} -- really % hidden\n\n\\includegraphics[width=0.5\\textwidth]{a.png}\n\\newpage\n",
        )
        .unwrap();
        assert_eq!(tags(&doc), ["p", "img", "div"]);
        assert_eq!(latex_width("width=0.5\\textwidth").as_deref(), Some("50%"));
        assert_eq!(inline("a -- b --- c"), "a – b — c");
    }

    #[test]
    fn symbols_expand() {
        assert_eq!(inline("\\texteuro{}5 \\ldots"), "€5 …");
    }

    #[test]
    fn unknown_commands_degrade_instead_of_failing() {
        let doc = parse_latex("\\noindent \\foo{kept} text\n").unwrap();
        assert_eq!(tags(&doc), ["p"]);
    }
}
