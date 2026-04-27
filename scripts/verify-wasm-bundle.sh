#!/usr/bin/env bash
# verify-wasm-bundle.sh — Post-publish integrity check for the per-commit
# wasm_activation bundle (Issue #48).
#
# Used by .github/workflows/wasm-bundle.yml after `gh release create` to
# re-download the just-published asset and prove the upload actually worked
# end-to-end. A silent upload regression (gh exits 0 but the asset is
# missing/truncated) would otherwise only surface when a downstream NEAT-AI
# bump-deps run fails on `build.sh` — far from the source of the breakage.
#
# Behaviour:
#   1. Extract the supplied tarball into a scratch directory.
#   2. Assert the archive unpacks to a top-level `pkg/` (matches
#      build-wasm-bundle.sh's layout and NEAT-AI's import paths).
#   3. Assert all four files NEAT-AI's `build.sh` validates are present:
#        - wasm_activation.js
#        - wasm_activation_bg.wasm
#        - wasm_activation.d.ts
#        - wasm_activation_bg.wasm.d.ts
#   4. Assert wasm_activation_bg.wasm is at least --min-size-bytes (default
#      131072 / 128 KiB — the same MIN_WASM_BYTES stub-detection threshold
#      used by NEAT-AI's build.sh).
#   5. If --rev <SHA> is supplied AND neat_core_rev.txt is present, assert
#      they match (ensures the asset attached to the release tag corresponds
#      to the tag's commit SHA).
#
# Exits non-zero on any failure so the workflow surfaces a broken release
# immediately instead of leaving downstream consumers to discover it.

set -euo pipefail

# 128 KiB — mirrors NEAT-AI build.sh MIN_WASM_BYTES.
DEFAULT_MIN_WASM_BYTES=131072

ARCHIVE=""
EXPECTED_REV=""
MIN_WASM_BYTES="$DEFAULT_MIN_WASM_BYTES"

usage() {
  cat <<EOF
Usage: verify-wasm-bundle.sh --archive <path> [options]

Verifies the integrity of a published wasm_activation bundle tarball.

Options:
  --archive <path>       Path to the wasm_activation-pkg.tar.gz to verify.
                         Required.
  --rev <SHA>            Expected commit SHA. When supplied, the embedded
                         neat_core_rev.txt (if present) must match.
  --min-size-bytes <N>   Minimum acceptable wasm_activation_bg.wasm size,
                         in bytes (default: ${DEFAULT_MIN_WASM_BYTES}).
  -h, --help             Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive)
      [[ $# -ge 2 ]] || { echo "error: --archive requires a value" >&2; exit 2; }
      ARCHIVE="$2"
      shift 2
      ;;
    --rev)
      [[ $# -ge 2 ]] || { echo "error: --rev requires a value" >&2; exit 2; }
      EXPECTED_REV="$2"
      shift 2
      ;;
    --min-size-bytes)
      [[ $# -ge 2 ]] || { echo "error: --min-size-bytes requires a value" >&2; exit 2; }
      MIN_WASM_BYTES="$2"
      shift 2
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

if [[ -z "$ARCHIVE" ]]; then
  echo "error: --archive is required" >&2
  exit 2
fi

if [[ ! -f "$ARCHIVE" ]]; then
  echo "error: archive not found: $ARCHIVE" >&2
  exit 1
fi

if ! [[ "$MIN_WASM_BYTES" =~ ^[0-9]+$ ]]; then
  echo "error: --min-size-bytes must be a non-negative integer (got '$MIN_WASM_BYTES')" >&2
  exit 2
fi

EXTRACT_DIR="$(mktemp -d)"
trap 'rm -rf "$EXTRACT_DIR"' EXIT

if ! tar -xzf "$ARCHIVE" -C "$EXTRACT_DIR"; then
  echo "error: failed to extract archive: $ARCHIVE" >&2
  exit 1
fi

PKG_DIR="$EXTRACT_DIR/pkg"
if [[ ! -d "$PKG_DIR" ]]; then
  echo "error: archive does not contain a top-level pkg/ directory" >&2
  exit 1
fi

# Same file list NEAT-AI's build.sh validates — keep these two in lock-step.
REQUIRED_FILES=(
  "wasm_activation.js"
  "wasm_activation_bg.wasm"
  "wasm_activation.d.ts"
  "wasm_activation_bg.wasm.d.ts"
)

for required in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "$PKG_DIR/$required" ]]; then
    echo "error: required file missing from bundle: pkg/$required" >&2
    exit 1
  fi
done

WASM_FILE="$PKG_DIR/wasm_activation_bg.wasm"
WASM_SIZE=$(wc -c <"$WASM_FILE" | tr -d ' ')
echo "wasm_activation_bg.wasm size: ${WASM_SIZE} bytes (threshold ${MIN_WASM_BYTES})"
if (( WASM_SIZE < MIN_WASM_BYTES )); then
  echo "error: wasm_activation_bg.wasm is below the minimum size threshold (${WASM_SIZE} < ${MIN_WASM_BYTES} bytes)" >&2
  exit 1
fi

REV_FILE="$PKG_DIR/neat_core_rev.txt"
if [[ -n "$EXPECTED_REV" && -f "$REV_FILE" ]]; then
  EMBEDDED_REV="$(tr -d '[:space:]' <"$REV_FILE")"
  if [[ "$EMBEDDED_REV" != "$EXPECTED_REV" ]]; then
    echo "error: neat_core_rev.txt rev '${EMBEDDED_REV}' does not match expected '${EXPECTED_REV}'" >&2
    exit 1
  fi
  echo "neat_core_rev.txt matches expected rev: ${EXPECTED_REV}"
fi

echo "✅ Bundle verified OK: $ARCHIVE"
