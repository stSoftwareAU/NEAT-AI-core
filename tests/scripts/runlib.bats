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
  export RUNLIB_SHIM_GRAPH_METADATA="${WORK}/graph-metadata.json"
  export RUNLIB_SHIM_ARTEFACTS="${WORK}/artefacts.txt"
  export RUNLIB_SHIM_RUSTUP_LOG="${WORK}/rustup-invocations.log"
  export RUNLIB_SHIM_RUSTC_LOG="${WORK}/rustc-invocations.log"
  export RUNLIB_SHIM_RUSTUP_STATE="${WORK}/rustup-state"
  export RUNLIB_SHIM_CARGO_TOOLCHAIN="${WORK}/cargo-toolchain.log"
  OUT="${WORK}/stdout.txt"
  ERR="${WORK}/stderr.txt"
  TARGET_DIR="${REPO}/target"

  mkdir -p "$REPO" "$SHIM_DIR" "$CARGO_HOME" "$RUNLIB_SHIM_RUSTUP_STATE"
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
    # `--no-deps` is the single-member call; everything else is the resolved
    # dependency graph the toolchain gate reads, served from its own fixture
    # when a test wrote one.
    case " $* " in
      *" --no-deps "*) cat "$RUNLIB_SHIM_METADATA" ;;
      *)
        if [ -f "$RUNLIB_SHIM_GRAPH_METADATA" ]; then
          cat "$RUNLIB_SHIM_GRAPH_METADATA"
        else
          cat "$RUNLIB_SHIM_METADATA"
        fi
        ;;
    esac
    ;;
  build)
    echo "${RUSTFLAGS-<unset>}" > "$RUNLIB_SHIM_RUSTFLAGS"
    echo "${RUSTUP_TOOLCHAIN-<unset>}" > "$RUNLIB_SHIM_CARGO_TOOLCHAIN"
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
  # Every rustup invocation is logged, so "no rustup call on a pass" is an
  # assertion over an empty log. Installing a toolchain leaves a marker the
  # rustc shim reads, which is how "the pinned toolchain was not installed
  # until rustup installed it" is modelled without a network.
  cat > "${SHIM_DIR}/rustup" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail
echo "$*" >> "$RUNLIB_SHIM_RUSTUP_LOG"
if [ "${RUNLIB_SHIM_RUSTUP_FAILS:-0}" = "1" ]; then
  echo "shim: rustup failed" >&2
  exit 1
fi
case "${1:-}" in
  update)
    : > "${RUNLIB_SHIM_RUSTUP_STATE}/updated"
    : > "${RUNLIB_SHIM_RUSTUP_STATE}/installed-${2:-}"
    ;;
  default)
    : > "${RUNLIB_SHIM_RUSTUP_STATE}/installed-${2:-}"
    ;;
  toolchain)
    if [ "${2:-}" = "install" ]; then
      : > "${RUNLIB_SHIM_RUSTUP_STATE}/installed-${3:-}"
    fi
    ;;
esac
exit 0
SHIM
  # RUNLIB_SHIM_RUSTC_NEEDS names a toolchain this rustc cannot run without —
  # a rustup proxy whose toolchain is not installed. RUSTUP_TOOLCHAIN is
  # honoured as rustup's proxies honour it, and RUNLIB_SHIM_RUSTC_AFTER_UPDATE
  # is what the toolchain reports once `rustup update` has run.
  cat > "${SHIM_DIR}/rustc" <<'SHIM'
#!/usr/bin/env bash
echo "$*" >> "$RUNLIB_SHIM_RUSTC_LOG"
state="${RUNLIB_SHIM_RUSTUP_STATE:-}"
needs="${RUNLIB_SHIM_RUSTC_NEEDS:-}"
if [ -n "$needs" ] && [ ! -e "${state}/installed-${needs}" ]; then
  echo "error: toolchain '${needs}' is not installed" >&2
  exit 1
fi
version="${RUNLIB_SHIM_RUSTC_VERSION:-1.92.0}"
if [ -n "${RUNLIB_SHIM_RUSTC_AFTER_UPDATE:-}" ] && [ -e "${state}/updated" ]; then
  version="$RUNLIB_SHIM_RUSTC_AFTER_UPDATE"
fi
if [ -n "${RUSTUP_TOOLCHAIN:-}" ]; then
  version="$RUSTUP_TOOLCHAIN"
fi
echo "rustc ${version} (0000000 2026-01-01)"
if [ "${1:-}" = "-vV" ]; then
  echo "host: x86_64-unknown-linux-gnu"
fi
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

# A PATH carrying the coreutils the script needs and no Rust toolchain at all.
#
# The toolchain fixtures used to fall back to "/usr/bin:/bin" and skip
# themselves when a system cargo was found there — which is most CI images, so
# the assertion quietly never ran. Symlinking a known set of utilities into a
# scratch directory makes the absence of cargo/rustc a property of the fixture
# rather than of the host, so these tests always execute.
minimal_path() {
  local dir="${WORK}/minimal-bin" tool resolved
  if [ ! -d "$dir" ]; then
    mkdir -p "$dir"
    for tool in bash env awk grep sed cat cp mv rm mkdir chmod dirname uname du ls; do
      resolved="$(command -v "$tool" 2>/dev/null || true)"
      [ -n "$resolved" ] || continue
      ln -sf "$resolved" "${dir}/${tool}"
    done
  fi
  printf '%s' "$dir"
}

cargo_invocations() {
  if [ -f "$RUNLIB_SHIM_LOG" ]; then wc -l < "$RUNLIB_SHIM_LOG" | tr -d ' '; else echo 0; fi
}

