# Spike 001 — Sustrato vectorial vs. rasterizado (Ola 1.1)

Estado: **cerrado — se elige rasterizado (resvg @ ~300dpi)**.
Fecha: 2026-08-22 · Ola: 1.1 · Bloquea: 1.2 (sustrato `%Vector`), 1.3 (generadores).

## Regla de decisión (escrita antes de medir)

El camino **vectorial** gana si y solo si se cumplen las tres:

1. Un `<svg>` inline sobrevive al bridge HTML de printpdf y llega al PDF
   colocado en su caja de layout (no hace falta colocar un `ExternalXObject`
   a mano, cosa que `from_html_with_cache` no permite: esconde la geometría
   de página).
2. Compilar `azul-layout/svg` + `azul-layout/cpurender` cabe en el presupuesto
   de CI sin romper el gate de tamaño del binario por defecto (la feature debe
   poder quedar **apagada por defecto**).
3. La fidelidad visual del resultado es comparable a la del camino rasterizado
   sobre el corpus golden.

Si cualquiera falla, gana el **rasterizado**: resvg a ~300dpi reusando el
terreno conocido de `prepare_assets`.

## Evidencia

Verificada directamente contra el upstream publicado (docs.rs, printpdf
0.12.6), no contra memoria:

- La feature `svg` existe tal como describía el roadmap y arrastra lo esperado:
  `svg = ["html", "azul-layout/cpurender", "azul-layout/svg", "dep:svg2pdf"]`
  (página de feature flags de docs.rs/crate/printpdf/0.12.6).
- El módulo `printpdf::html` — el único camino que este crate usa
  (`from_html_with_cache`) — declara su pipeline como *XML/HTML parse
  (azul_layout::xml) → layout → `DisplayList → PDF ops` (azul_layout::pdf) →
  bridge*. Ningún elemento de la superficie pública del módulo menciona `Svg`,
  `svg2pdf` ni `ExternalXObject`; `xml_to_pdf_pages` devuelve los recursos
  `<img>` **decodificados**, es decir raster.
- Conclusión sobre la pregunta 1 (la que el roadmap marcaba como primera):
  no hay ruta observable de un `<svg>` inline hacia el PDF. `Svg::parse`
  produce un `ExternalXObject`, pero colocarlo requeriría coordenadas de página
  que `from_html_with_cache` no expone; usarlo significaría abandonar el
  pipeline HTML por documento o parchear upstream. La regla de decisión falla
  en su condición 1.
- Coste de compilar `cpurender` + `svg2pdf`: no llegó a medirse porque la
  condición 1 ya descarta el camino; queda anotado como dato pendiente si el
  upstream alguna vez cablea SVG en el bridge (ver "Reapertura").

## Decisión

**Sustrato rasterizado.** Un pase `pdfcn-svg:` en el post-render de assets
(`prepare_assets`), calcado del patrón probado de `%QRCode`:

- Los generadores (charts v2, códigos de barras, y el `%Vector(svg=…)`
  genérico que cubre math/partituras/CAD traídos por el cliente) emiten SVG.
- El pase rasteriza cada SVG a PNG con **resvg** a la resolución de su caja ×
  `MAX_PRINT_SCALE` (~300dpi, el mismo criterio que ya normaliza fotos) y lo
  registra bajo el `src` exacto del placeholder, igual que los QR.
- Todo tras una cargo feature **`vector`, apagada por defecto**: el binario de
  Vercel sale byte a byte equivalente al de hoy y el gate de tamaño de CI
  sigue siendo la prueba.

## Reapertura

El camino vectorial se reabre solo si printpdf cablea SVG en su bridge HTML
(p. ej. resolviendo `<svg>` inline a `ExternalXObject` dentro del display
list). En ese momento el sustrato cambia de backend sin tocar componentes ni
generadores: los placeholders y las especificaciones son independientes del
renderizador.
