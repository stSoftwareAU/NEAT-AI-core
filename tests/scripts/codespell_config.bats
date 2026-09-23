#!/usr/bin/env bats
# Tests for .codespellrc (Issue #728).
#
# CI spell-checks a fresh checkout — tracked files only. A local run checks
# whatever sits on disk, so a generated tool cache in the worktree (the graft
# code-graph mirror ships a JSON index full of truncated identifiers) reddened
# the gate locally while every CI run stayed green. The skip list is what keeps
# the two file sets the same; these tests assert it skips the cache without
# going blind to real prose.

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  CONFIG="${REPO_ROOT}/.codespellrc"
}

@test "codespell config exists" {
  [ -f "$CONFIG" ]
}

@test "codespell skips the graft code-graph cache" {
  if ! command -v codespell &>/dev/null; then
    skip "codespell not installed locally"
  fi
  TMP="$(mktemp -d)"
  cp "$CONFIG" "$TMP/.codespellrc"
  mkdir -p "$TMP/graft/.cache"
  # Spliced from fragments: a literal misspelling in this file would make
  # codespell flag the test itself when it sweeps the repository.
  printf '%s\n' "err""o af""e earl""ies" > "$TMP/graft/.cache/ask-index.json"
  cd "$TMP"
  run codespell
  echo "$output"
  rm -rf "$TMP"
  [ "$status" -eq 0 ]
}

@test "codespell still catches a misspelling in a tracked file" {
  if ! command -v codespell &>/dev/null; then
    skip "codespell not installed locally"
  fi
  TMP="$(mktemp -d)"
  cp "$CONFIG" "$TMP/.codespellrc"
  # Spliced, as above, so the fixture typo does not redden the repository sweep.
  typo="rec""ie""ve"
  printf 'This sentence contains a %s typo.\n' "$typo" > "$TMP/notes.md"
  cd "$TMP"
  run codespell
  echo "$output"
  rm -rf "$TMP"
  [ "$status" -ne 0 ]
  [[ "$output" == *"$typo"* ]]
}

@test "codespell passes against the current tree" {
  if ! command -v codespell &>/dev/null; then
    skip "codespell not installed locally"
  fi
  cd "$REPO_ROOT"
  run codespell
  echo "$output"
  [ "$status" -eq 0 ]
}
