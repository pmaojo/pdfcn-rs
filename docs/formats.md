# Template formats

pdfcn's native syntax is HAML, but the pipeline after parsing only sees one
AST. Every other format lowers to it, so `{{ interpolation }}`, loops and
conditions, components, includes, Tailwind utilities, QR codes, icons and
image handling behave the same whatever you write.

| Format | Extension | Auto-detected from the text? |
|---|---|---|
| HAML | `.haml` | default |
| HTML | `.html` | yes: starts with `<` |
| Markdown | `.md` | yes: starts with `---` front matter or a `# Heading` |
| JSON block tree | `.json` | yes: starts with `{` or `[` |
| YAML block tree | `.yaml`, `.yml` | no: set the format |
| Typst subset | `.typ` | no: set the format |
| LaTeX subset | `.tex` | yes: starts with `\documentclass` or `\begin{document}` |

The CLI picks the format from the template's extension (`pdfcn build report.md
--data data.json`). The HTTP API takes an optional `format` field
(`haml`, `html`, `markdown`, `json`, `yaml`, `typst`, `latex`) next to
`template`; Node takes `format` in the options object. Without one, the text is
detected as above, and anything unrecognised is HAML. Partials follow the same
rule by extension: `- include "row.html"` from a HAML file works.

Working examples: `examples/format-{html,markdown,blocks,typst,latex}.*`, all
rendering `examples/formats.json`.

## HTML

Any HTML, parsed with the HTML5 algorithm (so sloppy markup behaves as in a
browser), with Tailwind classes and `{{ }}` bindings:

```html
<Header title="{{ company }} · {{ month }}"></Header>
<div class="grid grid-cols-3 gap-4">
  <div data-for="k in kpis" class="border rounded-lg p-4">
    <p class="text-3xl font-bold">{{ k.value }}</p>
    <p data-if="k.up" class="text-green-700">▲ {{ k.delta }}</p>
    <p data-else class="text-red-700">▼ {{ k.delta }}</p>
  </div>
</div>
<Icon name="camera" size="24px"/>
```

- **Components** are written as tags: `<Card title="x">`. Write them capitalised;
  `<card>`, `<x-card>` and `<pdfcn-card>` also work. A component whose name is
  also an HTML element (`Header`, `Table`, `Input`, `Textarea`, `Select`,
  `Label`, `Progress`, `Dialog`) is the component when capitalised and the HTML
  element when lowercase.
- **Component attributes** that are exactly `{{ expr }}` pass the value itself,
  as `rows={{ items }}` does in HAML: `<InvoiceTable rows="{{ items }}">`.
- **Control flow** is attributes: `data-for="x in xs"`, `data-if`, `data-elif`,
  `data-else` (the `elif`/`else` element must directly follow its `if`).
  `<template>` groups children without emitting a wrapper. Both
  `<include src="partials/row.html"/>` and `<set name="n" value="a + b"/>` must
  be self-closed.
- **Self-closing components** (`<Icon name="x"/>`) work, even though HTML itself
  would swallow the following siblings.
- **`<style>`** blocks (anywhere) are merged into the stylesheet after the
  utilities, so author CSS wins ties. `--strict` does not flag classes your own
  `<style>` defines.
- **Dropped:** `<script>`, `<link>`, `<meta>`, `<title>`, comments. A `<body>`
  with `class`/`style`/`id` becomes a wrapping `<div>` carrying them.

The layout engine is printpdf's, not a browser's: only the CSS it implements is
honoured (see "Known limitations" in the README). Arbitrary web pages will
lay out partially.

**Icons** need a wrapper to reserve their height: put them in a flex row
(`<div class="flex">…</div>`) before text that follows them.

### React / JSX

`renderToStaticMarkup` output is HTML, so React works as a front-end with no
extra code. `pdfcn/components` exports the component tags as strings:

```jsx
import { renderToStaticMarkup } from "react-dom/server";
import { renderPdf } from "pdfcn";
import { Header, Card } from "pdfcn/components";

const html = renderToStaticMarkup(
  <div className="p-4">
    <Header title="Report for {{ name }}" />
    <Card title="Total"><p className="text-3xl">{{ total }}</p></Card>
  </div>
);
const pdf = renderPdf(html, JSON.stringify({ name: "Ada", total: "€42" }));
```

(React warns about unknown-element casing in development; the output is right.
In JSX, write `{"{{ total }}"}` if your tooling treats `{{` specially.)

## Markdown

CommonMark plus tables, strikethrough and smart punctuation, with default
typography applied to elements that carry no class of their own. HTML and
components pass through, and a blank line inside one lets Markdown keep working:

```markdown
---
title: Monthly report
author: Northwind Studio
---
# {{ title }}

By **{{ author }}**. See the [dashboard](https://example.com).

| Project | Budget |
|---------|-------:|
| Website | €9,500 |

<Alert title="Heads up">

Markdown **inside** a component.

</Alert>
```

Front matter (YAML) is default template data; whatever you pass at render time
wins, key by key.

## JSON / YAML block trees

A document as data, for APIs and LLMs that would rather not write markup:

```yaml
- type: Header
  title: "Invoice for {{ company }}"
- type: div
  class: grid grid-cols-3 gap-4
  children:
    - for: k in kpis
      class: border rounded-lg p-4
      children:
        - { type: p, class: text-3xl, text: "{{ k.value }}" }
        - { type: p, if: k.up, text: "▲ {{ k.delta }}" }
        - { type: p, else: true, text: "▼ {{ k.delta }}" }
- { type: InvoiceTable, columns: '[...]', rows: "{{ rows }}" }
```

A node is a string (text) or an object. `type` is the tag, or a component when
capitalised (default `div`). Reserved keys: `type`, `class` (string or list),
`id`, `text`, `children`, `for`, `if`, `elif`, `else`, `include`, and
`set` + `value`. Every other key is an attribute: numbers and booleans are
stringified, arrays and objects become JSON text, and on components a value
that is exactly `{{ expr }}` passes the data itself. A top-level
`{"children": [...]}` object is the document body.

## Typst and LaTeX subsets

These are small converters for simple documents, **not** Typst or TeX
engines. They exist so a README-grade report written in either syntax can
become a PDF with the same data bindings and fonts, without a second toolchain.

**Typst:** `= Headings` (1–6), paragraphs, `- bullets`, `+ numbered`, `*bold*`,
`_italic_`, `` `code` ``, `#link("url")[text]`, `#image("path", width: 50%)`,
`#table(columns: N, [a], [b], …)`, `#line()`, `#pagebreak()`, comments.
`#set`, `#show`, `#let`, `#import`, math and scripting are skipped.

**LaTeX:** `\title/\author/\date` + `\maketitle`, `\section`/`\subsection`/
`\subsubsection`, `itemize`/`enumerate`, `\textbf`, `\textit`/`\emph`,
`\texttt`, `\underline`, `\href{url}{text}`, `\includegraphics[width=…]{path}`,
`tabular` (`&`, `\\`, `\hline`), `\newpage`, `\\`, `%` comments, the usual
escapes and dashes, `\texteuro`. Packages, math, floats, `\newcommand` and
column specs are ignored; an unknown command is dropped and its braced text
kept.

If your document needs real Typst or LaTeX (math, citations, custom layout),
compile it with the real tool; pdfcn is for templated business documents.

## Adding another front-end

A front-end is a function from source text to `pdfcn_parser::Document` in
`pdfcn-formats`, plus a variant in `Format`. Everything downstream is shared.
