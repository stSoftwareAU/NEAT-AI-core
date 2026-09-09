#!/usr/bin/env bats
# Tests for the bats shell-harness gate inside quality.sh (Issue #631).
#
# That block is the only gate over the repository's shell scripts, so a missing
# `bats` binary or a missing/empty tests/scripts suite must fail quality.sh
# rather than warn and let it reach "✅ All quality checks passed!".
#
# These are "what" tests: each extracts the real block from quality.sh, runs it
# under the same `set -euo pipefail` quality.sh uses with the final success line
# appended, and asserts on the exit status and the output.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SUCCESS_LINE='✅ All quality checks passed!'
  WORK="$(mktemp -d)"

  # The gate body, delimited in quality.sh so the test runs the real code
  # rather than a paraphrase of it.
  awk '/^# >>> bats-gate/{f=1;next} /^# <<< bats-gate/{f=0} f' \
    "${REPO_ROOT}/quality.sh" >"${WORK}/body.sh"
  if [ ! -s "${WORK}/body.sh" ]; then
    echo "quality.sh has no '# >>> bats-gate' … '# <<< bats-gate' block" >&2
    return 1
  fi
  {
    echo 'set -euo pipefail'
    cat "${WORK}/body.sh"
    printf 'echo "%s"\n' "$SUCCESS_LINE"
  } >"${WORK}/gate.sh"

  # A stand-in for bats itself, so the suite-shaped cases exercise the gate
  # without recursing into a real bats run.
  STUB_LOG="${WORK}/bats-invocations.log"
  mkdir -p "${WORK}/shim"
  cat >"${WORK}/shim/bats" <<'SH'
#!/usr/bin/env bash
printf 'bats %s\n' "$*" >>"$GATE_STUB_LOG"
SH
  chmod +x "${WORK}/shim/bats"

  cd "$WORK"
}

teardown() {
  cd /
  rm -rf "$WORK"
}

# Run the extracted gate with the stub bats on PATH.
run_gate_with_stub_bats() {
  run env PATH="${WORK}/shim:${PATH}" GATE_STUB_LOG="$STUB_LOG" \
    "$BASH" "${WORK}/gate.sh"
}

@test "a missing bats fails the gate rather than skipping the shell tests" {
  mkdir -p tests/scripts
  echo '@test "example" { true; }' >tests/scripts/example.bats

  run env PATH=/nonexistent "$BASH" "${WORK}/gate.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"bats is required"* ]]
  [[ "$output" != *"$SUCCESS_LINE"* ]]
}

@test "the missing-bats message names how to install it" {
  run env PATH=/nonexistent "$BASH" "${WORK}/gate.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"bats-core"* || "$output" == *"install"* ]]
}

@test "a missing tests/scripts directory fails the gate rather than passing vacuously" {
  run_gate_with_stub_bats
  [ "$status" -ne 0 ]
  [[ "$output" == *"tests/scripts"* ]]
  [[ "$output" != *"$SUCCESS_LINE"* ]]
  [ ! -s "$STUB_LOG" ]
}

@test "an empty tests/scripts directory fails the gate" {
  mkdir -p tests/scripts
  echo "not a suite" >tests/scripts/README.md

  run_gate_with_stub_bats
  [ "$status" -ne 0 ]
  [[ "$output" == *"tests/scripts"* ]]
  [[ "$output" != *"$SUCCESS_LINE"* ]]
  [ ! -s "$STUB_LOG" ]
}

@test "a populated tests/scripts suite is run and the gate passes" {
  mkdir -p tests/scripts
  echo '@test "example" { true; }' >tests/scripts/example.bats

  run_gate_with_stub_bats
  [ "$status" -eq 0 ]
  [[ "$output" == *"$SUCCESS_LINE"* ]]
  grep -q 'bats tests/scripts' "$STUB_LOG"
}

@test "the repository's own shell-harness suite satisfies the gate" {
  cd "$REPO_ROOT"
  run env PATH="${WORK}/shim:${PATH}" GATE_STUB_LOG="$STUB_LOG" \
    "$BASH" "${WORK}/gate.sh"
  [ "$status" -eq 0 ]
  grep -q 'bats tests/scripts' "$STUB_LOG"
}