rustup_invocations() {
  if [ -f "$RUNLIB_SHIM_RUSTUP_LOG" ]; then wc -l < "$RUNLIB_SHIM_RUSTUP_LOG" | tr -d ' '; else echo 0; fi
}

rustc_invocations() {
  if [ -f "$RUNLIB_SHIM_RUSTC_LOG" ]; then wc -l < "$RUNLIB_SHIM_RUSTC_LOG" | tr -d ' '; else echo 0; fi
}

# The `cargo metadata` reply for the *resolved* graph: one dependency package
# per argument, carrying that `rust_version`. `null` writes a package that
# declares none, which is what most of crates.io looks like.
write_graph_metadata() {
  local packages="" version index=0
  for version in "$@"; do
    index=$((index + 1))
    [ -z "$packages" ] || packages="${packages},"
    if [ "$version" = "null" ]; then
      packages="${packages}{\"name\":\"dep${index}\",\"version\":\"1.0.0\",\"rust_version\":null}"
    else
      packages="${packages}{\"name\":\"dep${index}\",\"version\":\"1.0.0\",\"rust_version\":\"${version}\"}"
    fi
  done
  cat > "$RUNLIB_SHIM_GRAPH_METADATA" <<JSON
{
  "packages": [${packages}],
  "target_directory": "${TARGET_DIR}"
}
JSON
}

# `[toolchain] channel` at the repository root — a pinned sibling.
write_toolchain_pin() {
  cat > "${REPO}/rust-toolchain.toml" <<TOML
[toolchain]
channel = "$1"
TOML
}

# A PATH carrying the shims, jq and the host utilities but **no** `rustup` at
# all. Removing the shim alone is not enough: most CI images carry a real
# rustup, so a test asserting "the script never reached for rustup" would
# quietly exercise the host's one — and `rustup update stable` for real.
shim_path_without_rustup() {
  local dir="${WORK}/no-rustup" resolved
  if [ ! -d "$dir" ]; then
    mkdir -p "$dir"
    cp "${SHIM_DIR}/cargo" "${SHIM_DIR}/rustc" "$dir/"
    chmod +x "${dir}/cargo" "${dir}/rustc"
    resolved="$(command -v jq 2>/dev/null || true)"
    [ -z "$resolved" ] || ln -sf "$resolved" "${dir}/jq"
  fi
  printf '%s:%s' "$dir" "$(minimal_path)"
}

invoke_with_path() {
  local path="$1"
  shift
  ( cd "$REPO" && PATH="$path" "$SCRIPT" "$@" > "$OUT" 2> "$ERR" )
}

# The script with arguments — `--toolchain-only`, or a bad one — on the shim
# PATH. stdout and stderr stay in separate files, because the toolchain-only
# stdout contract is asserted byte-exactly.
invoke_args() {
  ( cd "$REPO" && "$SCRIPT" "$@" > "$OUT" 2> "$ERR" )
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

# Issue #699 changed what "no toolchain" means: a missing `rustc` is now
# bootstrapped rather than refused, so this pair was re-pointed at the
# preconditions each one names. `rustc` is supplied here, which is what keeps
# the run out of the bootstrap and on the `cargo` check the title is about.
@test "a missing cargo exits non-zero naming rustup.rs and installs nothing" {
  make_crate "demo_app" "1.2.3" bin
  local only_rustc="${WORK}/only-rustc"
  mkdir -p "$only_rustc"
  cp "${SHIM_DIR}/rustc" "${only_rustc}/rustc"
  run env PATH="${only_rustc}:$(minimal_path)" CARGO_HOME="$CARGO_HOME" \
    bash -c 'cd "$0" && "$1" 2>&1' "$REPO" "$SCRIPT"
  [ "$status" -ne 0 ]
  [[ "$output" == *"cargo not found"* ]]
  [[ "$output" == *"https://rustup.rs"* ]]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

# Issue #700 made the remedy part of the gate: below the requirement with a
# rustup to hand the script updates rather than refusing, so the refusal this
# names is the one left — no rustup on PATH at all.
@test "a rustc below the crate's own rust-version with no rustup fails loud" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  export RUNLIB_SHIM_RUSTC_VERSION="1.80.0"
  run invoke_with_path "$(shim_path_without_rustup)"
  [ "$status" -ne 0 ]
  run grep -F "1.92.0" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
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

# --- the dependency-graph toolchain gate (Issue #700) -----------------------
#
# The requirement is the highest `rust-version` across the *resolved* graph,
# not the crate's own: the trigger was `serial_test@4.0.1 requires rustc
# 1.93.1` stopping a Discovery build where no family crate declares
# `rust-version` at all. The rustup shim logs every invocation, so "no rustup
# call" and "exactly this rustup call" are assertions over that log.

@test "a rustc below the graph maximum with no rustup on PATH fails loud" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.80.0" "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke_with_path "$(shim_path_without_rustup)"
  [ "$status" -ne 0 ]
  run grep -F "1.93.1" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "a rustc equal to the graph maximum builds" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.80.0" "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "a rustc above the graph maximum builds without invoking rustup" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.98.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "the crate's own rust-version above every dependency is the requirement" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.95.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  write_graph_metadata "1.93.1" "1.80.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.94.0"
  run invoke_with_path "$(shim_path_without_rustup)"
  [ "$status" -ne 0 ]
  run grep -F "1.95.0" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

# Lexically "1.9" sorts above "1.10"; numerically it does not, and `1.10` is
# the same requirement as `1.10.0`.
@test "a two-part rust_version is compared numerically, not as text" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.9" "1.10"
  export RUNLIB_SHIM_RUSTC_VERSION="1.9.9"
  run invoke_with_path "$(shim_path_without_rustup)"
  [ "$status" -ne 0 ]
  run grep -F "1.10" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

@test "a matching stamp runs no cargo, no rustc and no rustup at all" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.99.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.99.0"
  run invoke
  [ "$status" -eq 0 ]
  : > "$RUNLIB_SHIM_LOG"
  : > "$RUNLIB_SHIM_RUSTC_LOG"
  : > "$RUNLIB_SHIM_RUSTUP_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ "$(rustc_invocations)" -eq 0 ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "a graph declaring no rust_version at all builds on any rustc" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata null null
  export RUNLIB_SHIM_RUSTC_VERSION="1.10.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "an unpinned crate below the requirement is updated and then builds" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  export RUNLIB_SHIM_RUSTC_AFTER_UPDATE="1.93.1"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  run grep -Fx "update stable" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
}

@test "an unpinned crate still below after the update fails loud" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "1.93.1" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
}

