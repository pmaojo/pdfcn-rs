// Playwright + headless Chromium: the "browser route", fed pdfcn's own HTML+CSS.
// `cold`: launch the browser, render once, close (a cold serverless start).
// `warm`: one browser kept alive, a fresh page per document (a warm server).
import fs from "node:fs";
import { chromium } from "playwright-core";

const html = fs.readFileSync(new URL("./invoice.html", import.meta.url), "utf8");
const exe = process.env.CHROMIUM || "/opt/pw-browsers/chromium_headless_shell-1194/chrome-linux/headless_shell";
const one = async (browser) => {
  const page = await browser.newPage();
  await page.setContent(html);
  const bytes = await page.pdf({ format: "A4" });
  await page.close();
  return bytes;
};

const browser = await chromium.launch({ executablePath: exe });
if (process.argv[2] === "warm") {
  for (let i = 0; i < 5; i++) await one(browser);
  const t = [];
  for (let i = 0; i < 50; i++) {
    const s = process.hrtime.bigint();
    await one(browser);
    t.push(Number(process.hrtime.bigint() - s) / 1e6);
  }
  t.sort((a, b) => a - b);
  console.log(JSON.stringify({ median_ms: +t[25].toFixed(1), p95_ms: +t[47].toFixed(1) }));
} else {
  fs.writeFileSync(new URL("./pw.pdf", import.meta.url), await one(browser));
}
await browser.close();
