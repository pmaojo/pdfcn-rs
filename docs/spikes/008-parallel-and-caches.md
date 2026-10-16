# Spike 008 — Paralelismo y cachés por contenido, solo con la biblioteca estándar

Estado: **cerrado — implementado** (fase C de
`docs/prompts/rendimiento-y-arquitectura.md`).

## Cómo se midió

Método en [`scripts/bench/README.md`](../../scripts/bench/README.md):
procesos en frío con `scripts/bench/run.py` (mediana de 15, `PDFCN_THREADS`
para fijar los hilos); en proceso con `cargo run --release -p pdfcn-core
--example inprocess` (render en caliente, `render_files_html` como lo usa
`pdfcn dev`, y un lote de 8 facturas en 1/2/4 hilos con el mismo reparto que
`api/generate-pdf-batch.rs`); reparto del coste con callgrind sobre el
binario `profiling`.

## Qué se midió antes de tocar nada

Callgrind del catálogo tras las fases A y B (1 050 M instrucciones):

| | % |
|---|---|
| Layout de azul (`add_xml_to_document_tracked`) | 64 |
| Conversión SVG → PDF (`vector_pdf::prepare`) | 9 |
| `PdfDocument::save` | 6 |
| Cabeceras, recortes y orientación de fotos (`assets`) | 5 |
| Elegibilidad JPEG (`sniff`, orientación) | 1,4 |
| Plantilla + HTML + estilos | ~5 |

Lo independiente por imagen (~16 %) es lo paralelizable desde pdfcn; el
layout es una sola llamada a azul, en serie (fase E).

En proceso: `pdfcn dev` re-renderiza el HTML del catálogo en **1,1 ms** (el
objetivo de < 50 ms ya se cumplía: `pdfcn dev` solo produce HTML); un lote de
8 facturas escalaba **2,95×** en 4 hilos.

## Verificado en el código

- `std::thread::scope` (estable desde 1.63, `rust-version` del workspace
  1.82) permite prestar los bytes del mapa de imágenes a los hilos sin `Arc`.
- `lopdf::Document` es `Clone + Send` (`src/document.rs`), así que un Form
  convertido puede volver de un hilo y guardarse en caché.
- `lopdf` 0.44 ya usa rayon al parsear (aparece en el perfil): no se añade
  nada; pdfcn no depende de rayon fuera de `pdfcn-vercel`.
- El endpoint batch ya repartía documentos con rayon: con hilos por imagen
  dentro de cada documento se sobre-suscriben los núcleos (medido: 2,14×).

## Qué se implementó

1. **`parallel::map`** (`pdfcn-core/src/parallel.rs`): trozos contiguos, uno
   por hilo, resultados concatenados **en el orden de entrada**. Los pases
   solo calculan en paralelo; lo que se registra en el mapa de imágenes, en
   los placeholders o en el HTML se hace después, en serie y en orden de
   `src`, así que el PDF no depende de los hilos. Test
   `pdfcn-core/tests/threads.rs`: el mismo documento (fotos que pasan tal cual,
   recortes `cover`, SVG) da el mismo PDF con 1, 2, 4 y 7 hilos, salvo fecha e
   identificador. Usado en: conversión SVG, orientación EXIF, recortes
   `cover`, reducción y elegibilidad JPEG.
2. **`PDFCN_THREADS`** (por defecto `available_parallelism`) y
   **`pdfcn_core::with_threads(n, f)`**, un tope por hilo llamador; el
   endpoint batch lo usa con 1.
3. **Feature `cache`** (por defecto: sin crates, sin coste de tamaño) y
   `pdfcn-core/src/cache.rs`: LRU escrito a mano (mapa + contador de uso;
   desalojo del más antiguo) con presupuesto en bytes por caché
   (`PDFCN_CACHE_MB`, 64 por defecto, `0` apaga). Clave = hash de la entrada +
   su longitud; nunca por tiempo ni por mtime. El cálculo de un fallo va fuera
   del cerrojo. Cuatro cachés:
   - AST de plantillas (antes: FIFO de 128 entradas sin límite de memoria);
   - hojas de estilo por conjunto de clases + tema (antes: igual);
   - SVG convertidos (Form, tamaño o PNG de respaldo);
   - fotos recortadas, enderezadas o reducidas — consultada solo cuando la
     cabecera dice que hay trabajo, para no pagar el hash de una foto que no
     lo necesita.
   Sin la feature, todo se calcula cada vez. Test de transparencia: el de
   hilos renderiza cinco veces el mismo documento (aciertos de caché a partir
   del segundo) y exige el mismo PDF.

## Resultado

| | Antes | Después |
|---|---|---|
| Catálogo, proceso en frío, 1 hilo | 170 ms | — |
| Catálogo, proceso en frío, 4 hilos | (= 1 hilo) | **144 ms** |
| Catálogo, en caliente en proceso | 136 ms | **93 ms** |
| `pdfcn dev`, re-render del HTML del catálogo | 1,1 ms | 1,0 ms |
| Lote de 8 facturas, 1 hilo | 127 ms | 105–124 ms |
| Lote de 8 facturas, 4 hilos | 43 ms (2,95×) | **32–40 ms (3,1–3,3×)** |
| Factura, proceso en frío | 36 ms | 35 ms |

Golden: sin cambios (mismo dibujo, mismos digests). Dependencias y tamaño
sin cambio: ningún crate nuevo.

## Límites y deuda

- El 64 % del catálogo es el layout de azul, en un hilo: lo que queda de
  paralelismo está ahí dentro (fase E).
- El escalado del lote se queda en ~3,3× en 4 núcleos: cada render clona la
  caché de metadatos de fuentes del pool compartido
  (`FontManager::from_arc_shared((*fc_cache_arc).clone(), …)` en printpdf) y
  el mapa de fuentes parseadas va tras un `Mutex`; no se ha medido cuánto
  pesa cada cosa.
- La caché de fotos guarda una copia de la salida; un acierto la copia otra
  vez al mapa de imágenes (no puede prestarse más allá del cerrojo sin `Arc`
  en la API de printpdf).
