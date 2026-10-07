#!/usr/bin/env bats
# Tests for Issue #749: the root Cargo.toml declares `rust-version = "1.99"`
# in `[workspace.package]`, matching the family MSRV floor already enforced
# informally by scripts/runlib.sh's `_RUNLIB_FAMILY_MIN_RUST_DEFAULT`.
#
# These are "what" tests: each reads the real committed files and asserts on
# the observable declared value, never a private copy of either.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  CARGO_TOML="${REPO_ROOT}/Cargo.toml"
  RUNLIB="${REPO_ROOT}/scripts/runlib.sh"
}

workspace_rust_version() {
  # Extract the rust-version declared in [workspace.package], not any other
  # table that might one day carry the same key.
  awk '
    /^\[workspace\.package\]/ { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && /^rust-version[[:space:]]*=/ {
      match($0, /"[^"]*"/)
      print substr($0, RSTART + 1, RLENGTH - 2)
    }
  ' "$CARGO_TOML"
}

runlib_family_min_rust() {
  sed -n 's/^_RUNLIB_FAMILY_MIN_RUST_DEFAULT="\(.*\)"$/\1/p' "$RUNLIB"
}

@test "root Cargo.toml declares the family MSRV floor as rust-version" {
  run workspace_rust_version
  [ "$status" -eq 0 ]
  [ "$output" = "1.99" ]
}

@test "the declared rust-version matches runlib.sh's family floor default" {
  declared="$(workspace_rust_version)"
  floor="$(runlib_family_min_rust)"
  [ -n "$declared" ]
  [ -n "$floor" ]
  [ "$declared" = "$floor" ]
}
