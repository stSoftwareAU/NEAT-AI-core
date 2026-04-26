#!/usr/bin/env bash
# build-wasm-bundle.sh — Build wasm_activation/pkg via wasm-pack and tarball
# it for per-commit publication (Issue #37).
#
# Used by .github/workflows/wasm-bundle.yml. Kept as a standalone script so
# the build/packaging contract is unit-testable via bats without spinning up
# a runner.
#
# Behaviour:
#   1. Run `wasm-pack build neat-core --target web --out-name wasm_activation
#      --out-dir wasm_activation/pkg` (skipped when --pkg-dir is supplied).
#   2. Verify `wasm_activation_bg.wasm` exceeds the configured byte threshold
#      (defaults to 100 KB) so a stub build does not get published.
#   3. Embed the commit SHA in `pkg/neat_core_rev.txt` for downstream
#      integrity checks (NEAT-AI's `build.sh`).
#   4. Tar+gzip the `pkg/` directory so the resulting archive unpacks to a
#      `pkg/` subfolder, matching NEAT-AI's existing import paths.
#
# Exits non-zero (without producing a tarball) on any failure so the workflow
# never publishes an incomplete or undersized bundle.

set -euo pipefail

DEFAULT_MIN_WASM_BYTES=102400
REV="${GITHUB_SHA:-}"
OUT_TAR="wasm_activation-pkg.tar.gz"
MIN_WASM_BYTES="$DEFAULT_MIN_WASM_BYTES"
PKG_DIR=""
SKIP_BUILD=0

usage() {
  cat <<EOF
Usage: build-wasm-bundle.sh [options]

Builds the wasm_activation bundle and packages pkg/ as a tarball.

Options:
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

if [[ "$SKIP_BUILD" -eq 1 && -z "$PKG_DIR" ]]; then
  echo "error: --skip-build requires --pkg-dir" >&2
  exit 2
fi

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "error: wasm-pack is required on PATH" >&2
    exit 1
  fi
  PKG_DIR="neat-core/wasm_activation/pkg"
  rm -rf "neat-core/wasm_activation"
  echo "🛠️  Running wasm-pack build (target=web, out-name=wasm_activation)"
  wasm-pack build neat-core \
    --target web \
    --out-name wasm_activation \
    --out-dir wasm_activation/pkg
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
