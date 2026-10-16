# Spike 010 — Dónde está el tiempo después de A–D, y si cambiar de motor

Estado: **cerrado — se queda azul; se arreglan sus fugas en los forks**
(fase E de `docs/prompts/rendimiento-y-arquitectura.md`).

## Cómo se midió

Método en [`scripts/bench/README.md`](../../scripts/bench/README.md), sección 4:
binario `profiling` (release con tablas de líneas), `PDFCN_THREADS=1` para
que el reparto no dependa de los hilos, callgrind sobre los tres documentos
que pide el prompt:

```sh
cargo build --profile profiling -p pdfcn-cli
cd target/bench/catalog && PDFCN_THREADS=1 valgrind --tool=callgrind \
  --callgrind-out-file=c.out ../../profiling/pdfcn build catalog.haml -d catalog.json -o out.pdf --svg ...
callgrind_annotate --auto=no --threshold=99.9 c.out > c.txt               # coste propio
callgrind_annotate --auto=no --inclusive=yes --tree=caller c.out > i.txt  # quién llama a quién
python3 -I scripts/bench/callgrind_crates.py c.txt                         # reparto por crate
```

`callgrind_crates.py` (nuevo) suma el coste **propio** de cada función por el
crate de su nombre, así cada instrucción cuenta una vez; lo que queda en libc
(`memcpy`, `memset`, `malloc`) se atribuye después con el árbol de llamadas.
El tiempo de reloj (`scripts/bench/run.py`, mediana de 21) varía ±25 % entre
tandas; las decisiones se toman con instrucciones, y el reloj se compara solo
dentro de una misma tanda.

## 1. Qué parte sigue dentro de `azul-*`

Antes de esta fase (tras A–D), inclusivo en `xml_to_pdf_pages_tracked`
(maquetado de azul + el puente de printpdf que lo llama):

| Documento | Instrucciones | Maquetado (azul + puente) | Guardado (printpdf) | Pico de memoria |
|---|---|---|---|---|
| Catálogo (100 fotos + 100 SVG) | 1 049 M | 64 % | 6 % | 54 MB |
| Factura | 189 M | 86 % | 7 % | 31 MB |
| Texto largo (150 páginas) | 17 003 M | 58 % | 38 % | 618 MB |

Es **mucho más del 25 %**, así que el prompt pide evaluar alternativas. Pero
al abrir ese 64–86 % lo que apareció primero no fue el algoritmo de layout,
sino trabajo repetido alrededor de él, arreglable sin cambiar de motor.

## 2. Lo que se encontró, y se arregló

Cada fila es una fuga que el perfil señaló con nombre y apellido.

| # | Dónde | Qué pasaba | Coste | Arreglo |
|---|---|---|---|---|
| 1 | `pdfcn-core` assets | Tres pases (`apply_exif_orientation`, `oriented_dimensions`, `jpeg_passthrough::take`) abrían cada foto con `image::ImageReader`: copia del fichero entero al decodificador + todas las tablas Huffman, dos veces (tamaño y EXIF) | 6,5 % del catálogo | `jpeg_header.rs`: recorre los marcadores en sitio; la orientación la sigue leyendo `image` (`Orientation::from_exif_chunk`) de los mismos bytes que le pasaría su decodificador |
| 2 | `pdfcn-core` vector_pdf | svg2pdf comprimía cada stream (un compresor nuevo, 300 KB a cero, por stream), lopdf lo inflaba para leer el contenido y se volvía a comprimir a nivel 9 | ~9 % del catálogo | svg2pdf sin comprimir; un compresor por hilo, reiniciado entre streams, en el hilo de cada SVG; el contenido pasa al Form tal cual |
| 3 | fork azul `parse_face_style` | Para saber peso/cursiva de una fuente se llamaba a `FcParseFontBytes`: copia la fuente y construye su cobertura cmap y metadatos; printpdf registra cada fuente con tres grafías | 6,5 % del catálogo, ~30 % de la factura | `read_face_style`: lee `head`/`OS/2`/`name` con las mismas reglas; `None` exactamente cuando aquél no daría cara, y entonces se usa aquél. Test contra todas las fuentes del repo y del sistema |
| 4 | fork printpdf `register_builtin_fonts` | Las 14 fuentes base del PDF iban en gzip en el binario y se inflaban (746 KB) **en cada render**, más un parseo completo de cada una para su cobertura | 20 % de la factura | `defaultfonts/` sin comprimir (+~270 KB de binario: `generate-pdf` 20,0 → 20,2 MB), prestados como `&'static`; cobertura calculada una vez por proceso |

