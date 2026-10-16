# Spike 009 — Auditoría de dependencias y arquitectura

Estado: **informe** (fase D de `docs/prompts/rendimiento-y-arquitectura.md`).
Escrito antes de cambiar código; los cambios que de aquí salen van en PR
aparte, uno por paso (sección «Plan»), y cada uno actualiza la tabla de
resultados del final.

## Cómo se midió

Sobre `main` @ `cef49d4` (fases A–C fusionadas), 4 núcleos:

- Crates: `cargo tree -p pdfcn-vercel -e normal --prefix none | sort -u | wc -l`
  (la cifra del prompt, **417**) y crates únicos por nombre + versión (quitando
  los sufijos `(*)`/`(proc-macro)`), **329**. La primera cuenta dos veces un
  crate que aparece como `(*)`; la segunda es la que importa. Las dos se dan
  siempre.
- Peso exclusivo de una dependencia: crates únicos que desaparecen con
  `cargo tree --prune <crate>`.
- Tamaño: `cargo bloat --release -p pdfcn-vercel --bin generate-pdf --crates`
  sin `strip` (`.text` 14,7 MiB de un binario de 32 MiB; `.rodata` 9 MiB;
  27,0 MB con `strip`).
- Compilación: `cargo build --release -p pdfcn-vercel --bin generate-pdf
  --timings` en un target vacío: **508 s** de reloj, 1 012 s de unidades.
- Duplicados: `cargo tree -d` (15 crates con dos o tres versiones).
- Sin uso: `cargo machete`.

## Dónde está el peso

| | `.text` | crates exclusivos | compilación |
|---|---|---|---|
| Motor (`azul-*`, `printpdf`, `allsorts`, `taffy`…) | ~2,9 MiB | 38 (printpdf) | ~270 s de unidad |
| `lightningcss` | **2,7 MiB (18,6 %)** | 37 | 54 s |
| `reqwest` + rustls + `url`/`idna` | ~0,9 MiB | 39 (`url` solo: 25) | ~25 s |
| `lopdf` | 0,2 MiB | 28 | 38 s (+ `jiff` 7,6 s, `chrono` 3,4 s, `time` 2,9 s) |
| `image` (+ `moxcms`, `zune-jpeg`, `png`) | ~0,6 MiB | 7 | 19 s + 16 s |
| `resvg`/`usvg`/`svg2pdf`/`tiny-skia` | ~0,7 MiB | ~25 | ~16 s |
| `minijinja` | 0,5 MiB | 2 | 13 s |
| Enlace final (LTO completo, `codegen-units = 1`) | — | — | **183 s** |
| Fuentes embebidas (12 TTF) | 2,9 MB de `.rodata` | — | — |
| Diccionarios de `hyphenation` (`embed_all`, ~70 idiomas) | ~3 MB de `.rodata` | 4 | — |

## Inventario: cada dependencia directa

Veredicto: **M** mantener, **F** desactivar features que no se usan, **Q**
quitar, **S** sustituir por código propio, **O** opcional tras una feature.

