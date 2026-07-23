#!/usr/bin/env bats
# Tests for the CI quality workflow branch filters (Issue #327).
#
# Asserts on observable outcomes:
#   - the workflow YAML file exists and parses,
#   - its pull_request branch filter matches milestone/<slug> feature branches
#     as well as the existing default branch, so the quality gate runs on
#     milestone sub-issue PRs (Issue #327).

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

# Issue #327 — milestone sub-issue PRs target a shared milestone/<slug> branch.
# GitHub branch-filter globs treat `*` as "any chars except /", so a filter of
# ["Develop"] never matches milestone/<slug> and the quality gate silently skips
# those PRs. The filter must match milestone branches so the gate runs on them
# too, while still matching the existing default branch.
@test "ci workflow pull_request filter matches milestone branches" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
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
# The existing default branch must still match.
assert any(matches(p, "Develop") for p in patterns), (
    f"no branch pattern matches 'Develop': {patterns}"
)
PY
  [ "$status" -eq 0 ]
}