Resultado (instrucciones, `PDFCN_THREADS=1`):

| | Antes | 1+2 | +3 | +4 |
|---|---|---|---|---|
| Catálogo | 1 049 M | 904 M (−14 %) | 837 M (−20 %) | **811 M (−23 %)** |
| Factura | 189 M | 189 M | 122 M (−36 %) | **95 M (−50 %)** |
| Texto largo | 17 003 M | — | — | 16 918 M (−0,5 %) |

Reloj (`run.py`, mediana de 21, proceso en frío; misma máquina, tandas
distintas, ±25 %):

| | Antes de E | Después |
|---|---|---|
| Catálogo, 4 hilos | 161 ms | **122 ms** |
| Catálogo, 1 hilo | 167 ms | 128 ms |
| Factura | 40 ms | **26–29 ms** |
| Texto largo, 150 págs. | 2 558 ms | 2 436–2 736 ms (sin cambio: su coste está en el guardado) |

Memoria: sin cambio (catálogo 55 MB, factura 33 MB, texto largo 620 MB).

Las fotos en sí ya cuestan 0,56 % del catálogo: pasan al PDF tal cual
(`jpeg_passthrough`, spike 005) y su cabecera se lee sin decodificar.

Golden: los SVG convertidos cambian de digest de ops porque lopdf, al leer el
contenido de la página, le añadía un `\n` final; ahora el contenido pasa sin
tocar. Mismo dibujo (comprobado byte a byte: `antes == ahora + b"\n"`).

## 3. ¿Cambiar de motor? Las tres alternativas, con datos

Criterio del prompt: ≥ 2× en tiempo o memoria, **o** ≥ 80 crates menos, sin
perder utilidades del spike 003.

- **Crates**: todo lo que alcanza `azul-layout` en este build son 75 crates
  (`cargo tree -p azul-layout -e normal`), y buena parte los necesita
  igualmente quien maquete texto (allsorts para el shaping, rust-fontconfig,
  unicode-bidi/segmentation, image). Ni quitando azul entero se llega a 80.
- **Tiempo**: tras los arreglos el maquetado es ~72 % del catálogo. Para
  ganar 2× en total, un motor nuevo tendría que maquetar ~3,6× más rápido
  que azul cubriendo bloque, flex, grid, tablas, texto con bidi, guiones y
  paginación (spike 003). Nada en el perfil apunta a un algoritmo
  asintóticamente malo en azul: lo caro es resolver propiedades heredadas
  (`resolve_font_size_one` 5 %, `InheritedValues::get` 4,8 %,
  `get_property` 1,8 %) y el shaping (~5 %), es decir, constantes.

(a) **Seguir con el fork y recortarlo** — es lo que se hizo, guiado por el
perfil: tres arreglos en dos forks, −20 % / −36 %, sin cambiar dibujo ni
dependencias. Recortar features de UI (`fix/slim-*`) no movería el render:
ya están fuera del build (`default-features = false`), no aparecen en el
perfil.

(b) **Maquetador propio** para el subconjunto de CSS usado: elimina como
mucho 75 crates (no llega a 80), y el 2× exige batir a azul por ~3,6× en su
propio terreno; el coste es reescribir lo que el spike 003 enumera. Sin un
indicio medido de ganancia, el prototipo que pide el prompt (factura y
folleto contra los golden) no se hizo: es semanas de trabajo para una
apuesta que los números no sostienen.

(c) **Otro motor de Rust**: no se evaluó leyendo código. Los que se conocen
y paginan HTML/CSS con flex y grid traen un árbol de dependencias propio del
mismo orden, así que el criterio de crates tampoco se cumpliría; queda
anotado como no estudiado, no como descartado con datos.

**Decisión**: no se cambia de motor. Se siguen arreglando, en los forks y con
el perfil delante, las fugas que el maquetado arrastra.

## Deuda, por orden de lo que pesa

1. **Documento largo**: 38 % en el guardado de printpdf
   (`to_lopdf_doc` + `translate_operations`, 828 000 llamadas a
   `resolve_current_font` y `get_or_decode_glyph`) y **618 MB de pico** para
   150 páginas de texto. Es el siguiente objetivo, en el fork de printpdf.
2. Propiedades heredadas en azul (~12 % del catálogo entre las tres
   funciones de arriba): una caché por nodo de `font-size` resuelto.
3. `memcpy`/`memset` siguen siendo ~25 % del catálogo: lo que queda son los
   bytes de fuentes que `register_named_font_in_tier` copia al registro.
