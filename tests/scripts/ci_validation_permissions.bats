#!/usr/bin/env bats
# Tests for the CI `validation` job least-privilege token scopes (Issue #313).
#
# Contract under test: the `validation` job in .github/workflows/ci.yml only
# reads the repository (checkout, cargo metadata, grep/wc file checks). With no
# `permissions:` block it inherits the repository's broad default GITHUB_TOKEN
# scopes, so it must declare the least-privilege set it actually needs:
# `contents: read` and no write scopes at all.
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "validation job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["validation"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "validation job has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "validation job grants read-only contents and no write scopes" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["validation"]
perms = job.get("permissions") or data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
writes = sorted(k for k, v in perms.items() if v == "write")
assert writes == [], f"validation is read-only; unexpected write scopes: {writes}"
PY
  [ "$status" -eq 0 ]
}
