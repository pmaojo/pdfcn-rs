"""Invoice benchmark across PDF tools: cold process and warm in-process.

Cold = spawn a fresh process that loads the tool, renders examples/invoice
and writes the PDF (what a cold serverless start pays). Warm = the median of
many renders inside one already-running process (a warm server).
"""
import json, os, statistics, subprocess, sys, time

B = os.path.dirname(os.path.abspath(__file__))
R = os.path.abspath(os.path.join(B, "..", "..", ".."))
PY = f"{B}/venv/bin/python"
N = 15

COLD = {
    "pdfcn": ([f"{R}/target/release/pdfcn", "build", f"{R}/examples/invoice.haml",
               "-d", f"{R}/examples/invoice.json", "-o", f"{B}/k.pdf"], {}),
    "react-pdf": (["node", f"{B}/once.mjs", f"{B}/k.pdf"], {"FONT": "inter"}),
    "Typst": ([PY, "-I", "-c", "import typst; typst.compile('invoice.typ', output='k.pdf')"], {}),
    "WeasyPrint": ([PY, "-I", "-c", "import weasyprint; weasyprint.HTML('invoice.html').write_pdf('k.pdf')"], {}),
    "Playwright + Chromium": (["node", f"{B}/pw.mjs", "cold"], {}),
}

WARM_PY = {
    "Typst": """
import typst, time, json
c = typst.Compiler('invoice.typ')
for _ in range(5): c.compile()
t = []
for _ in range(50):
    s = time.perf_counter(); c.compile(); t.append((time.perf_counter() - s) * 1000)
t.sort(); print(json.dumps({'median_ms': round(t[25], 1)}))
""",
    "WeasyPrint": """
import weasyprint, time, json
html = open('invoice.html').read()
for _ in range(3): weasyprint.HTML(string=html).write_pdf()
t = []
for _ in range(30):
    s = time.perf_counter(); weasyprint.HTML(string=html).write_pdf(); t.append((time.perf_counter() - s) * 1000)
t.sort(); print(json.dumps({'median_ms': round(t[15], 1)}))
""",
}


def cold(name, cmd, env):
    e = dict(os.environ, **env)
    subprocess.run(cmd, env=e, cwd=B, capture_output=True, check=True)  # page cache warm-up
    ts = []
    for _ in range(N):
        s = time.perf_counter()
        subprocess.run(cmd, env=e, cwd=B, capture_output=True, check=True)
        ts.append((time.perf_counter() - s) * 1000)
    ts.sort()
    return {"median_ms": round(statistics.median(ts)), "min_ms": round(ts[0]), "max_ms": round(ts[-1])}


out = {"cold": {}, "warm": {}}
for name, (cmd, env) in COLD.items():
    out["cold"][name] = cold(name, cmd, env)
    print("cold", name, out["cold"][name], file=sys.stderr)
for name, code in WARM_PY.items():
    r = subprocess.run([PY, "-I", "-c", code], cwd=B, capture_output=True, text=True, check=True)
    out["warm"][name] = json.loads(r.stdout)
r = subprocess.run(["node", f"{B}/pw.mjs", "warm"], cwd=B, capture_output=True, text=True, check=True)
out["warm"]["Playwright + Chromium"] = json.loads(r.stdout)
r = subprocess.run(["node", f"{B}/warm.mjs"], cwd=B, capture_output=True, text=True, check=True,
                   env=dict(os.environ, FONT="inter"))
out["warm"]["react-pdf"] = json.loads(r.stdout)
json.dump(out, open(f"{B}/results.json", "w"), indent=2)
print(json.dumps(out, indent=2))
