# Earlier iteration, kept for reference only (paths are from the original scratch session).
"""Generates the LinkedIn carousel: one expressive SVG layer per slide
(rendered through %Vector) plus the HAML template and data on top."""
import base64, json, os, random

OUT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = "/home/user/pdfcn-rs"
INK, PAPER, RED, GREY, DIM = "#111111", "#EFEAE0", "#D7261E", "#8A857C", "#4A4640"
W = 1080
N = 9  # slides

# ---------------------------------------------------------------- SVG helpers
def svg(body, bg):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {W}" width="{W}" height="{W}">'
            f'<rect width="{W}" height="{W}" fill="{bg}"/>{body}</svg>')

def t(x, y, s, txt, fill=None, stroke=None, sw=2, rot=0, w="bold", op=1, ls=0, anchor="start"):
    st = f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ""
    tr = f' transform="rotate({rot} {x} {y})"' if rot else ""
    return (f'<text x="{x}" y="{y}" font-family="Inter" font-weight="{w}" font-size="{s}" '
            f'fill="{fill or "none"}"{st}{tr} opacity="{op}" letter-spacing="{ls}" '
            f'text-anchor="{anchor}">{txt}</text>')

def dots(c, step=36, r=1.3, op=0.5):
    return "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="{c}" opacity="{op}"/>'
                   for x in range(step // 2, W, step) for y in range(step // 2, W, step))

def micro(c, items, op=0.85):
    return "".join(t(x, y, 11, s, fill=c, w="normal", op=op, ls=1.5) for x, y, s in items)

def cross(x, y, c, n=14):
    return f'<path d="M{x-n} {y}H{x+n}M{x} {y-n}V{y+n}" stroke="{c}" stroke-width="1.5"/>'

def line(x1, y1, x2, y2, c, w=1.5, op=1):
    return f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{c}" stroke-width="{w}" opacity="{op}"/>'

S = {}

# 01 — the question
b = dots("#2E2E2E")
b += t(1180, 1130, 1100, "?", fill=RED, rot=8, anchor="end")
b += t(-40, 1060, 420, "PDF", stroke="#3A3A3A", sw=2, rot=-9)
b += t(1050, 40, 15, "NO BROWSER · NO CHROMIUM · NO PUPPETEER · NO NODE · NO SPAWN · NO NETWORK ·",
       fill=PAPER, w="normal", rot=90, ls=4, op=0.7)
b += cross(1000, 180, RED) + cross(140, 980, PAPER)
b += micro(GREY, [(860, 96, "Q.01 / 09"), (860, 114, "RESPUESTA → SIGUIENTE")])
S["01"] = svg(b, INK)

# 02 — the answer: zero
b = dots("#C9C2B5")
b += f'<circle cx="1010" cy="880" r="400" fill="none" stroke="{RED}" stroke-width="150"/>'
b += t(600, 560, 150, "Chrome", fill=INK, rot=-6, op=0.12)
b += f'<path d="M580 530 L1080 474" stroke="{RED}" stroke-width="20"/>'
b += micro(GREY, [(72, 1010, "HEADLESS_BROWSER = NULL · SPAWN() = 0 · FETCH() = 0")])
S["02"] = svg(b, PAPER)

# 03 — the language you already speak: className, minus "Name"
b = dots("#C9C2B5")
b += t(-20, 1010, 210, "className", stroke="#BFB8AB", sw=2, rot=-7)
b += f'<path d="M480 990 L1010 900" stroke="{RED}" stroke-width="18"/>'
b += micro(GREY, [(820, 60, "MISMAS CLASES"), (820, 78, "MISMOS COMPONENTES"), (820, 96, "CURVA DE APRENDIZAJE ≈ 0")])
S["03"] = svg(b, PAPER)

# 04 — write in what you already know: extensions colliding behind the cards
b = dots("#262626")
for txt, x, y, s, r in [(".md", 700, 330, 260, -10), (".tex", -60, 1060, 300, -6),
                        (".typ", 620, 1120, 280, 8), ("&lt;html&gt;", 380, 690, 200, -4)]:
    b += t(x, y, s, txt, stroke="#2F2F2F", sw=2, rot=r)
b += micro(GREY, [(820, 60, "7 FRONT-ENDS"), (820, 78, "1 AST · 0 DIFERENCIAS")])
S["04"] = svg(b, INK)

# 05 — seven inputs, one AST, one PDF (yugop-style precise diagram)
b = dots("#C9C2B5")
formats = ["HAML", "HTML", "Markdown", "JSON", "YAML", "Typst", "LaTeX"]
nx, ny = 520, 720
for i, f in enumerate(formats):
    y = 430 + i * 72
    b += t(72, y + 9, 28, f, fill=INK if i else RED)
    b += t(250, y + 7, 11, f"0{i+1}", fill=GREY, w="normal", ls=1.5)
    b += f'<path d="M285 {y} C 400 {y}, 420 {ny}, {nx-60} {ny}" fill="none" stroke="{INK}" stroke-width="1.5"/>'
    b += f'<circle cx="285" cy="{y}" r="4" fill="{INK}"/>'
b += f'<circle cx="{nx}" cy="{ny}" r="60" fill="{RED}"/>' + t(nx, ny + 10, 28, "AST", fill=PAPER, anchor="middle")
stages = [(650, "plantilla"), (740, "componentes"), (830, "estilos"), (920, "layout")]
b += line(nx + 60, ny, 1000, ny, INK, 3)
for x, lab in stages:
    b += f'<rect x="{x-7}" y="{ny-7}" width="14" height="14" fill="{PAPER}" stroke="{INK}" stroke-width="3"/>'
    b += t(x + 4, ny - 26, 17, lab, fill=INK, w="normal", rot=-55)
b += f'<rect x="980" y="{ny-40}" width="62" height="80" fill="{INK}"/>' + t(1011, ny + 8, 20, "PDF", fill=PAPER, anchor="middle")
b += micro(GREY, [(72, 966, "pdfcn-formats → pdfcn_parser::Document — interpolación, for/if, componentes e include iguales en todos"),
                  (72, 984, "Typst y LaTeX: subconjuntos documentados · YAML/JSON: un árbol de bloques, lo que emite una API o un LLM")])
S["05"] = svg(b, PAPER)

# 06 — live components
rng = random.Random(7)
b = dots("#2A2A2A", 24, 1.0)
for i in range(46):
    h = rng.randint(10, 120)
    b += f'<rect x="{40+i*22}" y="{1080-h}" width="6" height="{h}" fill="{RED if i % 7 == 0 else "#2E2E2E"}"/>'
b += t(1060, 60, 170, "VECTOR", stroke="#333", sw=2, rot=90)
S["06"] = svg(b, INK)

# 07 — twenty real examples, Carson collage of the prerendered thumbnails
names = ["invoice", "fmtMarkdown", "dashboard", "poster", "catalog", "fmtHtml", "charts",
         "certificate", "brochure", "fmtTypst", "codes", "pricing", "ticket", "fmtLatex",
         "verifactu", "brochureBleed", "icons", "fmtBlocks", "links", "utilities"]
b = dots("#C9C2B5")
b += t(1110, 1150, 600, "20", fill=RED, anchor="end", rot=-6)
rng = random.Random(20)
tw, th = 127, 180
for i, nm in enumerate(names):
    col, row = i % 7, i // 7
    x = 40 + col * 136 + (row % 2) * 30 + rng.randint(-6, 6)
    y = 410 + row * 200 + rng.randint(-8, 8)
    r = rng.uniform(-8, 8)
    data = base64.b64encode(open(f"{REPO}/thumbs/{nm}.jpg", "rb").read()).decode()
    fmt = nm.startswith("fmt")
    label = (nm[3:] if fmt else nm).upper()
    cx, cy = x + tw / 2, y + th / 2
    b += (f'<g transform="rotate({r:.1f} {cx:.0f} {cy:.0f})">'
          f'<rect x="{x+8}" y="{y+8}" width="{tw}" height="{th}" fill="{INK}" opacity="0.35"/>'
          f'<image href="data:image/jpeg;base64,{data}" x="{x}" y="{y}" width="{tw}" height="{th}" preserveAspectRatio="xMidYMin slice"/>'
          f'<rect x="{x}" y="{y}" width="{tw}" height="{th}" fill="none" stroke="{INK}" stroke-width="1.5"/>'
          f'<rect x="{x}" y="{y+th-20}" width="{tw}" height="20" fill="{RED if fmt else INK}"/>'
          + t(x + 6, y + th - 6, 9, f"{i+1:02d} {label}", fill=PAPER, w="normal", ls=1.5) + "</g>")
S["07"] = svg(b, PAPER)

# 08 — production
b = ""
for w_, x, y, s, r in [("FACTUR-X", -30, 300, 250, -12), ("VERI*FACTU", -80, 620, 230, 7),
                       ("EN 16931", 300, 980, 260, -4), ("XML", 700, 420, 300, 18)]:
    b += t(x, y, s, w_, stroke=INK, sw=2.2, rot=r, op=0.5)
b += t(560, 1060, 420, "§", fill=INK, op=0.9)
b += dots("#F2A29C", op=0.6)
S["08"] = svg(b, RED)

# 09 — meta / CTA
b = dots("#262626")
b += t(-40, 1215, 420, "pdfcn", fill=PAPER, rot=-4)
b += t(-20, 1185, 420, "pdfcn", stroke=RED, sw=3, rot=-4)
b += cross(1000, 180, RED)
b += micro(GREY, [(840, 96, "THIS FILE WAS COMPILED,"), (840, 114, "NOT PRINTED.")])
S["09"] = svg(b, INK)

for k, v in S.items():
    open(f"{OUT}/bg{k}.svg", "w").write(v)

# ---------------------------------------------------------------- template
MONO = "font-family:'JetBrains Mono'"
SERIF = "font-family:'Source Serif 4';font-style:italic"

def slide(n, fg, body):
    return f'''%div.relative(style="width:1080px;height:1078px;overflow:hidden")
  %Vector(id="bg{n}" w="1080px" h="1070px" alt="Composición tipográfica {n}")
{body}  %div.absolute(style="left:72px;bottom:44px;width:936px")
    .flex.justify-between
      %p(style="{MONO};font-size:15px;line-height:18px;letter-spacing:3px;color:{fg}") PDFCN-RS
      %p(style="{MONO};font-size:15px;line-height:18px;letter-spacing:3px;color:{fg}") {n} — 0{N}
'''

def at(top, left, width, inner):
    return f'  %div.absolute(style="top:{top}px;left:{left}px;width:{width}px")\n' + inner

def p(style, txt, ind=4):
    return " " * ind + f'%p(style="{style}") {txt}\n'

K = lambda c: f"{MONO};font-size:18px;line-height:22px;letter-spacing:4px;color:{c}"
BOLD = lambda c, s=84: f"font-weight:bold;font-size:{s}px;line-height:{s-4}px;letter-spacing:-4px;color:{c}"
ITAL = lambda c, s=84: f"{SERIF};font-size:{s}px;line-height:{s}px;color:{c};margin-top:-{s//4}px"

T = []
T.append(slide("01", GREY,
    at(72, 72, 700, p(K(RED), "[01] UNA PREGUNTA")) +
    at(150, 72, 760,
       p(BOLD(PAPER, 92), "¿Cuántos navegadores") +
       p(ITAL(PAPER, 92), "hacen falta para") +
       p(BOLD(RED, 92), "generar un PDF?")) +
    at(930, 72, 420, p(f"{MONO};font-size:16px;line-height:24px;letter-spacing:3px;color:{PAPER}", "DESLIZA →"))))

T.append(slide("02", GREY,
    at(72, 72, 600, p(K(RED), "[02] LA RESPUESTA") +
       p(BOLD(INK, 150) + ";margin-top:16px", "Ninguno.") +
       p(ITAL(DIM, 32) + ";line-height:40px;margin-top:8px;width:480px",
         "pdfcn compila el documento a bytes de PDF. En Rust, en memoria, sin intermediarios.")) +
    at(620, 72, 520, f'''    - for b in brutal
      .flex.items-center(style="border-top:2px solid {INK};height:84px")
        %p(style="font-weight:bold;font-size:56px;line-height:60px;letter-spacing:-2px;width:72px;color:{RED}") {{{{ b.k }}}}
        %div
          %p(style="font-weight:bold;font-size:24px;line-height:28px;color:{INK}") {{{{ b.t }}}}
          %p(style="{SERIF};font-size:20px;line-height:24px;color:{DIM}") {{{{ b.s }}}}
''')))

def code(top, left, bg, fg, accent, key):
    return at(top, left, 452, f'''    %div(style="background:{bg};border:2px solid {accent};padding:14px 20px")
      %p(style="{MONO};font-size:13px;line-height:16px;letter-spacing:3px;color:{accent};margin-bottom:10px") {{{{ sx.{key}.label }}}}
      - for l in sx.{key}.code
        %p(style="{MONO};font-size:14px;line-height:22px;color:{fg};white-space:pre") {{{{ l }}}}
''')

T.append(slide("03", GREY,
    at(72, 72, 936, p(K(RED), "[03] Y SIN EMBARGO") +
       p(BOLD(INK, 76) + ";margin-top:24px", "La semántica de la web,") +
       p(ITAL(RED, 76), "sin el navegador.")) +
    code(376, 72, INK, PAPER, GREY, "app") +
    code(376, 556, PAPER, INK, RED, "pdf") +
    at(572, 72, 936, f'''    %p(style="font-weight:bold;font-size:38px;line-height:44px;letter-spacing:-1px;color:{INK}") ¿Sabes Tailwind y shadcn? Ya sabes pdfcn.
    .flex(style="margin-top:20px")
      - for k in kpis
        %Card(title="{{{{ k.label }}}}" class="w-full m-1")
          %p.text-3xl.font-bold {{{{ k.value }}}}
          .flex(style="margin-top:8px")
            %Badge(variant="{{{{ k.variant }}}}" label="{{{{ k.delta }}}}")
    %p(style="{MONO};font-size:13px;line-height:18px;letter-spacing:3px;color:{DIM};margin-top:12px") ↑ %Card Y %Badge RENDERIZADOS POR PDFCN, NO UNA CAPTURA
''')))

def card(top, left, bg, fg, accent, key):
    return at(top, left, 420, f'''    %div(style="background:{bg};border:2px solid {accent};padding:12px 18px 12px 18px")
      %p(style="{MONO};font-size:13px;line-height:16px;letter-spacing:3px;color:{accent};margin-bottom:8px") {{{{ fmt.{key}.label }}}}
      - for l in fmt.{key}.code
        %p(style="{MONO};font-size:15px;line-height:22px;color:{fg};white-space:pre") {{{{ l }}}}
''')

T.append(slide("04", GREY,
    at(72, 72, 900, p(K(RED), "[04] LA ENTRADA") +
       p(BOLD(PAPER) + ";margin-top:24px", "Escribe en lo que") +
       p(ITAL(RED), "ya sabes escribir.")) +
    card(392, 72, PAPER, INK, INK, "haml") +
    card(424, 560, INK, PAPER, RED, "html") +
    card(570, 104, RED, INK, INK, "md") +
    card(602, 528, PAPER, INK, INK, "yaml") +
    card(748, 72, INK, PAPER, PAPER, "typst") +
    card(780, 560, RED, INK, INK, "latex")))

T.append(slide("05", GREY,
    at(72, 72, 936, p(K(RED), "[05] UN SOLO ÁRBOL") +
       p(BOLD(INK) + ";margin-top:24px", "Siete entradas.") +
       p(ITAL(RED), "Un AST. El mismo PDF."))))

cap = lambda t_, c: f'''        %p(style="{MONO};font-size:14px;line-height:18px;letter-spacing:2px;color:{c};margin-bottom:8px") {t_}\n'''
T.append(slide("06", GREY,
    at(72, 72, 820, p(K(RED), "[06] LA PRUEBA") +
       p(BOLD(PAPER, 72) + ";margin-top:24px", "Gráficos y códigos.") +
       p(ITAL(RED, 44) + ";line-height:52px", "Componentes, no capturas.")) +
    at(400, 72, 840, f'''    .flex
      %div(style="background:{PAPER};padding:20px;width:470px;margin-right:16px")
{cap("%LineChart", GREY)}        %LineChart(values={{{{ revenue }}}} xlabels={{{{ months }}}} colors={{{{ c_red }}}} w="470px" h="200px")
      %div(style="background:{RED};padding:20px;width:270px;margin-top:60px")
{cap("%PieChart", INK)}        %PieChart(values={{{{ mix }}}} labels={{{{ mix_labels }}}} donut="true" colors={{{{ c_pie }}}} w="270px" h="200px")
    .flex(style="margin-top:16px")
      %div(style="background:{PAPER};padding:20px;width:150px;margin-right:16px;margin-left:120px")
{cap("%QRCode", GREY)}        %QRCode(value="https://github.com/pmaojo/pdfcn-rs" size="150px")
      %div(style="background:{PAPER};padding:20px;width:320px")
{cap("%Barcode EAN-13", GREY)}        %Barcode(scheme="ean13" value="400638133393" w="320px" h="130px")
''')))

T.append(slide("07", GREY,
    at(72, 72, 936, p(K(RED), "[07] VEINTE EJEMPLOS · examples/") +
       p(BOLD(INK) + ";margin-top:24px", "Facturas, pósters,") +
       p(ITAL(RED), "folletos, entradas…"))))

T.append(slide("08", INK,
    at(72, 72, 900, p(K(INK), "[08] PRODUCCIÓN") +
       p(BOLD(INK) + ";margin-top:24px", "Facturas") +
       p(ITAL(PAPER), "que cumplen la norma.")) +
    at(420, 340, 668, f'''    - for e in extras
      .flex(style="background:{INK};padding:24px 24px 0 24px;margin-bottom:10px;height:62px")
        %p(style="font-weight:bold;font-size:28px;line-height:34px;letter-spacing:-1px;color:{PAPER};width:200px") {{{{ e.t }}}}
        %p(style="{SERIF};font-size:21px;line-height:26px;color:#C9C2B5;width:420px;padding-top:4px") {{{{ e.d }}}}
''')))

T.append(slide("09", GREY,
    at(72, 72, 760, p(K(RED), "[09] POSDATA") +
       p(ITAL(PAPER, 80) + ";margin-top:24px", "Este PDF no se imprimió.") +
       p(BOLD(RED, 92), "Se compiló.")) +
    at(500, 72, 700, f'''    %div(style="border:2px solid {PAPER};padding:20px 28px;width:640px")
      %p(style="{MONO};font-size:18px;line-height:28px;color:{PAPER}") $ pdfcn build carousel.haml -d data.json
      %p(style="{MONO};font-size:18px;line-height:28px;color:{GREY}") $ pdfcn build informe.md -d datos.json
    .flex.items-center(style="margin-top:28px")
      %div(style="background:{PAPER};padding:10px")
        %QRCode(value="https://github.com/pmaojo/pdfcn-rs" size="110px")
      %div(style="margin-left:28px")
        %p(style="{SERIF};font-size:24px;line-height:30px;color:#C9C2B5") La semántica de la web, sin el navegador · MIT
        %p(style="font-weight:bold;font-size:36px;line-height:42px;letter-spacing:-1px;color:{PAPER}") github.com/pmaojo/pdfcn-rs
''')))

open(f"{OUT}/carousel.haml", "w").write("\n.break-before-page\n".join(T))

# ---------------------------------------------------------------- data
data = {
    "brutal": [
        {"k": "0", "t": "navegadores headless", "s": "ni Chromium, ni Puppeteer"},
        {"k": "0", "t": "procesos lanzados", "s": "nada de spawn, nada de wkhtmltopdf"},
        {"k": "0", "t": "peticiones de red", "s": "las imágenes llegan como bytes"},
        {"k": "1", "t": "binario Rust", "s": "cabe en una Vercel Function"},
    ],
    "sx": {
        "app": {"label": "TU APP · React + shadcn", "code": ['<div className="grid grid-cols-3 gap-4">', '  <Card className="p-4">', '    <p className="text-3xl font-bold">', '  <Badge variant="outline">']},
        "pdf": {"label": "TU PDF · pdfcn (.html)", "code": ['<div class="grid grid-cols-3 gap-4">', '  <Card class="p-4">', '    <p class="text-3xl font-bold">', '  <Badge variant="outline" label="…"/>']},
    },
    "kpis": [
        {"label": "Ingresos", "value": "48.200 €", "delta": "+12 %", "variant": "success"},
        {"label": "Clientes nuevos", "value": "31", "delta": "+4", "variant": "outline"},
        {"label": "Bajas", "value": "2,1 %", "delta": "+0,3 pt", "variant": "destructive"},
    ],
    "fmt": {
        "haml": {"label": "HAML · .haml", "code": ['%Header(title="{{ empresa }}")', "- for k in kpis", '  %Card(title="{{ k.label }}")']},
        "html": {"label": "HTML · .html", "code": ['<Header title="{{ empresa }}"/>', '<div data-for="k in kpis">', '  <Card title="{{ k.label }}"/>']},
        "md": {"label": "MARKDOWN · .md", "code": ["# {{ title }}", "- Ingresos **+12 %**", "| Proyecto | Estado |"]},
        "yaml": {"label": "YAML / JSON · árbol de bloques", "code": ["- type: Header", '  title: "{{ empresa }}"', "- for: k in kpis"]},
        "json": {"label": "JSON · .json", "code": ['[{ "type": "Header",', '   "title": "{{ empresa }}" }]']},
        "typst": {"label": "TYPST · .typ", "code": ["= Informe mensual", "- Ingresos *+12 %*", "#table(columns: 3, ...)"]},
        "latex": {"label": "LATEX · .tex", "code": ["\\section{Resumen}", "\\textbf{+12\\%} de ingresos"]},
    },
    "revenue": [42, 51, 47, 63, 71, 68, 84],
    "months": ["ABR", "MAY", "JUN", "JUL", "AGO", "SEP", "OCT"],
    "mix": [46, 30, 24],
    "mix_labels": ["API", "CLI", "Node"],
    "c_red": [RED],
    "c_pie": [INK, PAPER, "#7A0300"],
    "extras": [
        {"t": "Factur-X", "d": "XML EN 16931 dentro del PDF"},
        {"t": "Veri*factu", "d": "QR fiscal por perfiles JSON"},
        {"t": "Serverless", "d": "una Vercel Function, sin Chrome"},
        {"t": "Next.js", "d": "bindings napi-rs, sin HTTP"},
    ],
}
json.dump(data, open(f"{OUT}/data.json", "w"), ensure_ascii=False, indent=2)
