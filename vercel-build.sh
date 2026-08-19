#!/usr/bin/env bash
# Builds pdfcn-vercel as a Lambda-compatible "bootstrap" binary and assembles
# a Vercel Build Output API v3 directory by hand — Vercel has no native Rust
# runtime, but it does run any "provided.al2" Lambda binary, which is exactly
# what `cargo lambda build` (and pdfcn-vercel's own [[bin]] name = "bootstrap")
# already produce. No Docker, no headless browser: a single static binary
# (NFR-2/NFR-3).
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"

if ! command -v cargo-lambda >/dev/null 2>&1; then
  cargo install cargo-lambda --locked
fi

cargo lambda build --release -p pdfcn-vercel

OUT=".vercel/output"
FUNC_DIR="$OUT/functions/api/generate-pdf.func"
rm -rf "$OUT"
mkdir -p "$FUNC_DIR" "$OUT/static"

cp target/lambda/bootstrap/bootstrap "$FUNC_DIR/bootstrap"
chmod +x "$FUNC_DIR/bootstrap"

cat > "$FUNC_DIR/.vc-config.json" <<'EOF'
{
  "runtime": "provided.al2",
  "handler": "bootstrap",
  "launcherType": "Native"
}
EOF

cp web/index.html "$OUT/static/index.html"

cat > "$OUT/config.json" <<'EOF'
{ "version": 3 }
EOF

echo "Build Output API assembled at $OUT"
