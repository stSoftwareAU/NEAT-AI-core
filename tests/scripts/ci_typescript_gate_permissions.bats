#!/usr/bin/env bats
# Tests for the CI `typescript-gate` job hardening (Issue #482).
#
# Contract under test: the `typescript-gate` job in .github/workflows/ci.yml
# only reads the repository (checkout, install Deno, run
# scripts/typescript-check.sh). With no `permissions:` block it inherits the
# repository's broad default GITHUB_TOKEN scopes, and a default checkout writes
# that token into .git/config where every later step can read it. The job never
# writes, so both are pure blast radius.
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow defines a typescript-gate job" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
assert "typescript-gate" in data["jobs"], sorted(data["jobs"])
PY
  [ "$status" -eq 0 ]
}

@test "typescript-gate job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["typescript-gate"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "typescript-gate has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "typescript-gate job grants read-only contents and no write scopes" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["typescript-gate"]
perms = job.get("permissions") or data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
writes = sorted(k for k, v in perms.items() if v == "write")
assert writes == [], f"typescript-gate is read-only; unexpected write scopes: {writes}"
PY
  [ "$status" -eq 0 ]
}

@test "typescript-gate checkout does not persist credentials on disk" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
checkouts = [
    s for s in data["jobs"]["typescript-gate"]["steps"]
    if str(s.get("uses", "")).startswith("actions/checkout@")
]
assert checkouts, "no actions/checkout step found in typescript-gate"
for step in checkouts:
    with_ = step.get("with") or {}
    assert with_.get("persist-credentials") is False, (
        f"checkout persists credentials: {step}"
    )
PY
  [ "$status" -eq 0 ]
}
