#!/usr/bin/env bats
# Tests for wasm-bench/run.sh argument validation (Issue #608).
#
# `SAMPLES` / `SHAPE` / `RECORDS` / `SESSIONS` are forwarded to
# `node runner.mjs` and `SHAPE` also builds the `results/shape<N>.csv` path, so
# each must be a plain non-negative integer. These are "what" tests: they run
# the real script and assert on the exit status and message it produces for a
# given argument.
#
# The guard runs before the cargo/node toolchain check, so every case here runs
# on a host with neither installed — PATH is emptied to prove it. That also
# means no case can reach a write: `mkdir -p results` sits after the toolchain
# check, so an "and no file was created" assertion here would be vacuous and is
# deliberately absent (AGENTS.md oracle rule 3). What discriminates a guarded
# script from an unguarded one is the exit status: 2 from the guard, 1 from the
# toolchain check the unguarded script falls through to.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/wasm-bench/run.sh"
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
  [[ "$output" == *"samples must be a non-negative integer (got 'not-a-number')"* ]]
}

@test "an option-shaped samples argument is rejected" {
  run_toolchainless "--help"
  [ "$status" -eq 2 ]
  [[ "$output" == *"samples must be a non-negative integer (got '--help')"* ]]
}

@test "a path-bearing shape index is rejected before it can build a CSV path" {
  # SHAPE builds `results/shape$SHAPE.csv`, so a traversal must die at the guard.
  run_toolchainless 1 "../../../../tmp/leak.csv" 8 1
  [ "$status" -eq 2 ]
  [[ "$output" == *"shape-index must be a non-negative integer"* ]]
}

@test "a non-integer records argument is rejected" {
  run_toolchainless 1 5 "4096; rm -rf /" 1
  [ "$status" -eq 2 ]
  [[ "$output" == *"records must be a non-negative integer (got '4096; rm -rf /')"* ]]
}

@test "a non-integer sessions argument is rejected" {
  run_toolchainless 1 5 8 "3.5"
  [ "$status" -eq 2 ]
  [[ "$output" == *"sessions must be a non-negative integer (got '3.5')"* ]]
}

@test "valid integer arguments are accepted on a host without cargo" {
  run_toolchainless 1 5 8 1
  # Exit 1 with the toolchain diagnostic, not exit 2 with a guard message: the
  # guard let all four legitimate values through. This is the negative control
  # for the five rejection cases above — without it, a guard that rejected
  # everything would satisfy them all.
  [ "$status" -eq 1 ]
  [[ "$output" == *"cargo is required"* ]]
  [[ "$output" != *"non-negative integer"* ]]
}
