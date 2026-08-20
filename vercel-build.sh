#!/usr/bin/env bash
# Builds pdfcn-vercel as a Lambda-compatible "bootstrap" binary and assembles
# a Vercel Build Output API v3 directory by hand — Vercel has no native Rust
# runtime, but it does run any "provided.al2023" Lambda binary, which is
# exactly what `cargo lambda build` (and pdfcn-vercel's own [[bin]] name =
# "bootstrap") already produce. No Docker, no headless browser: a single
# static binary (NFR-2/NFR-3).
set -euxo pipefail

# Everything under .vercel/cache survives between deploys (Vercel restores
# it before the build and persists it after) — pointing the toolchain,
# cargo registry/git checkouts, and build artifacts in there turns every
# build after the first into an incremental one instead of a from-scratch
# rustup install + full workspace compile every single time.
CACHE_DIR="$PWD/.vercel/cache"
export CARGO_HOME="${CARGO_HOME:-$CACHE_DIR/cargo-home}"
export RUSTUP_HOME="${RUSTUP_HOME:-$CACHE_DIR/rustup-home}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$CACHE_DIR/target}"
cargo_bin_dir="$CARGO_HOME"
cargo_bin_dir+=/bin
PATH=$cargo_bin_dir:$PATH
export PATH

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --default-toolchain stable --profile minimal
fi

rustc --version
cargo --version

# The prebuilt installer drops a ready-made binary in seconds; `cargo
# install cargo-lambda --locked` instead compiles cargo-lambda's own
# (large) dependency tree from source, which is by far the slowest single
# step in this script.
if ! command -v cargo-lambda >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSfL https://cargo-lambda.info/install.sh | sh
fi

# cargo-lambda cross-links against Lambda's Amazon Linux 2 glibc using zig,
# so it needs zig on PATH even when the build host is already Linux/x86_64 —
# without it, `cargo lambda build` refuses to run.
if ! command -v zig >/dev/null 2>&1; then
  if command -v npm >/dev/null 2>&1; then
    npm install -g @ziglang/cli
  elif command -v pip3 >/dev/null 2>&1; then
    # `pip3 install ziglang` doesn't add a `zig` shim to PATH — it ships the
    # real binary at .../site-packages/ziglang/zig, so PATH must point there.
    pip3 install ziglang
    zig_pkg_dir="$(python3 -c 'import ziglang, os; print(os.path.dirname(ziglang.__file__))')"
    PATH="$zig_pkg_dir:$PATH"
    export PATH
  else
    echo "error: zig is not installed and neither npm nor pip3 is available to install it" >&2
    exit 1
  fi
fi

cargo lambda build --release --target x86_64-unknown-linux-gnu -p pdfcn-vercel

OUT=".vercel/output"
FUNC_DIR="$OUT/functions/api/generate-pdf.func"
rm -rf "$OUT"
mkdir -p "$FUNC_DIR" "$OUT/static"

cp "$CARGO_TARGET_DIR/lambda/bootstrap/bootstrap" "$FUNC_DIR/bootstrap"
chmod +x "$FUNC_DIR/bootstrap"

cat > "$FUNC_DIR/.vc-config.json" <<'EOF'
{
  "runtime": "provided.al2023",
  "handler": "bootstrap",
  "launcherType": "Native",
  "architecture": "x86_64"
}
EOF

cp web/index.html "$OUT/static/index.html"

cat > "$OUT/config.json" <<'EOF'
{ "version": 3 }
EOF

echo "Build Output API assembled at $OUT"
