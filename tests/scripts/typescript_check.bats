#!/usr/bin/env bats
# Tests for scripts/typescript-check.sh — the TypeScript basic-validity gate
# (Issue #307).
#
# These are "what" tests: each builds a throwaway tree of real .ts files, runs
# the gate over it, and asserts on the exit status the gate reports.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/typescript-check.sh"
  WORK="$(mktemp -d)"
  cd "$WORK"
}

teardown() {
  rm -rf "$WORK"
}

require_deno() {
  if ! command -v deno &>/dev/null; then
    skip "deno not installed"
  fi
}

@test "a valid TypeScript file passes the gate" {
  require_deno
  cat >good.ts <<'TS'
export function add(a: number, b: number): number {
  return a + b;
}
TS
  run "$SCRIPT" "$WORK"
  [ "$status" -eq 0 ]
}

@test "a syntax error fails the gate" {
  require_deno
  cat >broken.ts <<'TS'
export function broken(: number {
  return
TS
  run "$SCRIPT" "$WORK"
  [ "$status" -ne 0 ]
}

@test "a type error fails the gate" {
  require_deno
  cat >mistyped.ts <<'TS'
export const count: number = "not a number";
TS
  run "$SCRIPT" "$WORK"
  [ "$status" -ne 0 ]
}

@test "a tree with no TypeScript files passes and says so" {
  require_deno
  echo "not typescript" >notes.md
  run "$SCRIPT" "$WORK"
  [ "$status" -eq 0 ]
  [[ "$output" == *"no TypeScript files"* ]]
}

@test "build artefacts under target/ are not checked" {
  require_deno
  mkdir -p target/debug
  cat >target/debug/generated.ts <<'TS'
export function broken(: number {
TS
  run "$SCRIPT" "$WORK"
  [ "$status" -eq 0 ]
}

@test "a missing deno fails loud rather than skipping the check" {
  cat >good.ts <<'TS'
export const answer: number = 42;
TS
  run env PATH=/nonexistent "$BASH" "$SCRIPT" "$WORK"
  [ "$status" -ne 0 ]
  [[ "$output" == *"deno is required"* ]]
}

@test "a non-existent root directory is rejected" {
  run "$SCRIPT" "$WORK/does-not-exist"
  [ "$status" -ne 0 ]
}

@test "the repository's own TypeScript sources pass the gate" {
  require_deno
  run "$SCRIPT" "$REPO_ROOT"
  [ "$status" -eq 0 ]
}
