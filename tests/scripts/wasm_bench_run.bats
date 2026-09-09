#!/usr/bin/env bats
# Tests for wasm-bench/run.sh argument validation (Issue #608).
#
# `SAMPLES` / `SHAPE` / `RECORDS` / `SESSIONS` are forwarded to
# `node runner.mjs` and `SHAPE` also builds the `results/shape<N>.csv` path, so
# each must be a plain non-negative integer. These are "what" tests: they run
# the real script and assert on its exit status, its message, and the absence of
# any file it would otherwise have written.
#
# The guard runs before the cargo/node toolchain check, so every case here runs
# on a host with neither installed — PATH is emptied to prove it.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/wasm-bench/run.sh"
  RESULTS="${REPO_ROOT}/wasm-bench/results"
  # A throwaway HOME so the script cannot source a real ~/.cargo/env, and an
  # empty PATH so cargo and node are unreachable.
  FAKE_HOME="${BATS_TEST_TMPDIR}/home"
  EMPTY_BIN="${BATS_TEST_TMPDIR}/empty-bin"
  mkdir -p "$FAKE_HOME" "$EMPTY_BIN"
  # Absolute, because `env -i` clears PATH before it resolves the interpreter.
  BASH_BIN="$(command -v bash)"
}

# run_toolchainless <arg>… — run run.sh with no cargo, no node and no real HOME.
run_toolchainless() {
  run env -i HOME="$FAKE_HOME" PATH="$EMPTY_BIN" "$BASH_BIN" "$SCRIPT" "$@"
}

@test "a non-integer samples argument is rejected without cargo or node" {
  run_toolchainless "not-a-number"
  [ "$status" -eq 2 ]
  [[ "$output" == *"samples"* ]]
  [[ "$output" == *"non-negative integer"* ]]
}

@test "an option-shaped samples argument is rejected" {
  run_toolchainless "--help"
  [ "$status" -eq 2 ]
  [[ "$output" == *"non-negative integer"* ]]
}

@test "a path-bearing shape index cannot escape the results directory" {
  local leak="${BATS_TEST_TMPDIR}/leak.csv"
  run_toolchainless 1 "../../../..${leak}" 8 1
  [ "$status" -eq 2 ]
  [[ "$output" == *"shape"* ]]
  [ ! -e "$leak" ]
}

@test "a non-integer records argument is rejected" {
  run_toolchainless 1 5 "4096; rm -rf /" 1
  [ "$status" -eq 2 ]
  [[ "$output" == *"records"* ]]
}

@test "a non-integer sessions argument is rejected" {
  run_toolchainless 1 5 8 "3.5"
  [ "$status" -eq 2 ]
  [[ "$output" == *"sessions"* ]]
}

@test "valid integer arguments pass the guard and reach the toolchain check" {
  run_toolchainless 1 5 8 1
  # No cargo on PATH, so the script must fail at the toolchain check — proving
  # the guard accepted every value rather than rejecting a legitimate one.
  [ "$status" -eq 1 ]
  [[ "$output" == *"cargo is required"* ]]
  [ ! -e "${RESULTS}/shape5.csv" ]
}
