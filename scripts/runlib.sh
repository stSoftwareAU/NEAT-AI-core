#!/usr/bin/env bash
# Canonical build → install → clean helper for the NEAT-AI Rust siblings.
#
# COPY CONTRACT (Issue #680): this file has one home — `scripts/runlib.sh` on
# NEAT-AI-core `Develop`. Every Rust sibling copies it byte-for-byte into its
# own `scripts/runlib.sh`; behaviour changes are made *here* and re-copied
# outward, never edited downstream. See README.md → "Canonical runlib.sh".
#
# Run it from the repository root of the sibling, or source it and call
# `runlib_install`. It:
#
#   * resolves the single workspace member — the root crate, or the one
#     `[workspace] members` entry — and fails loud on zero or more than one;
#   * installs the bin target named after the crate to `$CARGO_HOME/bin/<crate>`
#     and a `cdylib` target to `$CARGO_HOME/lib/lib<crate>.{so,dylib}`, both
#     with `-` → `_` in the crate name (CARGO_HOME defaults to `~/.cargo`). A
#     crate carrying both targets installs both. The installed names come from
#     the *crate*, never from the cargo target name, so the skip below looks
#     the artefacts up under the same names the install wrote;
#   * writes the stamp `.<crate>.version` — the crate semver — beside every
#     artefact it installs, and writes it *last*;
#   * runs **no** `cargo` command at all when artefact and stamp already match,
#     printing exactly one stderr line `[<crate>] already installed v<x>`.
#     There is no force flag: delete the stamp to force a rebuild;
#   * removes the checkout's `target/` after a successful install and names the
#     path removed and the bytes freed on stderr; a build directory outside the
#     checkout (a shared `CARGO_TARGET_DIR`) is kept, and that is reported. Any
#     failure keeps `target/` and leaves the installed artefacts and their
#     stamps untouched — every artefact is staged and moved into place only
#     once all of them are ready;
#   * honours the caller's RUSTFLAGS unchanged and sets no flags of its own;
#   * prints the installed bin path — or the lib path when there is no bin —
#     on stdout, and nothing else on stdout.
#
# The toolchain is a precondition, not something this script installs: with
# `cargo`, `rustup` or `jq` missing it exits non-zero naming what to install.
# MSRV comes from `rust-version` in the crate manifest when present; a
# `rust-toolchain.toml` is honoured by rustup itself.
#
# Run it as a subprocess — `path="$(./scripts/runlib.sh)"`. It can also be
# sourced, but note that sourcing applies `set -euo pipefail` to the calling
# shell, prepends `$CARGO_HOME/bin` to its PATH, and that any failure exits
# that shell rather than returning to it.
set -euo pipefail

_runlib_die() {
  printf 'runlib: %s\n' "$*" >&2
  exit 1
}

# Cargo's home, honouring CARGO_HOME exactly as rustup and cargo do.
_runlib_cargo_home() {
  printf '%s' "${CARGO_HOME:-$HOME/.cargo}"
}

