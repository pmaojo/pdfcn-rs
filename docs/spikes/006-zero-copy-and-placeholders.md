# Spike 006 — Cero copias: placeholders por tamaño y una sola pasada lopdf

Estado: **cerrado — implementado** (fase A de
`docs/prompts/rendimiento-y-arquitectura.md`).

## Problema

Tras el spike 005 el catálogo (100 JPEG 800×600 + 100 SVG) tardaba 392 ms con
un pico de 253 MB. El prompt atribuía la memoria a las copias de bytes de
imagen (`images.clone()`, `Base64OrRaw::Raw(bytes.clone())`, la copia de
printpdf a `xml_options.images`) y pedía `Arc<[u8]>` de extremo a extremo.

## Cómo se midió

Procedimiento completo y reproducible en [`scripts/bench/README.md`](../../scripts/bench/README.md):
tiempo = mediana del reloj de pared de N procesos nuevos de `pdfcn build`;
pico = `ru_maxrss` (RSS máximo) de cada proceso vía `wait4`
(`scripts/bench/run.py`); reparto del pico = massif + `massif_top.py`;
copias por imagen = asignador contador (`pdfcn-core/tests/zero_copy.rs`).

## Qué se midió (massif, binario release con símbolos, catálogo)

| Pico 244,7 MB | MB |
|---|---|
| `lopdf` `write_stream` (el PDF guardado) | 61,5 |
| `RawImage::decode_from_bytes` (placeholders 800×600 a píxeles) | 51,7 |
| `vector_pdf::vectorize` (lopdf carga el PDF entero) | 50,9 |
| `printpdf` `stream` (imágenes escritas sin comprimir) | 48,1 |
| `load_images` (**los bytes JPEG**) | 5,6 |

**El hallazgo contradice al prompt**: las tres copias de bytes JPEG suman
~17 MB. La memoria venía de los *placeholders*: cada JPEG retenido se cambiaba
por un PNG gris del mismo tamaño en píxeles, que printpdf decodificaba
(800×600 = 480 KB cada uno, 48 MB), escribía sin comprimir en el PDF (48 MB
más) y que luego `vector_pdf` y `jpeg_passthrough` cargaban y guardaban con
lopdf **dos veces**, cada una con su copia del documento.

## Verificado en el código fuente

`pmaojo/printpdf` @ `0e593f0` (antes de este cambio):

- `src/lib.rs` `from_html_with_cache` construye un `XmlRenderOptions` propio y
  copia ahí cada imagen y cada fuente (`b.clone()`): por eso pdfcn tenía que
  clonar antes a `Base64OrRaw::Raw`.
- `src/html/mod.rs` `add_xml_to_document(&mut PdfDocument, &str,
  &XmlRenderOptions)` es pública y **solo lee** las opciones: construyéndolas
  pdfcn mismo (con sus `Vec` movidos, no copiados) y pasándolas por
  referencia, la API del fork no necesita `Arc` ni `Cow`. Lo que pedía el
  punto A.2 («comprueba si basta con pasar la propiedad») basta.
- `src/html/bridge.rs` `resolve_html_images` decodifica **todo** `<img>` a
  `RawImage`; `image_cache_for` solo usa `width`/`height`; el bridge, al
  colocar la imagen, solo usa `raw_image.width/height` (escala =
  destino / tamaño natural a 300 dpi).
- `src/serialize.rs` `Op::UseXobject`: el `cm` es el producto de
  `Scale(tamaño natural)` (de `XObject::get_width_height`) por la escala del
  bridge y la traslación. `src/xobject.rs` `get_width_height` devuelve, para un
  `XObject::External`, su `width`/`height` declarados: un stand-in de 1×1 px
  que declare 800×600 produce **exactamente el mismo `cm`**.

`lopdf` 0.44 `src/writer.rs` escribe cada stream directamente al `Write`
destino, sin búferes intermedios.

## Qué se implementó

1. **Fork** (`pmaojo/printpdf`, rama `fix/image-placeholders`, fusionada en
   `pdfcn` @ `beaee65`): `XmlRenderOptions::image_placeholders: src → (ancho,
   alto)`. El layout trata cada entrada como una imagen de ese tamaño
   intrínseco y el documento recibe un XObject de 1 px que declara ese tamaño.
   Test `tests/html_image_placeholders.rs`: mismos ops de página y mismo `cm`
   que con la imagen real; un tamaño distinto cambia el layout; el stand-in es
   una imagen válida. Aditivo: propuesto para upstream.
2. **Mapa de imágenes prestado** (`assets::Images<'a> = BTreeMap<String,
   Cow<'a, [u8]>>`): cada entrada apunta a los bytes del llamador hasta que una
   pasada la sustituye. Sin cambio de API pública (`render_pdf_with_assets`
   sigue recibiendo `&BTreeMap<String, Vec<u8>>`); `Arc<[u8]>` habría obligado
   a cambiarla sin ganar nada medible.
3. **JPEG por tamaño** (`jpeg_passthrough::take`): el JPEG sale del mapa, su
   tamaño va a `image_placeholders`, y el original (prestado) se escribe al
   final como `/DCTDecode`. Ni un píxel decodificado.
