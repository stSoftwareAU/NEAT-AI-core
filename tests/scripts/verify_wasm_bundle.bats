#!/usr/bin/env bats
# Tests for scripts/verify-wasm-bundle.sh — post-publish verification gate
# (Issue #48). The verifier downloads (or, in tests, is handed) the published
# wasm_activation-pkg.tar.gz, asserts the archive extracts cleanly, contains
# the four files NEAT-AI's build.sh checks for, and that
# wasm_activation_bg.wasm clears the 128 KiB stub-detection threshold.

setup() {
  SCRIPT_UNDER_TEST="${BATS_TEST_DIRNAME}/../../scripts/verify-wasm-bundle.sh"
  [ -x "$SCRIPT_UNDER_TEST" ] || chmod +x "$SCRIPT_UNDER_TEST"

  TMP_DIR="$(mktemp -d)"
  PKG_DIR="$TMP_DIR/pkg"
  mkdir -p "$PKG_DIR"
  export TMP_DIR PKG_DIR
}

teardown() {
  rm -rf "$TMP_DIR"
}

# Build a fake pkg/ tree that the verifier should accept. Optional first arg
# is the wasm blob size in bytes (default 200000 — comfortably above the
# 128 KiB threshold). Optional second arg is the rev string written into
# neat_core_rev.txt (default "deadbeef"). Tarballs the result to
# $TMP_DIR/bundle.tar.gz.
make_good_bundle() {
  local wasm_bytes="${1:-200000}"
  local rev="${2:-deadbeef}"
  : >"$PKG_DIR/wasm_activation.js"
  : >"$PKG_DIR/wasm_activation.d.ts"
  : >"$PKG_DIR/wasm_activation_bg.wasm.d.ts"
  dd if=/dev/zero of="$PKG_DIR/wasm_activation_bg.wasm" \
    bs=1 count="$wasm_bytes" status=none
  printf '%s\n' "$rev" >"$PKG_DIR/neat_core_rev.txt"
  tar -czf "$TMP_DIR/bundle.tar.gz" -C "$TMP_DIR" pkg
}

@test "shows usage with --help" {
  run "$SCRIPT_UNDER_TEST" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
  [[ "$output" == *"--archive"* ]]
}

@test "rejects unknown options" {
  run "$SCRIPT_UNDER_TEST" --nonsense
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}

@test "fails when --archive is missing" {
  run "$SCRIPT_UNDER_TEST"
  [ "$status" -ne 0 ]
  [[ "$output" == *"--archive"* ]]
}

@test "fails when archive file does not exist" {
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/missing.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"not found"* ]]
}

@test "accepts a well-formed bundle" {
  make_good_bundle
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -eq 0 ]
  [[ "$output" == *"verified"* ]] || [[ "$output" == *"OK"* ]]
}

@test "fails when wasm_activation.js is missing" {
  make_good_bundle
  # Rebuild without the .js file.
  rm "$PKG_DIR/wasm_activation.js"
  tar -czf "$TMP_DIR/bundle.tar.gz" -C "$TMP_DIR" pkg
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"wasm_activation.js"* ]]
}

@test "fails when wasm_activation_bg.wasm.d.ts is missing" {
  make_good_bundle
  rm "$PKG_DIR/wasm_activation_bg.wasm.d.ts"
  tar -czf "$TMP_DIR/bundle.tar.gz" -C "$TMP_DIR" pkg
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"wasm_activation_bg.wasm.d.ts"* ]]
}

@test "fails when wasm blob is below 128 KiB threshold" {
  # 100 KB — below the 128 KiB (131072 bytes) stub-detection threshold.
  make_good_bundle 102400
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"below"* ]] || [[ "$output" == *"size"* ]]
}

@test "passes at exactly the 128 KiB threshold" {
  make_good_bundle 131072
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -eq 0 ]
}

@test "honours --min-size-bytes override" {
  make_good_bundle 50000
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz" --min-size-bytes 40000
  [ "$status" -eq 0 ]
}

@test "rejects non-integer --min-size-bytes" {
  make_good_bundle
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz" --min-size-bytes abc
  [ "$status" -ne 0 ]
  [[ "$output" == *"min-size-bytes"* ]]
}

@test "fails when --rev does not match neat_core_rev.txt" {
  make_good_bundle 200000 "abc1234"
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz" --rev "deadbeef"
  [ "$status" -ne 0 ]
  [[ "$output" == *"rev"* ]]
}

@test "passes when --rev matches neat_core_rev.txt" {
  make_good_bundle 200000 "abc1234"
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz" --rev "abc1234"
  [ "$status" -eq 0 ]
}

@test "passes when neat_core_rev.txt is absent and --rev not supplied" {
  make_good_bundle
  rm "$PKG_DIR/neat_core_rev.txt"
  tar -czf "$TMP_DIR/bundle.tar.gz" -C "$TMP_DIR" pkg
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -eq 0 ]
}

@test "fails on a malformed (non-gzip) archive" {
  printf 'not a tar archive' >"$TMP_DIR/bundle.tar.gz"
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -ne 0 ]
}

@test "fails when archive lacks a top-level pkg/ directory" {
  : >"$PKG_DIR/wasm_activation.js"
  : >"$PKG_DIR/wasm_activation.d.ts"
  : >"$PKG_DIR/wasm_activation_bg.wasm.d.ts"
  dd if=/dev/zero of="$PKG_DIR/wasm_activation_bg.wasm" bs=1 count=200000 status=none
  # Pack the *contents* of pkg/ at the archive root rather than the pkg/ dir.
  tar -czf "$TMP_DIR/bundle.tar.gz" -C "$PKG_DIR" .
  run "$SCRIPT_UNDER_TEST" --archive "$TMP_DIR/bundle.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"pkg"* ]]
}
