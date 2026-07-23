#!/usr/bin/env bats
# Tests for the CI `wasm64-memory64-smoke` job least-privilege token scopes
# (Issue #331).
#
# Contract under test: the `wasm64-memory64-smoke` job in
# .github/workflows/ci.yml only reads the repository (checkout, install Deno,
# run one committed smoke test). With no `permissions:` block it inherits the
# repository's broad default GITHUB_TOKEN scopes, so it must declare the
# least-privilege set it actually needs: `contents: read` and no write scopes.
#
# Asserts on the observable CI contract, not private implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "wasm64-memory64-smoke job declares an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["wasm64-memory64-smoke"]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, "wasm64-memory64-smoke has no permissions: block at job or workflow level"
PY
  [ "$status" -eq 0 ]
}

@test "wasm64-memory64-smoke job grants read-only contents and no write scopes" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["wasm64-memory64-smoke"]
perms = job.get("permissions") or data.get("permissions")
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", f"contents must be read, got {perms.get('contents')!r}"
writes = sorted(k for k, v in perms.items() if v == "write")
assert writes == [], f"wasm64-memory64-smoke is read-only; unexpected write scopes: {writes}"
PY
  [ "$status" -eq 0 ]
}
