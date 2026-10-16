# Spike 005 — Fotos al PDF sin re-codificar (el camino rápido de Typst)

Estado: **cerrado — implementado** (`jpeg_passthrough`, umbral de reescalado,
rutas simples en plantillas). Quedan abiertas las líneas de "Siguientes pasos".

## Problema

`examples/linkedin-carousel/bench/catalog` (100 fotos JPEG 800×600 + 100 SVG,
A4): **pdfcn 14,7 s frente a 0,56 s de Typst**. La factura iba a la par
(68 ms frente a 74 ms en frío); el problema era exclusivo de las imágenes.

## Qué se midió (callgrind, build `-O3` con símbolos, 12 fotos)

| Coste | % instrucciones |
|---|---|
| `normalize_resolutions` → `resize` Lanczos3 | 48 % |
| Re-codificación JPEG (nuestra, tras reescalar) | 17 % |
| Re-codificación JPEG (printpdf, al guardar) | 17 % |
| Decodificaciones (3 por foto) | ~10 % |
| Layout (azul) | < 10 % |

Por foto, el camino era: decodificar para ver si hacía falta recortar (cover),
decodificar otra vez para ver si había que reducir, reescalar con Lanczos3
(800 px en una caja de 200 px a 3× = 600 px, un 0,75), codificar JPEG,
printpdf la decodifica a píxeles, y al guardar la vuelve a codificar a JPEG.
Tres decodificaciones, un reescalado y dos codificaciones para bytes que nadie
cambió — y dos generaciones de pérdida JPEG.

Además, `[profile.release] opt-level = "z"` (ya cambiado a `3`) multiplica todo ese trabajo
numérico por ~4 (14,4 s → 3,5 s solo con `-O3`).

## Qué hace Typst (krilla)

- **JPEG passthrough**: `/DCTDecode` *es* JPEG; el fichero original va tal cual
  como stream de la imagen. Solo lee la cabecera (tamaño, componentes).
- Nunca reescala ni recorta píxeles: `fit: "cover"` es un clip en el PDF.
- PNG sí se decodifica y se comprime con Flate.
- Memoización (`comemo`) de evaluación, parsing y layout: en caliente o en
  `typst watch` solo recalcula lo que cambió.
- Codifica imágenes en paralelo.

## Verificado contra el fork fijado (`pmaojo/printpdf` @ `0e593f0`)

- `src/html/bridge.rs` `resolve_html_images` decodifica *todo* `<img>` a
  `RawImage` (píxeles) con `RawImage::decode_from_bytes`; el layout solo usa
  `width`/`height` (`image_cache_for` registra imágenes nulas de ese tamaño).
- `src/image.rs` `image_to_stream`: `optimize()` y luego, con
  `ImageCompression::Auto`, `jpeg_encode` para color (`DCTDecode`) o
  `flate_encode` para gris. Con `ImageCompression::None` copia los píxeles
  sin filtro. No hay ruta para conservar los bytes originales.
- `image_xobject_id(src)` nombra el XObject a partir del `src` (ya lo usa
  spike 004): el objeto se puede sustituir *in situ* tras guardar.

## Qué se implementó

1. **`jpeg_passthrough`** (feature `jpeg-passthrough`, por defecto; no añade
   crates — `lopdf` ya está en el árbol). Antes del layout, cada JPEG de
   8 bits, 1 o 3 componentes, baseline o progresivo, sin rotación EXIF, se
   cambia por un PNG negro en escala de grises del mismo tamaño. Tras guardar,
   ese XObject se sustituye por un stream `/DCTDecode` con el fichero original.
   CMYK, 12 bits, aritmético, sin pérdida, o cualquier opción de imagen del
   usuario que pida re-codificar (calidad, presupuesto, gris, formato) → el
   camino de siempre.
2. **Placeholders sin comprimir**: si *todas* las imágenes que printpdf va a
   escribir son placeholders que se sustituyen después (JPEG retenidos o SVG
   que pasan a vector), se guarda con `ImageCompression::None`; el pase final
   comprime con Flate lo que quede sin filtro (un SVG que no pudo convertirse).
   Comprimir ceros era un tercio del tiempo restante.
3. **Decidir por la cabecera**: el recorte cover y la normalización leen el
   tamaño (y la orientación) sin decodificar; solo decodifican si van a
   cambiar algo.
4. **Umbral de reescalado** (`MIN_WORTHWHILE_SHRINK = 0.5`), cuando se pide reducir: solo se reduce una
   foto que tenga al menos el doble de lo que su caja puede pintar a ~300 dpi
   (el tope absoluto de 4096 px se mantiene). Al reducir, promedio de área
   (`thumbnail_exact`) en vez de Lanczos3: en reducciones ≥2× es igual de nítido
   y mucho más barato. Las re-codificaciones son JPEG q92 y son la última
   generación (luego van en passthrough).
5. **Reducir es opt-in** (`RenderOptions::shrink_images`, `pdfcn build
   --shrink-images`; la API y napi lo dejan apagado). Preparar fotos es
   trabajo de `pdfcn images`, una vez, no de cada render: con el passthrough
   una foto grande ya no cuesta tiempo al renderizar, solo tamaño de PDF. El
   render sigue haciendo lo que hace falta para que la foto *se vea bien*:
   orientación EXIF y recorte cover centrado.
6. **Plantillas**: `{{ p.name }}` serializaba el contexto *entero* a valores de
   minijinja en cada expresión — cuadrático en el tamaño de los datos (≈21 % del
   catálogo). Las rutas simples (`a.b[0].c`) se resuelven ahora directamente
   sobre el JSON; cualquier otra cosa sigue yendo a minijinja.

## Resultado (binario de siempre, `opt-level = "z"`, 4 núcleos)

| | Antes | Después |
|---|---|---|
| Catálogo 100 fotos + 100 SVG | 14,7 s | 0,67 s |
| Catálogo 100 fotos, sin SVG | 14,4 s | 0,47 s |

Y con `opt-level = 3` en vez de `"z"` (ya aplicado): catálogo **0,39 s**,
factura 47 → 39 ms, a cambio de `generate-pdf` 20,9 → 26,8 MB (tope 60 MB).

Golden: solo cambia una foto de `brochure` (estaba a 0,68× de su límite y ya
no se re-muestrea; el redondeo del reescalado antiguo movía la caja 0,1 pt).

## Siguientes pasos (no implementados)

- **Copias de memoria** en `render_pdf_with_assets`: el mapa de imágenes se
  clona entero (`images.clone()`), otra vez a `Base64OrRaw::Raw(bytes.clone())`,
  y printpdf lo copia una tercera vez a `xml_options.images`; las fuentes
  incorporadas (`bytes.to_vec()`, 300–500 KB cada una) se copian dos veces por
  render. Con `Cow`/`Arc<[u8]>` en la API del fork serían cero copias.
- **Plantillas**: `with_binding` clona el contexto completo en cada iteración
  de `- for`; una cadena de ámbitos (base + overlays) lo haría O(1).
- **Enlaces**: con `<a href>` el documento se maqueta dos veces (render sonda
  en `links`). Exponer las cajas de los enlaces desde el bridge lo evitaría.
- **Memoización estilo comemo**: cachés por hash de contenido en proceso
  (imágenes preparadas, AST, CSS ya existe en parte, pool de fuentes ya
  existe) para `pdfcn dev` y servidores en caliente. Memoizar el layout por
  bloques exigiría cambios en azul; no antes de agotar lo anterior.
- **Paralelismo**: la preparación de imágenes y la conversión SVG→PDF son
  independientes por imagen (`std::thread::scope`, sin dependencia nueva).
