# Spike 004 — SVG como vectores reales: Form XObjects tras el render

Estado: **cerrado — se implementa el pase posterior**. Reabre la decisión de
[001](001-vector-vs-raster.md), que eligió rasterizar porque no encontró
ninguna ruta vectorial; esta es una que 001 no evaluó.

## Problema

Desde la Ola 1.2 todo SVG (`%Vector`, Charts v2, `%Barcode`, `%Icon`) llega al
PDF como PNG a ~300 dpi. Se ve bien impreso, pero no es vector: no escala, el
texto de los gráficos no es texto, y un logotipo SVG pesa más como PNG.

001 buscó una ruta *dentro* del bridge HTML de printpdf (un `<svg>` inline que
llegue al display list) y concluyó, con razón, que no existe y que colocar un
`ExternalXObject` a mano exigiría coordenadas de página que
`from_html_with_cache` no expone.

## La ruta que 001 no evaluó

No hace falta conocer la geometría: el layout ya coloca el PNG. Basta con
cambiar *qué* se dibuja en ese sitio después de guardar el documento.

Verificado contra el fork fijado (`pmaojo/printpdf` @ `0e593f0`):

- `src/html/bridge.rs` — `image_xobject_id(src)` nombra cada `<img>` como
  `HtmlImg_<src con todo lo no alfanumérico cambiado por _>`. El bridge (que
  emite `Op::UseXobject`) y el ensamblado (que registra el XObject) derivan el
  id de la misma función, así que el nombre del recurso es función pura del
  `src` del placeholder.
- `src/serialize.rs` — todas las páginas comparten un único diccionario
  `/XObject` (`global_xobject_dict`, nombre → referencia), y `Op::UseXobject`
  se serializa como `q <cm> /HtmlImg_… Do Q`.
- Una imagen PDF se pinta siempre sobre el cuadrado unidad del espacio que fija
  ese `cm` (ISO 32000-1 §8.9.4). Un Form XObject con `/BBox [0 0 w h]` y
  `/Matrix [1/w 0 0 1/h 0 0]` pinta su contenido en ese mismo cuadrado unidad
  (§8.10.1). Sustituir el objeto imagen por ese Form, **conservando su número
  de objeto**, deja intactos el diccionario de recursos y cada `Do`: el vector
  ocupa exactamente la caja que ocupaba el PNG, deformación incluida (el PNG ya
  se estiraba a la caja; no hay regresión de aspecto).

Verificado contra `svg2pdf` 0.13.0 (crates.io):

- Depende de `usvg ^0.45`, el mismo que `resvg` 0.45 ya en el árbol: el
  `usvg::Tree` que se parsea con la fontdb embebida de `raster.rs` sirve tal
  cual.
- `to_pdf(tree, ConversionOptions, PageOptions)` produce un PDF de una página
  con `MediaBox` = tamaño del SVG (a 72 dpi, 1 unidad SVG = 1 pt) y el
  contenido ya en coordenadas PDF (y hacia arriba). Su contenido y sus
  recursos son exactamente lo que necesita el Form.
- `src/render/mod.rs` 115-135: **sin la feature `text`, los nodos de texto se
  descartan** con un `log::warn!`. Se activa `text` y se pasa
  `embed_text: false`, que pinta `text.flattened()`: el texto queda como
  trazos vectoriales, sin incrustar un subconjunto de fuente por SVG.
- Lo mismo con los filtros (`feTurbulence`, `feGaussianBlur`…): sin la feature
  `filters` el grupo filtrado se pinta **sin** el filtro, una regresión de
  fidelidad respecto al PNG (comprobado: un velo de ruido al 10 % salía como un
  rectángulo negro al 10 %). Con `filters`, svg2pdf rasteriza solo ese grupo y
  el resto sigue siendo vector. Coste a tener presente: un filtro a página
  completa con ruido pesa varios MB por página, porque el ruido no comprime.

## Diseño

1. `assets::generate_vectors` registra, además del PNG de siempre, el SVG
   fuente bajo la misma clave (`src` final, ya acortado si hacía falta).
2. Tras `doc.save()`, `vector_pdf::vectorize(bytes, &fuentes)`:
   - carga el PDF con lopdf y localiza cada `HtmlImg_<clave>` en el
     diccionario `/XObject` compartido;
   - convierte el SVG con `svg2pdf::to_pdf`, carga ese PDF, renumera sus
     objetos por encima del máximo del documento y los incorpora;
   - sustituye el objeto imagen por un Form XObject con el contenido y los
     recursos de la página de svg2pdf, `BBox` = su `MediaBox` y la `Matrix`
     que la lleva al cuadrado unidad;
   - poda los objetos huérfanos (el catálogo y la página de svg2pdf).
3. **Nunca empeora**: si cualquier paso falla para un SVG, ese SVG se queda en
   PNG; si falla la carga o el guardado del documento, se devuelven los bytes
   originales. Nada de `unwrap`.

Todo tras la feature `vector` (que ahora arrastra `svg2pdf` y `lopdf`; `lopdf`
ya estaba en el árbol por printpdf y por `factur-x`).

## Lo que no cambia

- El PNG se sigue generando: es el fallback y lo que usa el layout para su
  tamaño intrínseco. Coste: un rasterizado por SVG, igual que antes.
- `--no-default-features` sigue sin vectores ni rasterizado, como hasta ahora.
- Los generadores y placeholders no cambian: el backend es intercambiable, como
  preveía 001.
