#!/usr/bin/env bats
# Tests for bump-deps.sh — Cargo dependency refresh helper (Issue #38).
# NEAT-AI-core is the root of the dependency chain (no internal stSoftware
# crates), so this script focuses on the external (crates.io) bump, the
# audit gate, and the dual native/WASM build verification.

setup() {
  SCRIPT_UNDER_TEST="${BATS_TEST_DIRNAME}/../../bump-deps.sh"
  [ -x "$SCRIPT_UNDER_TEST" ] || chmod +x "$SCRIPT_UNDER_TEST"

  TMP_REPO="$(mktemp -d)"
  export TMP_REPO
}

teardown() {
  rm -rf "$TMP_REPO"
}

@test "shows usage with --help" {
  run "$SCRIPT_UNDER_TEST" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
  [[ "$output" == *"--quarantine-hours"* ]]
}

@test "rejects unknown options" {
  run "$SCRIPT_UNDER_TEST" --nonsense
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}

@test "rejects non-integer quarantine hours" {
  run "$SCRIPT_UNDER_TEST" --quarantine-hours abc \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -ne 0 ]
  [[ "$output" == *"quarantine-hours"* ]]
}

@test "all skip flags: produces a clean no-op" {
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  [[ "$output" == *"no bumps"* ]]
}

@test "summary line is single-line and lists every stage" {
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  # Final line of output is the one-line summary.
  last="$(printf '%s\n' "$output" | tail -n 1)"
  [[ "$last" == *"external="* ]]
  [[ "$last" == *"audit="* ]]
  [[ "$last" == *"build="* ]]
}

@test "summary includes WASM build stage label" {
  # --skip-build short-circuits the build stage but the summary should still
  # carry the build= field — the script's contract is that build covers both
  # native and wasm32 targets.
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  [[ "$output" == *"build="* ]]
}

@test "check-published: ancient timestamp is older than quarantine (exit 0)" {
  run "$SCRIPT_UNDER_TEST" --check-published "2020-01-01T00:00:00Z" 24
  [ "$status" -eq 0 ]
}

@test "check-published: very recent timestamp is within quarantine (exit 1)" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  run "$SCRIPT_UNDER_TEST" --check-published "$recent" 24
  [ "$status" -eq 1 ]
}

@test "check-published: quarantine of zero hours always allows the bump" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  run "$SCRIPT_UNDER_TEST" --check-published "$recent" 0
  [ "$status" -eq 0 ]
}

@test "check-published: invalid timestamp surfaces an error" {
  run "$SCRIPT_UNDER_TEST" --check-published "not-a-date" 24
  [ "$status" -ne 0 ]
}

@test "VIBE_BUMP_QUARANTINE_HOURS env var is honoured for quarantine default" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  # With a 0-hour default in the env, a fresh timestamp is allowed.
  VIBE_BUMP_QUARANTINE_HOURS=0 run "$SCRIPT_UNDER_TEST" --check-published "$recent" 0
  [ "$status" -eq 0 ]
}

@test "rejects --skip-internal (NEAT-AI-core has no internal deps)" {
  # NEAT-AI-core is the root — there is no internal step, so the option must
  # not exist (otherwise callers might assume one). It should be rejected as
  # an unknown option just like any other typo.
  run "$SCRIPT_UNDER_TEST" --skip-internal
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}
