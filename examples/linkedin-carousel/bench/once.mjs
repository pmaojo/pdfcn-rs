// One cold process: import, read data, render, write. What a cold serverless start pays.
import fs from "node:fs";
import React from "react";
import { renderToBuffer } from "@react-pdf/renderer";
import { Invoice } from "./invoice.mjs";
const REPO = decodeURIComponent(new URL("../../../", import.meta.url).pathname).replace(/\/$/, "");

const data = JSON.parse(fs.readFileSync(`${REPO}/examples/invoice.json`, "utf8"));
const buf = await renderToBuffer(React.createElement(Invoice, data));
fs.writeFileSync(process.argv[2] || "out.pdf", buf);
