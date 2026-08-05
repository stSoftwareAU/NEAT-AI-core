#!/bin/bash
# Issue #509 — build the safe control and the unchecked-gather4 prototype, then
# benchmark them interleaved inside one wasm session.
#
# Usage: ./run.sh [samples] [shape-index] [records] [sessions]
#
#   shape-index indexes `NETWORKS` in neat-core/benches/common/mod.rs:
#   3 = production, 4 = production_2x, 5 = production_exact (default).
#
# Emits CSV rows (variant,bench,sample,nanos,checksum_bits) to
# `results/shape<N>.csv` and prints the summary table.
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
# Production wasm build flags: the release profile (opt-level 3, lto,
# codegen-units 1 — see Cargo.toml) plus the SIMD features the kernels require.
export RUSTFLAGS="-C target-feature=+simd128,+relaxed-simd"

for cmd in cargo node; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "run.sh: $cmd is required" >&2
    exit 1
  fi
done

build() {
  local variant="$1" out="$2"
  shift 2
  echo "building $variant …" >&2
  # Build under the repo-level target/ so the artefacts land where every
  # repo-wide scan (codespell, lint, git) already excludes them.
  cargo build --release --target "$TARGET" --target-dir "$ARTEFACTS/$variant" "$@" >&2
  cp "$ARTEFACTS/$variant/$TARGET/release/wasm_gather4_bench.wasm" "$out"
}

mkdir -p results
CONTROL="results/control.wasm"
EXPERIMENT="results/unchecked.wasm"
build control "$CONTROL" --features checked-gather4
build unchecked "$EXPERIMENT"

echo "binary size (bytes):" >&2
wc -c "$CONTROL" "$EXPERIMENT" >&2

CSV="results/shape$SHAPE.csv"
: >"$CSV"

# Several independent sessions: within a session the two variants are
# interleaved, across sessions the whole comparison is repeated so a
# session-wide artefact (a background job, a thermal excursion) is visible
# rather than baked into the answer.
for ((session = 0; session < SESSIONS; session++)); do
  echo "session $session …" >&2
  node runner.mjs "$CONTROL" "$EXPERIMENT" "$SAMPLES" "$SHAPE" "$RECORDS" "$session" >>"$CSV"
done

node analyse.mjs "$CSV"
