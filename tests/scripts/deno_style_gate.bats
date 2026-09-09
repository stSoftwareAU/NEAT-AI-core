#!/usr/bin/env bats
# Tests for the Deno style gate — `deno lint` and `deno fmt --check` over the
# committed TypeScript/JavaScript sources (Issue #647).
#
# The repository already type-checks its `.ts` helpers (`typescript-check.sh`,
# Issue #307), but nothing linted or format-checked them, so an unformatted or
# lint-broken helper landed unnoticed.
#
# "What" tests: each one drives a real `deno` subprocess over the committed
# tree or over a throwaway tree built from the committed config, and asserts on
# the exit status and the diagnostic Deno actually reports.

load helpers

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  QUALITY="${REPO_ROOT}/quality.sh"
  WORK="$(mktemp -d)"
}

teardown() {
  rm -rf "$WORK"
}

require_deno() {
  if ! command -v deno &>/dev/null; then
    skip "deno not installed"
  fi
}

@test "deno lint reports no problems in this repository" {
  require_deno
  run deno lint --quiet </dev/null
  [ "$status" -eq 0 ]
}

@test "deno fmt --check reports no unformatted files in this repository" {
  require_deno
  run deno fmt --check --quiet </dev/null
  [ "$status" -eq 0 ]
}

# The fmt gate must own JavaScript/TypeScript only: Markdown already has an
# implementation owner in markdownlint-cli2 (.markdownlint-cli2.jsonc), and two
# formatters over the same files would each undo the other's output.
@test "the committed fmt config checks TypeScript and leaves Markdown to markdownlint" {
  require_deno
  cp "${REPO_ROOT}/deno.json" "${REPO_ROOT}/deno.lock" "$WORK/"
  printf 'export const answer=42\n' >"$WORK/sample.ts"
  printf '# Heading\n\n\n\nloose    prose\n' >"$WORK/notes.md"

  # No path argument: an explicit CLI path overrides `fmt.include`, which is the
  # very fence under test, so the gate must discover the files itself.
  run bash -c "cd '$WORK' && deno fmt --check </dev/null"
  [ "$status" -ne 0 ]
  [[ "$output" == *"sample.ts"* ]]
  [[ "$output" != *"notes.md"* ]]
}

@test "the CI typescript-gate job runs deno lint and deno fmt --check" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
steps = data["jobs"]["typescript-gate"]["steps"]
bodies = "\n".join(s.get("run") or "" for s in steps)
for command in ("deno lint", "deno fmt --check"):
    assert command in bodies, f"typescript-gate never runs {command!r}"
PY
  [ "$status" -eq 0 ]
}

# The local gate and the CI gate must agree: a contributor who runs
# ./quality.sh should not discover the style failure only on the pull request.
@test "quality.sh runs the same deno lint and fmt gate as CI" {
  run grep -E '^[[:space:]]*deno lint' "$QUALITY"
  [ "$status" -eq 0 ]
  run grep -E '^[[:space:]]*deno fmt --check' "$QUALITY"
  [ "$status" -eq 0 ]
}
