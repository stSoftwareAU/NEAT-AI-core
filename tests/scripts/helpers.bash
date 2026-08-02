#!/usr/bin/env bash
# Shared helpers for the tests/scripts BATS suite (Issue #477).
#
# Load from any .bats file in this directory with `load helpers`. Everything
# here was previously copy-pasted across up to six test files — the GitHub
# branch-glob model lived in five, the step extractor in three — so a bug in one
# copy stayed invisible in the others and every new job or workflow had to be
# wired into each copy by hand.
#
# Convention: each assertion helper prints why it failed and returns non-zero,
# so a caller uses `run <helper> …` followed by `[ "$status" -eq 0 ]`.

# Skip the calling test when python3 — the YAML parser these helpers use — is
# unavailable, keeping the suite portable.
require_python3() {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
}

# Strip YAML comments from file $1 to stdout, so an assertion cannot be defeated
# by leaving the offending line behind as a comment.
strip_comments() {
  sed -E 's/[[:space:]]*#.*$//' "$1"
}

# Python model of GitHub's branch-filter glob semantics, emitted as source so
# every filter assertion shares one implementation: `*` matches any run of
# characters except `/`, `**` crosses `/`, and everything else is literal.
github_glob_py() {
  cat <<'PY'
import re


def matches(pattern, branch):
    regex = ""
    i = 0
    while i < len(pattern):
        c = pattern[i]
        if c == "*":
            if pattern[i + 1 : i + 2] == "*":
                regex += ".*"
                i += 2
                continue
            regex += "[^/]*"
        else:
            regex += re.escape(c)
        i += 1
    return re.fullmatch(regex, branch) is not None
PY
}

# github_glob_matches <pattern> <branch> — exit 0 when the GitHub filter glob
# <pattern> matches <branch>. Exposes the model above so it can be tested
# directly rather than only through a workflow file.
github_glob_matches() {
  python3 - "$1" "$2" <<PY
$(github_glob_py)
import sys

sys.exit(0 if matches(sys.argv[1], sys.argv[2]) else 1)
PY
}

# assert_pr_branch_filter_matches <workflow> <branch>… — the workflow's
# pull_request branches filter must match every named branch. Milestone
# sub-issue PRs target a shared milestone/<slug> branch, and a filter of ["*"]
# never matches it, so the gate silently skips those PRs.
assert_pr_branch_filter_matches() {
  local workflow="$1"
  shift
  python3 - "$workflow" "$@" <<PY
$(github_glob_py)
import sys

import yaml

workflow, wanted = sys.argv[1], sys.argv[2:]
data = yaml.safe_load(open(workflow))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
pr = triggers["pull_request"]
patterns = pr.get("branches") or []
assert patterns, f"pull_request has no branches filter: {pr}"
for branch in wanted:
    assert any(matches(p, branch) for p in patterns), (
        f"no branch pattern matches {branch!r}: {patterns}"
    )
PY
}

# assert_job_least_privilege <workflow> <job> [write-scope…] — the job must
# declare an explicit permissions: block (its own, or the workflow-level one it
# inherits), grant contents: read, and hold no write scope beyond those named.
# With no scopes named the job must be entirely read-only.
assert_job_least_privilege() {
  local workflow="$1" job="$2"
  shift 2
  python3 - "$workflow" "$job" "$@" <<'PY'
import sys

import yaml

workflow, job_name, allowed = sys.argv[1], sys.argv[2], sorted(sys.argv[3:])
data = yaml.safe_load(open(workflow))
jobs = data.get("jobs") or {}
assert job_name in jobs, f"no {job_name!r} job; jobs are {sorted(jobs)}"
job = jobs[job_name]
# Job-level permissions win; fall back to the workflow top-level block.
perms = job.get("permissions")
if perms is None:
    perms = data.get("permissions")
assert perms is not None, (
    f"{job_name} job has no permissions: block at job or workflow level"
)
assert isinstance(perms, dict), f"expected a mapping of scopes, got {perms!r}"
assert perms.get("contents") == "read", (
    f"{job_name}: contents must be read, got {perms.get('contents')!r}"
)
writes = sorted(k for k, v in perms.items() if v == "write")
assert writes == allowed, (
    f"{job_name} holds write scopes {writes}, expected {allowed}"
)
PY
}

