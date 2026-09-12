#!/usr/bin/env bats
# Tests for scripts/runlib.sh (Issue #680) — the canonical build → install →
# clean helper every NEAT-AI Rust sibling copies byte-for-byte.
#
# These are "what" tests: each one runs the real script against a fixture crate
# with a `cargo` shim on PATH and asserts on the observable outcome — the exit
# status, the single line on stdout, the stderr lines, the artefacts and stamps
# under CARGO_HOME, and whether `target/` survived. Nothing greps the script's
# source text.
#
# The shim records every `cargo` invocation, so "runs no cargo command" is an
# assertion over an empty log rather than an inference.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/runlib.sh"

  WORK="${BATS_TEST_TMPDIR}"
  REPO="${WORK}/repo"
  SHIM_DIR="${WORK}/shims"
  export CARGO_HOME="${WORK}/cargo"
  export RUNLIB_SHIM_LOG="${WORK}/cargo-invocations.log"
  export RUNLIB_SHIM_RUSTFLAGS="${WORK}/cargo-rustflags.log"
  export RUNLIB_SHIM_METADATA="${WORK}/metadata.json"
  export RUNLIB_SHIM_ARTEFACTS="${WORK}/artefacts.txt"
  OUT="${WORK}/stdout.txt"
  ERR="${WORK}/stderr.txt"
  TARGET_DIR="${REPO}/target"

  mkdir -p "$REPO" "$SHIM_DIR" "$CARGO_HOME"
  : > "$RUNLIB_SHIM_ARTEFACTS"
  write_shims
  export PATH="${SHIM_DIR}:${PATH}"
}

# --- fixture plumbing -------------------------------------------------------

# The shared-library extension this host's cargo would produce.
lib_ext() {
  if [ "$(uname -s)" = "Darwin" ]; then echo "dylib"; else echo "so"; fi
}

write_shims() {
  cat > "${SHIM_DIR}/cargo" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail
echo "$*" >> "$RUNLIB_SHIM_LOG"
case "${1:-}" in
  metadata)
    cat "$RUNLIB_SHIM_METADATA"
    ;;
  build)
    echo "${RUSTFLAGS-<unset>}" > "$RUNLIB_SHIM_RUSTFLAGS"
    if [ "${RUNLIB_SHIM_BUILD_FAILS:-0}" = "1" ]; then
      echo "shim: compilation failed" >&2
      exit 1
    fi
    while IFS= read -r artefact; do
      [ -n "$artefact" ] || continue
      mkdir -p "$(dirname "$artefact")"
      printf 'artefact %s\n' "$artefact" > "$artefact"
      chmod +x "$artefact"
    done < "$RUNLIB_SHIM_ARTEFACTS"
    ;;
  *)
    echo "shim: unexpected cargo subcommand: $*" >&2
    exit 2
    ;;
esac
SHIM
  cat > "${SHIM_DIR}/rustup" <<'SHIM'
#!/usr/bin/env bash
exit 0
SHIM
  cat > "${SHIM_DIR}/rustc" <<'SHIM'
#!/usr/bin/env bash
echo "rustc ${RUNLIB_SHIM_RUSTC_VERSION:-1.92.0} (0000000 2026-01-01)"
SHIM
  chmod +x "${SHIM_DIR}/cargo" "${SHIM_DIR}/rustup" "${SHIM_DIR}/rustc"
}

# write_manifest <crate> <version> <shape> [extra manifest lines...]
# Writes a manifest and a src tree cargo would actually produce that shape from,
# so the fast path's manifest read and the metadata reply agree.
write_manifest() {
  local crate="$1" version="$2" shape="$3"
  shift 3
  local underscored="${crate//-/_}"
  mkdir -p "${REPO}/src"
  {
    echo "[package]"
    echo "name = \"${crate}\""
    echo "version = \"${version}\""
    echo "edition = \"2024\""
    local line
    for line in "$@"; do echo "$line"; done
    if [ "$shape" = "cdylib" ] || [ "$shape" = "both" ]; then
      echo ""
      echo "[lib]"
      echo "name = \"${LIB_TARGET_NAME:-$underscored}\""
      echo 'crate-type = ["cdylib"]'
    fi
  } > "${REPO}/Cargo.toml"
  rm -f "${REPO}/src/main.rs" "${REPO}/src/lib.rs"
  case "$shape" in
    bin|both) echo 'fn main() {}' > "${REPO}/src/main.rs" ;;
  esac
  case "$shape" in
    cdylib|both) echo 'pub fn hello() {}' > "${REPO}/src/lib.rs" ;;
  esac
}