4. **SVG por tamaño, raster solo de respaldo** (`vector_pdf::prepare`): cada
   SVG se convierte con svg2pdf *antes* del layout; si convierte, se maqueta
   por tamaño (el mismo que tendría su PNG, `raster::raster_size`); solo si no
   convierte se rasteriza a PNG, como siempre. Ya no se renderiza, codifica y
   decodifica un PNG por SVG que luego se tira.
5. **Una sola pasada lopdf** (`pdf_xobjects::finish`): cargar una vez,
   sustituir Forms y JPEG, comprimir lo que quede sin filtro, podar y guardar
   una vez (antes: dos cargas y dos guardados). La salida se dimensiona para
   los JPEG que va a llevar.
6. **Fuentes**: una copia por render (las opciones de printpdf son dueñas de
   sus bytes) en vez de dos.
7. **Plantillas** (`pdfcn-template`): una cadena de ámbitos — el contexto del
   llamador, nunca copiado, bajo una pila de marcos para `- for` (ítem y
   `loop`) y `- set`. Entrar en una iteración es O(1); antes se clonaba el
   contexto entero. La semántica de sombreado es la de antes (tests nuevos:
   sombreado del contexto por el ítem, bucles anidados, `- set` por
   iteración, contexto no-objeto). Las expresiones que van a minijinja reciben
   solo las variables raíz que mencionan (`undeclared_variables`), no el
   contexto entero serializado.

## Resultado

`scripts/bench/run.py`, 4 núcleos, binario release, mediana de 7 procesos:

| | Antes | Después |
|---|---|---|
| Catálogo 100 fotos + 100 SVG | 392 ms · 253 MB | **178 ms · 59 MB** |
| Factura | 41 ms · 38 MB | 39 ms · 36 MB |
| 1 página de texto | 55 ms · 41 MB | 51 ms · 40 MB |
| 200 páginas de texto | 2,51 s · 624 MB | 2,6 s · 623 MB (sin cambio: es azul, fase E) |

Dependencias y tamaño, sin cambio: 417 crates en `pdfcn-vercel`, `generate-pdf`
26,9 MB (antes 26,8).

Plantilla aislada (`- for` sobre N ítems con el contexto que los contiene):

| N | Antes | Después |
|---|---|---|
| 500 | 83 ms | 0,7 ms |
| 2000 | 1,47 s | 4,5 ms |
| 8000 | 35,3 s | 15 ms |

`pdfcn-core/tests/zero_copy.rs` (asignador que cuenta el pico de memoria viva):
una foto de 7,3 MB cuesta **2,7 copias** de sí misma (el stream de lopdf, el PDF
de salida y el búfer del decodificador JPEG mientras lee la orientación EXIF);
antes 4,4. Una foto lisa de 3000×3000 (27 MB de píxeles) no cuesta píxeles.

**Golden: sin cambios.** Los snapshots comparan el dibujo (`render_to_svg`) y
un digest de cada payload; el `cm` de cada imagen es idéntico (punto 1) y los
streams que se insertan son los mismos.

## Fallos preexistentes arreglados en esta fase

- `cargo test -p pdfcn-core --no-default-features` fallaba el golden de
  `ticket`, y habría fallado los siguientes de la lista: los snapshots los
  escribe el build por defecto, y sin `vector` un QR/gráfico/icono acaba en
  PNG y sin `jpeg-passthrough` printpdf re-codifica cada foto. El test
  compara ahora, en un build reducido, todo ejemplo cuyo snapshot no dibuja
  ninguna imagen ni Form (10 de 18), nombra los que omite, y se niega a
  regenerar snapshots desde un build reducido.
- `pdfcn-components` daba 4 avisos de código muerto sin la feature `vector`
  (`DEFAULT_COLOR`, `hex`, `error`, `icon` de `icons`): solo los usa la
  expansión de `%Icon`, que sin `vector` es el marcador "feature disabled".
  Condicionados a `vector`, con sus imports y los tres tests que los usan.
- Escondido detrás del primero (`cargo test` se para en el primer binario
  que falla): `pdfcn-core/tests/icons.rs` esperaba, también sin `vector`, que
  un `%Icon` se dibujase como imagen. Sus tres tests de dibujo quedan bajo
  `vector`, y uno nuevo comprueba que sin `vector` el icono es el marcador
  visible "requires ... vector". `scripts/check.sh` corre ahora con
  `--no-fail-fast`, para que un fallo no vuelva a tapar otros.

## Deuda que queda

- Una copia de cada fuente usada por render (300–500 KB por cara):
  `XmlRenderOptions::fonts` es `Vec<u8>`; quitarla exige `Arc`/`Cow` en el fork.
- El decodificador de `image` copia el JPEG entero para leer la orientación
  EXIF (`has_orientation`, `exif_orientation`): transitorio; un lector propio
  del segmento APP1 lo evitaría.
- La imagen base de un recorte `cover` sigue registrada (y, con passthrough,
  embebida) aunque el HTML ya solo cite el recorte. Filtrar por los `src` del
  HTML final es seguro para `<img>`, pero no se ha verificado que ningún
  `background-image: url()` dependa del mapa.
- El documento de 200 páginas sigue en 620 MB: es el layout de azul (fase E).
