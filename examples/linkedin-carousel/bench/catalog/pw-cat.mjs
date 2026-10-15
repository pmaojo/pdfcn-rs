// The catalog through Playwright + headless Chromium (cold: launch, render, close).
import fs from "node:fs";
import path from "node:path";
import { chromium } from "../node_modules/playwright-core/index.mjs";

const dir = path.dirname(new URL(import.meta.url).pathname);
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || "/opt/pw-browsers/chromium_headless_shell-1194/chrome-linux/headless_shell",
});
const page = await browser.newPage();
await page.goto(`file://${dir}/catalog.html`, { waitUntil: "load" });
fs.writeFileSync(process.argv[2] || `${dir}/pw.pdf`, await page.pdf({ format: "A4" }));
await browser.close();
