#!/bin/bash
# Issue #511 — build the shipped control and the validated exact-size inference
# prototype as two wasm modules, then benchmark them interleaved in one session.
#
# Usage: ./run-exact.sh [samples] [shape-index] [records] [sessions]
#
#   shape-index indexes `NETWORKS` in neat-core/benches/common/mod.rs:
#   3 = production, 4 = production_2x, 5 = production_exact (default).
#
# Reuses `runner.mjs` / `analyse.mjs` from the Issue #509 harness verbatim, so
# the variant column is still labelled `unchecked` — here it means **variant B,
# the #511 prototype** (`activate_into_exact`). Only the `activate` bench
# differs between the two modules; `kernel` and `score` compile to identical
# code in both, so their paired ratios are this session's noise floor.
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

cd "${BASH_SOURCE[0]%/*}"

SAMPLES="${1:-15}"
SHAPE="${2:-5}"
RECORDS="${3:-4096}"
SESSIONS="${4:-3}"

TARGET="wasm32-unknown-unknown"
ARTEFACTS="../target/wasm-bench"
export RUSTFLAGS="-C target-feature=+simd128,+relaxed-simd"

for cmd in cargo node; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "run-exact.sh: $cmd is required" >&2
    exit 1
  fi
done

build() {
  local variant="$1" out="$2"
  shift 2
  echo "building $variant …" >&2
  cargo build --release --target "$TARGET" --target-dir "$ARTEFACTS/$variant" "$@" >&2
  cp "$ARTEFACTS/$variant/$TARGET/release/wasm_gather4_bench.wasm" "$out"
}

mkdir -p results
CONTROL="results/exact-control.wasm"
EXPERIMENT="results/exact-prototype.wasm"
build exact-control "$CONTROL"
build exact-prototype "$EXPERIMENT" --features exact-inference

echo "binary size (bytes):" >&2
wc -c "$CONTROL" "$EXPERIMENT" >&2

CSV="results/exact-shape$SHAPE.csv"
: >"$CSV"

for ((session = 0; session < SESSIONS; session++)); do
  echo "session $session …" >&2
  node runner.mjs "$CONTROL" "$EXPERIMENT" "$SAMPLES" "$SHAPE" "$RECORDS" "$session" >>"$CSV"
done

node analyse.mjs "$CSV"
