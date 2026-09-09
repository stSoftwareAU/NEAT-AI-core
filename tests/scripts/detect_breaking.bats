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

# Issue #608 — `range="$1"` was passed straight to `git log`, so a `-`-prefixed
# value was parsed as a git option: `--output=<path>` wrote a file of the
# caller's choosing, and `--all` widened the scan past the range asked about.

@test "an option-shaped range is rejected and writes no file" {
  local leak="${WORK}/leak.txt"
  run "$SCRIPT" "--output=${leak}"
  [ "$status" -ne 0 ]
  [ ! -e "$leak" ]
}

@test "an option-shaped range cannot widen the scan to the whole history" {
  # A breaking commit outside any range a caller would ask about.
  git commit -q --allow-empty -m "feat!: unrelated breaking change"
  run "$SCRIPT" "--all"
  [ "$status" -ne 0 ]
  [ "$output" != "true" ]
}
