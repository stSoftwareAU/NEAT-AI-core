#!/usr/bin/env bats
# Tests for the CI quality workflow branch filters (Issue #327).
#
# Asserts on observable outcomes:
#   - the workflow YAML file exists and parses,
#   - its pull_request branch filter matches milestone/<slug> feature branches
#     as well as the existing default branch, so the quality gate runs on
#     milestone sub-issue PRs (Issue #327).
#
# The branch-filter assertion runs through the shared glob model in
# helpers.bash (Issue #477), which is unit-tested in helpers_shared.bats.

load helpers

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
  require_python3
  # The existing default branch must still match too.
  run assert_pr_branch_filter_matches "$WORKFLOW" "milestone/clean-up-23-jul" Develop
  echo "$output"
  [ "$status" -eq 0 ]
}

# Issue #580 — CI is a test/lint/scan workflow, so it gates the pull request.
# The default branch is PR-only (`.github/rulesets/develop.json` requires a
# reviewed PR with no bypass actors), so the only `push` events Develop ever
# sees are the merges of PRs this workflow already gated. Re-running on that
# push duplicates the gating run for no new signal. The PR and
# `workflow_dispatch` triggers stay.
@test "ci workflow gates PRs only and does not re-run on push to Develop" {
  require_python3
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
assert "pull_request" in triggers, triggers
assert "workflow_dispatch" in triggers, triggers
push = triggers.get("push")
branches = (push or {}).get("branches") or []
assert "Develop" not in branches, branches
PY
  echo "$output"
  [ "$status" -eq 0 ]
}
