#!/usr/bin/env bash
# build-wasm-bundle.sh — Build wasm_activation/pkg and tarball it for
# per-commit publication (Issue #37; wasm64 lane, Issue #541).
#
# Used by .github/workflows/wasm-bundle.yml. Kept as a standalone script so
# the build/packaging contract is unit-testable via bats without spinning up
# a runner.
#
# Behaviour:
#   1. Build the bundle for --arch (skipped when --pkg-dir is supplied):
#        wasm32 — `wasm-pack build neat-core --target web …`
#        wasm64 — `cargo +nightly build --target wasm64-unknown-unknown
#                  -Z build-std=…` followed by the `wasm-bindgen` CLI, because
#                  wasm-pack hard-codes `wasm32-unknown-unknown` as its cargo
#                  target and cannot emit a Memory64 module.
#   2. Gate what was just built: `scripts/check_wasm64_bundle.ts` proves the
#      memory index type matches --arch and the activation/backprop surface
#      survived into both the module and the generated glue. wasm-bindgen
#      0.2.108 exited 0 while stripping that glue on wasm64, so "the CLI
#      returned 0" is not evidence of a usable bundle.
#   3. Verify `wasm_activation_bg.wasm` exceeds the configured byte threshold
#      (defaults to 100 KB) so a stub build does not get published.
#   4. Embed the commit SHA in `pkg/neat_core_rev.txt` for downstream
#      integrity checks (NEAT-AI's `build.sh`).
#   5. Tar+gzip the `pkg/` directory so the resulting archive unpacks to a
#      `pkg/` subfolder, matching NEAT-AI's existing import paths.
#
# Exits non-zero (without producing a tarball) on any failure so the workflow
# never publishes an incomplete, undersized or wrong-arch bundle.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

DEFAULT_MIN_WASM_BYTES=102400
DEFAULT_ARCH="wasm32"
# `wasm64-unknown-unknown` is a Rust Tier 3 target: no prebuilt `std`, so the
# build needs a nightly toolchain plus `rust-src` and `-Z build-std`.
WASM64_TOOLCHAIN="${WASM64_TOOLCHAIN:-nightly}"
WASM64_TARGET="wasm64-unknown-unknown"
REV="${GITHUB_SHA:-}"
OUT_TAR="wasm_activation-pkg.tar.gz"
MIN_WASM_BYTES="$DEFAULT_MIN_WASM_BYTES"
ARCH="$DEFAULT_ARCH"
PKG_DIR=""
SKIP_BUILD=0

