# Prompt: rendimiento, arquitectura y dependencias de pdfcn-rs

Copia todo lo que hay debajo de la línea en una sesión nueva sobre este repo.

---

Eres el responsable técnico de `pdfcn-rs`. Objetivo: que sea claramente más
rápido y ligero que Typst para documentos con fotos y SVG, sin perder
corrección, y con menos dependencias y menos código que mantener. Tienes
licencia para refactorizar la arquitectura, cambiar nuestros forks de
`printpdf` y `azul`, y escribir internamente piezas que hoy son dependencias.
No tienes licencia para romper el contrato público (plantillas `.pdfcn`, CLI,
API Vercel, bindings napi) sin una nota de migración explícita.

## 0. Punto de partida (léelo antes de tocar nada)

1. `AGENTS.md` (convenciones: features de Cargo, nada entra en pánico con
   entrada mala, snapshots golden, `.feature` con `@covers`, spike doc antes de
   código no trivial, no tocar código del motor fuera de los forks, nunca
   amend/force-push sin permiso, ramas de feature + PR).
2. `docs/spikes/005-image-fast-path.md`, sobre todo "Siguientes pasos". Ya
   están hechos: JPEG passthrough, reducción opt-in, rutas simples en
   plantillas, `opt-level = 3`. Catálogo 100 fotos + 100 SVG: 14,7 s → 0,39 s.
3. Lo que **no** está hecho y está medido:
   - Los bytes de imagen se copian 3 veces por render
     (`images.clone()`, `Base64OrRaw::Raw(bytes.clone())`, copia de printpdf
     a `xml_options.images`); las fuentes embebidas (300–500 KB) se copian 2.
   - `with_binding` clona el contexto completo en cada iteración de `- for`.
   - Con `<a href>` el documento se maqueta dos veces (render sonda en `links`).
   - Pico de memoria del catálogo ≈ 250 MB.
   - Preparación de imágenes y SVG→PDF son independientes por imagen y se
     hacen en serie.
   - No hay caché por contenido entre renders (`pdfcn dev`, servidores).
4. Línea base de dependencias: `cargo tree -p pdfcn-vercel --prefix none -e normal | sort -u | wc -l`
   → **417** crates. Tamaño de `generate-pdf` en release: **26,8 MB** (tope CI
   60 MB). Anota estas cifras y las de tiempo/memoria antes de empezar; cada
   fase termina comparando contra ellas.

## 1. Reglas de trabajo

- Mide antes y después. Sin número, no hay mejora. Benchmarks reproducibles
  (el catálogo, la factura, un documento de 1 página, uno de 200). Para
  memoria usa `/usr/bin/time -v` o `heaptrack`/`valgrind --tool=massif`; para
  CPU, callgrind como en el spike. Guarda los scripts en el repo
  (`benches/` o `scripts/`), no en `/tmp`.
- Una fase = una rama `feature` = un PR pequeño y revisable. Orden fijo de
  abajo; no mezcles fases.
- Cada fase: spike doc en `docs/spikes/00N-*.md` con lo **verificado en el
  código fuente real** (`~/.cargo/registry/src/` y los forks), tests (unitarios
  + `features/*.feature` con `@covers` exacto), golden regenerado
  deliberadamente y explicado en el commit.
- Cada fase pasa en limpio: `cargo test --workspace`,
  `cargo test -p pdfcn-core --no-default-features`,
  `cargo test -p pdfcn-cli --no-default-features`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, y `cargo build --release -p pdfcn-vercel --bin generate-pdf`
  (nunca con `--workspace`) comprobando el tamaño.
- Nada de `unwrap()`/`expect()` fuera de tests. Entrada mala degrada a
  error/marca, nunca a pánico.
- Cambios en `printpdf`/`azul`: rama `fix/*` propia en el fork con test,
  merge a `pdfcn`, y solo entonces subir el `rev` aquí. No parchear el motor
  dentro de este repo. Proponer upstream cuando sea razonable.
- Commits: sin atribuir a ninguna herramienta ni modelo en autor ni en
  descripción (autor = el usuario del repo). Sin amend ni force-push.
