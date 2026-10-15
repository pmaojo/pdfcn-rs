// Warm, in-process: same document rendered N times after a warm-up.
import fs from "node:fs";
import React from "react";
import { renderToBuffer } from "@react-pdf/renderer";
import { Invoice } from "./invoice.mjs";
const REPO = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/\/$/, "");

const data = JSON.parse(fs.readFileSync(`${REPO}/examples/invoice.json`, "utf8"));
const el = () => React.createElement(Invoice, data);
for (let i = 0; i < 10; i++) await renderToBuffer(el());
const N = 100, times = [];
for (let i = 0; i < N; i++) { const t = process.hrtime.bigint(); await renderToBuffer(el()); times.push(Number(process.hrtime.bigint() - t) / 1e6); }
times.sort((a, b) => a - b);
const med = times[N / 2], p95 = times[Math.floor(N * 0.95)];
console.log(JSON.stringify({ median_ms: +med.toFixed(2), p95_ms: +p95.toFixed(2), rss_mb: +(process.memoryUsage().rss / 1048576).toFixed(0) }));
