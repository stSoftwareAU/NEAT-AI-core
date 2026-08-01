#!/usr/bin/env bats
# Checkout credential hygiene, swept across every workflow (Issue #477,
# consolidating Issues #317, #318, #320, #322, #323, #332).
#
# actions/checkout writes the workflow GITHUB_TOKEN into .git/config by
# default, leaving a usable credential on disk for every later step in the job.
# A job that never talks to the remote gains nothing from it, so the credential
# is pure blast radius and must be dropped with `persist-credentials: false`.
#
# This used to be one hand-written @test per job, copy-pasted into six files —
# which meant every job added afterwards was silently unguarded. The sweep
# *derives* the exemption instead of listing it: a checkout may keep the
# credential only when its job actually uses the remote (a git push/pull/fetch/
# ls-remote/clone in a `run:` block, or an action that pushes on the job's
# behalf). Every other checkout, in every workflow, present or future, must drop
# it.
#
# These are "what" tests: they parse the YAML and assert on the observable
# checkout configuration the runner will execute.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "every checkout in a job that never uses the remote drops the credential" {
  require_python3
  run python3 - <<PY
import glob
import os
import sys

import yaml

workflows_dir = "$WORKFLOWS_DIR"
# A job needs the persisted credential only if it reaches the remote itself…
GIT_REMOTE_OPS = ("git push", "git pull", "git fetch", "git ls-remote", "git clone")
# …or delegates that push to an action.
PUSHING_ACTIONS = ("peter-evans/create-pull-request",)


def uses_remote(job):
    for step in job.get("steps") or []:
        if any(op in str(step.get("run", "")) for op in GIT_REMOTE_OPS):
            return True
        if any(action in str(step.get("uses", "")) for action in PUSHING_ACTIONS):
            return True
    return False


failures = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    data = yaml.safe_load(open(path))
    for job_name, job in (data.get("jobs") or {}).items():
        if not isinstance(job, dict) or uses_remote(job):
            continue
        for step in job.get("steps") or []:
            if not str(step.get("uses", "")).startswith("actions/checkout@"):
                continue
            with_ = step.get("with") or {}
            if with_.get("persist-credentials") is not False:
                failures.append(
                    f"{os.path.basename(path)} job={job_name}: checkout persists "
                    f"the GITHUB_TOKEN but the job never uses the remote"
                )

if failures:
    sys.stderr.write("Checkouts leaking a credential:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# The sweep is only as good as its reach: if a workflow stopped checking out
# altogether the test above would pass vacuously. Pin the jobs whose hardening
# was bought by a dedicated issue so a regression is unmistakable.
@test "the sweep actually reaches the jobs hardened for credential leakage" {
  require_python3
  run python3 - <<PY
import os

import yaml

workflows_dir = "$WORKFLOWS_DIR"
# job -> workflow, for the jobs hardened by Issues #317/#318/#320/#322/#323/#332.
HARDENED = {
    "actionlint": "actionlint.yml",
    "rust-gates": "ci.yml",
    "validation": "ci.yml",
    "wasm64-memory64-smoke": "ci.yml",
    "markdownlint": "markdown-lint.yml",
    "release": "release.yml",
}
for job_name, filename in HARDENED.items():
    data = yaml.safe_load(open(os.path.join(workflows_dir, filename)))
    jobs = data.get("jobs") or {}
    assert job_name in jobs, f"{filename}: no {job_name!r} job; jobs are {sorted(jobs)}"
    checkouts = [
        s for s in jobs[job_name].get("steps") or []
        if str(s.get("uses", "")).startswith("actions/checkout@")
    ]
    assert checkouts, f"{filename} job={job_name}: no actions/checkout step found"
    for step in checkouts:
        with_ = step.get("with") or {}
        assert with_.get("persist-credentials") is False, (
            f"{filename} job={job_name}: checkout persists credentials: {step}"
        )
PY
  [ "$status" -eq 0 ]
}