# write_metadata <crate> <version> <shape: bin|cdylib|both|none>
# Builds the `cargo metadata --no-deps` reply and the artefact list the shim
# creates, so the fixture's declared shape and its build output stay in step.
# BIN_TARGET_NAME / LIB_TARGET_NAME override the cargo target names, which is
# how a crate whose `[lib] name` differs from its package name is modelled.
write_metadata() {
  local crate="$1" version="$2" shape="$3"
  local underscored="${crate//-/_}"
  local bin_target="${BIN_TARGET_NAME:-$underscored}"
  local lib_target="${LIB_TARGET_NAME:-$underscored}"
  local targets="" ext
  ext="$(lib_ext)"
  : > "$RUNLIB_SHIM_ARTEFACTS"
  case "$shape" in
    bin)
      targets='{"kind":["bin"],"name":"'"${bin_target}"'"}'
      echo "${TARGET_DIR}/release/${bin_target}" >> "$RUNLIB_SHIM_ARTEFACTS"
      ;;
    cdylib)
      targets='{"kind":["cdylib"],"name":"'"${lib_target}"'"}'
      echo "${TARGET_DIR}/release/lib${lib_target}.${ext}" >> "$RUNLIB_SHIM_ARTEFACTS"
      ;;
    both)
      targets='{"kind":["bin"],"name":"'"${bin_target}"'"},{"kind":["cdylib"],"name":"'"${lib_target}"'"}'
      echo "${TARGET_DIR}/release/${bin_target}" >> "$RUNLIB_SHIM_ARTEFACTS"
      echo "${TARGET_DIR}/release/lib${lib_target}.${ext}" >> "$RUNLIB_SHIM_ARTEFACTS"
      ;;
    none)
      targets='{"kind":["lib"],"name":"'"${underscored}"'"}'
      ;;
    *)
      echo "unknown shape: $shape" >&2
      return 1
      ;;
  esac
  cat > "$RUNLIB_SHIM_METADATA" <<JSON
{
  "packages": [
    {
      "name": "${crate}",
      "version": "${version}",
      "manifest_path": "${REPO}/Cargo.toml",
      "targets": [${targets}]
    }
  ],
  "target_directory": "${TARGET_DIR}"
}
JSON
}

# A fixture crate of the given shape, manifest, src tree and metadata agreeing.
make_crate() {
  local crate="$1" version="$2" shape="$3"
  write_manifest "$crate" "$version" "$shape"
  write_metadata "$crate" "$version" "$shape"
  mkdir -p "${TARGET_DIR}"
}

invoke() {
  ( cd "$REPO" && "$SCRIPT" > "$OUT" 2> "$ERR" )
}

cargo_invocations() {
  if [ -f "$RUNLIB_SHIM_LOG" ]; then wc -l < "$RUNLIB_SHIM_LOG" | tr -d ' '; else echo 0; fi
}

# --- install shapes ---------------------------------------------------------

@test "a bin-only crate installs the binary under CARGO_HOME/bin and stamps it" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_app.version")" = "1.2.3" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_app" ]
}

@test "a cdylib-only crate installs the library under CARGO_HOME/lib and stdout is the lib path" {
  make_crate "demo_lib" "0.4.0" cdylib
  run invoke
  [ "$status" -eq 0 ]
  [ -f "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)" ]
  [ "$(cat "${CARGO_HOME}/lib/.demo_lib.version")" = "0.4.0" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)" ]
  [ ! -e "${CARGO_HOME}/bin/demo_lib" ]
}