| Crate | Dependencia | Para qué | Uso | Veredicto |
|---|---|---|---|---|
| parser | `winnow` | lexer/parser de indentación | 18 refs, 1 fichero; 1,7 s | **M**: 1 crate exclusivo; un lexer a mano ahorraría 1 crate y nada de tamaño |
| parser | `serde` | derive del AST | — | M |
| template | `minijinja` | expresiones no triviales (filtros, aritmética) | 5 refs; 0,5 MiB; 13 s | **M** por ahora: tras la fase A las rutas simples no lo tocan, pero los filtros (`upper`, `length`, `~`…) son contrato de las plantillas. Sustituir = reimplementar un lenguaje de expresiones: riesgo alto, ~800 líneas |
| formats | `serde_yaml` | front-ends YAML y datos `.yaml` | 6 refs; deprecated upstream | M (aviso: crate sin mantenimiento) |
| formats | `html5ever`, `markup5ever_rcdom` | front-end HTML | 2 refs | M |
| formats | `pulldown-cmark` | front-end Markdown | 1 ref; 3 s | M |
| components | `maud` | HTML de componentes | 23 refs, 14 ficheros; 0,1 s | M |
| styles | `lightningcss` | **minificar el CSS que pdfcn genera** | 1 función (`minify`) | **S**: 18,6 % del código, 37 crates, 54 s, para minificar CSS propio. Lo único que cambia lo que dibuja azul: reescribe colores con sintaxis moderna (`rgb(0 0 0/50%)` de un valor arbitrario) a hex, que azul (solo comas) no lee. Sustituto: normalizador de colores + compactado de espacios, ~150 líneas, con test de equivalencia contra lightningcss (dev-dependency) sobre todas las utilidades |
| styles | (default de `lightningcss`) | `bundler` (dashmap, rayon), `nodejs` (serde), `sourcemap` (rkyv, hashbrown 0.12, ahash 0.7…) | nada de eso se usa | **F**, si no se sustituye antes |
| core | `printpdf` (fork) | motor | — | M (fase E) |
| core | `image` | decodificar/recortar/reducir fotos | 90 refs; `png`+`jpeg` solo | M |
| core | `qrcodegen` | QR | 9 refs; 0,7 s | M |
| core | `resvg` | raster de respaldo de SVG | 1 fichero | **F**: `default-features = false, features = ["text"]` quita `system-fonts` (fontconfig-parser, memmap2, roxmltree 0.20 duplicado: pdfcn solo usa sus fuentes embebidas) y `raster-images` (gif 0.13, zune-jpeg 0.4, image-webp, weezl 0.1: duplicados). Efecto: un `<image>` *dentro* de un SVG no se dibuja en el raster de respaldo — que solo se usa si svg2pdf falla |
| core | `svg2pdf` | SVG → PDF | 1 fichero | M (ya sin defaults) |
| core | `lopdf` | post-proceso del PDF | 4 ficheros | **F**: el `lopdf` del workspace activa sus defaults (`chrono`, `jiff`, `rayon`, `time`) — printpdf lo pide sin ellos y pdfcn no usa ninguno |
| core | `serde` | — | 0 usos | **Q** (machete) |
| core | `cms`…`cmpv2` (11) | firma PAdES | feature `sign`, fuera del binario | M (`ecdsa` lo marca machete pero activa features de `p256::ecdsa`: se comprueba compilando `sign`) |
| cli | `serde_json` | — | 0 usos | **Q** (machete) |
| cli | `clap`, `notify`, `axum`, `tokio`, `anyhow`, `reqwest` | CLI, `pdfcn dev`, `--fetch-remote-images` | — | M (no están en el binario serverless) |
| vercel | `hyper` | — | 0 usos directos | **Q** (machete; `vercel_runtime` lo trae igual, con `full`) |
| vercel | `reqwest` | descarga de imágenes remotas con protección SSRF | 1 fichero | **S posible**: un cliente sobre `hyper-util` + `tokio-rustls` quitaría `url`/`idna` (25 crates) y unos 39 en total, pero reescribe la parte sensible a seguridad (resolución de DNS, IPs privadas, redirecciones). Riesgo alto: se propone, no se ejecuta sin decisión |
| vercel | `vercel_runtime`, `http`, `http-body-util`, `tokio`, `serde*`, `base64`, `rayon` | runtime y endpoints | — | M |
| motor | `hyphenation` (`embed_all`, vía `printpdf/html` → `azul-layout/text_layout_hyphenation`) | `hyphens: auto` | ninguna utilidad, ningún ejemplo | **O** en el fork de printpdf (`html` sin separación silábica + feature aparte): ~3 MB de binario. Cambia el comportamiento de un `hyphens: auto` de autor: se decide con el usuario |

Duplicados de versión y quién los trae: `gif`, `zune-jpeg`/`zune-core`,
`weezl` (resvg `raster-images`); `png` 0.17 + `bitflags` 1 (tiny-skia
`png-format`, para `encode_png` del raster de respaldo); `kurbo` 0.11
(svgtypes); `roxmltree` 0.20 (fontconfig-parser); `data-url` 0.1, `phf`
0.11, `hashbrown` 0.12, `ahash` 0.7, `getrandom` 0.2 (lightningcss
`sourcemap`); `syn` 1 (lightningcss-derive); `getrandom` 0.3 (ahash 0.8).
Casi todos desaparecen con los pasos F y S de lightningcss y resvg.

## Arquitectura

Grafo entre crates (hoy):

```
pdfcn-parser ── pdfcn-template ── pdfcn-components ── pdfcn-formats
      └──────────────┴──────────────────┴─────────────────┴── pdfcn-core ── pdfcn-cli
pdfcn-styles ─────────────────────────────────────────────────────┘    ├── pdfcn-node
                                                                        └── pdfcn-vercel
```

Problemas, con datos:

1. **Cuatro traducciones de «opciones de petición → `RenderOptions`»**:
   tamaño/orientación/margen de página en `api/generate-pdf.rs`,
   `api/generate-pdf-batch.rs` (`into_page_config`, ~20 líneas cada una),
   `pdfcn-node::to_render_options` (102 líneas) y `pdfcn-cli` `build::run`
   (136 líneas); opciones de imagen en `src/dto.rs`, napi y CLI. Cada
   front-end valida a su manera (un tamaño desconocido es error en uno y
   valor por defecto en otro). **Propuesta**: un `RenderRequest`
   deserializable en `pdfcn-core::options` (campos en texto, como llegan) con
   `into_render_options() -> Result<RenderOptions, String>`; los cuatro
   front-ends lo usan. Sube a core; baja ~250 líneas.
