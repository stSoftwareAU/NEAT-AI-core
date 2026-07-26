#!/usr/bin/env bats
# Tests for the CI bats gate (Issue #397).
#
# Contract under test: the `scripts-and-spelling` job in
# .github/workflows/ci.yml must actually run `bats tests/scripts`. Before #397
# that job ran shellcheck, `bash -n` and codespell only, so the whole bats suite
# — including the Issue #375 public-safety assertions over `tests/perf/` — was
# enforced by the local `quality.sh` gate alone. A regression therefore landed on
# `Develop` green and only surfaced on a contributor's machine.
#
# These are "what" tests over the published workflow: they parse the committed
# YAML and assert on the observable job contract (a step that installs bats, a
# step that runs the suite), the same style as the other workflow tests in this
# directory.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

require_python() {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
}

# Emit every `run:` script in the scripts-and-spelling job, one per line-block.
job_run_steps() {
  python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = data["jobs"]["scripts-and-spelling"]
for step in job["steps"]:
    run = step.get("run")
    if run:
        print(run)
PY
}

@test "ci workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "scripts-and-spelling job installs bats" {
  require_python
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = data["jobs"]["scripts-and-spelling"]
installs = [
    line
    for step in job["steps"]
    for line in (step.get("run") or "").splitlines()
    if "apt-get install" in line and "bats" in line.split()
]
assert installs, "no step apt-get installs bats"
PY
  [ "$status" -eq 0 ]
}

@test "scripts-and-spelling job runs the bats suite over tests/scripts" {
  require_python
  run job_run_steps
  [ "$status" -eq 0 ]
  [[ "$output" == *"bats tests/scripts"* ]]
}

@test "the bats step is named so a failure is attributable" {
  require_python
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = data["jobs"]["scripts-and-spelling"]
names = [
    s.get("name", "")
    for s in job["steps"]
    if "bats tests/scripts" in (s.get("run") or "")
]
assert names, "no step runs `bats tests/scripts`"
assert all(n.strip() for n in names), f"bats step has no name: {names!r}"
PY
  [ "$status" -eq 0 ]
}

@test "the bats step does not swallow a failing suite" {
  # Fail loud: the runner must not be piped into `|| true` or `continue-on-error`.
  require_python
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = data["jobs"]["scripts-and-spelling"]
found = False
for step in job["steps"]:
    run = step.get("run") or ""
    if "bats tests/scripts" not in run:
        continue
    found = True
    assert "|| true" not in run, "bats failure is swallowed by `|| true`"
    assert step.get("continue-on-error") is not True, (
        "bats step sets continue-on-error"
    )
assert found, "no step runs `bats tests/scripts`"
PY
  [ "$status" -eq 0 ]
}
