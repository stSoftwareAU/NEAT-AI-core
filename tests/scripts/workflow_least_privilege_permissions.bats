#!/usr/bin/env bats
# Tests that every job in every workflow runs under an explicit least-privilege
# `permissions:` block (Issue #309).
#
# Rationale: a job with no `permissions:` at either the workflow or the job
# level inherits the repository's default GITHUB_TOKEN scopes, which are often
# broad (contents: write and more). If any step is compromised the token can
# act with those scopes. Declaring an explicit block — a top-level default that
# every job inherits, or a per-job override — closes that gap. Every other
# workflow in this repo already sets a top-level `permissions:` block; the
# regression this guards is the `quality` job (and its siblings) in ci.yml
# inheriting the broad default because ci.yml had no workflow-level default.
#
# These are "what" tests: they parse the YAML and assert on the observable
# outcome (each job has effective permissions), not on source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "every job in every workflow has an explicit permissions block" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"

failures = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    with open(path) as fh:
        data = yaml.safe_load(fh)
    top_level = "permissions" in (data or {})
    jobs = (data or {}).get("jobs", {}) or {}
    for job_name, job in jobs.items():
        if not isinstance(job, dict):
            continue
        job_level = "permissions" in job
        if not (top_level or job_level):
            failures.append(f"{os.path.basename(path)} job={job_name}")

if failures:
    sys.stderr.write(
        "Jobs without an explicit permissions block:\n  "
        + "\n  ".join(failures) + "\n"
    )
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "ci.yml quality job runs under contents: read (no write scopes)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml

with open("$WORKFLOWS_DIR/ci.yml") as fh:
    data = yaml.safe_load(fh)

# Effective permissions for the quality job: its own block, else the
# workflow-level default.
quality = data["jobs"]["quality"]
perms = quality.get("permissions", data.get("permissions"))
assert perms is not None, "quality job has no effective permissions block"
assert isinstance(perms, dict), f"expected a scoped permissions mapping, got {perms!r}"

# Least privilege: no write scope granted to the quality job.
write_scopes = [scope for scope, level in perms.items() if level == "write"]
assert not write_scopes, f"quality job granted write scopes: {write_scopes}"
assert perms.get("contents") == "read", f"expected contents: read, got {perms.get('contents')!r}"
PY
  [ "$status" -eq 0 ]
}