@test "a crate with both targets installs both and stdout is the bin path" {
  make_crate "demo_both" "2.0.1" both
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_both" ]
  [ -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_both.version")" = "2.0.1" ]
  [ "$(cat "${CARGO_HOME}/lib/.demo_both.version")" = "2.0.1" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_both" ]
}

@test "a dashed crate installs under the underscored crate name, not the cargo target name" {
  # Real cargo keeps the dash in the default bin target name, so the fixture
  # does too; the installed name must still be the underscored crate name.
  BIN_TARGET_NAME="demo-dashed"
  make_crate "demo-dashed" "0.9.0" bin
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_dashed" ]
  [ ! -e "${CARGO_HOME}/bin/demo-dashed" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo-dashed.version")" = "0.9.0" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_dashed" ]
  # And the same name is what the skip path looks for on the next run.
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
}

@test "a lib target renamed away from the crate still installs under the crate name" {
  # `[lib] name = "custom_name"` makes cargo emit libcustom_name.so; installing
  # under that name would leave the skip path looking for a file that is not
  # there, and the crate would rebuild on every run for ever.
  LIB_TARGET_NAME="custom_name"
  make_crate "demo-lib" "1.0.0" cdylib
  run invoke
  [ "$status" -eq 0 ]
  [ -f "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)" ]
  [ ! -e "${CARGO_HOME}/lib/libcustom_name.$(lib_ext)" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)" ]
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ "$(cat "$ERR")" = "[demo-lib] already installed v1.0.0" ]
}

@test "stdout carries the installed path and nothing else" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  [ "$(wc -l < "$OUT" | tr -d ' ')" -eq 1 ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_app" ]
}

# --- the checkout's target/ -------------------------------------------------

@test "a successful install removes target/ and names the path and bytes freed" {
  make_crate "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target/debug"
  dd if=/dev/zero of="${REPO}/target/debug/ballast" bs=1024 count=64 2>/dev/null
  run invoke
  [ "$status" -eq 0 ]
  [ ! -d "${REPO}/target" ]
  run grep -F "removed ${REPO}/target (freed " "$ERR"
  [ "$status" -eq 0 ]
  run grep -E "freed [0-9]+ bytes" "$ERR"
  [ "$status" -eq 0 ]
}

@test "the bytes freed are a positive count measured before removal" {
  make_crate "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target/debug"
  dd if=/dev/zero of="${REPO}/target/debug/ballast" bs=1024 count=256 2>/dev/null
  run invoke
  [ "$status" -eq 0 ]
  freed="$(sed -n 's/.*freed \([0-9]*\) bytes.*/\1/p' "$ERR")"
  [ -n "$freed" ]
  [ "$freed" -ge 262144 ]
}

# --- the already-installed fast path ----------------------------------------

@test "a matching stamp skips the build without invoking cargo at all" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_app" ]
}

@test "the skip prints exactly one already-installed stderr line" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  run invoke
  [ "$status" -eq 0 ]
  [ "$(wc -l < "$ERR" | tr -d ' ')" -eq 1 ]
  [ "$(cat "$ERR")" = "[demo_app] already installed v1.2.3" ]
}

@test "a cdylib-only crate also skips on a matching stamp" {
  make_crate "demo_lib" "0.4.0" cdylib
  run invoke
  [ "$status" -eq 0 ]
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ "$(cat "$ERR")" = "[demo_lib] already installed v0.4.0" ]
  [ "$(cat "$OUT")" = "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)" ]
}

@test "the version in the manifest is what the skip compares, not the file mtime" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  touch "${CARGO_HOME}/bin/demo_app"
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
}

# --- rebuild triggers -------------------------------------------------------

@test "a version change rebuilds and refreshes the stamp" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  make_crate "demo_app" "1.3.0" bin
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -ge 1 ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_app.version")" = "1.3.0" ]
}

@test "deleting the stamp forces a rebuild — there is no force flag" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  rm -f "${CARGO_HOME}/bin/.demo_app.version"
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -ge 1 ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_app.version")" = "1.2.3" ]
}

@test "a missing artefact beside a matching stamp rebuilds" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  rm -f "${CARGO_HOME}/bin/demo_app"
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -ge 1 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

# --- failure keeps everything it found --------------------------------------