- Si un hallazgo contradice este prompt, gana el hallazgo: escríbelo en el
  spike y ajusta el plan.

## 2. Fases

### Fase A — Cero copias y menos asignaciones (bajo riesgo, hacer primero)

1. Cambiar el mapa de imágenes y las fuentes a `Arc<[u8]>` (o `Bytes`) de
   extremo a extremo en `pdfcn-core`. Eliminar `images.clone()` y los
   `bytes.clone()`/`to_vec()` de `render_pdf_with_assets`.
2. Si hace falta, ampliar la API del fork de printpdf para aceptar
   `Arc<[u8]>`/`Cow<[u8]>` (rama `fix/zero-copy-images` en el fork + test).
   Primero comprueba si basta con pasar la propiedad (move) en vez de clonar.
3. Cadena de ámbitos para `- for`/`- include` en `pdfcn-template`: base +
   overlays inmutables, de modo que entrar en una iteración sea O(1) y no
   clone el contexto. Mantener la semántica actual (sombreado de variables).
4. Objetivo: pico de memoria del catálogo < 120 MB y cero copias de imagen
   verificadas con un test que cuente asignaciones o con massif.

### Fase B — Un solo maquetado con enlaces

1. Leer cómo `links` hace el render sonda y qué datos del layout necesita
   (cajas de los `<a>`).
2. Exponer esas cajas desde el bridge de `azul-layout` (rama `fix/*` en el
   fork), calculándolas en el mismo pase que el render final.
3. Quitar el segundo maquetado. Test: un documento con 50 enlaces, mismas
   anotaciones `/Link` byte a byte, y tiempo ≈ el de uno sin enlaces.

### Fase C — Paralelismo y cachés (sin dependencias nuevas)

1. `std::thread::scope` para: decodificación/normalización de imágenes,
   conversión SVG→PDF y generación de gráficos/códigos de barras. Tope de
   hilos configurable (`PDFCN_THREADS`, por defecto `available_parallelism`).
   Resultado determinista: mismo PDF byte a byte con 1 o N hilos.
2. Cachés por hash de contenido en el proceso, detrás de una feature
   `cache` (decide si por defecto activada según el tamaño): imágenes
   preparadas, AST parseado, SVG convertidos. Con límite de memoria y
   desalojo LRU escrito a mano. Invalidación por hash, nunca por mtime.
3. Objetivo: `pdfcn dev` re-render de un cambio de texto en el catálogo
   < 50 ms; batch de 8 documentos escala ≥ 3× en 4 núcleos.

### Fase D — Auditoría de arquitectura y dependencias

Esta es la fase grande. Haz **primero un informe, sin cambiar código**:

1. Inventario: `cargo tree -d` (duplicados), `cargo tree --edges features`,
   `cargo bloat --release --crates -p pdfcn-vercel` (instalarlo si falta),
   `cargo machete`/`cargo udeps` para dependencias sin uso, y tiempo de
   compilación en limpio (`cargo build --timings`).
2. Para cada dependencia directa de cada crate del workspace, una fila:
   para qué se usa, cuántas líneas la usan, qué aporta de tamaño y de
   compilación, y veredicto: **mantener**, **sustituir por código propio**
   (con estimación de líneas y riesgo), **hacer opcional tras una feature**,
   o **quitar**. Candidatos obvios a examinar, con criterio y datos, no por
   fe: `winnow` (¿basta un lexer a mano de indentación?), `minijinja`
   (¿qué subconjunto usamos de verdad?), `lightningcss`, `image` (¿qué
   códecs se compilan?, desactivar los que no se usan), `resvg`/`lopdf`,
   `serde_json`, crates de fechas y de errores.
3. Problemas de arquitectura: acoplamientos entre crates
   (`pdfcn-core` de 1278 líneas en `lib.rs` y 1283 en `assets.rs`),
   funciones largas, módulos que mezclan IO y lógica, tipos duplicados entre
   `pdfcn-core`, `pdfcn-cli` y `pdfcn-node`. Propón una división concreta
   (qué sube, qué baja, qué se parte), con el grafo de dependencias entre
   crates antes y después.
