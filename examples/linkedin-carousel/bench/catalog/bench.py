import json, os, statistics, subprocess, time, resource
C = os.path.dirname(os.path.abspath(__file__)); PY = f"{C}/../venv/bin/python"
svg = open(f"{C}/svgflags.txt").read().split()
cases = {
  "pdfcn": [os.path.join(C, "..", "..", "..", "..", "target", "release", "pdfcn"), "build", "catalog.haml", "-d", "catalog.json", "-o", "k.pdf", *svg],
  "Typst": [PY, "-I", "-c", "import typst; typst.compile('catalog.typ', output='k.pdf')"],
  "react-pdf (sin SVG)": ["node", "catalog-rp.mjs", "k.pdf"],
  "Playwright + Chromium": ["node", "pw-cat.mjs", "k.pdf"],
  "WeasyPrint": [PY, "-I", "-c", "import weasyprint; weasyprint.HTML('catalog.html').write_pdf('k.pdf')"],
}
N = 7; out = {}
for name, cmd in cases.items():
    subprocess.run(cmd, cwd=C, capture_output=True, check=True)
    ts = []
    for _ in range(N):
        s = time.perf_counter(); subprocess.run(cmd, cwd=C, capture_output=True, check=True); ts.append((time.perf_counter() - s) * 1000)
    ts.sort(); out[name] = {"median_ms": round(statistics.median(ts)), "min_ms": round(ts[0]), "max_ms": round(ts[-1])}
    print(name, out[name], flush=True)
json.dump(out, open(f"{C}/results.json", "w"), indent=2, ensure_ascii=False)
