#!/usr/bin/env bats
# Tests for the Rust CI lint + compile gates (Issue #143).
#
# The contract under test IS the CI wiring: the issue requires that every push
# and every pull request run a Rust lint gate (cargo clippy) and a
# compile/syntax gate (cargo check / cargo build) so lint regressions and
# syntax errors fail the build. These are "what" tests in the sense allowed by
# AGENTS.md — they assert on the observable CI contract, not on private
# implementation detail.
#
# Asserts on observable outcomes:
#   - ci.yml parses as YAML,
#   - the workflow triggers on pushes to Develop AND on pull requests,
#   - a job invokes the clippy lint gate with `-D warnings`,
#   - a job invokes an explicit compile/syntax gate (cargo check / cargo build),
#   - the gate job runs on push (not restricted to pull_request only), so direct
#     pushes to Develop are gated too,
#   - third-party actions in the gate job are SHA-pinned (Issue #77).

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "ci workflow is valid YAML" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml,sys; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "ci workflow triggers on PRs and on pushes to Develop" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
assert "pull_request" in triggers, triggers
push = triggers.get("push") or {}
branches = push.get("branches") or []
assert "Develop" in branches, branches
PY
  [ "$status" -eq 0 ]
}

@test "ci workflow runs a clippy lint gate with -D warnings" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
runs = [
    s.get("run", "")
    for job in data["jobs"].values()
    for s in job.get("steps", [])
]
assert any("cargo clippy" in r and "-D warnings" in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

@test "ci workflow runs an explicit compile/syntax gate (cargo check or build)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
runs = [
    s.get("run", "")
    for job in data["jobs"].values()
    for s in job.get("steps", [])
]
def is_gate(r):
    return ("cargo check" in r or "cargo build" in r) and "--all-targets" in r
assert any(is_gate(r) for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

@test "ci workflow gates lint + compile on push (not pull_request only)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))

def has_lint(job):
    return any(
        "cargo clippy" in s.get("run", "") and "-D warnings" in s.get("run", "")
        for s in job.get("steps", [])
    )

def has_compile(job):
    return any(
        ("cargo check" in s.get("run", "") or "cargo build" in s.get("run", ""))
        and "--all-targets" in s.get("run", "")
        for s in job.get("steps", [])
    )

# At least one job must carry BOTH gates and must not be restricted to
# pull_request only, so direct pushes to Develop are gated too.
gate_jobs = [
    job for job in data["jobs"].values()
    if has_lint(job) and has_compile(job)
]
assert gate_jobs, "no single job carries both the lint and compile gates"
runs_on_push = [
    job for job in gate_jobs
    if "pull_request" not in str(job.get("if", ""))
]
assert runs_on_push, "lint+compile gate job is restricted to pull_request only"
PY
  [ "$status" -eq 0 ]
}

@test "ci workflow pins third-party actions to commit SHAs" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
sha_re = re.compile(r"^[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
for job in data["jobs"].values():
    for step in job.get("steps", []):
        uses = step.get("uses")
        if uses is None:
            continue
        assert sha_re.match(uses), f"action not SHA-pinned: {uses}"
PY
  [ "$status" -eq 0 ]
}
