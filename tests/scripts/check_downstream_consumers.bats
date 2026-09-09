#!/usr/bin/env bats
# Tests for scripts/check-downstream-consumers.sh (Issue #644).
#
# "What" tests: a stub `cargo` records the consumer it ran in, its arguments
# and RUSTFLAGS, and fails on demand; the script is driven in --workspace mode
# against throwaway sibling directories, so no network, toolchain or real
# consumer is needed. The last two tests read the committed registry and the
# CI wiring — the gate only protects consumers it knows about and only if it
# runs.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/check-downstream-consumers.sh"
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  RULESET="${REPO_ROOT}/.github/rulesets/develop.json"
  WORK="$(mktemp -d)"
  CORE="$WORK/NEAT-AI-core"
  mkdir -p "$CORE/neat-core"
  printf '[package]\nname = "neat-core"\n' >"$CORE/neat-core/Cargo.toml"
  BIN="$WORK/bin"
  mkdir -p "$BIN"
  CARGO_LOG="$WORK/cargo.log"
  cat >"$BIN/cargo" <<'STUB'
#!/bin/bash
# consumer|arguments|RUSTFLAGS — and a compile error for consumers named in
# FAKE_CARGO_FAIL (space-separated).
printf '%s|%s|%s\n' "$(basename "$PWD")" "$*" "${RUSTFLAGS-<unset>}" >>"$CARGO_LOG"
case " ${FAKE_CARGO_FAIL:-} " in
  *" $(basename "$PWD") "*)
    echo 'error[E0616]: field `neurons` of struct `CompiledNetwork` is private' >&2
    exit 101
    ;;
esac
exit 0
STUB
  chmod +x "$BIN/cargo"
  REGISTRY="$WORK/registry.txt"
  printf '# test registry\nstSoftwareAU/Alpha\nstSoftwareAU/Beta\n' >"$REGISTRY"
  for consumer in Alpha Beta; do
    mkdir -p "$WORK/$consumer"
    printf '[workspace]\nmembers = []\n' >"$WORK/$consumer/Cargo.toml"
  done
}

teardown() {
  rm -rf "$WORK"
}

# run_gate [VAR=value ...] — run the script in --workspace mode with the stub
# cargo first on PATH and any extra environment the test needs.
run_gate() {
  run env PATH="$BIN:$PATH" CARGO_LOG="$CARGO_LOG" "$@" \
    "$SCRIPT" --registry "$REGISTRY" --core "$CORE" --workspace "$WORK"
}

@test "every registered consumer is compiled in its own checkout and the gate passes" {
  run_gate
  [ "$status" -eq 0 ]
  [[ "$output" == *"✅ stSoftwareAU/Alpha compiles"* ]]
  [[ "$output" == *"✅ stSoftwareAU/Beta compiles"* ]]
  [[ "$output" == *"all 2 registered consumers compile"* ]]
  [ "$(wc -l <"$CARGO_LOG" | tr -d ' ')" -eq 2 ]
  grep -q '^Alpha|check --workspace --all-targets|' "$CARGO_LOG"
  grep -q '^Beta|check --workspace --all-targets|' "$CARGO_LOG"
}

@test "a consumer that stops compiling fails the gate by name and cites the three-phase flow" {
  run_gate FAKE_CARGO_FAIL=Beta
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Beta no longer compiles"* ]]
  [[ "$output" == *"E0616"* ]]
  [[ "$output" == *"1 of 2 registered consumers broke"* ]]
  [[ "$output" == *"three-phase flow"* ]]
  [[ "$output" == *"✅ stSoftwareAU/Alpha compiles"* ]]
}

@test "every consumer is checked even after the first one fails" {
  run_gate FAKE_CARGO_FAIL="Alpha Beta"
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Alpha no longer compiles"* ]]
  [[ "$output" == *"❌ stSoftwareAU/Beta no longer compiles"* ]]
  [[ "$output" == *"2 of 2 registered consumers broke"* ]]
  [ "$(wc -l <"$CARGO_LOG" | tr -d ' ')" -eq 2 ]
}

@test "RUSTFLAGS from the environment never reach a consumer build" {
  # -D warnings would turn a phase-1 #[deprecated], or a warning the consumer
  # already carries, into a failed gate; only "does not compile" may fail it.
  run_gate RUSTFLAGS="-D warnings"
  [ "$status" -eq 0 ]
  grep -q '|<unset>$' "$CARGO_LOG"
  ! grep -q -- '-D warnings' "$CARGO_LOG"
}

@test "a registered consumer missing from the workspace fails the gate by name" {
  rm -rf "$WORK/Beta"
  run_gate
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Beta: no checkout"* ]]
  [[ "$output" == *"missing from workspace"* ]]
  # The consumer that is present is still compiled.
  grep -q '^Alpha|' "$CARGO_LOG"
}

@test "a malformed registry line is a usage error" {
  printf 'stSoftwareAU/Alpha\nnot-a-repo\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"expected owner/name, got 'not-a-repo'"* ]]
  [ ! -f "$CARGO_LOG" ]
}

@test "an empty registry is a usage error, not a green gate" {
  printf '# nothing registered\n\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"lists no consumers"* ]]
}

@test "a duplicate registry entry is a usage error" {
  printf 'stSoftwareAU/Alpha\nstSoftwareAU/Alpha\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"duplicate entry"* ]]
}

@test "the workspace's NEAT-AI-core must be the core under test" {
  local other="$WORK/elsewhere/NEAT-AI-core"
  mkdir -p "$other/neat-core"
  printf '[package]\nname = "neat-core"\n' >"$other/neat-core/Cargo.toml"
  run env PATH="$BIN:$PATH" CARGO_LOG="$CARGO_LOG" \
    "$SCRIPT" --registry "$REGISTRY" --core "$other" --workspace "$WORK"
  [ "$status" -eq 2 ]
  [[ "$output" == *"must be the core under test"* ]]
  [ ! -f "$CARGO_LOG" ]
}

@test "--list validates the committed registry, which names every path-dependency consumer" {
  run "$SCRIPT" --list
  [ "$status" -eq 0 ]
  local line
  while IFS= read -r line; do
    [[ "$line" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]
  done <<<"$output"
  # The consumer whose break takes the production fleet down.
  [[ "$output" == *"stSoftwareAU/NEAT-AI-scorer"* ]]
}

@test "ci.yml runs the downstream gate unconditionally on every pull request, read-only" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = (data.get("jobs") or {}).get("downstream-consumers")
assert job is not None, "no downstream-consumers job"
assert "if" not in job, f"the gate must be unconditional, got if: {job['if']!r}"
runs = [s.get("run", "") for s in job.get("steps") or []]
assert any("check-downstream-consumers.sh" in r for r in runs), runs
print(job["name"])
PY
  [ "$status" -eq 0 ]
  run assert_job_least_privilege "$WORKFLOW" downstream-consumers
  [ "$status" -eq 0 ]
}

@test "the Develop ruleset requires the downstream gate" {
  require_python3
  run python3 - "$WORKFLOW" "$RULESET" <<'PY'
import json
import sys

import yaml

name = yaml.safe_load(open(sys.argv[1]))["jobs"]["downstream-consumers"]["name"]
ruleset = json.load(open(sys.argv[2]))
contexts = {
    c["context"]
    for rule in ruleset["rules"]
    if rule["type"] == "required_status_checks"
    for c in rule["parameters"]["required_status_checks"]
}
assert name in contexts, f"{name!r} is not a required check; required: {sorted(contexts)}"
PY
  [ "$status" -eq 0 ]
}