@test "a pin above the requirement passes with no rustup call" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.98.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "a pin below the requirement is overridden for this run, not rewritten" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  local before
  before="$(cat "${REPO}/rust-toolchain.toml")"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  run grep -Fx "toolchain install 1.93.1" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
  [ "$(cat "$RUNLIB_SHIM_CARGO_TOOLCHAIN")" = "1.93.1" ]
  # Install mode still prints only the artefact path on stdout.
  [ "$(cat "$OUT")" = "${CARGO_HOME}/bin/demo_app" ]
  [ "$(cat "${REPO}/rust-toolchain.toml")" = "$before" ]
  # Exactly one line names the pin, the requirement and the file to bump.
  [ "$(grep -c -e "1.92.0.*1.93.1.*rust-toolchain.toml" "$ERR")" -eq 1 ]
}

# `stable` is not "below" anything — comparing a channel with a version number
# reads it as 0.0.0. The remedy for a channel pin is to move that channel, not
# to swap the repository's deliberate pin for an exact version.
@test "a channel pin below the requirement is updated, not replaced by a version" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "stable"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  export RUNLIB_SHIM_RUSTC_AFTER_UPDATE="1.93.1"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  run grep -Fx "update stable" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
  run grep -F "toolchain install" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -ne 0 ]
  [ "$(cat "$RUNLIB_SHIM_CARGO_TOOLCHAIN")" = "<unset>" ]
  [ "$(cat "${REPO}/rust-toolchain.toml")" = "$(printf '[toolchain]\nchannel = "stable"')" ]
}

# `1.93` is rustup's two-part channel, resolving to the newest 1.93.x — a
# moving pin, so it is moved rather than overridden.
@test "a two-part version pin is treated as a channel, not as an exact pin" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.93"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.0"
  export RUNLIB_SHIM_RUSTC_AFTER_UPDATE="1.93.1"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  run grep -Fx "update 1.93" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
  run grep -F "toolchain install" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -ne 0 ]
}

# The override belongs to this run's `cargo build` and nowhere else: a sourced
# caller must not come away with its shell pinned to a toolchain it never
# asked for. `_RUNLIB_TOOLCHAIN_OVERRIDE` is the record of what was selected.
@test "the override is recorded but never exported into the caller's shell" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run bash -c '
    set -euo pipefail
    cd "$1"
    . "$2"
    runlib_install > /dev/null 2>&1
    printf "override=%s toolchain=%s\n" \
      "$_RUNLIB_TOOLCHAIN_OVERRIDE" "${RUSTUP_TOOLCHAIN-<unset>}"
  ' _ "$REPO" "$SCRIPT"
  [ "$status" -eq 0 ]
  [ "$output" = "override=1.93.1 toolchain=<unset>" ]
  [ "$(cat "$RUNLIB_SHIM_CARGO_TOOLCHAIN")" = "1.93.1" ]
}

@test "a build needing no override records an empty _RUNLIB_TOOLCHAIN_OVERRIDE" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run bash -c '
    set -euo pipefail
    cd "$1"
    . "$2"
    runlib_install > /dev/null 2>&1
    printf "override=[%s]\n" "$_RUNLIB_TOOLCHAIN_OVERRIDE"
  ' _ "$REPO" "$SCRIPT"
  [ "$status" -eq 0 ]
  [ "$output" = "override=[]" ]
}

@test "a pinned toolchain that is not installed is installed before the build" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.98.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.98.0"
  export RUNLIB_SHIM_RUSTC_NEEDS="1.98.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  run grep -Fx "toolchain install 1.98.0" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
}

@test "a rustup update that fails exits non-zero naming the required version" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  export RUNLIB_SHIM_RUSTUP_FAILS="1"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "1.93.1" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
}

@test "a pinned toolchain install that fails exits non-zero naming the required version" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  export RUNLIB_SHIM_RUSTUP_FAILS="1"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "1.93.1" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
}

# `rust-toolchain.toml` is repository input, so what it names reaches rustup
# only as a plain toolchain name.
@test "a channel outside the toolchain-name allowlist is refused before rustup runs" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin 'stable; touch /tmp/runlib-pwned'
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "not a toolchain name" "$ERR"
  [ "$status" -eq 0 ]
  [ "$(rustup_invocations)" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
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

# --- a manifest shape the fast reader cannot decode must never report installed

# `_runlib_expected_shape` reads one line per key, so a `crate-type` array split
# over several lines is unreadable. Under-claiming the cdylib on the skip path
# is not "conservative": it makes a half-installed both-crate report as
# complete. The fast path must decline the shape and let `cargo metadata` rule.
@test "a multi-line crate-type array with the library missing rebuilds instead of reporting installed" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]

  # Re-write the manifest with the array split across lines — an ordinary
  # rustfmt-style manifest the single-line reader cannot see the whole of.
  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[lib]
