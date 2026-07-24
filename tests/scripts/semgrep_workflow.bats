#!/usr/bin/env bats
# Tests for the Semgrep GitHub Actions workflow.
#
# Issue #330 — milestone sub-issue PRs target a shared `milestone/<slug>`
# branch. GitHub branch-filter globs treat `*` as "any chars except /", so a
# filter of ["*"] never matches `milestone/<slug>` and this scan silently
# skips those PRs; the gap only surfaces on the single rollup PR into the
# default branch.
#
# These are "what" tests — they assert on the YAML the runner will execute,
# not on commentary or surrounding prose.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/semgrep.yml"
}

@test "semgrep.yml exists and is valid YAML" {
  [ -f "$WF" ]
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml; yaml.safe_load(open('$WF'))"
  [ "$status" -eq 0 ]
}

@test "semgrep.yml pull_request filter matches milestone branches" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WF"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
pr = triggers["pull_request"]
patterns = pr.get("branches") or []
assert patterns, f"pull_request has no branches filter: {pr}"

def matches(pattern, branch):
    # GitHub filter globbing: ** crosses '/', * does not.
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

branch = "milestone/clean-up-23-jul"
assert any(matches(p, branch) for p in patterns), (
    f"no branch pattern matches {branch!r}: {patterns}"
)
# The existing default branches must still match.
for keep in ("Develop", "main"):
    assert any(matches(p, keep) for p in patterns), (
        f"no branch pattern matches {keep!r}: {patterns}"
    )
PY
  [ "$status" -eq 0 ]
}

@test "semgrep.yml runs the semgrep scan on pull requests" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WF"))
job = data["jobs"]["semgrep"]
runs = [s.get("run", "") for s in job["steps"]]
assert any("semgrep" in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}
