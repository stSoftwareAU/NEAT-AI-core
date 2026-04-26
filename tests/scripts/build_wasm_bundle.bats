#!/usr/bin/env bats
# Tests for scripts/build-wasm-bundle.sh — wasm_activation packaging gate
# (Issue #37). Verifies argument parsing, the size threshold, the embedded
# rev marker, and that the produced archive unpacks to a `pkg/` subfolder.

setup() {
  SCRIPT_UNDER_TEST="${BATS_TEST_DIRNAME}/../../scripts/build-wasm-bundle.sh"
  [ -x "$SCRIPT_UNDER_TEST" ] || chmod +x "$SCRIPT_UNDER_TEST"

  TMP_DIR="$(mktemp -d)"
  PKG_DIR="$TMP_DIR/pkg"
  mkdir -p "$PKG_DIR"
  export TMP_DIR PKG_DIR
}

teardown() {
  rm -rf "$TMP_DIR"
}

# Build a fake pkg/ that satisfies the script's structural checks. The wasm
# blob is sized via the first arg (bytes).
populate_fake_pkg() {
  local wasm_bytes="${1:-200000}"
  : >"$PKG_DIR/wasm_activation.js"
  : >"$PKG_DIR/wasm_activation.d.ts"
  dd if=/dev/zero of="$PKG_DIR/wasm_activation_bg.wasm" \
    bs=1 count="$wasm_bytes" status=none
}

@test "shows usage with --help" {
  run "$SCRIPT_UNDER_TEST" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
  [[ "$output" == *"--rev"* ]]
  [[ "$output" == *"--min-size-bytes"* ]]
}

@test "rejects unknown options" {
  run "$SCRIPT_UNDER_TEST" --nonsense
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}

@test "rejects non-integer --min-size-bytes" {
  populate_fake_pkg
  run "$SCRIPT_UNDER_TEST" \
    --rev deadbeef --pkg-dir "$PKG_DIR" \
    --out "$TMP_DIR/out.tar.gz" \
    --min-size-bytes abc
  [ "$status" -ne 0 ]
  [[ "$output" == *"min-size-bytes"* ]]
}

@test "requires --rev when GITHUB_SHA is unset" {
  populate_fake_pkg
  run env -u GITHUB_SHA "$SCRIPT_UNDER_TEST" \
    --pkg-dir "$PKG_DIR" --out "$TMP_DIR/out.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"--rev"* ]]
}

@test "uses GITHUB_SHA env when --rev not provided" {
  populate_fake_pkg
  GITHUB_SHA=cafef00d run "$SCRIPT_UNDER_TEST" \
    --pkg-dir "$PKG_DIR" --out "$TMP_DIR/out.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$PKG_DIR/neat_core_rev.txt" ]
  [ "$(cat "$PKG_DIR/neat_core_rev.txt")" = "cafef00d" ]
}

@test "fails when wasm size is below threshold" {
  # 50 KB blob, threshold 100 KB — must fail.
  populate_fake_pkg 51200
  run "$SCRIPT_UNDER_TEST" \
    --rev deadbeef --pkg-dir "$PKG_DIR" \
    --out "$TMP_DIR/out.tar.gz" \
    --min-size-bytes 102400
  [ "$status" -ne 0 ]
  [[ "$output" == *"below the minimum size threshold"* ]]
  [ ! -f "$TMP_DIR/out.tar.gz" ]
}

@test "fails when pkg directory does not exist" {
  run "$SCRIPT_UNDER_TEST" \
    --rev deadbeef --pkg-dir "$TMP_DIR/missing-pkg" \
    --out "$TMP_DIR/out.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"not found"* ]]
}

@test "fails when wasm-pack output files are missing" {
  # Only a directory exists; required files absent.
  run "$SCRIPT_UNDER_TEST" \
    --rev deadbeef --pkg-dir "$PKG_DIR" \
    --out "$TMP_DIR/out.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"missing"* ]]
}

@test "produces a tarball that unpacks to pkg/ subfolder with required files" {
  populate_fake_pkg 200000
  run "$SCRIPT_UNDER_TEST" \
    --rev "abc1234" --pkg-dir "$PKG_DIR" \
    --out "$TMP_DIR/out.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/out.tar.gz" ]

  EXTRACT_DIR="$TMP_DIR/extracted"
  mkdir -p "$EXTRACT_DIR"
  tar -xzf "$TMP_DIR/out.tar.gz" -C "$EXTRACT_DIR"

  [ -d "$EXTRACT_DIR/pkg" ]
  [ -f "$EXTRACT_DIR/pkg/wasm_activation_bg.wasm" ]
  [ -f "$EXTRACT_DIR/pkg/wasm_activation.d.ts" ]
  [ -f "$EXTRACT_DIR/pkg/wasm_activation.js" ]
  [ -f "$EXTRACT_DIR/pkg/neat_core_rev.txt" ]
  [ "$(cat "$EXTRACT_DIR/pkg/neat_core_rev.txt")" = "abc1234" ]
}

@test "passes with exact threshold match" {
  populate_fake_pkg 102400
  run "$SCRIPT_UNDER_TEST" \
    --rev deadbeef --pkg-dir "$PKG_DIR" \
    --out "$TMP_DIR/out.tar.gz" \
    --min-size-bytes 102400
  [ "$status" -eq 0 ]
}

@test "--skip-build without --pkg-dir is rejected" {
  run "$SCRIPT_UNDER_TEST" --rev deadbeef --skip-build
  [ "$status" -ne 0 ]
  [[ "$output" == *"--pkg-dir"* ]]
}
