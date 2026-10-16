"""Reproducible render benchmark for pdfcn (docs/prompts/rendimiento-y-arquitectura.md).

Builds the fixtures under target/bench/ (never committed), then runs the
release CLI on each one N times in fresh processes, reporting the median wall
time and the peak RSS (from wait4's rusage, so no /usr/bin/time is needed).

    python3 -I scripts/bench/run.py               # all cases, N=7
    python3 -I scripts/bench/run.py catalog -n 15 # one case
    python3 -I scripts/bench/run.py --json out.json

Cases:
    catalog   100 JPEG photos + 100 SVG marks (examples/linkedin-carousel/bench/catalog)
    invoice   examples/invoice
    one-page  a single page of text
    long      ~200 pages of text
    links     the long-ish document with 50 hyperlinks
    nolinks   the same document with the links turned into plain spans

The catalog fixture needs Pillow (python3 -m pip install pillow) the first
time only; the generated photos are cached in target/bench/catalog.
"""
import argparse, json, os, shutil, statistics, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
OUT = os.path.join(REPO, "target", "bench")
PDFCN = os.path.abspath(os.environ.get("PDFCN", os.path.join(REPO, "target", "release", "pdfcn")))


def catalog():
    d = os.path.join(OUT, "catalog")
    if not os.path.exists(os.path.join(d, "catalog.json")):
        os.makedirs(os.path.join(d, "img"), exist_ok=True)
        gen = os.path.join(REPO, "examples", "linkedin-carousel", "bench", "catalog", "gen.py")
        # gen.py resolves the source photos relative to its own location and
        # writes everything into its working directory.
        src = open(gen).read().replace(
            'os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "assets", "popcorn")',
            repr(os.path.join(REPO, "examples", "assets", "popcorn")),
        )
        subprocess.run([sys.executable, "-I", "-c", src], cwd=d, check=True, capture_output=True)
    svg = []
    for i in range(100):
        svg += ["--svg", f"s{i:03d}=img/s{i:03d}.svg"]
    return d, ["build", "catalog.haml", "-d", "catalog.json", "-o", "out.pdf", *svg]


def invoice():
    ex = os.path.join(REPO, "examples")
    return ex, ["build", "invoice.haml", "-d", "invoice.json", "-o", os.path.join(OUT, "invoice.pdf")]


PARA = ("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor "
        "incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud "
        "exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. ")


def text_doc(name, pages, links=None):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    # ~6 paragraphs of 4 sentences fill an A4 page at the default size.
    sections = [{"id": f"s{i}", "title": f"Section {i + 1}", "body": PARA * 4} for i in range(pages * 2)]
    haml = ['%DocumentLayout(size="a4")', '  %h1.text-2xl.font-bold.mb-4 {{ title }}']
    if links is not None:
        tag = "%a.text-indigo-600.underline" if links else "%span.text-indigo-600.underline"
        haml += ["  .mb-6", "    - for l in links", f'      %p.mb-1', f'        {tag}(href="{{{{ l.href }}}}") {{{{ l.text }}}}']
    haml += ["  - for s in sections", "    %h2.text-lg.font-semibold(id=\"{{ s.id }}\") {{ s.title }}",
             "    %p.text-sm.mb-2 {{ s.body }}", "    %p.text-sm.mb-2 {{ s.body }}", "    %p.text-sm.mb-2 {{ s.body }}"]
    data = {"title": name, "sections": sections}
    if links is not None:
        data["links"] = [{"href": f"https://example.com/{i}" if i % 2 else f"#s{i}", "text": f"Link {i}"} for i in range(50)]
    open(os.path.join(d, "doc.haml"), "w").write("\n".join(haml) + "\n")
    json.dump(data, open(os.path.join(d, "doc.json"), "w"))
    return d, ["build", "doc.haml", "-d", "doc.json", "-o", "out.pdf"]


CASES = {
    "catalog": catalog,
    "invoice": invoice,
    "one-page": lambda: text_doc("one-page", 1),
    "long": lambda: text_doc("long", 150),
    "links": lambda: text_doc("links", 10, links=True),
    "nolinks": lambda: text_doc("nolinks", 10, links=False),
}


def run_once(cwd, args, env):
    start = time.perf_counter()
    p = subprocess.Popen([PDFCN, *args], cwd=cwd, env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    _, status, rusage = os.wait4(p.pid, 0)
    wall = (time.perf_counter() - start) * 1000
    err = p.stderr.read().decode(errors="replace")
    p.stderr.close()
    if status != 0:
        raise SystemExit(f"pdfcn failed in {cwd}: {err}")
    return wall, rusage.ru_maxrss / 1024  # KiB -> MiB on Linux


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cases", nargs="*", default=list(CASES))
    ap.add_argument("-n", type=int, default=7)
    ap.add_argument("--json")
    a = ap.parse_args()
    if not os.path.exists(PDFCN):
        raise SystemExit(f"{PDFCN} missing: cargo build --release -p pdfcn-cli")
    os.makedirs(OUT, exist_ok=True)
    env = dict(os.environ)
    results = {}
    for name in a.cases:
        cwd, args = CASES[name]()
        run_once(cwd, args, env)  # page cache warm-up
        walls, peaks = [], []
        for _ in range(a.n):
            w, m = run_once(cwd, args, env)
            walls.append(w)
            peaks.append(m)
        walls.sort()
        results[name] = {"median_ms": round(statistics.median(walls), 1), "min_ms": round(walls[0], 1),
                         "peak_mb": round(max(peaks), 1)}
        print(f"{name:10s} median {results[name]['median_ms']:8.1f} ms   min {results[name]['min_ms']:8.1f} ms"
              f"   peak {results[name]['peak_mb']:7.1f} MB", flush=True)
    if a.json:
        json.dump(results, open(a.json, "w"), indent=2)


if __name__ == "__main__":
    main()