@test "a failing build exits non-zero, keeps target/ and leaves the old artefact and stamp" {
  make_crate "demo_app" "1.2.3" bin
  run invoke
  [ "$status" -eq 0 ]
  cp "${CARGO_HOME}/bin/demo_app" "${WORK}/installed-before"

  make_crate "demo_app" "1.3.0" bin
  mkdir -p "${REPO}/target/release"
  export RUNLIB_SHIM_BUILD_FAILS=1
  run invoke
  [ "$status" -ne 0 ]
  [ -d "${REPO}/target" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_app.version")" = "1.2.3" ]
  run cmp -s "${WORK}/installed-before" "${CARGO_HOME}/bin/demo_app"
  [ "$status" -eq 0 ]
}

@test "a build that produces no artefact fails loud and keeps target/" {
  make_crate "demo_app" "1.2.3" bin
  : > "$RUNLIB_SHIM_ARTEFACTS"
  run invoke
  [ "$status" -ne 0 ]
  [ -d "${REPO}/target" ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ ! -e "${CARGO_HOME}/bin/.demo_app.version" ]
  run grep -F "produced no binary" "$ERR"
  [ "$status" -eq 0 ]
}

# --- workspace member resolution --------------------------------------------

@test "a virtual workspace with one member resolves that member" {
  mkdir -p "${REPO}/member"
  cat > "${REPO}/Cargo.toml" <<'TOML'
[workspace]
members = ["member"]
resolver = "2"

[workspace.package]
version = "3.1.4"
TOML
  cat > "${REPO}/member/Cargo.toml" <<'TOML'
[package]
name = "demo_member"
version.workspace = true
edition = "2024"
TOML
  mkdir -p "${REPO}/member/src"
  echo 'fn main() {}' > "${REPO}/member/src/main.rs"
  write_metadata "demo_member" "3.1.4" bin
  sed -i.bak "s|${REPO}/Cargo.toml|${REPO}/member/Cargo.toml|" "$RUNLIB_SHIM_METADATA"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_member" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_member.version")" = "3.1.4" ]
  # The workspace-inherited version is what the fast path reads back.
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ "$(cat "$ERR")" = "[demo_member] already installed v3.1.4" ]
}

@test "more than one workspace member fails loud and names them" {
  write_manifest "demo_app" "1.2.3" bin
  cat > "$RUNLIB_SHIM_METADATA" <<JSON
{
  "packages": [
    {"name": "one", "version": "1.0.0", "manifest_path": "${REPO}/one/Cargo.toml", "targets": []},
    {"name": "two", "version": "1.0.0", "manifest_path": "${REPO}/two/Cargo.toml", "targets": []}
  ],
  "target_directory": "${REPO}/target"
}
JSON
  rm -f "${CARGO_HOME}/bin/.demo_app.version"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "expected exactly one workspace member" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "one, two" "$ERR"
  [ "$status" -eq 0 ]
}

@test "no workspace member at all fails loud" {
  write_manifest "demo_app" "1.2.3" bin
  cat > "$RUNLIB_SHIM_METADATA" <<JSON
{ "packages": [], "target_directory": "${REPO}/target" }
JSON
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "no workspace member" "$ERR"
  [ "$status" -eq 0 ]
}

@test "a crate with neither an eligible bin nor a cdylib fails loud" {
  make_crate "demo_rlib" "1.0.0" none
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "nothing to install" "$ERR"
  [ "$status" -eq 0 ]
}

@test "running outside a repository root fails loud" {
  run bash -c 'cd "$BATS_TEST_TMPDIR" && "$0" 2>&1' "$SCRIPT"
  [ "$status" -ne 0 ]
  [[ "$output" == *"run runlib.sh from the repository root"* ]]
}

# --- toolchain preconditions ------------------------------------------------

@test "a missing cargo exits non-zero naming rustup.rs and installs nothing" {
  if PATH="/usr/bin:/bin" command -v cargo >/dev/null 2>&1; then
    skip "a system cargo on the minimal PATH would defeat this fixture"
  fi
  make_crate "demo_app" "1.2.3" bin
  run env PATH="/usr/bin:/bin" CARGO_HOME="$CARGO_HOME" \
    bash -c 'cd "$0" && "$1" 2>&1' "$REPO" "$SCRIPT"
  [ "$status" -ne 0 ]
  [[ "$output" == *"https://rustup.rs"* ]]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

@test "a rustc below the manifest MSRV fails loud without installing" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  export RUNLIB_SHIM_RUSTC_VERSION="1.80.0"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "below the crate MSRV 1.92.0" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
}