# extract_step <workflow> <name-substring> <outdir> — write the run: body of the
# first step whose name contains <name-substring> to <outdir>/step.sh, and the
# argv GitHub would launch it with to <outdir>/shell.cmd, so a test can execute
# the real script under the real shell.
extract_step() {
  python3 - "$1" "$2" "$3" <<'PY'
import os
import sys

import yaml

workflow, needle, out = sys.argv[1:4]
with open(workflow) as fh:
    data = yaml.safe_load(fh)


def declared_shell(step, job):
    for scope in (step, job.get("defaults", {}).get("run", {}),
                  (data.get("defaults") or {}).get("run", {})):
        if scope.get("shell"):
            return scope["shell"]
    return None


for job in (data.get("jobs") or {}).values():
    for step in job.get("steps") or []:
        if needle not in (step.get("name") or "") or "run" not in step:
            continue
        body = step["run"]
        assert "${{" not in body, "step body interpolates a GitHub expression"
        shell = declared_shell(step, job)
        # GitHub: no `shell:` → `bash -e {0}`; `shell: bash` → `bash
        # --noprofile --norc -eo pipefail {0}`.
        argv = {
            None: "bash -e",
            "bash": "bash --noprofile --norc -eo pipefail",
        }.get(shell)
        assert argv, f"unsupported shell for this harness: {shell!r}"
        with open(os.path.join(out, "step.sh"), "w") as fh:
            fh.write(body)
        with open(os.path.join(out, "shell.cmd"), "w") as fh:
            fh.write(argv)
        sys.exit(0)

sys.exit(f"no step named like {needle!r} with a run: block in {workflow}")
PY
}

# assert_just_in_time_push_credential <workflow> <job> <secret> — the job must
# never leave <secret> where code it did not write can read it (Issue #483).
# Concretely: every checkout in the job runs with `persist-credentials: false`
# (so no credential lands in .git/config while PR-head scripts such as
# bump-deps.sh execute), the secret is not exposed job-wide or to any
# non-pushing step, and the one step that pushes receives it as its own env var
# and pushes to an explicitly authenticated https remote rather than `origin`.
assert_just_in_time_push_credential() {
  python3 - "$1" "$2" "$3" <<'PY'
import re
import sys

import yaml

workflow, job_name, secret = sys.argv[1], sys.argv[2], sys.argv[3]
data = yaml.safe_load(open(workflow))
jobs = data.get("jobs") or {}
assert job_name in jobs, f"no {job_name!r} job; jobs are {sorted(jobs)}"
job = jobs[job_name]
steps = job.get("steps") or []
ref = re.compile(r"\$\{\{[^}]*\bsecrets\." + re.escape(secret) + r"\b")


def mentions(node):
    return bool(ref.search(yaml.safe_dump(node, default_flow_style=False)))


checkouts = [
    i for i, s in enumerate(steps)
    if str(s.get("uses", "")).startswith("actions/checkout@")
]
assert checkouts, f"{job_name}: no actions/checkout step found"
for i in checkouts:
    with_ = steps[i].get("with") or {}
    assert with_.get("persist-credentials") is False, (
        f"{job_name}: checkout persists credentials into .git/config: {steps[i]}"
    )
    assert not mentions(with_), (
        f"{job_name}: checkout is handed {secret}; the PAT must reach only the "
        f"pushing step"
    )

assert not mentions(job.get("env") or {}), (
    f"{job_name}: {secret} is exposed to every step through job-level env"
)

pushers = [i for i, s in enumerate(steps) if "git push" in (s.get("run") or "")]
assert pushers, f"{job_name}: no step runs git push"
for i, step in enumerate(steps):
    if i in pushers:
        continue
    assert not mentions(step), (
        f"{job_name}: non-pushing step {step.get('name')!r} sees {secret}"
    )

for i in pushers:
    step = steps[i]
    env = step.get("env") or {}
    names = sorted(k for k, v in env.items() if ref.search(str(v)))
    assert names, (
        f"{job_name}: pushing step {step.get('name')!r} does not receive "
        f"{secret} just-in-time as a step env var"
    )
    for line in step["run"].splitlines():
        if not re.search(r"\bgit push\b", line):
            continue
        assert re.search(r'git push\s+"?https://', line), (
            f"{job_name}: push target is not an explicitly authenticated https "
            f"remote — `origin` carries no credential once persist-credentials "
            f"is false: {line.strip()!r}"
        )
        assert any(n in line for n in names), (
            f"{job_name}: push URL does not interpolate {names}: {line.strip()!r}"
        )
PY
}

