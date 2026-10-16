# Carrusel de LinkedIn de pdfcn

Carrusel cuadrado (1080×1080, 9 slides) que presenta pdfcn, **generado con el
propio pdfcn**. Estética vorticista / brutalista (Carson, Sagmeister, yugop):
negro, papel y bermellón; Inter Bold, Source Serif 4 Italic y JetBrains Mono.

```sh
./build.sh            # → out/pdfcn-linkedin.pdf
GRAIN=1 ./build.sh    # con grano risográfico (rasterizado: ~40 MB)
```

## Cómo está hecho

- `make.py` genera en `out/` una capa SVG expresiva por slide (`bg01.svg`…),
  la plantilla `carousel.haml` y `data.json`.
- Cada slide es un `%Vector` a página completa con el texto legible y los
  componentes reales (`%Card`, `%Badge`, `%LineChart`, `%QRCode`…) encima, en
  bloques `.absolute`.
- Desde el spike 004 las capas SVG llegan al PDF como **vectores**. Las únicas
  imágenes raster son las 20 miniaturas de `thumbs/` del collage.

## Narrativa (3 actos)

1. **Tailwind + shadcn → PDF.** En milisegundos, sin navegador, sin aprender nada nuevo.
2. **Compílalo.** 0 navegadores, 0 procesos, 0 red, 1 binario.
3. **68 ms en frío.** Benchmark medido (ver abajo).
4. **Sin lenguajes nuevos:** tu app (React + shadcn) frente a tu PDF.
5. **Cero lock-in:** 7 entradas → un AST → un PDF.
6. La prueba (gráficos, QR, código de barras) · 7. Veinte ejemplos ·
8. Producción (Factur-X, Veri\*factu) · 9. «Este PDF no se imprimió. Se compiló.»

`history/` guarda las iteraciones anteriores (v2: pregunta inicial; v3: antes
de la slide de velocidad).

## Benchmark (`bench/`)

Factura de `examples/invoice` en cinco herramientas, mediana de 15 procesos,
4 núcleos (octubre 2026):

| | En frío | En caliente |
|---|---|---|
| pdfcn | 68 ms | 34 ms |
| Typst 0.15 | 74 ms | 2,9 ms |
| Playwright + Chromium | 718 ms | 96 ms |
| react-pdf 4.9 (Inter) | 733 ms | 32 ms |
| WeasyPrint 70 | 1661 ms | 557 ms |

`bench/catalog/`: catálogo de 100 fotos + 100 SVG. Era el punto débil
(≈18,6 s frente a 0,56 s de Typst): cada foto se decodificaba tres veces y se
re-codificaba a JPEG dos. Desde el spike 005 los JPEG van al PDF tal cual,
como en Typst: **≈0,4 s** en este contenedor (4 núcleos; aún sin re-medir las
demás herramientas aquí).

Requisitos para reproducir: `npm i @react-pdf/renderer@4.9.0 react@19
playwright-core` y un venv con `typst weasyprint` dentro de `bench/`;
`bench/invoice.html` es el HTML que sirve `pdfcn dev` para la factura, sin el
script de recarga. `CHROMIUM=` apunta a otro binario de Chromium.

## Pendiente

- Re-medir el catálogo con todas las herramientas tras el spike 005.
- Revisar el texto de la slide 03 si cambian las cifras.
