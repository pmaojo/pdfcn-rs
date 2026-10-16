# Spike 007 — Enlaces con un solo maquetado

Estado: **cerrado — implementado** (fase B de
`docs/prompts/rendimiento-y-arquitectura.md`).

## Problema

`links` encontraba dónde cayó cada `<a>` con un render sonda: una copia del
documento en la que cada enlace (y cada `id` destino) llevaba un
`background-color` marcador único; los rectángulos de ese color en los ops
eran las cajas. Coste: **dos maquetados** completos por documento con
enlaces. Medido (`scripts/bench/run.py`, 20 páginas, 50 enlaces): 316 ms
frente a 195 ms del mismo documento sin enlaces.

## Verificado en el código fuente

`pmaojo/azul` @ `0818e7b` (`layout/src/solver3/display_list.rs`):

- Por cada tramo de glifos de un contexto inline se emite un
  `DisplayListItem::HitTestArea` (cursor de texto) con
  `run_bounds = (x del primer glifo, baseline − 0,8·font_size, ancho del
  tramo, font_size)` y una etiqueta cuyo valor bajo es el `NodeId` del nodo
  de texto (`TAG_TYPE_CURSOR`). Es **la misma fórmula** con la que
  `push_inline_backgrounds_and_border` pinta el fondo de un inline, salvo que
  el fondo se amplía con padding y borde.
- Un nodo de bloque «interactivo» recibe `HitTestArea` con su `paint_rect`,
  el mismo rectángulo en que se pinta su fondo. `core/src/prop_cache.rs`
  `generate_tag_ids`: un nodo es interactivo, entre otras cosas, si tiene la
  propiedad `cursor`.
- El paginador recorta `HitTestArea` por página igual que un `Rect`
  (`clip_rect_bounds`), y el bridge de printpdf las ignora: no pintan nada.

`pmaojo/printpdf` (`src/html/bridge.rs`): un `Rect` se coloca con
`CoordTransform` (`x·0,75 + margen`, `alto_página − (y + h)·0,75 − margen`).

Y un hallazgo que corrige al prompt: **las anotaciones no pueden ser «byte a
byte» las del método viejo** sin copiar sus rarezas. El fondo inline de un
`<a>` lo pintan dos caminos: el display list de azul (dos veces el mismo
rectángulo, la caja de glifos) y además el renderizador de texto de printpdf,
con una caja que incluye el espacio final del tramo. La unión de los tres era
el rectángulo del enlace: unos pt más ancho que el texto. Además, los puntos
de un polígono pintado pasan por pt → mm → pt (diferencias de 5·10⁻⁴ pt).

## Qué se implementó

1. **Fork** (`pmaojo/printpdf`, rama `fix/tracked-boxes`, en `pdfcn` @
   `66719e9`): `XmlRenderOptions::track_class_prefix` y
   `add_xml_to_document_tracked` / `xml_to_pdf_pages_tracked`, que devuelven,
   por cada clase con ese prefijo, cada `HitTestArea` del elemento o de algo
   dentro de él (el ancestro más cercano con la clase gana), con su página y
   su rectángulo en pt, colocado como un `Rect` de esos límites. Se calcula en
   el mismo bucle que convierte cada página. `add_xml_to_document` y
   `xml_to_pdf_pages` conservan su firma. Test
   `tests/html_tracked_boxes.rs`: tramos inline y bloque interactivo donde se
   pinta su fondo; rastrear no cambia ningún op; una caja partida entre
   páginas aparece en cada una.
2. **pdfcn** (`links.rs`): cada enlace recibe la clase
   `pdfcn-track-a<i>` y cada destino `pdfcn-track-t<i>`, más
   `cursor:pointer` (que hace interactivo a un bloque). Ningún estilo nombra
   esas clases y el cursor no pinta: el documento marcado **es** el
   documento — test `tracking_draws_exactly_what_the_document_draws`
   compara sus ops con los del HTML original, op por op. Se maqueta una vez
   y las anotaciones se añaden a ese mismo documento.
3. Desaparecen el render sonda, los colores marcadores, el marcado de los
   hijos inline (el ancestro más cercano lo resuelve) y el tope de 65 520
   enlaces que imponía codificar el índice en un color.

## Resultado

`scripts/bench/run.py links nolinks -n 11` (método en `scripts/bench/README.md`):

| | Antes | Después |
|---|---|---|
| 50 enlaces, 20 páginas | 316 ms · 99 MB | **216 ms · 77 MB** |
| El mismo sin enlaces | 195 ms · 78 MB | 229 ms · 77 MB (ruido; mínimo 190 ms) |

Con enlaces cuesta ya lo mismo que sin ellos: el segundo maquetado era todo
el sobrecoste.

**Golden**: el dibujo de `links`, `format-latex`, `format-markdown` y
`format-typst` es idéntico (comparado quitando las anotaciones); cambian solo
los rectángulos de sus anotaciones, en el mismo número: misma `y` y altura
(±5·10⁻⁴ pt), y el ancho es el de los glifos, sin el espacio final que
añadía el relleno propio de printpdf (p. ej. 75,44 → 69,11 pt). El bloque
enlazable sigue cubriendo su caja de borde.

## Límites y deuda

- El rectángulo de un enlace inline cubre sus glifos, no su padding: la
  `HitTestArea` de azul no se amplía con los insets como el fondo. Ampliarla
  (en el fork de azul) sería además lo correcto para hit-testing.
- Un enlace dentro de un elemento con `transform` se sigue colocando sin
  transformar (como antes).
