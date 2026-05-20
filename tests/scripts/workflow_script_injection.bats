#!/usr/bin/env bats
# Tests that GitHub Actions workflows do not interpolate attacker-controlled
# context expressions directly into `run:` shell blocks (Issue #79).
#
# Rationale: contexts such as `github.head_ref`, `github.event.pull_request.*`,
# `github.event.issue.*`, `github.event.comment.*`, `github.event.review.*`,
# `github.event.commits.*`, and `github.event.workflow_run.*` are populated
# from user-controlled input (branch names, PR titles/bodies, etc.).
# `git check-ref-format` permits shell metacharacters such as `$`, `;`, `(`,
# `)`, backtick, `&`, `|`, `"`, `'`, `<`, and `>` in branch names, so any
# `${{ ... }}` expansion of these contexts inside a `run:` body lets the
# branch name escape the surrounding shell command and execute arbitrary
# code on the runner — see
# https://securitylab.github.com/research/github-actions-untrusted-input/.
#
# The accepted mitigation is to bind the tainted context to an `env:` value
# on the step, then reference the resulting shell variable from `run:`.
# Shell parameter expansion makes metacharacters inert.
#
# These are "what" tests: they parse the YAML and assert on observable
# outcomes (the contents of each step's `run:` body and `env:` map), not
# on source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  export WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "no run: block interpolates tainted github contexts directly" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<'PY'
import glob, os, re, sys, yaml

workflows_dir = os.environ["WORKFLOWS_DIR"]

# Contexts that come from outside the trust boundary. Branch names, PR
# titles/bodies, issue bodies, comments, commit messages, and workflow_run
# event payloads are all user-controlled.
tainted_re = re.compile(
    r"\$\{\{\s*github\.(?:"
    r"head_ref"
    r"|event\.pull_request\.(?:title|body|head\.ref|head\.label|head\.repo\.[A-Za-z_.]+)"
    r"|event\.issue\.(?:title|body)"
    r"|event\.comment\.body"
    r"|event\.review\.body"
    r"|event\.commits\.[^}]*"
    r"|event\.workflow_run\.head_branch"
    r")\s*\}\}"
)

failures = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    with open(path) as fh:
        data = yaml.safe_load(fh)
    jobs = (data.get("jobs") or {}) if isinstance(data, dict) else {}
    for job_name, job in jobs.items():
        if not isinstance(job, dict):
            continue
        for idx, step in enumerate(job.get("steps", []) or []):
            if not isinstance(step, dict):
                continue
            run = step.get("run")
            if not isinstance(run, str):
                continue
            m = tainted_re.search(run)
            if m:
                failures.append(
                    f"{os.path.basename(path)} job={job_name} "
                    f"step[{idx}]={step.get('name', '<unnamed>')!r} "
                    f"tainted={m.group(0)}"
                )

if failures:
    sys.stderr.write(
        "Tainted github context interpolated into run: block(s):\n  "
        + "\n  ".join(failures)
        + "\n\nBind the context to an `env:` value on the step and reference\n"
        + "the resulting shell variable from `run:` instead.\n"
    )
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "tainted-context detector regex catches known-bad patterns" {
  run python3 - <<'PY'
import re

tainted_re = re.compile(
    r"\$\{\{\s*github\.(?:"
    r"head_ref"
    r"|event\.pull_request\.(?:title|body|head\.ref|head\.label|head\.repo\.[A-Za-z_.]+)"
    r"|event\.issue\.(?:title|body)"
    r"|event\.comment\.body"
    r"|event\.review\.body"
    r"|event\.commits\.[^}]*"
    r"|event\.workflow_run\.head_branch"
    r")\s*\}\}"
)

bad = [
    "git pull origin ${{ github.head_ref }}",
    'git pull --ff-only origin "${{ github.head_ref }}" || true',
    "echo ${{ github.event.pull_request.title }}",
    "echo ${{ github.event.issue.body }}",
    "echo ${{ github.event.workflow_run.head_branch }}",
]
for snippet in bad:
    assert tainted_re.search(snippet), f"regex failed to catch: {snippet}"

# Safe patterns: env-var binding (the mitigation) and contexts that are not
# user-controlled (e.g. github.ref_name on push to a protected branch is
# still untrusted in general, but github.repository / github.sha are not).
safe = [
    'echo "$HEAD_REF"',
    "echo ${{ github.repository }}",
    "echo ${{ github.sha }}",
    "echo ${{ secrets.GITHUB_TOKEN }}",
]
for snippet in safe:
    assert not tainted_re.search(snippet), f"regex falsely flagged: {snippet}"
PY
  [ "$status" -eq 0 ]
}