2. **`pdfcn-core/src/lib.rs` mezcla IO y lógica**: `render_files*` leen
   disco y red (`RemoteImageFetcher`) junto al pipeline puro. **Propuesta**:
   `pdfcn-core/src/fs.rs` con `FsPartialLoader`, `render_files*` y
   `load_images`; `lib.rs` queda en re-exports y el pipeline.
3. **`render_pdf_with_assets` (207 líneas)** encadena preparación, opciones de
   printpdf, fuentes, layout, enlaces, metadatos y post-proceso.
   **Propuesta**: partirla en `prepare` (assets + placeholders), `layout`
   (opciones + fuentes + layout + enlaces) y `finish` (metadatos + guardado +
   `pdf_xobjects`), sin cambiar su firma pública.
4. **`assets.rs` (1 400 líneas, mitad tests)** junta QR, vectores, recortes,
   orientación y reducción. **Propuesta**: `assets/{scan,qr,vectors,photos}.rs`.
5. `pdfcn-styles/utilities.rs::resolve_with` es una función de 545 líneas
   (una tabla de utilidades en forma de `match`): legible como tabla; no se
   toca sin un motivo medido.
6. `pdfcn-formats` depende de `pdfcn-components` solo para la lista de
   nombres de componentes; aceptable.

Grafo propuesto: el mismo, con `pdfcn-core` exponiendo `RenderRequest`;
`pdfcn-cli`, `pdfcn-node` y `pdfcn-vercel` dejan de depender de los tipos
internos de página e imagen.

## Objetivos del prompt frente a lo que permiten los datos

- **≤ 300 crates** (cifra del prompt): los pasos F (lopdf, resvg,
  lightningcss si no se sustituye) y Q quitan del orden de 30–45 «líneas»;
  sustituir lightningcss, ~50; sustituir reqwest, ~60. Con F + S(lightningcss)
  se estima ~330; llegar a 300 exige además reqwest o la separación
  silábica. Se medirá tras cada paso.
- **Binario ≤ 22 MB**: lightningcss (~2,7 MiB de código) + separación
  silábica (~3 MB de datos) lo dejarían en ~21–22 MB; sin la segunda,
  ~24 MB.
- **Compilación en limpio −25 %** (508 s → ≤ 381 s): la ruta crítica es motor
  → `pdfcn-core` → enlace LTO (183 s). Quitar dependencias acorta la suma pero
  poco la ruta crítica; lo que la mueve es el enlace: `lto = "thin"` o más
  `codegen-units` — a medir contra tiempo de render y tamaño, porque la fase
  005 ya eligió velocidad sobre tamaño.

## Plan (un PR por paso, en este orden)

1. **D1 — features**: `lopdf` y `lightningcss` sin defaults; `resvg` solo
   con `text`. Sin código nuevo. Golden igual, tests igual.
2. **D2 — dependencias sin uso**: `serde` (core), `serde_json` (cli),
   `hyper` (vercel).
3. **D3 — lightningcss → código propio** con test de equivalencia (sobre
   todas las utilidades, temas claro y oscuro, y los `examples/`).
4. **D4 — arquitectura**: `RenderRequest` compartido; `fs.rs`; partir
   `render_pdf_with_assets` y `assets.rs`.
5. **Decisiones del usuario** antes de tocar: separación silábica opcional
   en el fork; sustituir `reqwest`; perfil de enlace.

## Resultados

Mismo método que arriba. «Líneas» = la cifra del prompt; «únicos» = nombre +
versión.

| Paso | Líneas | Únicos | `generate-pdf` | Notas |
|---|---|---|---|---|
| Antes (fases A–C) | 417 | 329 | 27,0 MB | |
| D1 features | 390 | 308 | 26,9 MB | golden igual |
| D2 sin uso | 390 | 308 | 26,9 MB | manifiestos honestos; ningún crate sale del grafo |
| D3 lightningcss → `minify.rs` | **365** | **285** | **22,9 MB** | render igual (instrucciones: factura +0,1 %, catálogo −0,5 %); compilación en limpio 508 → **458 s** (−10 %; enlace 183 → 148 s; 440 → 376 unidades) |
| D5 separación silábica opcional | **361** | **281** | **20,0 MB** | fork `fix/optional-hyphenation`; decisión del usuario: apagada por defecto |
| D4 arquitectura (tras D5) | 361 | 281 | 20,0 MB | sin cambio de dependencias ni de render |

