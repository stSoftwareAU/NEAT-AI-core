#!/usr/bin/env bats
# Least-privilege GITHUB_TOKEN scopes for every gated ci.yml job — one
# data-driven file replacing the six near-identical ci_*_permissions.bats
# copies (Issue #477, consolidating Issues #309, #310, #311, #312, #313, #331).
#
# Contract under test: a job with no `permissions:` block silently inherits the
# repository's broad default GITHUB_TOKEN scopes. Every job below must declare
# the least-privilege set it actually needs — `contents: read`, plus only the
# write scopes named in the table.
#
# Adding a gated job means adding one table row here, not copying a file.
#
# Asserts on the observable CI contract, not private implementation detail.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  CALLED="${REPO_ROOT}/.github/workflows/security.yml"
}

# Read-only jobs: checkout plus read-only checks, no push, so no write scope at
# all. quality (#309), rust-gates (#310), scripts-and-spelling (#311),
# validation (#313), wasm64-memory64-smoke (#331).
READ_ONLY_JOBS=(
  quality
  rust-gates
  scripts-and-spelling
  validation
  wasm64-memory64-smoke
)

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "every read-only ci job grants contents: read and holds no write scope" {
  require_python3
  local failures=0
  for job in "${READ_ONLY_JOBS[@]}"; do
    run assert_job_least_privilege "$WORKFLOW" "$job"
    if [ "$status" -ne 0 ]; then
      echo "$job: $output"
      failures=1
    fi
  done
  [ "$failures" -eq 0 ]
}

# Issue #312 — the `security` job calls the reusable .github/workflows/
# security.yml. A caller with no permissions block hands the called workflow the
# repository's broad defaults, so it must declare exactly what the callee needs:
# contents: read (checkout, cargo audit) plus pull-requests: write (the
# dependency-review PR comment summary).
@test "security caller job grants contents read plus only pull-requests write" {
  require_python3
  run assert_job_least_privilege "$WORKFLOW" security pull-requests
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "caller scopes cover every scope the called workflow declares" {
  require_python3
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
