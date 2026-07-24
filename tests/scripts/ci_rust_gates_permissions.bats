#!/usr/bin/env bats
# Tests for the CI `rust-gates` job least-privilege token scopes (Issue #310).
#
# Contract under test: the `rust-gates` job in .github/workflows/ci.yml must
# declare an explicit `permissions:` block so it does not silently inherit the
# repository's broad default GITHUB_TOKEN scopes. The job only reads the repo
# (checkout + cargo check/clippy), so `contents: read` is the correct
# least-privilege grant.
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "rust-gates job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["rust-gates"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "rust-gates job has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "rust-gates job grants contents: read (least privilege, no write)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["rust-gates"]
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
# Least privilege: no scope may be granted write on the rust-gates (read-only) job.
writes = [k for k, v in perms.items() if v == "write"]
assert not writes, f"rust-gates job must not hold write scopes: {writes}"
PY
  [ "$status" -eq 0 ]
}
