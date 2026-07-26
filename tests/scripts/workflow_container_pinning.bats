#!/usr/bin/env bats
# Tests for digest-pinning of job container/service images across every
# workflow (Issue #335).
#
# Rationale: a bare image name such as `semgrep/semgrep` resolves to the
# mutable `:latest` tag. The registry owner — or anyone who compromises that
# Docker Hub account — can re-point the tag at any time, and the new image
# then executes with whatever the job holds in scope (GITHUB_TOKEN,
# SEMGREP_APP_TOKEN, the checked-out source). This is the same substitution
# class already closed for `uses:` steps by 40-char commit SHAs (Issue #77)
# and for CLI installs by SHA-256-verified tarballs (Issues #78/#96/#99).
# Pinning by `@sha256:<64-hex>` makes the image content-addressed and
# immutable.
#
# These are "what" tests: they parse the YAML and assert on observable
# outcomes (the resolved image ref shape), not on source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "every job container and service image is pinned to a sha256 digest" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, re, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
digest_re = re.compile(r"^[A-Za-z0-9_.\-/:]+@sha256:[0-9a-f]{64}$")

def image_of(spec):
    """A container/service entry is either a bare image string or a mapping."""
    if isinstance(spec, str):
        return spec
    if isinstance(spec, dict):
        return spec.get("image")
    return None

failures = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    with open(path) as fh:
        data = yaml.safe_load(fh)
    for job_name, job in (data.get("jobs", {}) or {}).items():
        if not isinstance(job, dict):
            continue
        refs = [("container", image_of(job.get("container")))]
        for svc_name, svc in (job.get("services", {}) or {}).items():
            refs.append((f"services.{svc_name}", image_of(svc)))
        for where, image in refs:
            if not image:
                continue
            if not digest_re.match(image):
                failures.append(
                    f"{os.path.basename(path)} job={job_name} {where}={image}"
                )

if failures:
    sys.stderr.write("Unpinned container images:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# PR #393 — an image can also reach `docker pull`/`docker run` through a
# workflow/job `env:` var rather than a `container:` key (done there to control
# the pull backoff, which the runner's job-init pull does not expose). Such a
# ref is exactly as substitutable as a `container:` one, so the same digest pin
# must hold; without this the gate above would pass vacuously for that
# workflow.
@test "every *_IMAGE env value used as an image ref is pinned to a sha256 digest" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, re, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
digest_re = re.compile(r"^[A-Za-z0-9_.\-/:]+@sha256:[0-9a-f]{64}$")

def env_blocks(data):
    yield "workflow", data.get("env") or {}
    for job_name, job in (data.get("jobs", {}) or {}).items():
        if not isinstance(job, dict):
            continue
        yield f"job={job_name}", job.get("env") or {}
        for i, step in enumerate(job.get("steps", []) or []):
            if isinstance(step, dict):
                yield f"job={job_name} step={i}", step.get("env") or {}

failures = []
checked = 0
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        data = yaml.safe_load(fh)
    for where, env in env_blocks(data):
        for key, value in env.items():
            if not key.endswith("_IMAGE") or not isinstance(value, str):
                continue
            checked += 1
            if not digest_re.match(value):
                failures.append(
                    f"{os.path.basename(path)} {where} {key}={value}"
                )

if failures:
    sys.stderr.write("Unpinned image refs:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
# Guard against the check silently covering nothing.
assert checked > 0, "no *_IMAGE env vars found — has the semgrep pin moved?"
PY
  [ "$status" -eq 0 ]
}

@test "every digest-pinned image carries a human-readable version comment" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, re, sys

workflows_dir = "$WORKFLOWS_DIR"
image_re = re.compile(r"^(\s*)image:\s*([^\s#]+)(?:\s*#\s*(.+))?\s*$")

failures = []
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        lines = fh.readlines()
    for i, line in enumerate(lines):
        m = image_re.match(line)
        if not m:
            continue
        if "@sha256:" not in m.group(2):
            continue
        if m.group(3):  # inline trailing comment
            continue
        # Otherwise the tag must be named in a comment directly above.
        if i > 0 and lines[i - 1].strip().startswith("#"):
            continue
        failures.append(
            f"{os.path.basename(path)}:{i + 1}: missing version comment for {m.group(2)}"
        )

if failures:
    sys.stderr.write("Missing image version comments:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# Behavioural sanity check: the digest regex must reject the mutable forms
# this gate exists to keep out. If someone weakens it, this test fails.
@test "digest regex rejects bare image names and mutable tags" {
  run python3 - <<PY
import re
digest_re = re.compile(r"^[A-Za-z0-9_.\-/:]+@sha256:[0-9a-f]{64}\$")
bad = [
    "semgrep/semgrep",
    "semgrep/semgrep:latest",
    "semgrep/semgrep:1.170.1",
    "ghcr.io/owner/image:v1",
    "semgrep/semgrep@sha256:deadbeef",
]
for ref in bad:
    assert not digest_re.match(ref), f"regex incorrectly accepted {ref}"

good = [
    "semgrep/semgrep@sha256:" + "a" * 64,
    "ghcr.io/owner/image:1.2.3@sha256:" + "0" * 64,
]
for ref in good:
    assert digest_re.match(ref), f"regex incorrectly rejected {ref}"
PY
  [ "$status" -eq 0 ]
}