name = "demo_both"
crate-type = [
  "cdylib",
]
TOML
  rm -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)"
  : > "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  run grep -F "already installed" "$ERR"
  [ "$status" -ne 0 ]
  [ -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)" ]
}

# An explicit `[[bin]]` table can rename or suppress the binary, so the fast
# reader cannot know the shape. Declining is what keeps stdout stable: the
# build run and the next skip run must name the same artefact.
@test "an explicit [[bin]] table with the binary missing rebuilds instead of reporting installed" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  local first_path
  first_path="$(cat "$OUT")"

  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[[bin]]
name = "demo_both"
path = "src/main.rs"

[lib]
name = "demo_both"
crate-type = ["cdylib"]
TOML
  rm -f "${CARGO_HOME}/bin/demo_both" "${CARGO_HOME}/bin/.demo_both.version"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_both" ]
  [ "$(cat "$OUT")" = "$first_path" ]
}

@test "an explicit [[bin]] table still names the bin path on a skip, not the lib path" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  local build_path
  build_path="$(cat "$OUT")"

  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[[bin]]
name = "demo_both"
path = "src/main.rs"

[lib]
name = "demo_both"
crate-type = ["cdylib"]
TOML
  run invoke
  [ "$status" -eq 0 ]
  run grep -F "already installed v1.0.0" "$ERR"
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "$build_path" ]
}

# One `[[bin]]` table that names the crate is unambiguous — it is the shape
# every sibling shipping a CLI writes (NEAT-AI-Backpropagation #152) — so the
# fast path reads it and the skip costs no cargo call at all. Declining here
# made "runs no cargo command at all" untrue for most of the family.
@test "a single [[bin]] table naming the crate skips without invoking cargo at all" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  local build_path
  build_path="$(cat "$OUT")"

  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[[bin]]
name = "demo_both"
path = "src/main.rs"

[lib]
name = "demo_both"
crate-type = ["cdylib"]
TOML
  : > "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  run grep -F "already installed v1.0.0" "$ERR"
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "$build_path" ]
}

# A table naming something else is still ambiguous: cargo can autodiscover a
# second binary named after the package beside it, so the reader must decline
# and let `cargo metadata` name the shape.
@test "a [[bin]] table naming another binary still falls through to cargo metadata" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]

  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[[bin]]
name = "demo_helper"
path = "src/bin/demo_helper.rs"

[lib]
name = "demo_both"
crate-type = ["cdylib"]
TOML
  : > "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -gt 0 ]
  run grep -F "already installed v1.0.0" "$ERR"
  [ "$status" -eq 0 ]
}

# Two tables name two binaries, and only one of them can be the crate's — the
# single-line reader cannot tell which, so it declines.
@test "several [[bin]] tables still fall through to cargo metadata" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]

  cat > "${REPO}/Cargo.toml" <<'TOML'
[package]
name = "demo_both"
version = "1.0.0"
edition = "2024"

[[bin]]
name = "demo_both"
path = "src/main.rs"

[[bin]]
name = "demo_helper"
path = "src/bin/demo_helper.rs"

[lib]
name = "demo_both"
crate-type = ["cdylib"]
TOML
  : > "$RUNLIB_SHIM_LOG"
  run invoke
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -gt 0 ]
  run grep -F "already installed v1.0.0" "$ERR"
  [ "$status" -eq 0 ]
}

# --- the checkout reached through a symlink is still the checkout -----------

# `$PWD` is the logical path; cargo reports `target_directory` canonicalised.
# Comparing them literally made the checkout's own target/ classify as
# "outside" and survive — on macOS (/tmp -> /private/tmp) that is every run.
@test "a symlinked checkout still has its own target/ removed" {
  make_crate "demo_app" "1.2.3" bin
  local link="${WORK}/link"
  ln -s "$REPO" "$link"
  run bash -c 'cd "$0" && "$1" > "$2" 2> "$3"' "$link" "$SCRIPT" "$OUT" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "removed" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -d "${TARGET_DIR}" ]
}

# --- a toolchain that cannot be read fails loud, never silently -------------

# `rustc --version 2>/dev/null | sed ...` under `set -euo pipefail` aborted the
# assignment, so the _runlib_die beneath it was unreachable and the caller got
# an empty stdout with a bare status. A rustup shim with no default toolchain
# is an ordinary host state, so this must name the fault.
@test "a rustc that cannot report its version fails loud rather than silently" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  cat > "${SHIM_DIR}/rustc" <<'SHIM'
#!/usr/bin/env bash
echo "error: rustup could not choose a version of rustc to run" >&2
exit 1
SHIM
  chmod +x "${SHIM_DIR}/rustc"
  run invoke
  [ "$status" -ne 0 ]
  [ ! -s "$OUT" ]
  run grep -F "cannot read the rustc version" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

@test "a rustc whose version string is unparsable fails loud" {
  write_manifest "demo_app" "1.2.3" bin 'rust-version = "1.92.0"'
  write_metadata "demo_app" "1.2.3" bin
  mkdir -p "${REPO}/target"
  cat > "${SHIM_DIR}/rustc" <<'SHIM'
#!/usr/bin/env bash
echo "some other toolchain wrapper"
SHIM
  chmod +x "${SHIM_DIR}/rustc"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "rustc version" "$ERR"
  [ "$status" -eq 0 ]
}

