#!/usr/bin/env bats
# Tests for Issue #749: neat-core declares `rust-version = "1.99"` (via
# `rust-version.workspace = true` inheriting `[workspace.package]`), matching
# the family MSRV floor already enforced informally by scripts/runlib.sh's
# `_RUNLIB_FAMILY_MIN_RUST_DEFAULT`.
#
# These are "what" tests: each asserts on the rust-version cargo actually
# resolves for the neat-core package (via `cargo metadata`), not on the text
# of the `[workspace.package]` table alone. A `[workspace.package]` key does
# nothing to a crate unless the member inherits it with `*.workspace = true`
# — reading the root manifest's text can't see whether neat-core still does
# that, so the oracle here is cargo's own resolution (PR #750 review).

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  RUNLIB="${REPO_ROOT}/scripts/runlib.sh"
}

neat_core_rust_version() {
  cargo metadata --no-deps --format-version 1 --offline \
    --manifest-path "${REPO_ROOT}/Cargo.toml" \
    | jq -r '.packages[] | select(.name == "neat-core") | .rust_version'
}

runlib_family_min_rust() {
  sed -n 's/^_RUNLIB_FAMILY_MIN_RUST_DEFAULT="\(.*\)"$/\1/p' "$RUNLIB"
}

@test "neat-core resolves rust-version 1.99 via workspace inheritance" {
  run neat_core_rust_version
  [ "$status" -eq 0 ]
  [ "$output" = "1.99" ]
}

@test "the resolved rust-version matches runlib.sh's family floor default" {
  resolved="$(neat_core_rust_version)"
  floor="$(runlib_family_min_rust)"
  [ -n "$resolved" ]
  [ -n "$floor" ]
  [ "$resolved" = "$floor" ]
}