@test "a rustc at or above the manifest MSRV builds" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

@test "a manifest without rust-version skips the MSRV check" {
  make_crate "demo_app" "1.2.3" bin
  export RUNLIB_SHIM_RUSTC_VERSION="1.10.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

# --- RUSTFLAGS is the caller's ----------------------------------------------

@test "the caller's RUSTFLAGS reach cargo unchanged" {
  make_crate "demo_app" "1.2.3" bin
  export RUSTFLAGS="-C target-cpu=skylake -D warnings"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cat "$RUNLIB_SHIM_RUSTFLAGS")" = "-C target-cpu=skylake -D warnings" ]
}

@test "no RUSTFLAGS from the caller means none are set by the script" {
  make_crate "demo_app" "1.2.3" bin
  unset RUSTFLAGS
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cat "$RUNLIB_SHIM_RUSTFLAGS")" = "<unset>" ]
}

# --- a partly-removed install is not "already installed" --------------------

@test "a both-crate whose library was removed rebuilds instead of reporting installed" {
  make_crate "demo_both" "2.0.1" both
  run invoke
  [ "$status" -eq 0 ]
  rm -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)" "${CARGO_HOME}/lib/.demo_both.version"
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -ge 1 ]
  [ -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)" ]
  [ "$(cat "${CARGO_HOME}/lib/.demo_both.version")" = "2.0.1" ]
}

@test "a both-crate whose binary was removed rebuilds instead of reporting installed" {
  make_crate "demo_both" "2.0.1" both
  run invoke
  [ "$status" -eq 0 ]
  rm -f "${CARGO_HOME}/bin/demo_both" "${CARGO_HOME}/bin/.demo_both.version"
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -ge 1 ]
  [ -x "${CARGO_HOME}/bin/demo_both" ]
}

# --- a shared build directory is not the checkout's -------------------------

@test "a target directory outside the checkout is kept, and that is reported" {
  TARGET_DIR="${WORK}/shared-cache"
  make_crate "demo_app" "1.2.3" bin
  mkdir -p "${TARGET_DIR}/other-crate"
  echo "not ours" > "${TARGET_DIR}/other-crate/keepme"
  run invoke
  [ "$status" -eq 0 ]
  [ -d "${TARGET_DIR}" ]
  [ -f "${TARGET_DIR}/other-crate/keepme" ]
  run grep -F "kept ${TARGET_DIR} (outside the checkout ${REPO})" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "removed" "$ERR"
  [ "$status" -ne 0 ]
}

# --- a failure after the build still leaves the old install alone -----------

@test "a missing library after a successful build does not replace the installed binary" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  cp "${CARGO_HOME}/bin/demo_both" "${WORK}/bin-before"

  # v2 builds the binary but never produces the cdylib.
  make_crate "demo_both" "2.0.0" both
  head -n 1 "$RUNLIB_SHIM_ARTEFACTS" > "${WORK}/only-bin"
  mv "${WORK}/only-bin" "$RUNLIB_SHIM_ARTEFACTS"
  run invoke
  [ "$status" -ne 0 ]
  [ -d "${TARGET_DIR}" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_both.version")" = "1.0.0" ]
  run cmp -s "${WORK}/bin-before" "${CARGO_HOME}/bin/demo_both"
  [ "$status" -eq 0 ]
}

@test "no staging temporary survives a failed install" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  make_crate "demo_both" "2.0.0" both
  head -n 1 "$RUNLIB_SHIM_ARTEFACTS" > "${WORK}/only-bin"
  mv "${WORK}/only-bin" "$RUNLIB_SHIM_ARTEFACTS"
  run invoke
  [ "$status" -ne 0 ]
  run bash -c 'ls "$1"/*.runlib.* 2>/dev/null' _ "${CARGO_HOME}/bin"
  [ "$status" -ne 0 ]
}

# --- shapes the manifest reader declines still avoid a rebuild --------------

