# Spike 011 — Documentos largos: dónde se va la memoria

Estado: **abierto — medido, sin implementar**. El cambio irá en el fork de
printpdf (`pmaojo/printpdf`), en su propia rama `fix/*`.

## Problema

El caso `long` de `scripts/bench/run.py` (texto puro, sin imágenes) tarda
**2,55 s** (mediana, mín. 2,32 s) y llega a **620 MB** de pico RSS.

## Premisa corregida

Se había atribuido el coste a «printpdf escribiendo el fichero». Massif
(binario `profiling`, mismo documento) dice otra cosa: el pico de heap
(681,7 MB) está **antes de serializar**, al construir las operaciones de
página y la lista de visualización.

| Pico 681,7 MB | MB |
|---|---|
| `Vec<Op>` creciendo en `bridge::render_unified_layout_with_margins` | 311,7 |
| `azul_layout::solver3::display_list::clip_unified_layout` (`grow`) | 177,9 |
| `extract_item_metrics_dense` (`choose_item_metrics`) | 33,1 |
| `Box<[TextItem; 1]>` por glifo | 26,3 |
| `Box<[Codepoint; 1]>` por glifo | 26,3 |
| `layout_document_paged_impl` | 23,7 |
| `actualtext_span_begin` | 9,1 |

## Verificado en el código fuente

`pmaojo/printpdf` @ `797d515` (rama `pdfcn`), `src/html/bridge.rs`:

- Por **cada glifo** se emiten dos `Op`: `SetTextMatrix` y
  `ShowText { items: vec![TextItem::GlyphIds(vec![Codepoint { cid:
  Some(String) }])] }` → tres asignaciones de heap por glifo (`Vec<TextItem>`,
  `Vec<Codepoint>`, `String`).
- `size_of::<Op>()` = **128 B** (medido; `TextItem` 32 B, `Codepoint` 32 B,
  `TextMatrix` 28 B). Dos ops por glifo = 256 B en línea + ~100 B de heap, es
  decir ~350 B vivos por glifo hasta que se serializa la página.
- `azul` `clip_unified_layout` (@ `cf361ea`) filtra con `.cloned().collect()`
  los `PositionedItem` de cada texto **por cada página que cruza** y construye
  un `UnifiedLayout` nuevo: un párrafo largo se copia entero una vez por
  página.

## Propuesta (en este orden, midiendo tras cada paso)

1. **Una sola operación por glifo agrupado**: emitir un `ShowText` por run con
   los desplazamientos como `offset` de `Codepoint` (o `TextMatrix` solo al
   cambiar de línea), en vez de `SetTextMatrix` + `ShowText` por glifo.
   Menos ops y un `Vec` en vez de tres asignaciones por glifo. Riesgo:
   cambia el contenido de los streams → regenerar los golden y decirlo.
2. **`cid` sin `String`** cuando el codepoint es un solo `char` (p. ej.
   `SmallString`/`char`), o `Arc<str>` compartido.
3. **`clip_unified_layout` sin clonar los ítems fuera de la página**: filtrar
   por rango (los ítems están ordenados por `y`) y clonar solo el tramo. Va
   en el fork de azul, rama `fix/*` aparte.
4. Si sigue alto: volcar y liberar el `Vec<Op>` de cada página al serializarla
   en vez de retener todas.

## Cómo reproducir

```sh
cargo build --release -p pdfcn-cli
python3 -I scripts/bench/run.py long -n 5          # 2554 ms, 619,9 MB
cargo build --profile profiling -p pdfcn-cli
cd target/bench/long
valgrind --tool=massif --massif-out-file=/tmp/m.out \
  ../../profiling/pdfcn build doc.haml -d doc.json -o /tmp/out.pdf
python3 -I ../../../scripts/bench/massif_top.py /tmp/m.out --depth 2
```

Objetivo a validar tras el paso 1–3: pico < 300 MB y < 1,5 s en `long`, sin
regresión en `invoice`/`catalog`.
