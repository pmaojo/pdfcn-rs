// The 100-product catalog in @react-pdf/renderer. Its <Image> takes PNG/JPG
// only, so the per-product SVG mark is left out (this favours react-pdf).
import fs from "node:fs";
import path from "node:path";
import React from "react";
import { renderToBuffer, Document, Page, View, Text, Image, StyleSheet, Font } from "../node_modules/@react-pdf/renderer/lib/react-pdf.js";

const h = React.createElement;
const dir = path.dirname(new URL(import.meta.url).pathname);
const REPO = decodeURIComponent(new URL("../../../../", import.meta.url).pathname).replace(/\/$/, "");
const FONTS = `${REPO}/pdfcn-core/assets/fonts/inter`;
Font.register({ family: "Inter", fonts: [
  { src: `${FONTS}/Inter-Regular.ttf` },
  { src: `${FONTS}/Inter-Bold.ttf`, fontWeight: "bold" },
]});
const d = JSON.parse(fs.readFileSync(`${dir}/catalog.json`, "utf8"));
const s = StyleSheet.create({
  page: { padding: 28, fontFamily: "Inter", fontSize: 10 },
  title: { fontSize: 18, fontWeight: "bold", marginBottom: 10 },
  row: { flexDirection: "row" },
  card: { width: "33.33%", padding: 4 },
  inner: { border: "1px solid #e2e8f0", borderRadius: 8, padding: 8 },
  photo: { width: "100%", height: 110, objectFit: "cover" },
  name: { fontWeight: "bold", marginTop: 6 },
});
const doc = h(Document, null,
  h(Page, { size: "A4", style: s.page },
    h(Text, { style: s.title }, d.title),
    d.rows.map((row, i) => h(View, { key: i, style: s.row, wrap: false },
      row.map(p => h(View, { key: p.id, style: s.card },
        h(View, { style: s.inner },
          h(Image, { style: s.photo, src: `${dir}/${p.photo}` }),
          h(Text, { style: s.name }, p.name),
          h(Text, null, p.price))))))));
fs.writeFileSync(process.argv[2] || `${dir}/rp.pdf`, await renderToBuffer(doc));
