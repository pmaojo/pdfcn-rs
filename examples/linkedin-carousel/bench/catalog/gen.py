"""100-product catalog fixture: 100 distinct JPG crops of the repo's photos,
100 distinct SVG marks, and the same document for each tool."""
import json, random
from PIL import Image
import os
R = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "assets", "popcorn")
srcs = [Image.open(f"{R}/{n}.jpg").convert("RGB") for n in ["hand", "poster-sea", "poster-texture", "texture", "sea"]]
rng = random.Random(100)
cols = ["#D7261E", "#111111", "#2563eb", "#16a34a", "#d97706", "#7c3aed"]
products = []
for i in range(100):
    im = srcs[i % len(srcs)]
    w = rng.randint(int(im.width * 0.3), int(im.width * 0.9)); h = min(int(w * 0.75), im.height)
    w = int(h / 0.75)
    x = rng.randint(0, im.width - w); y = rng.randint(0, im.height - h)
    im.crop((x, y, x + w, y + h)).resize((800, 600)).save(f"img/p{i:03d}.jpg", quality=82)
    a, b_ = rng.sample(cols, 2); r = rng.randint(4, 30); k = rng.randint(3, 8)
    pts = " ".join(f"{48+40*__import__('math').cos(6.283*j/k):.1f},{48+40*__import__('math').sin(6.283*j/k):.1f}" for j in range(k))
    open(f"img/s{i:03d}.svg", "w").write(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96" width="96" height="96">'
        f'<rect width="96" height="96" rx="{r}" fill="{a}"/><polygon points="{pts}" fill="{b_}"/>'
        f'<circle cx="48" cy="48" r="{rng.randint(6,16)}" fill="#ffffff"/></svg>')
    products.append({"id": f"s{i:03d}", "name": f"Producto {i+1:03d}", "price": f"{rng.randint(9,299)},{rng.randint(0,99):02d} €",
                     "photo": f"img/p{i:03d}.jpg", "svg": f"img/s{i:03d}.svg"})
rows = [products[i:i + 3] for i in range(0, 100, 3)]
json.dump({"title": "Catálogo · 100 productos", "rows": rows, "products": products}, open("catalog.json", "w"), ensure_ascii=False)

# pdfcn (HAML). Photos resolve from disk next to the template; SVGs via %Vector + --svg.
open("catalog.haml", "w").write('''%DocumentLayout(size="a4")
  %h1.text-2xl.font-bold.mb-4 {{ title }}
  - for row in rows
    .flex.break-inside-avoid
      - for p in row
        .border.rounded-lg.m-1(style="width:200px;padding:8px")
          %img(src="{{ p.photo }}" style="width:200px;height:150px;object-fit:cover")
          .flex.items-center(style="margin-top:6px")
            %Vector(id="{{ p.id }}" w="32px" h="32px" alt="logo")
            %div(style="margin-left:8px")
              %p.font-semibold.text-sm {{ p.name }}
              %p.text-xs {{ p.price }}
''')

# Plain HTML for Chromium / WeasyPrint: same structure, same files.
cards = "".join(f'<div class="c"><img class="ph" src="{p["photo"]}"><div class="r"><img class="lg" src="{p["svg"]}">'
                f'<div><b>{p["name"]}</b><br><small>{p["price"]}</small></div></div></div>' for p in products)
open("catalog.html", "w").write(f'''<!doctype html><html><head><meta charset="utf-8"><style>
@page {{ size: A4; margin: 10mm }} body {{ font-family: Inter, sans-serif; margin: 0 }}
.g {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px }}
.c {{ border: 1px solid #e2e8f0; border-radius: 8px; padding: 8px; break-inside: avoid }}
.ph {{ width: 100%; height: 150px; object-fit: cover; display: block }}
.r {{ display: flex; align-items: center; gap: 8px; margin-top: 6px; font-size: 13px }} .lg {{ width: 32px; height: 32px }}
</style></head><body><h1>Catálogo · 100 productos</h1><div class="g">{cards}</div></body></html>''')

# Typst
open("catalog.typ", "w").write('''#let d = json("catalog.json")
#set page(paper: "a4", margin: 10mm)
#set text(font: "Inter", size: 10pt)
#text(size: 18pt, weight: "bold")[#d.title]
#grid(columns: (1fr, 1fr, 1fr), gutter: 8pt,
  ..d.products.map(p => block(stroke: rgb("#e2e8f0"), radius: 8pt, inset: 8pt, breakable: false)[
    #image(p.photo, width: 100%, height: 110pt, fit: "cover")
    #grid(columns: (24pt, 1fr), gutter: 6pt, align: horizon,
      image(p.svg, width: 24pt), [*#p.name* \\ #text(size: 8pt)[#p.price]])
  ]))
''')
print("ok", len(products))