# rustup is not a precondition; rustc is. Requiring the one the script only
# reaches for when the toolchain is missing or too old rejected a working
# distro-packaged toolchain (Issue #700 keeps that, and asks rustup for a
# newer toolchain only when the graph demands one).
@test "a satisfied toolchain without rustup still installs" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.90.0" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke_with_path "$(shim_path_without_rustup)"
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

# With no `rustc` the script now tries to bootstrap one, so what this asserts
# is the fail-closed end of that attempt: a host it cannot verify a download on
# still exits non-zero naming rustup.rs, and installs nothing.
@test "a missing rustc with no way to bootstrap exits non-zero naming rustup.rs" {
  make_crate "demo_app" "1.2.3" bin
  rm -f "${SHIM_DIR}/rustc"
  # The shim dir supplies cargo; the minimal PATH supplies the coreutils and
  # no toolchain — and no SHA-256 tool, so the bootstrap refuses rather than
  # reaching for the network.
  run env PATH="${SHIM_DIR}:$(minimal_path)" CARGO_HOME="$CARGO_HOME" \
    RUNLIB_SHIM_LOG="$RUNLIB_SHIM_LOG" RUNLIB_SHIM_METADATA="$RUNLIB_SHIM_METADATA" \
    RUNLIB_SHIM_ARTEFACTS="$RUNLIB_SHIM_ARTEFACTS" \
    RUNLIB_SHIM_RUSTFLAGS="$RUNLIB_SHIM_RUSTFLAGS" \
    bash -c 'cd "$0" && "$1" 2>&1' "$REPO" "$SCRIPT"
  [ "$status" -ne 0 ]
  [[ "$output" == *"https://rustup.rs"* ]]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

# --- an install that did not land is not a success --------------------------

# `mv -f file dir/` moves the file *into* the directory and exits 0, so the
# script stamped, cleaned target/ and printed a path that was a directory.
@test "a directory sitting at the install path fails loud instead of being installed into" {
  make_crate "demo_lib" "1.0.0" cdylib
  mkdir -p "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)"
  run invoke
  [ "$status" -ne 0 ]
  [ ! -s "$OUT" ]
  [ -d "${TARGET_DIR}" ]
  [ ! -f "${CARGO_HOME}/lib/.demo_lib.version" ]
}

# The same corruption must not then read as a valid install for ever: a
# directory satisfies `-e`, which is why the skip path needs `-f`.
@test "a directory at the artefact path never reads as an installed artefact" {
  make_crate "demo_lib" "1.0.0" cdylib
  mkdir -p "${CARGO_HOME}/lib/libdemo_lib.$(lib_ext)"
  printf '1.0.0\n' > "${CARGO_HOME}/lib/.demo_lib.version"
  run invoke
  [ "$status" -ne 0 ]
  run grep -F "already installed" "$ERR"
  [ "$status" -ne 0 ]
}

# --- all-or-nothing means the old binary comes back -------------------------

# The bin was committed, then the lib mv failed: the previously installed
# binary had already been overwritten, which is the state the header comment
# and the README both say is impossible.
@test "a library that cannot be installed restores the previously installed binary" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  cp "${CARGO_HOME}/bin/demo_both" "${WORK}/bin-v1"

  make_crate "demo_both" "2.0.0" both
  # A directory at the library's install path makes the second commit fail
  # after the binary's has already succeeded.
  rm -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)"
  mkdir -p "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)"
  run invoke
  [ "$status" -ne 0 ]
  run cmp -s "${WORK}/bin-v1" "${CARGO_HOME}/bin/demo_both"
  [ "$status" -eq 0 ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_both.version")" = "1.0.0" ]
  [ -d "${TARGET_DIR}" ]
}

@test "no staging temporary survives a failed install in either directory" {
  make_crate "demo_both" "1.0.0" both
  run invoke
  [ "$status" -eq 0 ]
  make_crate "demo_both" "2.0.0" both
  rm -f "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)"
  mkdir -p "${CARGO_HOME}/lib/libdemo_both.$(lib_ext)"
  run invoke
  [ "$status" -ne 0 ]
  run bash -c 'ls "$1"/*.runlib.* "$2"/*.runlib.* 2>/dev/null' _ \
    "${CARGO_HOME}/bin" "${CARGO_HOME}/lib"
  [ "$status" -ne 0 ]
}

# --- the rustup bootstrap (Issue #699) --------------------------------------
#
# With no `rustc` on PATH the script installs rustup itself, from the pinned
# `rustup-init` whose SHA-256 is inlined in the script. Nothing here touches
# the network: a `curl` shim serves fixture bytes to the `-o` path, and the
# fixture is a *fake* rustup-init that writes $BOOT_MARKER when executed — so
# "the downloaded file is never executed" is asserted by that file's absence
# rather than inferred.

# The fixture installer, the marker it writes, and the logs the shims keep.
boot_fixture() {
  BOOT_DIR="${WORK}/boot"
  BOOT_FIXTURE="${BOOT_DIR}/served-rustup-init"
  BOOT_MARKER="${BOOT_DIR}/installer-ran.txt"
  BOOT_CURL_LOG="${BOOT_DIR}/curl-invocations.log"
  BOOT_TOOLCHAIN_SRC="${SHIM_DIR}"
  export BOOT_FIXTURE BOOT_MARKER BOOT_CURL_LOG BOOT_TOOLCHAIN_SRC
  mkdir -p "$BOOT_DIR"
  rm -f "$BOOT_MARKER" "$BOOT_CURL_LOG"
  cat > "$BOOT_FIXTURE" <<'FIXTURE'
#!/usr/bin/env bash
# The fake rustup-init. Running it is the thing the fail-closed paths must
# never do, so it records that it ran — and, like the real one, leaves a
# working toolchain under CARGO_HOME/bin.
set -euo pipefail
printf '%s\n' "$*" > "$BOOT_MARKER"
# BOOT_INSTALLS=0 models the installer that exits 0 having left no toolchain
# behind, which is what the post-bootstrap preconditions are there to catch.
[ "${BOOT_INSTALLS:-1}" = "1" ] || exit 0
mkdir -p "${CARGO_HOME}/bin"
cp "${BOOT_TOOLCHAIN_SRC}/cargo" "${CARGO_HOME}/bin/cargo"
cp "${BOOT_TOOLCHAIN_SRC}/rustc" "${CARGO_HOME}/bin/rustc"
FIXTURE
  chmod +x "$BOOT_FIXTURE"
}

