#!/usr/bin/env bats
# Pins that the persisted creature JSON presentation corpus gate (Issue #765)
# runs from both quality.sh and the CI wasm64-memory64-smoke job, with read
# permission only and no condition on the CI step.
#
# The two checkers are defined once (setup) and used on the real files and on
# deliberately broken temp copies, so they are shown to be able to fail.

load helpers

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  QUALITY="${REPO_ROOT}/quality.sh"
  TMP="$(mktemp -d)"

  export WORKFLOW_CHECK_PY='
import sys, yaml
data = yaml.safe_load(open(sys.argv[1]))
steps = data["jobs"]["wasm64-memory64-smoke"]["steps"]
target = "tests/creature_presentation_test.ts"
found = False
for step in steps:
    run = step.get("run")
    if not run:
        continue
    tokens = run.split()
    if tokens[:2] != ["deno", "test"] or target not in tokens:
        continue
    perms = [t for t in tokens if t == "-A" or t.startswith("--allow-")]
    if perms != ["--allow-read"]:
        sys.exit("permissions not exactly --allow-read: %r" % perms)
    if "if" in step:
        sys.exit("step is conditional: %r" % step["if"])
    found = True
if not found:
    sys.exit("no deno test step for " + target)
'
  export QUALITY_CHECK_PY='
import re, sys
want = ["deno", "test", "--allow-read", "tests/creature_presentation_test.ts", "</dev/null"]
for line in open(sys.argv[1]):
    if line.split() == want:
        sys.exit(0)
sys.exit("no uncommented exact deno test line")
'
}

teardown() {
  rm -rf "$TMP"
}

check_workflow() { python3 -c "$WORKFLOW_CHECK_PY" "$1"; }
check_quality() { python3 -c "$QUALITY_CHECK_PY" "$1"; }

@test "ci.yml runs the presentation corpus gate with --allow-read only" {
  require_python3
  run check_workflow "$WORKFLOW"
  [ "$status" -eq 0 ]
}

@test "quality.sh runs the presentation corpus gate with --allow-read only" {
  require_python3
  run check_quality "$QUALITY"
  [ "$status" -eq 0 ]
}

@test "ci.yml without the gate step fails the check" {
  require_python3
  grep -v 'tests/creature_presentation_test.ts' "$WORKFLOW" > "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
}

@test "ci.yml gate step with --allow-all fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-all tests/creature_presentation_test.ts|' "$WORKFLOW" > "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
}

@test "ci.yml gate step with an extra --allow-write fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-read --allow-write tests/creature_presentation_test.ts|' "$WORKFLOW" > "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
}

@test "ci.yml gate step with an if condition fails the check" {
  require_python3
  sed 's|^\(      - name: Run the persisted creature JSON presentation corpus tests\)$|\1\n        if: false|' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q 'if: false' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
}

@test "quality.sh with the gate line commented out fails the check" {
  require_python3
  sed 's|^deno test --allow-read tests/creature_presentation_test.ts|# &|' "$QUALITY" > "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
}

@test "quality.sh without the gate line fails the check" {
  require_python3
  grep -v 'tests/creature_presentation_test.ts' "$QUALITY" > "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
}

@test "quality.sh gate line with --allow-write fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-read --allow-write tests/creature_presentation_test.ts|' "$QUALITY" > "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
}
