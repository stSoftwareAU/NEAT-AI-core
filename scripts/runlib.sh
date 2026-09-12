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
#   * installs the bin target named after the crate (`-` → `_`) to
#     `$CARGO_HOME/bin/<bin>` and a `cdylib` target to
#     `$CARGO_HOME/lib/lib<crate>.{so,dylib}` (CARGO_HOME defaults to
#     `~/.cargo`). A crate carrying both targets installs both;
#   * writes the stamp `.<crate>.version` — the crate semver — beside every
#     artefact it installs, and writes it *last*;
#   * runs **no** `cargo` command at all when artefact and stamp already match,
#     printing exactly one stderr line `[<crate>] already installed v<x>`.
#     There is no force flag: delete the stamp to force a rebuild;
#   * removes the checkout's `target/` after a successful install and names the
#     path removed and the bytes freed on stderr. A failed build keeps
#     `target/` and leaves the installed artefact and its stamp untouched;
#   * honours the caller's RUSTFLAGS unchanged and sets no flags of its own;
#   * prints the installed bin path — or the lib path when there is no bin —
#     on stdout, and nothing else on stdout.
#
# The toolchain is a precondition, not something this script installs: with
# `cargo` or `rustup` missing it exits non-zero naming https://rustup.rs. MSRV
# comes from `rust-version` in the crate manifest when present; a
# `rust-toolchain.toml` is honoured by rustup itself.
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

# Path of the already-installed bin artefact for crate $2 under $1, if any.
_runlib_installed_bin_path() {
  local bin_dir="$1" crate="$2" candidate
  for candidate in "$bin_dir/$crate" "$bin_dir/${crate//-/_}"; do
    if [[ -x "$candidate" && ! -d "$candidate" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 0
}

# Path of the already-installed cdylib artefact for crate $2 under $1, if any.
_runlib_installed_lib_path() {
  local lib_dir="$1" crate="${2//-/_}" candidate
  for candidate in "$lib_dir/lib$crate.so" "$lib_dir/lib$crate.dylib"; do
    if [[ -f "$candidate" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 0
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
    if ((10#$x > 10#$y)); then return 0; fi
    if ((10#$x < 10#$y)); then return 1; fi
  done
  return 0
}

# The artefact-and-stamp fast path. Prints the `already installed` line and the
# installed path and returns 0 when nothing needs building; returns non-zero —
# silently — otherwise. Runs no cargo command on either branch.
_runlib_try_skip() {
  local repo_root="$1" root_manifest="$2"
  local manifest crate version bin_dir lib_dir bin_stamp lib_stamp
  local bin_path lib_path found=0

  manifest="$(_runlib_fast_member_manifest "$repo_root")" || return 1
  crate="$(_runlib_crate_field "$manifest" "$root_manifest" name)"
  version="$(_runlib_crate_field "$manifest" "$root_manifest" version)"
  [[ -n "$crate" && -n "$version" ]] || return 1

  bin_dir="$(_runlib_cargo_home)/bin"
  lib_dir="$(_runlib_cargo_home)/lib"
  bin_stamp="$bin_dir/.${crate}.version"
  lib_stamp="$lib_dir/.${crate}.version"
  bin_path="$(_runlib_installed_bin_path "$bin_dir" "$crate")"
  lib_path="$(_runlib_installed_lib_path "$lib_dir" "$crate")"

  # A stamp deleted beside a surviving artefact is the documented way to force
  # a rebuild, so it must not be read as "nothing installed here".
  if [[ -f "$bin_stamp" ]]; then
    [[ -n "$bin_path" ]] || return 1
    [[ "$(cat "$bin_stamp")" == "$version" ]] || return 1
    found=1
  elif [[ -n "$bin_path" ]]; then
    return 1
  fi
  if [[ -f "$lib_stamp" ]]; then
    [[ -n "$lib_path" ]] || return 1
    [[ "$(cat "$lib_stamp")" == "$version" ]] || return 1
    found=1
  elif [[ -n "$lib_path" ]]; then
    return 1
  fi
  [[ "$found" -eq 1 ]] || return 1

  printf '[%s] already installed v%s\n' "$crate" "$version" >&2
  if [[ -n "$bin_path" ]]; then
    printf '%s\n' "$bin_path"
  else
    printf '%s\n' "$lib_path"
  fi
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
_runlib_remove_target() {
  local crate="$1" target_dir="$2" kilobytes bytes
  [[ -d "$target_dir" ]] || return 0
  [[ "$target_dir" == /* ]] ||
    _runlib_die "refusing to remove the relative target directory '$target_dir'"
  [[ "$target_dir" != "/" ]] ||
    _runlib_die "refusing to remove '/' as a target directory"
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
  install_name_tool -id "@rpath/$lib_file" "$lib_path" >&2 ||
    _runlib_die "install_name_tool failed on $lib_path"
  codesign --force --sign - --timestamp=none "$lib_path" >&2 ||
    _runlib_die "codesign failed on $lib_path"
  return 0
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

  local bin_dir lib_dir release_dir installed_bin="" installed_lib=""
  bin_dir="$(_runlib_cargo_home)/bin"
  lib_dir="$(_runlib_cargo_home)/lib"
  release_dir="$target_dir/release"

  if [[ -n "$bin_name" ]]; then
    local built_bin="$release_dir/$bin_name"
    [[ -f "$built_bin" ]] || _runlib_die "the build produced no binary at $built_bin"
    mkdir -p "$bin_dir"
    _runlib_install_file "$built_bin" "$bin_dir/$bin_name"
    chmod +x "$bin_dir/$bin_name"
    installed_bin="$bin_dir/$bin_name"
  fi

  if [[ -n "$lib_name" ]]; then
    local lib_file lib_extension
    lib_extension="$(_runlib_lib_extension)"
    lib_file="lib${lib_name//-/_}.${lib_extension}"
    local built_lib="" candidate
    # A cdylib lands in target/release/, and in target/release/deps/ on the
    # toolchains that only hard-link the former.
    for candidate in "$release_dir/$lib_file" "$release_dir/deps/$lib_file"; do
      if [[ -f "$candidate" ]]; then
        built_lib="$candidate"
        break
      fi
    done
    [[ -n "$built_lib" ]] ||
      _runlib_die "the build produced no $lib_file in $release_dir or $release_dir/deps"
    mkdir -p "$lib_dir"
    _runlib_install_file "$built_lib" "$lib_dir/$lib_file"
    _runlib_macos_fixups "$lib_dir/$lib_file" "$lib_file"
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

  _runlib_remove_target "$crate" "$target_dir"

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
