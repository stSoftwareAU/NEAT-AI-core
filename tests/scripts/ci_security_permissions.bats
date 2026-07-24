#!/usr/bin/env bats
# Tests for the CI `security` job least-privilege token scopes (Issue #312).
#
# Contract under test: the `security` job in .github/workflows/ci.yml calls the
# reusable workflow .github/workflows/security.yml. A caller job with no
# `permissions:` block hands the called workflow the repository's broad default
# GITHUB_TOKEN scopes, so the caller must declare the least-privilege set the
# called workflow actually needs: `contents: read` (checkout, cargo audit) plus
# `pull-requests: write` (dependency-review PR comment summary).
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  CALLED="${REPO_ROOT}/.github/workflows/security.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "security job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["security"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "security job has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "security job grants only the scopes the reusable workflow needs" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["security"]
perms = job.get("permissions") or data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
# Least privilege: dependency-review's PR comment is the only write needed.
writes = sorted(k for k, v in perms.items() if v == "write")
assert writes == ["pull-requests"], f"unexpected write scopes on security job: {writes}"
PY
  [ "$status" -eq 0 ]
}

@test "caller scopes cover every scope the called workflow declares" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
caller = yaml.safe_load(open("$WORKFLOW"))["jobs"]["security"]
called = yaml.safe_load(open("$CALLED"))["jobs"]["security"]
caller_perms = caller.get("permissions") or {}
called_perms = called.get("permissions") or {}
rank = {"none": 0, "read": 1, "write": 2}
for scope, level in called_perms.items():
    got = caller_perms.get(scope, "none")
    assert rank[got] >= rank[level], (
        f"caller grants {scope}: {got}, called workflow needs {level}"
    )
PY
  [ "$status" -eq 0 ]
}
