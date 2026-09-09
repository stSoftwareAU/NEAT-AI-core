#!/usr/bin/env bats
# Tests for the bats shell-harness gate inside quality.sh (Issue #631).
#
# That block is the only gate over the *behaviour* of the repository's shell
# scripts — `bash -n` and shellcheck above it read them without running them —
# so a missing `bats` binary, a missing/empty tests/scripts suite, or a red
# suite must fail quality.sh rather than let it reach its success line.
#
# These are "what" tests: each extracts the real block from quality.sh, runs it
# under the same `set -euo pipefail` quality.sh uses with quality.sh's own final
# success line appended, and asserts on the exit status and the output.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORK="$(mktemp -d)"

  # quality.sh's real success line, so a reworded line cannot leave these
  # assertions passing against a stale copy.
  SUCCESS_LINE="$(sed -n 's/^echo "\(✅ .*\)"$/\1/p' "${REPO_ROOT}/quality.sh" | tail -1)"
  if [ -z "$SUCCESS_LINE" ]; then
    echo "quality.sh has no final '✅ …' success line to pin" >&2
    return 1
  fi

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
  # without recursing into a real bats run. GATE_STUB_EXIT chooses the status
  # the stubbed suite reports.
  STUB_LOG="${WORK}/bats-invocations.log"
  mkdir -p "${WORK}/shim"
  cat >"${WORK}/shim/bats" <<'SH'
#!/usr/bin/env bash
printf 'bats %s\n' "$*" >>"$GATE_STUB_LOG"
exit "${GATE_STUB_EXIT:-0}"
SH
  chmod +x "${WORK}/shim/bats"

  cd "$WORK" || return 1
}

teardown() {
  cd / || return 1
  rm -rf "$WORK"
}

# Run the extracted gate with the stub bats on PATH, reporting $1 (default 0)
# as the stubbed suite's exit status.
run_gate_with_stub_bats() {
  run env PATH="${WORK}/shim:${PATH}" GATE_STUB_LOG="$STUB_LOG" \
    GATE_STUB_EXIT="${1:-0}" "$BASH" "${WORK}/gate.sh"
}

# Populate the cwd with a suite the gate should accept.
write_suite() {
  mkdir -p tests/scripts
  echo '@test "example" { true; }' >tests/scripts/example.bats
}

@test "a missing bats fails the gate rather than skipping the shell tests" {
  write_suite

  run env PATH=/nonexistent "$BASH" "${WORK}/gate.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"bats is required"* ]]
  [[ "$output" != *"$SUCCESS_LINE"* ]]
}

@test "the missing-bats message names both documented install routes" {
  run env PATH=/nonexistent "$BASH" "${WORK}/gate.sh"
  [ "$status" -ne 0 ]
  [[ "$output" == *"bats-core"* ]]
  [[ "$output" == *"apt-get"* ]]
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

@test "a red suite fails the gate" {
  write_suite

  run_gate_with_stub_bats 1
  [ "$status" -ne 0 ]
  [[ "$output" != *"$SUCCESS_LINE"* ]]
}

@test "a populated tests/scripts suite is run and the gate passes" {
  write_suite

  run_gate_with_stub_bats
  [ "$status" -eq 0 ]
  [[ "$output" == *"$SUCCESS_LINE"* ]]
  grep -q 'bats tests/scripts' "$STUB_LOG"
}

@test "the repository's own shell-harness suite satisfies the gate" {
  cd "$REPO_ROOT" || return 1

  run_gate_with_stub_bats
  [ "$status" -eq 0 ]
  grep -q 'bats tests/scripts' "$STUB_LOG"
}
