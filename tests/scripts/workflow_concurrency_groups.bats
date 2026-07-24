#!/usr/bin/env bats
# Tests for top-level `concurrency:` groups on pile-up-prone workflows (Issue #334).
#
# Rationale: every `synchronize` push to an open PR queues a fresh run while the
# previous one is still compiling. Without a concurrency group the superseded
# runs keep burning runner minutes producing results nobody reads — and on
# `ci.yml`, where a single run compiles the workspace several times over, a few
# rapid pushes (including the bot pushes from `version-increment`/`auto-format`)
# stack runs several deep.
#
# Publishing workflows are deliberately excluded from `cancel-in-progress: true`:
# `release.yml` cuts a `v<version>` tag and `wasm-bundle.yml` publishes a
# per-commit bundle, so cancelling a superseded run would silently skip an
# artefact downstream consumers pin by SHA.
#
# These are "what" tests: they parse the YAML and assert on the observable
# effective configuration each workflow declares, not on source text. The
# heredocs are quoted (`<<'PY'`) and the directory arrives through the
# environment, so GitHub `${{ … }}` expressions survive into Python untouched.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  export WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "pile-up-prone workflows cancel superseded runs per ref" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<'PY'
import os, sys, yaml

workflows_dir = os.environ["WORKFLOWS_DIR"]
GATES = [
    "ci.yml",
    "actionlint.yml",
    "gitleaks.yml",
    "markdown-lint.yml",
    "semgrep.yml",
]

bad = []
for filename in GATES:
    path = os.path.join(workflows_dir, filename)
    with open(path) as fh:
        data = yaml.safe_load(fh)
    concurrency = data.get("concurrency")
    if not isinstance(concurrency, dict):
        bad.append(f"{filename}: no top-level concurrency mapping")
        continue
    group = concurrency.get("group", "")
    if "github.workflow" not in group:
        bad.append(f"{filename}: group must include github.workflow, got {group!r}")
    if "github.ref" not in group:
        bad.append(f"{filename}: group must include github.ref, got {group!r}")
    if concurrency.get("cancel-in-progress") is not True:
        bad.append(
            f"{filename}: cancel-in-progress must be true, got "
            f"{concurrency.get('cancel-in-progress')!r}"
        )

if bad:
    sys.stderr.write("Concurrency gate failures:\n  " + "\n  ".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# Two distinct workflows must never share a queue: a group keyed only on the ref
# would let a gitleaks run cancel the CI run for the same branch.
@test "each gated workflow's concurrency group is distinct for the same ref" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<'PY'
import glob, os, re, sys, yaml

workflows_dir = os.environ["WORKFLOWS_DIR"]
groups = {}
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        data = yaml.safe_load(fh)
    concurrency = data.get("concurrency")
    if not isinstance(concurrency, dict):
        continue
    # `github.workflow` resolves to the workflow's own name, so substituting the
    # declared name models the group each workflow actually joins at run time.
    resolved = re.sub(
        r"\$\{\{\s*github\.workflow\s*\}\}", data["name"], concurrency["group"]
    )
    groups.setdefault(resolved, []).append(os.path.basename(path))

collisions = {g: f for g, f in groups.items() if len(f) > 1}
if collisions:
    sys.stderr.write(f"Colliding concurrency groups: {collisions}\n")
    sys.exit(1)
assert groups, "expected at least one workflow to declare a concurrency group"
PY
  [ "$status" -eq 0 ]
}

# Publishing workflows produce artefacts downstream consumers pin by SHA or
# version; a cancelled run silently skips one.
@test "publishing workflows never cancel in-progress runs" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<'PY'
import os, sys, yaml

workflows_dir = os.environ["WORKFLOWS_DIR"]
PUBLISHERS = ["release.yml", "wasm-bundle.yml"]

bad = []
for filename in PUBLISHERS:
    path = os.path.join(workflows_dir, filename)
    with open(path) as fh:
        data = yaml.safe_load(fh)
    concurrency = data.get("concurrency")
    if concurrency is not None:
        if not isinstance(concurrency, dict):
            bad.append(f"{filename}: concurrency must be a mapping, got {concurrency!r}")
        elif concurrency.get("cancel-in-progress") is True:
            bad.append(f"{filename}: cancel-in-progress must not be true")
    for job_name, job in (data.get("jobs", {}) or {}).items():
        job_concurrency = job.get("concurrency") if isinstance(job, dict) else None
        if isinstance(job_concurrency, dict) and job_concurrency.get("cancel-in-progress") is True:
            bad.append(f"{filename} job={job_name}: cancel-in-progress must not be true")

if bad:
    sys.stderr.write("Publishing workflow failures:\n  " + "\n  ".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# A reusable workflow runs inside the caller's context; giving it its own group
# would let one caller cancel another caller's in-flight security scan.
@test "the reusable security workflow declares no concurrency group" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<'PY'
import os, sys, yaml

with open(os.path.join(os.environ["WORKFLOWS_DIR"], "security.yml")) as fh:
    data = yaml.safe_load(fh)

# PyYAML resolves the bare `on:` key to the boolean True.
triggers = data.get("on", data.get(True))
assert "workflow_call" in triggers, "security.yml must stay reusable"
if "concurrency" in data:
    sys.stderr.write("security.yml must not declare its own concurrency group\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}
