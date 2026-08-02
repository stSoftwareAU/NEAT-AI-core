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
