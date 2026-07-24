#!/usr/bin/env bats
# Tests for the CI `scripts-and-spelling` job least-privilege token scopes
# (Issue #311).
#
# Contract under test: the `scripts-and-spelling` job in
# .github/workflows/ci.yml must declare an explicit `permissions:` block so it
# does not silently inherit the repository's broad default GITHUB_TOKEN scopes.
# The job only reads the repo (checkout + shellcheck/bash -n/codespell); it
# never pushes, so `contents: read` is the correct least-privilege grant.
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "scripts-and-spelling job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["scripts-and-spelling"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "scripts-and-spelling job has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "scripts-and-spelling job grants contents: read (least privilege, no write)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["scripts-and-spelling"]
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
# Least privilege: no scope may be granted write on this read-only job.
writes = [k for k, v in perms.items() if v == "write"]
assert not writes, f"scripts-and-spelling job must not hold write scopes: {writes}"
PY
  [ "$status" -eq 0 ]
}
