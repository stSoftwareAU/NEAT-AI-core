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

# The usage error is reported before the toolchain probe, so the exit status is
# 2 whether or not deno is installed on the runner.
@test "a non-existent root directory is rejected" {
  run env PATH=/nonexistent "$BASH" "$SCRIPT" "$WORK/does-not-exist"
  [ "$status" -eq 2 ]
  [[ "$output" == *"not a directory"* ]]
}

@test "the repository's own TypeScript sources pass the gate" {
  require_deno
  run "$SCRIPT" "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

# Issue #608 — `find "$root"` parses a `-`-prefixed root as find's own option.
# `[ -d "-P" ]` is true whenever a directory named `-P` exists in the cwd, so the
# existence test alone let such a value through and find walked the cwd instead
# of the named tree — silently checking the wrong files.

@test "an option-shaped root directory checks the named tree, not the cwd" {
  require_deno
  # A broken file in the cwd, which find would reach if the root were optioned
  # away, and a clean tree in the directory actually named on the command line.
  mkdir -p -- "-P"
  printf 'const x: number = "not a number";\n' >broken.ts
  printf 'export const ok: number = 1;\n' >"./-P/good.ts"

  run "$SCRIPT" "-P"
  [ "$status" -eq 0 ]
  [[ "$output" == *"checking 1 TypeScript file"* ]]
}

@test "an option-shaped root directory that does not exist is rejected" {
  run "$SCRIPT" "-P"
  [ "$status" -eq 2 ]
  [[ "$output" == *"not a directory"* ]]
}
