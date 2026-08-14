#!/usr/bin/env bats
# Tests for the wasm64 (Memory64) build lane of scripts/build-wasm-bundle.sh
# (Issue #541).
#
# The wasm32 lane goes through wasm-pack; wasm64 cannot — wasm-pack 0.15.0
# hard-codes `wasm32-unknown-unknown` as its cargo target, so the wasm64 bundle
# is built with `cargo +nightly … -Z build-std` and the `wasm-bindgen` CLI
# directly. These tests drive the script against stub toolchain binaries on
# PATH, so they assert on what it *invokes* and what it *produces* without a
# 70-second nightly build.
#
# They also pin the fail-loud contract the July 2026 spike recorded: a CLI that
# exits 0 while stripping the bindings must not yield a published tarball.

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  SCRIPT_UNDER_TEST="$REPO_ROOT/scripts/build-wasm-bundle.sh"
  [ -x "$SCRIPT_UNDER_TEST" ] || chmod +x "$SCRIPT_UNDER_TEST"

  TMP_DIR="$(mktemp -d)"
  STUB_BIN="$TMP_DIR/bin"
  WORK_DIR="$TMP_DIR/work"
  mkdir -p "$STUB_BIN" "$WORK_DIR/neat-core"
  export REPO_ROOT SCRIPT_UNDER_TEST TMP_DIR STUB_BIN WORK_DIR
}

teardown() {
  rm -rf "$TMP_DIR"
}

# Write a stub that appends its argv to $TMP_DIR/<name>.log and exits 0.
stub() {
  local name="$1"
  shift
  {
    echo '#!/usr/bin/env bash'
    echo "printf '%s\n' \"\$*\" >> \"$TMP_DIR/${name}.log\""
    printf '%s\n' "$@"
    echo 'exit 0'
  } >"$STUB_BIN/$name"
  chmod +x "$STUB_BIN/$name"
}

# Materialise a plausible pkg/ at the --out-dir in argv, prefixed by $1 (empty
# for wasm-bindgen, which takes a path; "neat-core/" for wasm-pack, which
# resolves --out-dir relative to the crate directory).
emit_pkg_body() {
  cat <<BODY
out_dir=""
prev=""
for arg in "\$@"; do
  if [ "\$prev" = "--out-dir" ]; then out_dir="${1}\$arg"; fi
  prev="\$arg"
done
[ -n "\$out_dir" ] || { echo "stub: no --out-dir in argv" >&2; exit 3; }
mkdir -p "\$out_dir"
: >"\$out_dir/wasm_activation.js"
: >"\$out_dir/wasm_activation.d.ts"
: >"\$out_dir/wasm_activation_bg.wasm.d.ts"
dd if=/dev/zero of="\$out_dir/wasm_activation_bg.wasm" bs=1024 count=200 status=none
BODY
}

# A cargo stub that leaves the raw Tier 3 artefact where the real one lands.
emit_cargo_body() {
  cat <<'BODY'
mkdir -p target/wasm64-unknown-unknown/release
dd if=/dev/zero of=target/wasm64-unknown-unknown/release/neat_core.wasm \
  bs=1024 count=600 status=none
BODY
}

# Install a full stub toolchain: cargo, wasm-bindgen, wasm-pack and deno.
stub_toolchain() {
  stub cargo "$(emit_cargo_body)"
  stub wasm-bindgen "$(emit_pkg_body '')"
  stub wasm-pack "$(emit_pkg_body 'neat-core/')"
  stub deno
}

run_build() {
  cd "$WORK_DIR" || return 1
  PATH="$STUB_BIN:$PATH" run "$SCRIPT_UNDER_TEST" "$@"
}

@test "--help documents the --arch selector and both wasm targets" {
  run "$SCRIPT_UNDER_TEST" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--arch"* ]]
  [[ "$output" == *"wasm64"* ]]
  [[ "$output" == *"wasm32"* ]]
}

@test "rejects an --arch value that is neither wasm32 nor wasm64" {
  run "$SCRIPT_UNDER_TEST" --rev deadbeef --arch wasm128
  [ "$status" -ne 0 ]
  [[ "$output" == *"--arch"* ]]
  [[ "$output" == *"wasm128"* ]]
}

