#!/usr/bin/env bats
# Tests for job-level `timeout-minutes` across every workflow (Issue #333).
#
# Rationale: a job with no `timeout-minutes` inherits GitHub's default of
# 6 hours. A wedged step (a hung `cargo` build, a stalled network fetch in
# `bump-deps.sh`, a `deno test` waiting on stdin) then holds a runner for six
# hours, burning the Actions minutes quota and blocking queued runs. An
# explicit per-job budget kills the wedge in minutes instead.
#
# These are "what" tests: they parse the YAML and assert on the observable
# effective configuration (the timeout each job declares), not on source text.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "every runner job in every workflow declares a job-level timeout-minutes" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
missing = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    with open(path) as fh:
        data = yaml.safe_load(fh)
    for job_name, job in (data.get("jobs", {}) or {}).items():
        if not isinstance(job, dict):
            continue
        # Reusable-workflow callers cannot set timeout-minutes (GitHub rejects
        # the key); the called workflow's own job carries the budget.
        if "uses" in job:
            continue
        if "timeout-minutes" not in job:
            missing.append(f"{os.path.basename(path)} job={job_name}")

if missing:
    sys.stderr.write("Jobs without timeout-minutes:\n  " + "\n  ".join(missing) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "every declared timeout-minutes is a positive integer no greater than 60" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
bad = []
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        data = yaml.safe_load(fh)
    for job_name, job in (data.get("jobs", {}) or {}).items():
        if not isinstance(job, dict) or "timeout-minutes" not in job:
            continue
        value = job["timeout-minutes"]
        if not isinstance(value, int) or isinstance(value, bool) or not 1 <= value <= 60:
            bad.append(f"{os.path.basename(path)} job={job_name} timeout-minutes={value!r}")

if bad:
    sys.stderr.write("Out-of-range timeout-minutes:\n  " + "\n  ".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# The compile-heavy jobs need a bigger budget than the lint jobs: a cold
# `cargo build --release` plus clippy, tests and docs can legitimately run for
# half an hour. Guard both directions so a future edit cannot starve them.
@test "compile-heavy jobs budget at least 30 minutes" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import os, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
HEAVY = {
    "ci.yml": ["quality", "rust-gates"],
    "security.yml": ["security"],
    "upgrade-dependencies.yml": ["upgrade"],
    "wasm-bundle.yml": ["publish"],
}

bad = []
for filename, job_names in HEAVY.items():
    path = os.path.join(workflows_dir, filename)
    with open(path) as fh:
        data = yaml.safe_load(fh)
    jobs = data.get("jobs", {}) or {}
    for job_name in job_names:
        assert job_name in jobs, f"{filename}: expected job {job_name}"
        value = jobs[job_name].get("timeout-minutes")
        if not isinstance(value, int) or value < 30:
            bad.append(f"{filename} job={job_name} timeout-minutes={value!r}")

if bad:
    sys.stderr.write("Compile-heavy jobs under-budgeted:\n  " + "\n  ".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# GitHub rejects `timeout-minutes` on a job that calls a reusable workflow.
# The budget must live on the called workflow's own job instead, so assert the
# caller stays clean and the callee carries a timeout.
@test "reusable-workflow callers carry no timeout-minutes and the callee does" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
callers = []
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        data = yaml.safe_load(fh)
    for job_name, job in (data.get("jobs", {}) or {}).items():
        if isinstance(job, dict) and "uses" in job:
            callers.append((os.path.basename(path), job_name, job))

assert callers, "expected at least one reusable-workflow caller (ci.yml security)"
offenders = [
    f"{f} job={j}" for f, j, job in callers if "timeout-minutes" in job
]
if offenders:
    sys.stderr.write("Callers must not set timeout-minutes:\n  " + "\n  ".join(offenders) + "\n")
    sys.exit(1)

# The called workflow must budget its own job, or the caller is unbounded.
with open(os.path.join(workflows_dir, "security.yml")) as fh:
    called = yaml.safe_load(fh)
assert isinstance(called["jobs"]["security"].get("timeout-minutes"), int)
PY
  [ "$status" -eq 0 ]
}