usage() {
  cat <<EOF
Usage: build-wasm-bundle.sh [options]

Builds the wasm_activation bundle and packages pkg/ as a tarball.

Options:
  --arch <wasm32|wasm64>   Address size to build and gate against
                           (default: ${DEFAULT_ARCH}). wasm64 emits a Memory64
                           module and needs the ${WASM64_TOOLCHAIN} toolchain,
                           the rust-src component and the wasm-bindgen CLI.
  --rev <SHA>              Commit SHA to embed in neat_core_rev.txt.
                           Defaults to \$GITHUB_SHA if set.
  --out <path>             Output tarball path
                           (default: wasm_activation-pkg.tar.gz).
  --min-size-bytes <N>     Minimum acceptable wasm_activation_bg.wasm size,
                           in bytes (default: ${DEFAULT_MIN_WASM_BYTES}).
  --pkg-dir <path>         Use a pre-built pkg/ directory instead of running
                           wasm-pack. Implies --skip-build (used by tests).
  --skip-build             Skip wasm-pack invocation. Requires --pkg-dir.
  -h, --help               Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      [[ $# -ge 2 ]] || { echo "error: --arch requires a value" >&2; exit 2; }
      ARCH="$2"
      shift 2
      ;;
    --rev)
      [[ $# -ge 2 ]] || { echo "error: --rev requires a value" >&2; exit 2; }
      REV="$2"
      shift 2
      ;;
    --out)
      [[ $# -ge 2 ]] || { echo "error: --out requires a value" >&2; exit 2; }
      OUT_TAR="$2"
      shift 2
      ;;
    --min-size-bytes)
      [[ $# -ge 2 ]] || { echo "error: --min-size-bytes requires a value" >&2; exit 2; }
      MIN_WASM_BYTES="$2"
      shift 2
      ;;
    --pkg-dir)
      [[ $# -ge 2 ]] || { echo "error: --pkg-dir requires a value" >&2; exit 2; }
      PKG_DIR="$2"
      SKIP_BUILD=1
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$REV" ]]; then
  echo "error: --rev (or GITHUB_SHA) is required" >&2
  exit 2
fi

if ! [[ "$MIN_WASM_BYTES" =~ ^[0-9]+$ ]]; then
  echo "error: --min-size-bytes must be a non-negative integer (got '$MIN_WASM_BYTES')" >&2
  exit 2
fi

if [[ "$ARCH" != "wasm32" && "$ARCH" != "wasm64" ]]; then
  echo "error: --arch must be wasm32 or wasm64 (got '$ARCH')" >&2
  exit 2
fi

if [[ "$SKIP_BUILD" -eq 1 && -z "$PKG_DIR" ]]; then
  echo "error: --skip-build requires --pkg-dir" >&2
  exit 2
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: $1 is required on PATH$2" >&2
    exit 1
  fi
}

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  PKG_DIR="neat-core/wasm_activation/pkg"
  rm -rf "neat-core/wasm_activation"

  if [[ "$ARCH" == "wasm32" ]]; then
    require_cmd wasm-pack ""
    echo "🛠️  Running wasm-pack build (target=web, out-name=wasm_activation)"
    wasm-pack build neat-core \
      --target web \
      --out-name wasm_activation \
      --out-dir wasm_activation/pkg
  else
    # wasm-pack cannot reach this target: 0.15.0 still hard-codes
    # `wasm32-unknown-unknown` as the cargo target it builds and reads back.
    # Driving cargo + the wasm-bindgen CLI by hand is the documented
    # equivalent, and it is the *same* wasm-bindgen post-processing step
    # wasm-pack would run — no raw `extern "C"` fallback needed since CLI
    # 0.2.120 landed Memory64 codegen.
    require_cmd cargo ""
    require_cmd wasm-bindgen " (cargo install wasm-bindgen-cli --version <crate version>)"
    target_dir="${CARGO_TARGET_DIR:-target}"
    raw_wasm="${target_dir}/${WASM64_TARGET}/release/neat_core.wasm"
    echo "🛠️  Building neat-core for ${WASM64_TARGET} (${WASM64_TOOLCHAIN} + -Z build-std)"
    cargo "+${WASM64_TOOLCHAIN}" build \
      --package neat-core \
      --release \
      --target "$WASM64_TARGET" \
      -Z build-std=std,panic_abort
    if [[ ! -f "$raw_wasm" ]]; then
      echo "error: cargo produced no ${WASM64_TARGET} artefact at ${raw_wasm}" >&2
      exit 1
    fi
    echo "🔗  Running wasm-bindgen (target=web, out-name=wasm_activation)"
    wasm-bindgen "$raw_wasm" \
      --target web \
      --out-name wasm_activation \
      --out-dir "$PKG_DIR"
  fi

  # Gate what was just built, before anything is packaged. A CLI that exits 0
  # while stripping the bindings is a silent failure, not a pass.
  require_cmd deno " (needed to gate the built bundle)"
  echo "🚦 Gating the built bundle (arch=${ARCH})"
  deno run --allow-read "${SCRIPT_DIR}/check_wasm64_bundle.ts" "$PKG_DIR" --arch "$ARCH"
fi

if [[ ! -d "$PKG_DIR" ]]; then
  echo "error: pkg directory '$PKG_DIR' not found" >&2
  exit 1
fi

WASM_FILE="$PKG_DIR/wasm_activation_bg.wasm"
DTS_FILE="$PKG_DIR/wasm_activation.d.ts"
JS_FILE="$PKG_DIR/wasm_activation.js"

for required in "$WASM_FILE" "$DTS_FILE" "$JS_FILE"; do
  if [[ ! -f "$required" ]]; then
    echo "error: expected wasm-pack output missing: $required" >&2
    exit 1
  fi
done

WASM_SIZE=$(wc -c <"$WASM_FILE" | tr -d ' ')
echo "wasm_activation_bg.wasm size: ${WASM_SIZE} bytes (threshold ${MIN_WASM_BYTES})"
if (( WASM_SIZE < MIN_WASM_BYTES )); then
  echo "error: wasm_activation_bg.wasm is below the minimum size threshold (${WASM_SIZE} < ${MIN_WASM_BYTES} bytes)" >&2
  exit 1
fi

printf '%s\n' "$REV" >"$PKG_DIR/neat_core_rev.txt"

PARENT_DIR="$(cd "$(dirname "$PKG_DIR")" && pwd)"
PKG_BASE="$(basename "$PKG_DIR")"

# Resolve OUT_TAR to an absolute path so the -C below does not move it.
case "$OUT_TAR" in
  /*) OUT_ABS="$OUT_TAR" ;;
  *)  OUT_ABS="$PWD/$OUT_TAR" ;;
esac

tar -czf "$OUT_ABS" -C "$PARENT_DIR" "$PKG_BASE"

ARCHIVE_SIZE=$(wc -c <"$OUT_ABS" | tr -d ' ')
echo "✅ Built bundle: $OUT_ABS (${ARCHIVE_SIZE} bytes, rev=${REV})"