@test "a globbed workspace member still skips the build once installed" {
  mkdir -p "${REPO}/crates/member/src"
  echo 'fn main() {}' > "${REPO}/crates/member/src/main.rs"
  cat > "${REPO}/Cargo.toml" <<'TOML'
[workspace]
members = ["crates/*"]
resolver = "2"
TOML
  cat > "${REPO}/crates/member/Cargo.toml" <<'TOML'
[package]
name = "demo_globbed"
version = "1.0.0"
edition = "2024"
TOML
  write_metadata "demo_globbed" "1.0.0" bin
  sed -i.bak "s|${REPO}/Cargo.toml|${REPO}/crates/member/Cargo.toml|" "$RUNLIB_SHIM_METADATA"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_globbed" ]

  # The manifest reader declines a glob, so cargo metadata runs again — but the
  # build must not, and the already-installed line is still printed.
  rm -f "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
  run grep -F "[demo_globbed] already installed v1.0.0" "$ERR"
  [ "$status" -eq 0 ]
}

# --- the macOS branch -------------------------------------------------------

@test "on macOS the library is signed and re-identified before it is installed" {
  cat > "${SHIM_DIR}/uname" <<'SHIM'
#!/usr/bin/env bash
if [ "${1:-}" = "-s" ]; then echo "Darwin"; else /usr/bin/uname "$@"; fi
SHIM
  for tool in install_name_tool codesign; do
    cat > "${SHIM_DIR}/${tool}" <<SHIM
#!/usr/bin/env bash
echo "${tool} \$*" >> "${WORK}/macos-tools.log"
exit 0
SHIM
    chmod +x "${SHIM_DIR}/${tool}"
  done
  chmod +x "${SHIM_DIR}/uname"

  make_crate "demo_lib" "0.4.0" cdylib
  run invoke
  [ "$status" -eq 0 ]
  [ -f "${CARGO_HOME}/lib/libdemo_lib.dylib" ]
  # Both tools ran, and both ran against the staging temporary rather than the
  # installed path, so a signing failure cannot damage a working install.
  run grep -F "install_name_tool -id @rpath/libdemo_lib.dylib" "${WORK}/macos-tools.log"
  [ "$status" -eq 0 ]
  run grep -F "codesign" "${WORK}/macos-tools.log"
  [ "$status" -eq 0 ]
  run grep -F ".runlib." "${WORK}/macos-tools.log"
  [ "$status" -eq 0 ]
}

@test "a macOS signing failure exits non-zero and leaves the old library installed" {
  cat > "${SHIM_DIR}/uname" <<'SHIM'
#!/usr/bin/env bash
if [ "${1:-}" = "-s" ]; then echo "Darwin"; else /usr/bin/uname "$@"; fi
SHIM
  printf '#!/usr/bin/env bash\nexit 0\n' > "${SHIM_DIR}/install_name_tool"
  printf '#!/usr/bin/env bash\nexit 0\n' > "${SHIM_DIR}/codesign"
  chmod +x "${SHIM_DIR}/uname" "${SHIM_DIR}/install_name_tool" "${SHIM_DIR}/codesign"

  make_crate "demo_lib" "0.4.0" cdylib
  run invoke
  [ "$status" -eq 0 ]
  cp "${CARGO_HOME}/lib/libdemo_lib.dylib" "${WORK}/lib-before"

  printf '#!/usr/bin/env bash\nexit 3\n' > "${SHIM_DIR}/codesign"
  chmod +x "${SHIM_DIR}/codesign"
  make_crate "demo_lib" "0.5.0" cdylib
  run invoke
  [ "$status" -ne 0 ]
  [ "$(cat "${CARGO_HOME}/lib/.demo_lib.version")" = "0.4.0" ]
  run cmp -s "${WORK}/lib-before" "${CARGO_HOME}/lib/libdemo_lib.dylib"
  [ "$status" -eq 0 ]
  [ -d "${TARGET_DIR}" ]
}

# --- the sourced entry point ------------------------------------------------

@test "sourcing the script and calling runlib_install installs the same artefact" {
  make_crate "demo_app" "1.2.3" bin
  run bash -c 'cd "$1" && . "$2" && runlib_install 2>/dev/null' _ "$REPO" "$SCRIPT"
  [ "$status" -eq 0 ]
  [ "$output" = "${CARGO_HOME}/bin/demo_app" ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

# --- odd version strings do not crash the MSRV comparison -------------------

@test "a non-numeric MSRV component is compared, not crashed on" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0+build5"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${TARGET_DIR}"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}