# A PATH with no Rust toolchain on it at all, carrying a `curl` shim that
# serves $BOOT_FIXTURE, and the digest and temp-directory tools the bootstrap
# needs. Echoes the PATH.
boot_path() {
  local dir="${WORK}/boot-shims" tool resolved
  mkdir -p "$dir"
  for tool in mktemp sha256sum shasum jq; do
    resolved="$(command -v "$tool" 2>/dev/null || true)"
    [ -n "$resolved" ] || continue
    ln -sf "$resolved" "${dir}/${tool}"
  done
  cat > "${dir}/curl" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$BOOT_CURL_LOG"
if [ "${BOOT_CURL_FAILS:-0}" = "1" ]; then
  echo "curl shim: could not resolve host" >&2
  exit 6
fi
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then out="$arg"; fi
  prev="$arg"
done
if [ -z "$out" ]; then
  echo "curl shim: no -o path in: $*" >&2
  exit 2
fi
cp "$BOOT_FIXTURE" "$out"
SHIM
  chmod +x "${dir}/curl"
  printf '%s:%s' "$dir" "$(minimal_path)"
}

curl_invocations() {
  if [ -f "$BOOT_CURL_LOG" ]; then wc -l < "$BOOT_CURL_LOG" | tr -d ' '; else echo 0; fi
}

# Run the script from the fixture repository with the given PATH.
boot_invoke() {
  ( cd "$REPO" && PATH="$1" "$SCRIPT" > "$OUT" 2> "$ERR" )
}

@test "a served rustup-init whose digest does not match is never executed" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  run boot_invoke "$(boot_path)"
  [ "$status" -ne 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
  run grep -F "digest mismatch" "$ERR"
  [ "$status" -eq 0 ]
  # The line names both digests, so the operator can tell a stale pin from a
  # tampered download without re-running anything.
  run grep -E "expected [0-9a-f]{64}.*(got|actual)[^0-9a-f]*[0-9a-f]{64}" "$ERR"
  [ "$status" -eq 0 ]
  # And the URL, so the operator can fetch the published digest and tell a
  # stale pin from a tampered download.
  run grep -F "https://static.rust-lang.org/rustup/archive/" "$ERR"
  [ "$status" -eq 0 ]
}

@test "a failed rustup-init download exits non-zero and executes nothing" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  export BOOT_CURL_FAILS=1
  run boot_invoke "$(boot_path)"
  [ "$status" -ne 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
  [ "$(curl_invocations)" -eq 1 ]
  run grep -F "download" "$ERR"
  [ "$status" -eq 0 ]
}

@test "a host target with no pinned rustup-init fails loud without downloading" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  cat > "${WORK}/boot-shims/uname" <<'SHIM'
#!/usr/bin/env bash
case "${1:-}" in
  -s) echo "Plan9" ;;
  -m) echo "sparc64" ;;
  *) echo "Plan9" ;;
esac
SHIM
  chmod +x "${WORK}/boot-shims/uname"
  run boot_invoke "$path"
  [ "$status" -ne 0 ]
  [ "$(curl_invocations)" -eq 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
  run grep -F "Plan9" "$ERR"
  [ "$status" -eq 0 ]
}

@test "no SHA-256 tool on PATH means no rustup install at all" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  rm -f "${WORK}/boot-shims/sha256sum" "${WORK}/boot-shims/shasum"
  run boot_invoke "$path"
  [ "$status" -ne 0 ]
  [ ! -f "$BOOT_MARKER" ]
  # Nothing is fetched either: an installer that cannot be verified is not one
  # to download first and refuse afterwards.
  [ "$(curl_invocations)" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
  run grep -E "sha256sum|shasum" "$ERR"
  [ "$status" -eq 0 ]
}

@test "a matching digest installs rustup with a minimal profile and the build proceeds" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  # The digest is the one seam: the fixture bytes are not the published
  # rustup-init, so the happy path redefines the hashing helper — in a sourced
  # subshell, exactly as the sourced-entry-point test above does — to report
  # the digest this script pins for this host. Everything else is the real
  # code path.
  run bash -c '
    set -euo pipefail
    cd "$1"
    PATH="$2"
    . "$3"
    _runlib_sha256_of() { _runlib_pinned_rustup_digest "$(_runlib_host_target)"; }
    runlib_install 2> "$4"
  ' _ "$REPO" "$(boot_path)" "$SCRIPT" "$ERR"
  [ "$status" -eq 0 ]
  [ "$output" = "${CARGO_HOME}/bin/demo_app" ]
  [ "$(cat "$BOOT_MARKER")" = "-y --no-modify-path --profile minimal" ]
  [ "$(curl_invocations)" -eq 1 ]
  # The build really ran on the freshly installed toolchain.
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  [ "$(cat "${CARGO_HOME}/bin/.demo_app.version")" = "1.2.3" ]
  run grep -F "build --release" "$RUNLIB_SHIM_LOG"
  [ "$status" -eq 0 ]
}

