#!/usr/bin/env bash
# Every check a phase must pass before it is committed (AGENTS.md and
# docs/prompts/rendimiento-y-arquitectura.md, section 1), in one go:
#
#   scripts/check.sh          # tests, both feature states, clippy, fmt
#   scripts/check.sh --size   # ... plus the release serverless binary's size
#
# Prints only failures and a verdict; the full output of a failing step is
# kept in target/check-<n>.log.
set -uo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
status=0
n=0
run() {
  n=$((n + 1))
  local log="target/check-$n.log"
  echo "== $*"
  if "$@" >"$log" 2>&1; then
    grep -E "^test result" "$log" | awk '{p += $4; f += $6} END {if (NR) printf "   %d passed, %d failed\n", p, f}'
  else
    grep -E "^test .*FAILED|panicked at|^error" "$log" | head -20
    echo "!! failed: $* (see $log)"
    status=1
  fi
}
run cargo test --workspace --no-fail-fast
run cargo test -p pdfcn-core --no-default-features --no-fail-fast
run cargo test -p pdfcn-cli --no-default-features --no-fail-fast
run cargo clippy --workspace --all-targets -- -D warnings
run cargo clippy -p pdfcn-core -p pdfcn-components --all-targets --no-default-features -- -D warnings
run cargo clippy -p pdfcn-core --all-targets --features hyphenation,sign -- -D warnings
run cargo fmt --all --check
if [ "${1:-}" = "--size" ]; then
  # Never with --workspace: resolver 2 would unify features across packages.
  run cargo build --release -p pdfcn-vercel --bin generate-pdf
  ls -l target/release/generate-pdf | awk '{printf "generate-pdf: %.1f MB\n", $5 / 1e6}'
  echo "crates (pdfcn-vercel, normal edges): $(cargo tree -p pdfcn-vercel --prefix none -e normal | sort -u | wc -l)"
fi
if [ $status -eq 0 ]; then echo "ALL CHECKS PASSED"; else echo "SOME CHECKS FAILED"; fi
exit $status
