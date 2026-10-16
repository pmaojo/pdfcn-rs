# Medir pdfcn: velocidad, pico de memoria y dónde se va cada cosa

Todo número de `docs/spikes/` sale de estas herramientas. Sin número no hay
mejora: se mide antes, se cambia, se mide después con el mismo procedimiento.

## 1. Velocidad y pico de memoria por proceso — `run.py`

```sh
cargo build --release -p pdfcn-cli
python3 -I scripts/bench/run.py                 # todos los casos, 7 procesos cada uno
python3 -I scripts/bench/run.py catalog -n 25   # un caso, más repeticiones
PDFCN=target/baseline/pdfcn python3 -I scripts/bench/run.py   # otro binario
python3 -I scripts/bench/run.py --json out.json
```

Qué hace, exactamente:

- Genera los documentos en `target/bench/` (nunca en el repo): `catalog`
  (100 JPEG 800×600 + 100 SVG, el fixture de
  `examples/linkedin-carousel/bench/catalog/gen.py`; necesita Pillow solo la
  primera vez), `invoice` (`examples/invoice`), `one-page`, `long` (200
  páginas de texto), `links` (50 enlaces, 20 páginas) y `nolinks` (el mismo
  documento con los enlaces como `<span>`).
- Lanza `pdfcn build` en **un proceso nuevo cada vez** (en frío, como una
  función serverless), una vez de calentamiento (caché de páginas del disco)
  y luego N veces.
- **Tiempo**: reloj de pared alrededor del proceso entero
  (`time.perf_counter`), arranque y escritura del PDF incluidos. Se informa
  la **mediana** (y el mínimo); la mediana resiste el ruido de un contenedor
  compartido.
- **Pico de memoria**: `ru_maxrss` del `rusage` que devuelve `os.wait4` para
  ese hijo: el máximo de memoria residente (RSS) que tuvo el proceso, en el
  kernel, sin instrumentar nada. Se informa el máximo de las N ejecuciones.
  Es lo que mide `/usr/bin/time -v` («Maximum resident set size»), sin
  necesitarlo instalado.

Reglas para comparar: cuando dos medianas se solapan (este contenedor oscila
±25 % entre tandas), decide el conteo de instrucciones de callgrind (sección
4), que es idéntico entre ejecuciones. Además: mismo contenedor, mismo número de núcleos (`nproc`),
nada más compilando a la vez, mismo `-n`. Para comparar dos versiones,
guarda el binario de antes (`cp target/release/pdfcn target/baseline/`) y
ejecuta las dos en la misma sesión. Diferencias por debajo de ~10 % en los
casos de 30–50 ms son ruido: repite con `-n 25`.

## 2. Dónde está la memoria — massif

```sh
cargo build --profile profiling -p pdfcn-cli     # release con símbolos (Cargo.toml)
cd target/bench/catalog
valgrind --tool=massif --massif-out-file=/tmp/m.out \
  ../../profiling/pdfcn build catalog.haml -d catalog.json -o out.pdf \
  $(for i in $(seq -f %03g 0 99); do printf -- "--svg s$i=img/s$i.svg "; done)
python3 -I ../../../scripts/bench/massif_top.py /tmp/m.out --depth 2
```

`massif_top.py` toma la instantánea de pico y agrupa los bytes vivos por la
primera función «interesante» de cada pila (salta el asignador, `alloc::`,
`core::`, `Vec::grow`…), para leer «quién pidió la memoria» y no «qué `Vec`
creció». Así se encontró que el pico del catálogo (245 MB) eran
placeholders decodificados a píxeles y PDFs recargados por lopdf, no los
bytes JPEG (spike 006).

## 3. Cuántas copias cuesta una imagen — test con asignador contador

`pdfcn-core/tests/zero_copy.rs` instala un `#[global_allocator]` que cuenta
bytes vivos y su pico. Renderiza el mismo documento con una foto grande y con
una pequeña; la diferencia de picos dividida por la diferencia de tamaños es
«cuántas copias de sí misma cuesta la foto». Corre en CI con el resto de
tests (`cargo test -p pdfcn-core --test zero_copy -- --nocapture` imprime las
cifras). Falla si vuelven las copias o los píxeles.

## 4. Dónde está el tiempo — callgrind

```sh
valgrind --tool=callgrind --callgrind-out-file=/tmp/c.out \
  target/profiling/pdfcn build ...
callgrind_annotate --inclusive=yes /tmp/c.out | head -60
```

Instrucciones por función (inclusivas), estables entre ejecuciones a
diferencia del tiempo. Es como se atribuyó el coste en los spikes 005 y en la
fase E (cuánto queda dentro de `azul-*`).

Reparto por crate, contando cada instrucción una vez (coste propio):

```sh
callgrind_annotate --auto=no --threshold=99.9 /tmp/c.out > /tmp/c.txt
python3 -I scripts/bench/callgrind_crates.py /tmp/c.txt
```

Lo que cae en libc (`memcpy`, `memset`, `malloc`) se atribuye a quien lo
llama con `callgrind_annotate --inclusive=yes --tree=caller`; así se
encontraron las fugas del spike 010.

## 5. Micro-mediciones de una sola pieza

Cuando una pieza se puede aislar (la evaluación de plantillas, por ejemplo),
se mide con un binario mínimo en el scratchpad que llama a la API pública con
un `Instant` alrededor, compilado en release, antes y después del cambio
(`git stash` de solo esa crate). El código de esa medición va en el spike
correspondiente si hace falta repetirla.

## 6. Comprobaciones antes de cada commit — `scripts/check.sh`

```sh
scripts/check.sh          # tests (con y sin features), clippy (ambos estados), fmt
scripts/check.sh --size   # ... y tamaño de generate-pdf + nº de crates
```

Usa `--no-fail-fast`: un binario de test que falla no esconde a los
siguientes (así aparecieron los fallos preexistentes de la fase A).