4. Ejecuta **solo** los cambios con mejor relación ahorro/riesgo, uno por
   PR: primero desactivar features de crates que no usamos, luego
   dependencias que se pueden quitar sin escribir código, luego las
   sustituciones por código propio con tests de equivalencia contra la
   implementación vieja (comparar salida sobre todos los `examples/` y un
   corpus de fuzz sencillo).
5. Objetivo: ≤ 300 crates en `pdfcn-vercel` (si el informe demuestra que es
   inviable, di por qué y fija el objetivo realista), binario ≤ 22 MB con
   `opt-level = 3`, compilación en limpio al menos 25 % más rápida.

### Fase E — Cambiar el fork de azul (solo si A–D lo justifican)

No empieces aquí. Llega con datos: perfila el maquetado y el pintado con
callgrind sobre el catálogo, la factura y un documento de texto largo, y
responde por escrito en un spike:

1. ¿Qué parte del tiempo y de la memoria sigue estando dentro de `azul-*`
   después de A–C? Si es < 25 %, **no cambies de motor**: documenta la
   decisión y para.
2. Si es mayor: evalúa alternativas reales, leyendo su código, no su README:
   (a) seguir con el fork y recortar `azul-layout`/`azul-core`/`azul-css`
   (quitar ventanas, GL, fuentes que no usamos, features de UI) en una rama
   `fix/slim-*`; (b) un maquetador propio mínimo para el subconjunto de CSS
   que de verdad usamos (`docs/spikes/003-style-coverage.md` lo enumera):
   bloque, flex, texto con saltos, tablas, imágenes, paginación; (c) otro
   motor existente de Rust si encaja. Para cada opción: líneas, dependencias
   que elimina, cobertura de CSS que pierde, riesgo, y un prototipo que
   renderice `examples/invoice` y `examples/brochure` para comparar contra
   los golden.
3. Criterio de decisión: solo se cambia si gana ≥ 2× en tiempo o memoria
   **o** quita ≥ 80 crates, sin perder ninguna utilidad del spike 003 que
   usen los ejemplos, y con un plan de migración por etapas detrás de una
   feature (`engine-azul` / `engine-own`) para poder volver atrás.
4. Cualquier cambio de motor es un PR aparte, con la prueba golden completa
   y una tabla de diferencias visuales aceptadas una a una.

## 3. Entregables y criterio de terminado

- Un PR por fase (A, B, C, D por partes, E solo si procede), cada uno con:
  tabla antes/después (tiempo, pico de memoria, nº de crates, tamaño del
  binario), spike doc, tests y `.feature`, golden explicado.
- `docs/spikes/` actualizado; `AGENTS.md` actualizado si cambian convenciones
  (nuevas features, nuevo reparto de crates, nuevo flujo de forks).
- Un resumen final con: lo conseguido contra los números de la sección 0, lo
  descartado y por qué, y lo que queda como deuda con su coste estimado.
- Está terminado cuando: catálogo ≤ 0,3 s y ≤ 120 MB de pico, enlaces sin
  doble maquetado, PDF idéntico con 1 o N hilos, dependencias y tamaño dentro
  de los objetivos de la fase D, todas las comprobaciones de la sección 1 en
  verde, y ningún fallo preexistente escondido (los que ya existen hoy:
  `pdfcn-core --no-default-features` falla el golden de `ticket`, y
  `pdfcn-components/icons` da 4 avisos de código muerto sin la feature
  `vector`; arréglalos o documenta por qué no, en la fase que toque ese código).

## 4. Qué NO hacer

- No optimizar sin perfil, ni "limpiar" por estética.
- No añadir dependencias nuevas para ganar velocidad: el paralelismo y las
  cachés se hacen con la biblioteca estándar.
- No aceptar un diff de golden sin entenderlo.
- No subir el `rev` de un fork sin test en la rama `fix/*` correspondiente.
- No abrir un PR gigante. Si una fase pasa de ~800 líneas netas, pártela.
- Ante una decisión que cambie el contrato público o borre una feature,
  detente y pregunta.
