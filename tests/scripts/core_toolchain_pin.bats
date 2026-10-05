#!/usr/bin/env bats
# Regression assertion for Issue #747 — the canonical `scripts/runlib.sh`
# toolchain gate can only enforce a Rust version something declares, and
# NEAT-AI-core previously had neither a `rust-toolchain.toml` nor a
# `rust-version`. The family convention is that each crate pins
# `rust-toolchain.toml` to an exact version AND declares a matching
# `rust-version`, so CI builds with the declared minimum and a newer std API
# cannot ship without bumping both.
#
# These are "what" tests: they source the real `scripts/runlib.sh` and call
# its own helpers — `_runlib_pinned_channel`, `_runlib_is_exact_version` and
# `_runlib_crate_field` — against this repository's own manifests, the same
# functions the canonical toolchain gate itself runs. Nothing here re-parses
# TOML by hand, so a drift between the pin and the declared `rust-version`
# fails for the same reason the gate would fail it.

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  RUNLIB="${REPO_ROOT}/scripts/runlib.sh"
}

@test "the repository root pins an exact rust-toolchain.toml channel" {
  run bash -c '
    source "'"${RUNLIB}"'"
    pin="$(_runlib_pinned_channel "'"${REPO_ROOT}"'")"
    [ -n "$pin" ] || { echo "no channel pinned"; exit 1; }
    _runlib_is_exact_version "$pin" || { echo "channel '"'"'$pin'"'"' is not an exact version"; exit 1; }
    printf "%s" "$pin"
  '
  [ "$status" -eq 0 ]
}

@test "neat-core resolves a non-empty rust-version" {
  run bash -c '
    source "'"${RUNLIB}"'"
    value="$(_runlib_crate_field "'"${REPO_ROOT}"'/neat-core/Cargo.toml" "'"${REPO_ROOT}"'/Cargo.toml" rust-version)"
    [ -n "$value" ] || { echo "neat-core declares no rust-version"; exit 1; }
    printf "%s" "$value"
  '
  [ "$status" -eq 0 ]
}

@test "neat-core's declared rust-version matches the rust-toolchain.toml pin" {
  run bash -c '
    source "'"${RUNLIB}"'"
    pin="$(_runlib_pinned_channel "'"${REPO_ROOT}"'")"
    value="$(_runlib_crate_field "'"${REPO_ROOT}"'/neat-core/Cargo.toml" "'"${REPO_ROOT}"'/Cargo.toml" rust-version)"
    [ "$pin" = "$value" ] || {
      echo "rust-toolchain.toml pins '"'"'$pin'"'"' but neat-core declares rust-version '"'"'$value'"'"'"
      exit 1
    }
  '
  [ "$status" -eq 0 ]
}
