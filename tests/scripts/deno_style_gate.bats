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
  run bash -c "cd '$REPO_ROOT' && deno lint --quiet </dev/null"
  [ "$status" -eq 0 ]
}

@test "deno fmt --check reports no unformatted files in this repository" {
  require_deno
  run bash -c "cd '$REPO_ROOT' && deno fmt --check --quiet </dev/null"
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

# scripts/build-wasm-bundle.sh writes wasm-pack output to
# neat-core/wasm_activation/pkg/, which .gitignore does not cover. Unfenced, the
# generated .d.ts made ./quality.sh red for every contributor who had built the
# bundle — a gate failing on code nobody wrote.
@test "generated wasm-pack output does not fail the style gate" {
  require_deno
  local pkg="${REPO_ROOT}/neat-core/wasm_activation/pkg"
  [ ! -e "$pkg" ] || skip "a real wasm-pack build is present; refusing to touch it"
  mkdir -p "$pkg"
  printf 'export function foo( a:number ):number\n' >"$pkg/neat_core.d.ts"

  run bash -c "cd '$REPO_ROOT' && deno fmt --check </dev/null"
  local fmt_status="$status"
  local fmt_output="$output"
  run bash -c "cd '$REPO_ROOT' && deno lint --quiet </dev/null"
  local lint_status="$status"
  local lint_output="$output"
  rm -rf "${REPO_ROOT}/neat-core/wasm_activation"

  [ "$fmt_status" -eq 0 ] || { echo "$fmt_output"; false; }
  [ "$lint_status" -eq 0 ] || { echo "$lint_output"; false; }
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
# The block is extracted from quality.sh and executed, so these assert on what
# the gate does rather than on the text it is written in.

# Extract the delimited block from quality.sh into $WORK/gate.sh, wrapped in the
# same `set -euo pipefail` quality.sh itself runs under.
extract_local_gate() {
  awk '/^# >>> deno-style-gate/{f=1;next} /^# <<< deno-style-gate/{f=0} f' \
    "$QUALITY" >"${WORK}/body.sh"
  if [ ! -s "${WORK}/body.sh" ]; then
    echo "quality.sh has no '# >>> deno-style-gate' … '# <<< deno-style-gate' block" >&2
    return 1
  fi
  {
    echo 'set -euo pipefail'
    cat "${WORK}/body.sh"
  } >"${WORK}/gate.sh"
}

# A stand-in for deno, so the gate is observed without a real lint/fmt run.
# It records each invocation and reports the status the caller chose per
# subcommand.
stub_deno() {
  mkdir -p "${WORK}/shim"
  cat >"${WORK}/shim/deno" <<'SH'
#!/usr/bin/env bash
printf 'deno %s
' "$*" >>"$GATE_STUB_LOG"
case "$1" in
  lint) exit "${GATE_STUB_EXIT_LINT:-0}" ;;
  fmt) exit "${GATE_STUB_EXIT_FMT:-0}" ;;
  *) exit 0 ;;
esac
SH
  chmod +x "${WORK}/shim/deno"
}

# Run the extracted gate with the stub deno on PATH. $1/$2 are the statuses the
# stubbed `deno lint` and `deno fmt` report (default 0).
run_local_gate() {
  extract_local_gate || return 1
  stub_deno
  : >"${WORK}/deno-invocations.log"
  run env PATH="${WORK}/shim:${PATH}" \
    GATE_STUB_LOG="${WORK}/deno-invocations.log" \
    GATE_STUB_EXIT_LINT="${1:-0}" GATE_STUB_EXIT_FMT="${2:-0}" \
    "$BASH" "${WORK}/gate.sh"
}

@test "quality.sh runs deno lint and deno fmt --check" {
  run_local_gate
  [ "$status" -eq 0 ]
  run cat "${WORK}/deno-invocations.log"
  [[ "$output" == *"deno lint"* ]]
  [[ "$output" == *"deno fmt --check"* ]]
}

@test "a failing deno lint fails quality.sh instead of falling through to fmt" {
  run_local_gate 1 0
  [ "$status" -ne 0 ]
  run cat "${WORK}/deno-invocations.log"
  [[ "$output" != *"deno fmt"* ]]
}

@test "a failing deno fmt --check fails quality.sh" {
  run_local_gate 0 1
  [ "$status" -ne 0 ]
}