**Nota de migración (D5)**: un CSS de autor con `hyphens: auto` deja de
partir palabras en el build por defecto (y en la función Vercel). Quien lo
necesite compila con la feature `hyphenation` (`pdfcn-core` o `pdfcn-cli`).
Ninguna utilidad ni ejemplo la usaba. Decisiones del usuario sobre lo demás:
`reqwest` se mantiene (reescribir la protección SSRF no compensa ~39 crates)
y el perfil de enlace no se toca (velocidad de render primero, spike 005).
Con eso, los objetivos de ≤ 300 crates y −25 % de compilación quedan fuera
de alcance: lo realista es ~360 líneas / ~280 crates únicos y −10 %.

### D4 — arquitectura, sin cambiar el comportamiento

- **Página e imagen se interpretan en un solo sitio**: `PageSize::from_name`,
  `Orientation::from_name`, `PageConfig::from_request` (`page.rs`) e
  `ImageFormat::from_name` (`options.rs`). Los dos endpoints HTTP comparten
  `pdfcn_vercel::dto::PageConfigDto` (antes uno por binario); napi usa
  `from_request`; la CLI usa `from_name` y sigue dando error ante un nombre
  desconocido. No se hizo un `RenderRequest` completo: cada front-end tiene
  campos propios (la CLI lee ficheros, napi recibe `Buffer`s) y unificarlos
  cambiaría su API pública; lo duplicado de verdad era esto. Test
  `from_request_reads_every_input_as_the_front_ends_did` compara con el
  parser antiguo, copiado tal cual, en todas las combinaciones.
- **IO fuera del pipeline**: `pdfcn-core/src/fs.rs` (`FsPartialLoader`,
  `render_files*`, `RemoteImageFetcher`, `load_images`); mismas rutas
  públicas por re-export.
- **`render_pdf_with_assets`** 207 → 144 líneas: `content_box_px`,
  `embedded_fonts` y `apply_metadata` salen a funciones; lo que depende de
  features (`vector`, `jpeg-passthrough`) se queda a la vista en la función.
- Neto: −65 líneas en 8 ficheros. Golden igual; dependencias y tamaño igual.
- **Deuda**: partir `assets.rs` en `assets/{scan,qr,vectors,photos}.rs` no se
  hizo: es mover ~1 400 líneas sin beneficio medible y con alto coste de
  revisión; queda propuesto.

### D3 — lo que se encontró al sustituir lightningcss

`pdfcn-styles` queda sin ninguna dependencia. `minify.rs` (≈200 líneas con
tests) compacta espacios, escribe todo `rgb/rgba/hsl/hsla` como hex y pone
`px` a un número suelto en una propiedad de longitud. La prueba de
equivalencia (`pdfcn-core/tests/css_equivalence.rs`, lightningcss solo como
dev-dependency) renderiza **todo el corpus** (`fixtures/utility-corpus.txt`,
1 719 utilidades que resuelven por tema, claro y oscuro) en bloques de 64 con
la hoja nueva y con lo que el código viejo enviaba, y compara los ops de
página. Resultado: **idénticos** (colores a ±1/255), con dos excepciones que
resultaron ser bugs del código viejo, no del nuevo:

1. Cualquier clase que necesita selector de atributo — `w-1/2`, `p-2.5`,
   todo valor arbitrario — hacía que lightningcss soltase una `\`, y el código
   viejo enviaba entonces **la hoja sin minificar**. En ese camino azul
   convertía `hsl()` por su cuenta (±1 en un canal) y no podía leer
   `rgb(0 0 0/50%)`, que además le hacía perder las reglas siguientes.
2. En ese mismo camino, `border-color: hsl(214.3, 31.8%, 91.4%)` no lo lee el
   parser de `border-color` de azul (cuatro lados separados por espacios): el
   token `border` de shadcn salía **negro, o no salía**.

Los dos casos tienen test (`a_space_separated_colour_no_longer_costs_the_rules_after_it`,
`a_border_takes_its_token_colour_whatever_else_the_document_uses`), que
fallan con la hoja vieja.

**Golden** (regenerado a propósito, 7 páginas de `invoice`, `showcase` y
`verifactu`, que usan esas clases): los bordes de tablas y tarjetas pasan de
negro o ausentes al color del token (`rgb(226, 232, 240)`), y algunos
colores cambian en 1 en un canal (`rgb(1, 8, 22)` → `rgb(2, 8, 23)`, que es
`#020817`, el valor exacto de shadcn).
