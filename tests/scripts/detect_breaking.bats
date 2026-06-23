#!/usr/bin/env bats
# Tests for scripts/detect-breaking.sh (Issue #251).
#
# These are "what" tests: they build a throwaway git history and assert on the
# true/false the script prints for the commit range.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/detect-breaking.sh"
  WORK="$(mktemp -d)"
  cd "$WORK"
  git init -q
  git config user.email "t@example.com"
  git config user.name "Test"
  git commit -q --allow-empty -m "chore: base commit"
  BASE="$(git rev-parse HEAD)"
}

teardown() {
  rm -rf "$WORK"
}

@test "plain non-breaking commits report false" {
  git commit -q --allow-empty -m "feat: add a knob"
  git commit -q --allow-empty -m "fix: correct a typo"
  run "$SCRIPT" "${BASE}..HEAD"
  [ "$status" -eq 0 ]
  [ "$output" = "false" ]
}

@test "a type!: subject marker reports true" {
  git commit -q --allow-empty -m "perf(network)!: narrow from_index u32 -> u16"
  run "$SCRIPT" "${BASE}..HEAD"
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "a BREAKING CHANGE footer reports true" {
  printf 'feat: rework api\n\nBREAKING CHANGE: SynapseData layout changed\n' >msg.txt
  git commit -q --allow-empty -F msg.txt
  run "$SCRIPT" "${BASE}..HEAD"
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "missing range argument is rejected" {
  run "$SCRIPT"
  [ "$status" -ne 0 ]
}