@test "the installed rustup-init and its temporary directory are not left behind" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  run bash -c '
    set -euo pipefail
    cd "$1"
    PATH="$2"
    . "$3"
    _runlib_sha256_of() { _runlib_pinned_rustup_digest "$(_runlib_host_target)"; }
    runlib_install 2> "$4"
    printf "%s\n" "${_RUNLIB_TEMPS[@]+${_RUNLIB_TEMPS[@]}}" > "$5"
  ' _ "$REPO" "$(boot_path)" "$SCRIPT" "$ERR" "${WORK}/temps.txt"
  [ "$status" -eq 0 ]
  while IFS= read -r leftover; do
    [ -n "$leftover" ] || continue
    [ ! -e "$leftover" ]
  done < "${WORK}/temps.txt"
}

@test "with rustc on PATH nothing is downloaded and no installer is run" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  run boot_invoke "$(boot_path):${SHIM_DIR}"
  [ "$status" -eq 0 ]
  [ "$(curl_invocations)" -eq 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
}

# --- what the bootstrap actually fetches, and over what transport -----------
#
# The URL and the transport flags are the pin. A base URL, a version or a
# `--proto`/`--tlsv1.2` flag lost in a later edit would leave every test above
# green — the shim log records the argv, so these assert it. The host is fixed
# by `uname`/`ldd` shims rather than read from the machine, so the expected
# target is derived here independently of the script's own probe.

# A `uname` reporting $1 (kernel) and $2 (machine), and an `ldd` whose
# --version output is $3, exiting $4 — musl's `ldd` exits non-zero.
fake_host() {
  local dir="${WORK}/boot-shims"
  mkdir -p "$dir"
  cat > "${dir}/uname" <<SHIM
#!/usr/bin/env bash
case "\${1:-}" in
  -s) echo "$1" ;;
  -m) echo "$2" ;;
  *) echo "$1" ;;
esac
SHIM
  cat > "${dir}/ldd" <<SHIM
#!/usr/bin/env bash
echo "$3" >&2
exit $4
SHIM
  chmod +x "${dir}/uname" "${dir}/ldd"
}

@test "the download names the pinned version and target over pinned HTTPS" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  fake_host Linux x86_64 "ldd (GNU libc) 2.39" 0
  run boot_invoke "$path"
  # The fixture bytes are not the published installer, so this still refuses —
  # what matters here is what it asked for before refusing.
  [ "$status" -ne 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ "$(curl_invocations)" -eq 1 ]
  run grep -F "https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-gnu/rustup-init" "$BOOT_CURL_LOG"
  [ "$status" -eq 0 ]
  run grep -F -- "--proto =https" "$BOOT_CURL_LOG"
  [ "$status" -eq 0 ]
  run grep -F -- "--tlsv1.2" "$BOOT_CURL_LOG"
  [ "$status" -eq 0 ]
}

@test "a musl host asks for the musl installer, not the gnu one" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  # musl's `ldd` writes its banner to stderr and exits 1; reading that through
  # a bare pipe under `pipefail` reports the failure instead of the match, and
  # the host would silently take the gnu installer.
  fake_host Linux x86_64 "musl libc (x86_64)" 1
  run boot_invoke "$path"
  [ "$status" -ne 0 ]
  run grep -F "/x86_64-unknown-linux-musl/rustup-init" "$BOOT_CURL_LOG"
  [ "$status" -eq 0 ]
}

@test "an Apple host asks for the darwin installer, with no libc suffix" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  fake_host Darwin arm64 "no ldd here" 127
  run boot_invoke "$path"
  [ "$status" -ne 0 ]
  run grep -F "/aarch64-apple-darwin/rustup-init" "$BOOT_CURL_LOG"
  [ "$status" -eq 0 ]
}

# --- the digest table -------------------------------------------------------

@test "every pinned target carries a distinct 64-character lower-case digest" {
  # The values themselves are checked against the published `.sha256` files
  # when the pin moves — that needs the network and is recorded in the PR
  # summary. What runs here is the shape a typo or a truncated paste breaks.
  run bash -c '
    . "$1"
    seen=""
    for target in \
      x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
      x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
      x86_64-apple-darwin aarch64-apple-darwin; do
      digest="$(_runlib_pinned_rustup_digest "$target")" || exit 1
      [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || { echo "bad digest for $target: $digest"; exit 1; }
      case "$seen" in *"$digest"*) echo "duplicate digest for $target"; exit 1 ;; esac
      seen="$seen $digest"
    done
    # An unpinned target must refuse rather than print something.
    if _runlib_pinned_rustup_digest "riscv64-unknown-linux-gnu"; then exit 1; fi
  ' _ "$SCRIPT"
  [ "$status" -eq 0 ]
}

# --- the toolchain must actually be there afterwards ------------------------

@test "a bootstrap that leaves no toolchain exits non-zero naming rustup.rs" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  export BOOT_INSTALLS=0
  run bash -c '
    set -euo pipefail
    cd "$1"
    PATH="$2"
    . "$3"
    _runlib_sha256_of() { _runlib_pinned_rustup_digest "$(_runlib_host_target)"; }
    runlib_install
  ' _ "$REPO" "$(boot_path)" "$SCRIPT"
  [ "$status" -ne 0 ]
  # The installer ran — and still left nothing behind, which is the case the
  # post-bootstrap preconditions exist for.
  [ -f "$BOOT_MARKER" ]
  [[ "$output" == *"https://rustup.rs"* ]]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
  [ -d "${REPO}/target" ]
}

# --- the tools the bootstrap itself needs -----------------------------------

