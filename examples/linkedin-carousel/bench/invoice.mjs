// The examples/invoice.haml document, rebuilt with @react-pdf/renderer primitives.
import React from "react";
import { Document, Page, View, Text, StyleSheet, Font } from "@react-pdf/renderer";

const h = React.createElement;
const REPO = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/\/$/, "");
const FONTS = `${REPO}/pdfcn-core/assets/fonts/inter`;
export const useInter = process.env.FONT === "inter";
if (useInter) {
  Font.register({ family: "Inter", fonts: [
    { src: `${FONTS}/Inter-Regular.ttf` },
    { src: `${FONTS}/Inter-Bold.ttf`, fontWeight: "bold" },
  ]});
}
const family = useInter ? "Inter" : "Helvetica";
const bold = useInter ? { fontWeight: "bold" } : { fontFamily: "Helvetica-Bold" };

const s = StyleSheet.create({
  page: { padding: 28, fontFamily: family, fontSize: 11, color: "#0f172a" },
  header: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", marginBottom: 16 },
  title: { fontSize: 22, ...bold },
  subtitle: { fontSize: 11, color: "#64748b", marginTop: 4 },
  badge: { borderRadius: 999, paddingVertical: 2, paddingHorizontal: 8, fontSize: 9, color: "#fff", ...bold },
  card: { border: "1px solid #e2e8f0", borderRadius: 8, padding: 14, marginBottom: 16 },
  cardTitle: { fontSize: 13, marginBottom: 6, ...bold },
  small: { fontSize: 9, color: "#64748b" },
  row: { flexDirection: "row", borderBottom: "1px solid #e2e8f0", paddingVertical: 6 },
  th: { flex: 1, fontSize: 10, color: "#64748b", ...bold },
  td: { flex: 1 },
  totals: { flexDirection: "row", justifyContent: "space-between", marginTop: 14 },
  sig: { marginTop: 40, width: 220 },
  sigLine: { borderTop: "1px solid #0f172a", marginBottom: 4 },
});

export function Invoice({ invoice, customer }) {
  return h(Document, { title: `Invoice ${invoice.number}` },
    h(Page, { size: "A4", style: s.page },
      h(View, { style: s.header },
        h(View, null,
          h(Text, { style: s.title }, `Invoice ${invoice.number}`),
          h(Text, { style: s.subtitle }, invoice.date)),
        h(Text, { style: [s.badge, { backgroundColor: invoice.paid ? "#16a34a" : "#dc2626" }] },
          invoice.paid ? "Paid" : "Unpaid")),
      h(View, { style: s.card },
        h(Text, { style: s.cardTitle }, "Bill To"),
        h(Text, null, customer.name),
        h(Text, { style: s.small }, customer.email)),
      h(View, null,
        h(View, { style: s.row }, invoice.columns.map(c => h(Text, { key: c.key, style: s.th }, c.label))),
        invoice.items.map((it, i) => h(View, { key: i, style: s.row },
          invoice.columns.map(c => h(Text, { key: c.key, style: s.td }, String(it[c.key])))))),
      h(View, { style: s.totals },
        h(Text, null, `Subtotal: ${invoice.subtotal}`),
        h(Text, null, `Total: ${invoice.total}`)),
      h(View, { style: s.sig },
        h(View, { style: s.sigLine }),
        h(Text, null, customer.name),
        h(Text, { style: s.small }, "Customer signature"))));
}