# assert_cyclonedx_sbom_release <workflow> <job> — the job must build a
# CycloneDX SBOM with a version-pinned, --locked cargo-cyclonedx install and
# publish the .cdx.json as an asset of the Release it cuts, generated before the
# Release is created.
assert_cyclonedx_sbom_release() {
  python3 - "$1" "$2" <<'PY'
import re
import sys

import yaml

workflow, job_name = sys.argv[1], sys.argv[2]
data = yaml.safe_load(open(workflow))
steps = data["jobs"][job_name]["steps"]
runs = [s.get("run", "") for s in steps]

# A step must actually invoke cargo-cyclonedx to build the SBOM.
assert any("cargo cyclonedx" in r for r in runs), runs

# The install is pinned to an explicit --version and --locked (mirrors the
# wasm-pack pinning, Issue #78).
install_lines = [
    line
    for run in runs
    for line in run.splitlines()
    if "cargo install cargo-cyclonedx" in line
]
assert install_lines, "no cargo install cargo-cyclonedx step found"
pin_re = re.compile(r"--version[= ]\S+")
for line in install_lines:
    assert pin_re.search(line), f"install not version-pinned: {line!r}"
    assert "--locked" in line, f"install not --locked: {line!r}"

# The CycloneDX JSON asset must be handed to `gh release create` or
# `gh release upload` so it lands on the same Release.
assert any(
    ".cdx.json" in r and ("gh release create" in r or "gh release upload" in r)
    for r in runs
), runs

gen_idx = next(i for i, r in enumerate(runs) if "cargo cyclonedx" in r)
pub_idx = next(i for i, r in enumerate(runs) if "gh release create" in r)
assert gen_idx < pub_idx, f"SBOM generated after the Release: {gen_idx} > {pub_idx}"
PY
}

# assert_pinned_cli_install <workflow> <ENV_PREFIX> <release-url-regex> — the
# workflow must fetch the CLI from a version-pinned release URL, verify the
# download with `sha256sum -c`, and declare a 64-hex <PREFIX>_SHA256 and a
# semver <PREFIX>_VERSION env var. Pinning version + checksum kills the
# re-fetch window a `curl … | sh` bootstrap leaves open.
assert_pinned_cli_install() {
  local workflow="$1" prefix="$2" url_re="$3"
  local failed=0
  if ! grep -Eq "$url_re" "$workflow"; then
    echo "no version-pinned release URL matching ${url_re} in ${workflow}" >&2
    failed=1
  fi
  if ! grep -Eq 'sha256sum[[:space:]]+-c' "$workflow"; then
    echo "no sha256sum -c verification in ${workflow}" >&2
    failed=1
  fi
  if ! grep -Eq "${prefix}_SHA256:[[:space:]]*\"?[0-9a-f]{64}\"?" "$workflow"; then
    echo "no 64-hex ${prefix}_SHA256 env var in ${workflow}" >&2
    failed=1
  fi
  if ! grep -Eq "${prefix}_VERSION:[[:space:]]*\"?[0-9]+\.[0-9]+\.[0-9]+\"?" "$workflow"; then
    echo "no semver ${prefix}_VERSION env var in ${workflow}" >&2
    failed=1
  fi
  return "$failed"
}