@test "no curl on PATH names curl rather than blaming the download" {
  make_crate "demo_app" "1.2.3" bin
  boot_fixture
  path="$(boot_path)"
  rm -f "${WORK}/boot-shims/curl"
  run boot_invoke "$path"
  [ "$status" -ne 0 ]
  [ ! -f "$BOOT_MARKER" ]
  [ -d "${REPO}/target" ]
  run grep -F "curl not found" "$ERR"
  [ "$status" -eq 0 ]
}

# --- the toolchain-only entry point (Issue #701) ----------------------------
#
# `--toolchain-only` runs the same bootstrap → pin repair → graph → gate chain
# install mode runs, builds nothing and installs nothing, and prints the
# override toolchain name — or an empty line — on stdout for a caller that runs
# cargo itself and exports it as `RUSTUP_TOOLCHAIN`. The stdout contract is
# asserted byte-exactly here: a stray line would be exported as a toolchain
# name.

@test "--toolchain-only on a satisfied toolchain builds nothing and prints an empty line" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run invoke_args --toolchain-only
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "" ]
  [ "$(wc -l < "$OUT" | tr -d ' ')" -eq 1 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
  [ ! -e "${CARGO_HOME}/bin" ]
  [ ! -e "${CARGO_HOME}/lib" ]
  [ -d "${REPO}/target" ]
  [ "$(rustup_invocations)" -eq 0 ]
}

@test "--toolchain-only prints the required version when the pin is below it" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke_args --toolchain-only
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "1.93.1" ]
  [ "$(wc -l < "$OUT" | tr -d ' ')" -eq 1 ]
  run grep -Fx "toolchain install 1.93.1" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
  [ "$(grep -c -e "1.92.0.*1.93.1.*rust-toolchain.toml" "$ERR")" -eq 1 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
  [ ! -e "${CARGO_HOME}/bin" ]
}

@test "--toolchain-only updates an unpinned channel and still prints an empty line" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  export RUNLIB_SHIM_RUSTC_AFTER_UPDATE="1.93.1"
  run invoke_args --toolchain-only
  [ "$status" -eq 0 ]
  run grep -Fx "update stable" "$RUNLIB_SHIM_RUSTUP_LOG"
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "" ]
  [ "$(wc -l < "$OUT" | tr -d ' ')" -eq 1 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
}

@test "--toolchain-only below the requirement with no rustup fails loud and prints nothing" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.80.0" "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke_with_path "$(shim_path_without_rustup)" --toolchain-only
  [ "$status" -ne 0 ]
  [ ! -s "$OUT" ]
  run grep -F "1.93.1" "$ERR"
  [ "$status" -eq 0 ]
  run grep -F "https://rustup.rs" "$ERR"
  [ "$status" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin" ]
  [ -d "${REPO}/target" ]
}

@test "an unknown argument exits 2 with a usage line and never calls cargo" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run invoke_args --wat
  [ "$status" -eq 2 ]
  [ ! -s "$OUT" ]
  [ "$(wc -l < "$ERR" | tr -d ' ')" -eq 1 ]
  run grep -F -- "--toolchain-only" "$ERR"
  [ "$status" -eq 0 ]
  [ "$(cargo_invocations)" -eq 0 ]
  [ ! -e "${CARGO_HOME}/bin/demo_app" ]
}

@test "an argument after --toolchain-only exits 2 and never calls cargo" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run invoke_args --toolchain-only extra
  [ "$status" -eq 2 ]
  [ ! -s "$OUT" ]
  [ "$(cargo_invocations)" -eq 0 ]
}

# The already-installed skip is install mode's; a caller running its own cargo
# always needs a good toolchain, so the gate runs whatever the stamp says.
@test "a matching stamp does not short-circuit --toolchain-only" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run invoke
  [ "$status" -eq 0 ]
  [ -x "${CARGO_HOME}/bin/demo_app" ]
  : > "$RUNLIB_SHIM_LOG"
  : > "$RUNLIB_SHIM_RUSTUP_LOG"
  run invoke_args --toolchain-only
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "1.93.1" ]
  run grep -F "metadata" "$RUNLIB_SHIM_LOG"
  [ "$status" -eq 0 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
}

@test "sourcing the script and calling runlib_ensure_toolchain behaves the same" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1" null
  export RUNLIB_SHIM_RUSTC_VERSION="1.93.1"
  run bash -c '
    set -euo pipefail
    cd "$1"
    . "$2"
    runlib_ensure_toolchain > "$3" 2> "$4"
  ' _ "$REPO" "$SCRIPT" "$OUT" "$ERR"
  [ "$status" -eq 0 ]
  [ "$(cat "$OUT")" = "" ]
  [ "$(wc -l < "$OUT" | tr -d ' ')" -eq 1 ]
  run grep -F "build" "$RUNLIB_SHIM_LOG"
  [ "$status" -ne 0 ]
  [ ! -e "${CARGO_HOME}/bin" ]
}

@test "a sourced runlib_ensure_toolchain reports the override it selected" {
  make_crate "demo_app" "1.2.3" bin
  write_graph_metadata "1.93.1"
  write_toolchain_pin "1.92.0"
  export RUNLIB_SHIM_RUSTC_VERSION="1.92.0"
  run bash -c '
    set -euo pipefail
    cd "$1"
    . "$2"
    override="$(runlib_ensure_toolchain 2> "$3")"
    printf "override=%s toolchain=%s\n" "$override" "${RUSTUP_TOOLCHAIN-<unset>}"
  ' _ "$REPO" "$SCRIPT" "$ERR"
  [ "$status" -eq 0 ]
  [ "$output" = "override=1.93.1 toolchain=<unset>" ]
}
