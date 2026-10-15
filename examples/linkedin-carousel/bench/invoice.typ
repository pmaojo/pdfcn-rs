// examples/invoice.haml rebuilt in Typst, reading the same JSON.
#let d = json("invoice.json")
#let inv = d.invoice
#let cust = d.customer
#set page(paper: "a4", margin: 10mm)
#set text(font: "Inter", size: 11pt, fill: rgb("#0f172a"))
#grid(columns: (1fr, auto), align: (left, horizon),
  [#text(size: 22pt, weight: "bold")[Invoice #inv.number] \ #text(fill: rgb("#64748b"))[#inv.date]],
  box(fill: if inv.paid { rgb("#16a34a") } else { rgb("#dc2626") }, inset: (x: 8pt, y: 3pt), radius: 99pt,
    text(fill: white, size: 9pt, weight: "bold")[#if inv.paid [Paid] else [Unpaid]]))
#v(12pt)
#block(stroke: rgb("#e2e8f0"), radius: 8pt, inset: 14pt, width: 100%)[
  #text(size: 13pt, weight: "bold")[Bill To] \ #cust.name \ #text(size: 9pt, fill: rgb("#64748b"))[#cust.email]]
#v(8pt)
#table(columns: (1fr, 1fr, 1fr), stroke: (x: none, y: rgb("#e2e8f0")),
  ..inv.columns.map(c => text(weight: "bold", fill: rgb("#64748b"))[#c.label]),
  ..inv.items.map(it => inv.columns.map(c => [#it.at(c.key)])).flatten())
#grid(columns: (1fr, auto), [Subtotal: #inv.subtotal], [Total: #inv.total])
#v(40pt)
#line(length: 220pt)
#cust.name \ #text(size: 9pt, fill: rgb("#64748b"))[Customer signature]