@test "--arch wasm64 builds through cargo nightly with -Z build-std" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/cargo.log" ]
  grep -q -- "--target wasm64-unknown-unknown" "$TMP_DIR/cargo.log"
  grep -q -- "-Z build-std" "$TMP_DIR/cargo.log"
  grep -q -- "+nightly" "$TMP_DIR/cargo.log"
}

@test "--arch wasm64 runs wasm-bindgen directly, not wasm-pack" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/wasm-bindgen.log" ]
  grep -q -- "--target web" "$TMP_DIR/wasm-bindgen.log"
  grep -q -- "--out-name wasm_activation" "$TMP_DIR/wasm-bindgen.log"
  [ ! -f "$TMP_DIR/wasm-pack.log" ]
}

@test "--arch wasm32 still builds through wasm-pack" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm32 --out "$TMP_DIR/out32.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/wasm-pack.log" ]
  grep -q -- "--target web" "$TMP_DIR/wasm-pack.log"
  [ ! -f "$TMP_DIR/cargo.log" ]
}

@test "a built bundle is gated before it is packaged" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/deno.log" ]
  grep -q "check_wasm64_bundle.ts" "$TMP_DIR/deno.log"
  grep -q -- "--arch wasm64" "$TMP_DIR/deno.log"
}

@test "the arch reaches the gate, so a wasm32 build cannot be gated as wasm64" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm32 --out "$TMP_DIR/out32.tar.gz"
  [ "$status" -eq 0 ]
  grep -q -- "--arch wasm32" "$TMP_DIR/deno.log"
}

@test "a failing gate blocks the tarball — no silent publish" {
  stub_toolchain
  # Re-stub deno to fail the way check_wasm64_bundle.ts does on a stripped glue.
  {
    echo '#!/usr/bin/env bash'
    echo 'echo "check_wasm64_bundle: generated glue is missing bindings" >&2'
    echo 'exit 1'
  } >"$STUB_BIN/deno"
  chmod +x "$STUB_BIN/deno"

  run_build --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -ne 0 ]
  [ ! -f "$TMP_DIR/out64.tar.gz" ]
}

@test "a missing deno fails loud rather than skipping the gate" {
  stub cargo "$(emit_cargo_body)"
  stub wasm-bindgen "$(emit_pkg_body '')"
  # No deno stub, and a PATH that cannot reach a real one: the gate genuinely
  # cannot run. Absence of a failure marker must not be reconciled as a pass.
  cd "$WORK_DIR" || return 1
  PATH="$STUB_BIN:/usr/bin:/bin" run "$SCRIPT_UNDER_TEST" \
    --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -ne 0 ]
  [[ "$output" == *"deno"* ]]
  [ ! -f "$TMP_DIR/out64.tar.gz" ]
}

@test "the wasm64 tarball still unpacks to a top-level pkg/" {
  stub_toolchain
  run_build --rev cafe1234 --arch wasm64 --out "$TMP_DIR/out64.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/out64.tar.gz" ]
  run tar -tzf "$TMP_DIR/out64.tar.gz"
  [ "$status" -eq 0 ]
  [[ "$output" == *"pkg/wasm_activation_bg.wasm"* ]]
  [[ "$output" == *"pkg/neat_core_rev.txt"* ]]
}

@test "--pkg-dir packages a pre-gated directory without invoking a toolchain" {
  stub_toolchain
  pkg="$TMP_DIR/prebuilt/pkg"
  mkdir -p "$pkg"
  : >"$pkg/wasm_activation.js"
  : >"$pkg/wasm_activation.d.ts"
  dd if=/dev/zero of="$pkg/wasm_activation_bg.wasm" bs=1024 count=200 status=none

  run_build --rev cafe1234 --arch wasm64 --pkg-dir "$pkg" --out "$TMP_DIR/pre.tar.gz"
  [ "$status" -eq 0 ]
  [ -f "$TMP_DIR/pre.tar.gz" ]
  [ ! -f "$TMP_DIR/cargo.log" ]
  [ ! -f "$TMP_DIR/wasm-bindgen.log" ]
}