# Value of `key` inside `[section]` of the TOML file $1, unquoted. Prints
# nothing when the file, the section or the key is absent.
_runlib_toml_value() {
  local file="$1" section="$2" key="$3"
  [[ -f "$file" ]] || return 0
  awk -v want="[$section]" -v key="$key" '
    {
      line = $0
      sub(/[[:space:]]*#.*$/, "", line)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
    }
    line ~ /^\[/ {
      hdr = line
      gsub(/[[:space:]]/, "", hdr)
      cur = hdr
      next
    }
    cur != want { next }
    {
      eq = index(line, "=")
      if (eq == 0) next
      k = substr(line, 1, eq - 1)
      gsub(/[[:space:]]/, "", k)
      if (k != key) next
      v = substr(line, eq + 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)
      gsub(/^"|"$/, "", v)
      print v
      exit
    }
  ' "$file"
  return 0
}

# The `members` entries of the root manifest $1, one per line.
_runlib_workspace_members() {
  [[ -f "$1" ]] || return 0
  awk '
    { line = $0; sub(/[[:space:]]*#.*$/, "", line) }
    line ~ /^[[:space:]]*\[/ {
      hdr = line
      gsub(/[[:space:]]/, "", hdr)
      in_ws = (hdr == "[workspace]") ? 1 : 0
      collecting = 0
      next
    }
    !in_ws { next }
    !collecting {
      k = line
      sub(/=.*$/, "", k)
      gsub(/[[:space:]]/, "", k)
      if (k != "members") next
      sub(/^[^=]*=/, "", line)
      collecting = 1
    }
    collecting {
      buf = buf " " line
      if (index(line, "]") > 0) collecting = 0
    }
    END {
      if (buf == "") exit 0
      n = split(buf, parts, /"/)
      for (i = 2; i <= n; i += 2) print parts[i]
    }
  ' "$1"
  return 0
}

# Manifest path of the single workspace member, resolved without running cargo.
# Returns non-zero — silently — whenever the shape is anything but the two
# unambiguous ones, leaving `cargo metadata` on the build path to be the
# authority that fails loud.
_runlib_fast_member_manifest() {
  local repo_root="$1" root_manifest="$1/Cargo.toml"
  [[ -f "$root_manifest" ]] || return 1

  local has_package=0
  if grep -qE '^[[:space:]]*\[package\]' "$root_manifest"; then
    has_package=1
  fi

  local member count=0 first=""
  while IFS= read -r member; do
    [[ -n "$member" ]] || continue
    count=$((count + 1))
    first="$member"
  done <<EOF
$(_runlib_workspace_members "$root_manifest")
EOF

  if [[ "$has_package" -eq 1 && "$count" -eq 0 ]]; then
    printf '%s\n' "$root_manifest"
    return 0
  fi
  if [[ "$has_package" -eq 0 && "$count" -eq 1 ]]; then
    case "$first" in
      *'*'*) return 1 ;;
    esac
    [[ -f "$repo_root/$first/Cargo.toml" ]] || return 1
    printf '%s\n' "$repo_root/$first/Cargo.toml"
    return 0
  fi
  return 1
}

# A `[package]` field of manifest $1, following `<key>.workspace = true` into
# the `[workspace.package]` table of root manifest $2.
_runlib_crate_field() {
  local manifest="$1" root_manifest="$2" key="$3" value inherited
  value="$(_runlib_toml_value "$manifest" package "$key")"
  if [[ -z "$value" ]]; then
    inherited="$(_runlib_toml_value "$manifest" package "${key}.workspace")"
    if [[ "$inherited" == "true" ]]; then
      value="$(_runlib_toml_value "$root_manifest" workspace.package "$key")"
    fi
  fi
  printf '%s' "$value"
  return 0
}

# Shared-library extension for this platform.
_runlib_lib_extension() {
  case "$(uname -s)" in
    Darwin) printf 'dylib' ;;
    *) printf 'so' ;;
  esac
}

# Installed basenames. Both derive from the *crate* name with `-` -> `_`, never
# from the cargo target name: the skip path knows only the crate, so a crate
# whose `[lib] name` differs from its package name would otherwise be installed
# under one name and looked up under another — and rebuild on every run forever.
_runlib_bin_basename() {
  printf '%s' "${1//-/_}"
}

_runlib_lib_basename() {
  local extension
  extension="$(_runlib_lib_extension)"
  printf 'lib%s.%s' "${1//-/_}" "$extension"
}

# Returns 0 when semver $1 >= semver $2.
_runlib_version_ge() {
  local left="$1" right="$2" i x y
  local -a a b
  local IFS='.'
  read -r -a a <<< "${left%%-*}"
  read -r -a b <<< "${right%%-*}"
  for ((i = 0; i < ${#a[@]} || i < ${#b[@]}; i++)); do
    x="${a[i]:-0}"
    y="${b[i]:-0}"
    # A non-numeric component (a build-metadata tail, a typo in `rust-version`)
    # must not become an arithmetic crash under `set -u`.
    case "$x" in ''|*[!0-9]*) x=0 ;; esac
    case "$y" in ''|*[!0-9]*) y=0 ;; esac
    if ((10#$x > 10#$y)); then return 0; fi
    if ((10#$x < 10#$y)); then return 1; fi
  done
  return 0
}

# Sets RUNLIB_EXPECTS_BIN / RUNLIB_EXPECTS_LIB from manifest $1 alone, for the
# fast path. Deliberately conservative: it claims a target only where cargo is
# certain to produce one, because an over-claimed target would make every run
# rebuild for ever, and an under-claimed one only costs one `cargo metadata`.
_runlib_expected_shape() {
  local manifest="$1" crate_underscored="$2" manifest_dir crate_types
  manifest_dir="$(dirname "$manifest")"
  RUNLIB_EXPECTS_BIN=0
  RUNLIB_EXPECTS_LIB=0

  # A cdylib is never implicit — it exists only where `[lib] crate-type` says so.
  crate_types="$(_runlib_toml_value "$manifest" lib crate-type)"
  case "$crate_types" in
    *cdylib*) RUNLIB_EXPECTS_LIB=1 ;;
  esac

  # An explicit `[[bin]]` table, or `autobins`, can rename or suppress the
  # binary cargo would otherwise name after the package, so the claim is made
  # only for the two unambiguous auto-discovered shapes.
  if ! grep -qE '^[[:space:]]*\[\[bin\]\]' "$manifest" &&
    ! grep -qE '^[[:space:]]*autobins[[:space:]]*=' "$manifest"; then
    if [[ -f "$manifest_dir/src/main.rs" ||
      -f "$manifest_dir/src/bin/${crate_underscored}.rs" ]]; then
      RUNLIB_EXPECTS_BIN=1
    fi
  fi
  return 0
}

# True when the stamp in directory $1 matches version $3 for crate $2 and the
# artefact $4 it stands for is still there. A stamp deleted beside a surviving
# artefact is the documented way to force a rebuild, so it reads as "not
# current" rather than as "nothing was ever installed here".
_runlib_stamp_current() {
  local dir="$1" crate="$2" version="$3" artefact="$4" stamp
  stamp="$dir/.${crate}.version"
  [[ -f "$stamp" ]] || return 1
  [[ -e "$artefact" ]] || return 1
  [[ "$(cat "$stamp")" == "$version" ]] || return 1
  return 0
}

# Prints the already-installed line and the installed path and returns 0 when
# *every* artefact the crate's shape calls for is present with a matching
# stamp; returns non-zero — silently — otherwise. Runs no cargo command.
#
# Checking the whole shape is what stops a crate that ships both a bin and a
# cdylib from reporting "already installed" once one half has been removed.
_runlib_report_current() {
  local crate="$1" version="$2" expects_bin="$3" expects_lib="$4"
  local bin_dir lib_dir bin_path lib_path
  bin_dir="$(_runlib_cargo_home)/bin"
  lib_dir="$(_runlib_cargo_home)/lib"
  bin_path="$bin_dir/$(_runlib_bin_basename "$crate")"
  lib_path="$lib_dir/$(_runlib_lib_basename "$crate")"

  # An undetermined shape claims nothing: rebuilding costs a build, reporting a
  # half-installed tree as complete costs a fleet host the wrong artefact.
  [[ "$expects_bin" -eq 1 || "$expects_lib" -eq 1 ]] || return 1

  if [[ "$expects_bin" -eq 1 ]]; then
    _runlib_stamp_current "$bin_dir" "$crate" "$version" "$bin_path" || return 1
  fi
  if [[ "$expects_lib" -eq 1 ]]; then
    _runlib_stamp_current "$lib_dir" "$crate" "$version" "$lib_path" || return 1
  fi

  printf '[%s] already installed v%s\n' "$crate" "$version" >&2
  if [[ "$expects_bin" -eq 1 ]]; then
    printf '%s\n' "$bin_path"
  else
    printf '%s\n' "$lib_path"
  fi
  return 0
}

# The fast path: resolve the crate from the manifests alone and report an
# up-to-date install without running cargo at all. Returns non-zero — silently
# — whenever the repository shape is anything but the two unambiguous ones,
# leaving `cargo metadata` on the build path to be the authority.
_runlib_try_skip() {
  local repo_root="$1" root_manifest="$2" manifest crate version
  manifest="$(_runlib_fast_member_manifest "$repo_root")" || return 1
  crate="$(_runlib_crate_field "$manifest" "$root_manifest" name)"
  version="$(_runlib_crate_field "$manifest" "$root_manifest" version)"
  [[ -n "$crate" && -n "$version" ]] || return 1
  _runlib_expected_shape "$manifest" "${crate//-/_}"
  _runlib_report_current "$crate" "$version" \
    "$RUNLIB_EXPECTS_BIN" "$RUNLIB_EXPECTS_LIB" || return 1
  return 0
}

# The toolchain is a precondition. No network install happens here.
_runlib_require_toolchain() {
  PATH="$(_runlib_cargo_home)/bin:$PATH"
  export PATH
  command -v cargo >/dev/null 2>&1 ||
    _runlib_die "cargo not found — install the Rust toolchain from https://rustup.rs and re-run"
  command -v rustup >/dev/null 2>&1 ||
    _runlib_die "rustup not found — install the Rust toolchain from https://rustup.rs and re-run"
  command -v jq >/dev/null 2>&1 ||
    _runlib_die "jq not found — install jq (it parses \`cargo metadata\`) and re-run"
}

# MSRV gate. `rust-version` in the crate manifest is the single source of
# truth; a `rust-toolchain.toml` is rustup's business, not this script's.
_runlib_check_msrv() {
  local manifest="$1" root_manifest="$2" msrv rust_version
  msrv="$(_runlib_crate_field "$manifest" "$root_manifest" rust-version)"
  [[ -n "$msrv" ]] || return 0
  rust_version="$(rustc --version 2>/dev/null |
    sed -n 's/^rustc \([0-9][0-9]*\.[0-9][0-9]*\(\.[0-9][0-9]*\)\{0,1\}\).*/\1/p')"
  [[ -n "$rust_version" ]] ||
    _runlib_die "cannot read the rustc version; this crate needs Rust >= $msrv"
  _runlib_version_ge "$rust_version" "$msrv" ||
    _runlib_die "rustc $rust_version is below the crate MSRV $msrv — run: rustup update stable"
  return 0
}

# Copy $1 over $2 through a temporary in the destination directory, so a
# half-written artefact never replaces a working one.
_runlib_install_file() {
  local source="$1" dest="$2" temp="$2.runlib.$$"
  if ! cp "$source" "$temp"; then
    rm -f "$temp"
    _runlib_die "could not copy $source to $dest"
  fi
  if ! mv -f "$temp" "$dest"; then
    rm -f "$temp"
    _runlib_die "could not install $dest"
  fi
  return 0
}

# Remove the checkout's build directory and report what that freed.
#
# Only the checkout's own directory is removed. `CARGO_TARGET_DIR` (or
# `[build] target-dir`) can point cargo at a cache shared with other checkouts,
# and deleting that would destroy builds this script never made — so a target
# directory outside the repository is kept, and the fact is reported rather
# than passed over in silence.
_runlib_remove_target() {
  local crate="$1" target_dir="$2" repo_root="$3" kilobytes bytes
  [[ -d "$target_dir" ]] || return 0
  [[ "$target_dir" == /* ]] ||
    _runlib_die "refusing to remove the relative target directory '$target_dir'"
  [[ "$target_dir" != "/" ]] ||
    _runlib_die "refusing to remove '/' as a target directory"
  case "$target_dir" in
    "$repo_root"/*) : ;;
    *)
      printf '[%s] kept %s (outside the checkout %s)\n' \
        "$crate" "$target_dir" "$repo_root" >&2
      return 0
      ;;
  esac
  # `du -sk` is the portable reading — macOS bash 3.2 has no `du -b`.
  kilobytes="$(du -sk "$target_dir" | awk 'NR == 1 { print $1 }')"
  if [[ ! "$kilobytes" =~ ^[0-9]+$ ]]; then
    _runlib_die "could not measure $target_dir before removing it"
  fi
  bytes=$((kilobytes * 1024))
  rm -rf "$target_dir"
  printf '[%s] removed %s (freed %s bytes)\n' "$crate" "$target_dir" "$bytes" >&2
  return 0
}

# macOS refuses to dlopen an unsigned dylib, and a cdylib copied out of
# target/ still carries its build-tree install_name. Both are no-ops elsewhere.
_runlib_macos_fixups() {
  local lib_path="$1" lib_file="$2"
  [[ "$(uname -s)" == "Darwin" ]] || return 0
  install_name_tool -id "@rpath/$lib_file" "$lib_path" >&2 || return 1
  codesign --force --sign - --timestamp=none "$lib_path" >&2 || return 1
  return 0
}

# Discard every staged artefact, then die. Nothing has been moved into place at
# this point, so the previously installed artefacts and their stamps survive.
_runlib_abort_staged() {
  local message="$1"
  shift
  rm -f "$@"
  _runlib_die "$message"
}

runlib_install() {
  local repo_root="$PWD"
  local root_manifest="$repo_root/Cargo.toml"
  [[ -f "$root_manifest" ]] ||
    _runlib_die "no Cargo.toml in $repo_root — run runlib.sh from the repository root"

  if _runlib_try_skip "$repo_root" "$root_manifest"; then
    return 0
  fi

  _runlib_require_toolchain

  local metadata package_count
  metadata="$(cargo metadata --no-deps --format-version 1)"
  package_count="$(printf '%s' "$metadata" | jq '.packages | length')"
  if [[ "$package_count" -eq 0 ]]; then
    _runlib_die "cargo metadata reports no workspace member in $repo_root"
  fi
  if [[ "$package_count" -gt 1 ]]; then
    local names
    names="$(printf '%s' "$metadata" | jq -r '[.packages[].name] | join(", ")')"
    _runlib_die "expected exactly one workspace member, found $package_count: $names"
  fi

  local crate version manifest target_dir crate_underscored bin_name lib_name
  crate="$(printf '%s' "$metadata" | jq -r '.packages[0].name')"
  version="$(printf '%s' "$metadata" | jq -r '.packages[0].version')"
  manifest="$(printf '%s' "$metadata" | jq -r '.packages[0].manifest_path')"
  target_dir="$(printf '%s' "$metadata" | jq -r '.target_directory')"
  crate_underscored="${crate//-/_}"
  bin_name="$(printf '%s' "$metadata" | jq -r --arg c "$crate_underscored" '
    .packages[0].targets
    | map(select((.kind | index("bin")) and ((.name | gsub("-"; "_")) == $c)))
    | (.[0].name // empty)')"
  lib_name="$(printf '%s' "$metadata" | jq -r '
    .packages[0].targets
    | map(select(.kind | index("cdylib")))
    | (.[0].name // empty)')"

  [[ -n "$crate" && "$crate" != "null" ]] || _runlib_die "cargo metadata named no crate"
  [[ -n "$version" && "$version" != "null" ]] || _runlib_die "cargo metadata gave no version for $crate"
  if [[ -z "$bin_name" && -z "$lib_name" ]]; then
    _runlib_die "crate '$crate' has no bin target named '$crate_underscored' and no cdylib target — nothing to install"
  fi

  _runlib_check_msrv "$manifest" "$root_manifest"

  # Cargo has now named the shape authoritatively, so the up-to-date check runs
  # again over it. The fast path above declines every repository layout it
  # cannot read unambiguously (a globbed `members` entry, say); without this
  # second check those layouts would rebuild on every single invocation.
  local expects_bin=0 expects_lib=0
  if [[ -n "$bin_name" ]]; then expects_bin=1; fi
  if [[ -n "$lib_name" ]]; then expects_lib=1; fi
  if _runlib_report_current "$crate" "$version" "$expects_bin" "$expects_lib"; then
    return 0
  fi

  local -a build_args
  build_args=(build --release --package "$crate")
  if [[ -n "$lib_name" ]]; then
    build_args+=(--lib)
  fi
  if [[ -n "$bin_name" ]]; then
    build_args+=(--bin "$bin_name")
  fi
  # RUSTFLAGS is the caller's: this script neither sets nor edits it.
  cargo "${build_args[@]}" >&2

  local bin_dir lib_dir release_dir bin_file lib_file
  local staged_bin="" staged_lib="" installed_bin="" installed_lib=""
  bin_dir="$(_runlib_cargo_home)/bin"
  lib_dir="$(_runlib_cargo_home)/lib"
  release_dir="$target_dir/release"
  bin_file="$(_runlib_bin_basename "$crate")"
  lib_file="$(_runlib_lib_basename "$crate")"

  # Everything is staged first and moved into place only once every artefact is
  # ready: a crate that ships both a bin and a cdylib must not leave the new
  # binary installed beside the old library when the library step fails.
  if [[ -n "$bin_name" ]]; then
    local built_bin="$release_dir/$bin_name"
    [[ -f "$built_bin" ]] || _runlib_die "the build produced no binary at $built_bin"
    mkdir -p "$bin_dir"
    staged_bin="$bin_dir/$bin_file.runlib.$$"
    cp "$built_bin" "$staged_bin" ||
      _runlib_abort_staged "could not stage $built_bin" "$staged_bin"
    chmod +x "$staged_bin"
  fi

  if [[ -n "$lib_name" ]]; then
    local built_lib="" candidate built_file lib_extension
    lib_extension="$(_runlib_lib_extension)"
    built_file="lib${lib_name//-/_}.${lib_extension}"
    # A cdylib lands in target/release/, and in target/release/deps/ on the
    # toolchains that only hard-link the former.
    for candidate in "$release_dir/$built_file" "$release_dir/deps/$built_file"; do
      if [[ -f "$candidate" ]]; then
        built_lib="$candidate"
        break
      fi
    done
    [[ -n "$built_lib" ]] ||
      _runlib_abort_staged \
        "the build produced no $built_file in $release_dir or $release_dir/deps" \
        "$staged_bin"
    mkdir -p "$lib_dir"
    staged_lib="$lib_dir/$lib_file.runlib.$$"
    cp "$built_lib" "$staged_lib" ||
      _runlib_abort_staged "could not stage $built_lib" "$staged_bin" "$staged_lib"
    _runlib_macos_fixups "$staged_lib" "$lib_file" ||
      _runlib_abort_staged "macOS signing failed for $lib_file" "$staged_bin" "$staged_lib"
  fi

  if [[ -n "$staged_bin" ]]; then
    mv -f "$staged_bin" "$bin_dir/$bin_file" ||
      _runlib_abort_staged "could not install $bin_dir/$bin_file" "$staged_bin" "$staged_lib"
    installed_bin="$bin_dir/$bin_file"
  fi
  if [[ -n "$staged_lib" ]]; then
    mv -f "$staged_lib" "$lib_dir/$lib_file" ||
      _runlib_abort_staged "could not install $lib_dir/$lib_file" "$staged_lib"
    installed_lib="$lib_dir/$lib_file"
  fi

  # The stamps go last: until they are written, a half-finished install still
  # reads as "needs building" rather than as an up-to-date one.
  if [[ -n "$installed_bin" ]]; then
    printf '%s\n' "$version" > "$bin_dir/.${crate}.version"
  fi
  if [[ -n "$installed_lib" ]]; then
    printf '%s\n' "$version" > "$lib_dir/.${crate}.version"
  fi

  _runlib_remove_target "$crate" "$target_dir" "$repo_root"

  if [[ -n "$installed_bin" ]]; then
    printf '%s\n' "$installed_bin"
  else
    printf '%s\n' "$installed_lib"
  fi
  return 0
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  runlib_install
fi
