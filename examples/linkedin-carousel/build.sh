#!/usr/bin/env bash
# Regenerates the LinkedIn carousel: python3 make.py writes the SVG layers,
# template and data into ./out, then pdfcn compiles them to a square PDF.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
python3 -I "$HERE/make.py"
cargo build -q --release -p pdfcn-cli --manifest-path "$REPO/Cargo.toml"
svgs=()
for f in "$HERE"/out/bg*.svg; do
  id="$(basename "$f" .svg)"
  svgs+=(--svg "$id=$f")
done
"$REPO/target/release/pdfcn" build "$HERE/out/carousel.haml" -d "$HERE/out/data.json" \
  -o "$HERE/out/pdfcn-linkedin.pdf" --size 285.75x285.75 --margin 0 "${svgs[@]}" \
  --title "pdfcn-rs: Tailwind + shadcn → PDF" --author pmaojo
