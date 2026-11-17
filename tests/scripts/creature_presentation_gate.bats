#!/usr/bin/env bats
# Pins that the persisted creature JSON presentation corpus gate (Issue #765)
# runs from both quality.sh and the CI wasm64-memory64-smoke job, with read
# permission only, and that neither run can be disabled, made conditional or
# allowed to fail silently.
#
# The two checkers are defined once (setup) and used on the real files and on
# deliberately broken temp copies, so they are shown to be able to fail. Every
# refusal test asserts the checker's specific message, so a YAML parse error or
# a traceback cannot satisfy it.

load helpers

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  QUALITY="${REPO_ROOT}/quality.sh"
  TMP="$(mktemp -d)"

  export WORKFLOW_CHECK_PY='
import sys, yaml
data = yaml.safe_load(open(sys.argv[1]))
job = data["jobs"]["wasm64-memory64-smoke"]
steps = job["steps"]
target = "tests/creature_presentation_test.ts"
gate = ["deno", "test", "--allow-read", target, "<", "/dev/null"]
# The draft-skip every step of this job inherits (\x27 is a single quote).
draft_skip = "github.event_name != \x27pull_request\x27 || !github.event.pull_request.draft"
if job.get("if") != draft_skip:
    sys.exit("job condition differs from the draft-skip condition: %r" % job.get("if"))
if "continue-on-error" in job:
    sys.exit("job sets continue-on-error: %r" % job["continue-on-error"])
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
    if tokens != gate:
        if tokens[:len(gate)] == gate:
            sys.exit("run has trailing tokens after the gate command: %r" % tokens[len(gate):])
        sys.exit("run is not exactly the gate command: %r" % run)
    if "if" in step:
        sys.exit("step is conditional: %r" % step["if"])
    if "continue-on-error" in step:
        sys.exit("step sets continue-on-error: %r" % step["continue-on-error"])
    found = True
if not found:
    sys.exit("no deno test step for " + target)
'
  export QUALITY_CHECK_PY='
import re, sys
want = ["deno", "test", "--allow-read", "tests/creature_presentation_test.ts", "</dev/null"]
exited = False
for line in open(sys.argv[1]):
    if re.match(r"exit\b", line):
        exited = True
    if line.split() == want:
        if line[:1].isspace():
            sys.exit("gate line is indented, not at column 0: %r" % line.rstrip())
        if exited:
            sys.exit("column-0 exit precedes the gate line")
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
  [[ "$output" == *"no deno test step for tests/creature_presentation_test.ts"* ]]
}

@test "ci.yml gate step with --allow-all fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-all tests/creature_presentation_test.ts|' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q 'deno test --allow-all tests/creature_presentation_test.ts' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"permissions not exactly --allow-read"* ]]
}

@test "ci.yml gate step with an extra --allow-write fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-read --allow-write tests/creature_presentation_test.ts|' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q 'deno test --allow-read --allow-write tests/creature_presentation_test.ts' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"permissions not exactly --allow-read"* ]]
}

@test "ci.yml gate step with an if condition fails the check" {
  require_python3
  awk '{print} $0 == "      - name: Run the persisted creature JSON presentation corpus tests" {print "        if: false"}' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q '^        if: false$' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"step is conditional"* ]]
}

@test "ci.yml gate step with || true appended fails the check" {
  require_python3
  sed 's#creature_presentation_test.ts < /dev/null$#creature_presentation_test.ts < /dev/null || true#' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q 'creature_presentation_test.ts < /dev/null || true$' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"run has trailing tokens after the gate command"* ]]
}

@test "ci.yml gate step with continue-on-error fails the check" {
  require_python3
  awk '{print} $0 == "      - name: Run the persisted creature JSON presentation corpus tests" {print "        continue-on-error: true"}' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q '^        continue-on-error: true$' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"step sets continue-on-error"* ]]
}

@test "ci.yml job with its draft-skip if replaced by if: false fails the check" {
  require_python3
  awk '$0 == "  wasm64-memory64-smoke:" {injob = 1} injob && /^    if: / {print "    if: false"; injob = 0; next} {print}' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q '^    if: false$' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"job condition differs from the draft-skip condition"* ]]
}

@test "ci.yml job with continue-on-error fails the check" {
  require_python3
  awk '{print} $0 == "  wasm64-memory64-smoke:" {print "    continue-on-error: true"}' "$WORKFLOW" > "$TMP/ci.yml"
  grep -q '^    continue-on-error: true$' "$TMP/ci.yml"
  run check_workflow "$TMP/ci.yml"
  [ "$status" -ne 0 ]
  [[ "$output" == *"job sets continue-on-error"* ]]
}

@test "quality.sh with the gate line commented out fails the check" {
  require_python3
  sed 's|^deno test --allow-read tests/creature_presentation_test.ts|# &|' "$QUALITY" > "$TMP/quality.sh"
  grep -q '^# deno test --allow-read tests/creature_presentation_test.ts' "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"no uncommented exact deno test line"* ]]
}

@test "quality.sh without the gate line fails the check" {
  require_python3
  grep -v 'tests/creature_presentation_test.ts' "$QUALITY" > "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"no uncommented exact deno test line"* ]]
}

@test "quality.sh gate line with --allow-write fails the check" {
  require_python3
  sed 's|deno test --allow-read tests/creature_presentation_test.ts|deno test --allow-read --allow-write tests/creature_presentation_test.ts|' "$QUALITY" > "$TMP/quality.sh"
  grep -q 'deno test --allow-read --allow-write tests/creature_presentation_test.ts' "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"no uncommented exact deno test line"* ]]
}

@test "quality.sh gate line indented inside an if-false block fails the check" {
  require_python3
  awk '$0 == "deno test --allow-read tests/creature_presentation_test.ts </dev/null" {print "if false; then"; print "  " $0; print "fi"; next} {print}' "$QUALITY" > "$TMP/quality.sh"
  grep -q '^  deno test --allow-read tests/creature_presentation_test.ts </dev/null$' "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"gate line is indented, not at column 0"* ]]
}

@test "quality.sh with a column-0 exit before the gate line fails the check" {
  require_python3
  awk '$0 == "deno test --allow-read tests/creature_presentation_test.ts </dev/null" {print "exit 0"} {print}' "$QUALITY" > "$TMP/quality.sh"
  grep -q '^exit 0$' "$TMP/quality.sh"
  run check_quality "$TMP/quality.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"column-0 exit precedes the gate line"* ]]
}
