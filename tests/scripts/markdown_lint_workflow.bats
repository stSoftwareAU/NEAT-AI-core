#!/usr/bin/env bats
# Tests for the Markdown Lint GitHub Actions workflow (Issue #56).
# Asserts on observable outcomes:
#   - the workflow YAML parses,
#   - the markdownlint-cli2 config file is valid JSONC and applies,
#   - markdownlint-cli2 actually passes against the current tree.
#
# The checkout credential check (Issue #322) now lives in the repo-wide sweep in
# workflow_checkout_credentials.bats, and the branch-filter assertion runs
# through the shared glob model in helpers.bash (Issue #477).

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/markdown-lint.yml"
  CONFIG="${REPO_ROOT}/.markdownlint-cli2.jsonc"
}

@test "markdown-lint workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "markdown-lint workflow is valid YAML" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml,sys; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "markdown-lint workflow gates PRs only and does not re-run on push to Develop" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
assert "pull_request" in triggers, triggers
# Issue #316 — a lint/check workflow gates the PR; re-running it on push to
# the default branch duplicates the run that already gated the merge.
push = triggers.get("push")
branches = (push or {}).get("branches") or []
assert "Develop" not in branches, branches
PY
  [ "$status" -eq 0 ]
}

# Issue #329 — milestone sub-issue PRs target a shared milestone/<slug> branch.
# GitHub branch-filter globs treat `*` as "any chars except /", so a filter of
# ["*"] never matches milestone/<slug> and this lint gate silently skips those
# PRs. The filter must match milestone branches so the gate runs on them too.
@test "markdown-lint workflow pull_request filter matches milestone branches" {
  require_python3
  # The existing default branches must still match too.
  run assert_pr_branch_filter_matches "$WORKFLOW" \
    "milestone/clean-up-23-jul" Develop main
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "markdown-lint workflow exposes a markdownlint job that runs markdownlint-cli2" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml
data = yaml.safe_load(open("$WORKFLOW"))
job = data["jobs"]["markdownlint"]
assert job["runs-on"] == "ubuntu-latest", job
steps = job["steps"]
runs = [s.get("run","") for s in steps]
assert any("markdownlint-cli2" in r and "install" not in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

@test "markdown-lint workflow pins third-party actions to commit SHAs" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
sha_re = re.compile(r"^[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
for step in data["jobs"]["markdownlint"]["steps"]:
    uses = step.get("uses")
    if uses is None:
        continue
    assert sha_re.match(uses), f"action not SHA-pinned: {uses}"
PY
  [ "$status" -eq 0 ]
}

@test "markdown-lint workflow does not pin actions/setup-node to a deprecated Node 20 SHA" {
  # Regression test for Issue #98 — actions/setup-node@v4
  # (SHA 49933ea5288caeca8642d1e84afbd3f7d6820020) ships the Node 20
  # runtime that GitHub-hosted runners are deprecating. Catch any
  # accidental rollback to that SHA.
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
DEPRECATED_NODE20_SHAS = {
    # actions/setup-node@v4 — Node 20 runtime.
    "49933ea5288caeca8642d1e84afbd3f7d6820020",
    # actions/setup-node@v3 — Node 16 runtime (also deprecated).
    "1a4442cacd436585916779262731d5b162bc6ec7",
}
setup_node_steps = [
    s for s in data["jobs"]["markdownlint"]["steps"]
    if isinstance(s.get("uses"), str) and s["uses"].startswith("actions/setup-node@")
]
assert setup_node_steps, "actions/setup-node step missing"
for step in setup_node_steps:
    ref = step["uses"].split("@", 1)[1]
    assert ref not in DEPRECATED_NODE20_SHAS, (
        f"actions/setup-node pinned to deprecated Node 20/16 SHA: {ref}"
    )
PY
  [ "$status" -eq 0 ]
}

# Issue #379 — business-logic change, documented deliberately. This test
# previously asserted the opposite ("gates Mermaid validation on a Deno worker
# module"): the step was conditional on `worker/deno/mod.ts`, a module that
# never exists in this repository, so the gate self-skipped on every run and a
# broken diagram in docs/archive/pr-summaries/pr-summary-334.md landed
# unnoticed. The gate is now unconditional and repo-owned.
@test "markdown-lint workflow validates Mermaid unconditionally with the repo-owned gate" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["markdownlint"]["steps"]
mermaid = [s for s in steps if s.get("name") == "Validate Mermaid blocks"]
assert mermaid, "Validate Mermaid blocks step missing"
step = mermaid[0]
assert "if" not in step, f"Mermaid validation is conditional: {step}"
assert "scripts/check_mermaid.ts" in step["run"], step["run"]
# No step may reference a module owned by another repository.
for s in steps:
    assert "worker/deno/mod.ts" not in str(s.get("run", "")), s
    assert "worker/deno/mod.ts" not in str(s.get("if", "")), s
PY
  [ "$status" -eq 0 ]
}

# The gate is worthless if its own unit tests never run in CI.
@test "markdown-lint workflow runs the Mermaid gate unit tests" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
runs = [s.get("run", "") for s in data["jobs"]["markdownlint"]["steps"]]
assert any(
    "deno test" in r and "tests/check_mermaid_test.ts" in r for r in runs
), runs
PY
  [ "$status" -eq 0 ]
}

# Behavioural check: the committed gate must actually reject a broken diagram
# and accept the current tree.
@test "repo-owned Mermaid gate rejects a broken diagram and passes the tree" {
  if ! command -v deno &>/dev/null; then
    skip "deno required for the Mermaid gate"
  fi
  TMP="$(mktemp -d)"
  printf '```mermaid\nsequenceDiagram\n    A->>B: one; two\n```\n' > "$TMP/bad.md"
  run deno run --allow-read "${REPO_ROOT}/scripts/check_mermaid.ts" "$TMP"
  rm -rf "$TMP"
  [ "$status" -ne 0 ]

  run deno run --allow-read "${REPO_ROOT}/scripts/check_mermaid.ts" "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "markdownlint config file exists and is valid JSONC" {
  [ -f "$CONFIG" ]
  if ! command -v node &>/dev/null; then
    skip "node required for JSONC parse"
  fi
  # Strip // and /* */ comments, then JSON.parse.
  run node -e "
    const fs = require('fs');
    const src = fs.readFileSync(process.argv[1], 'utf8')
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .replace(/^\s*\/\/.*$/gm, '');
    JSON.parse(src);
  " "$CONFIG"
  [ "$status" -eq 0 ]
}

@test "markdownlint-cli2 passes against the current tree" {
  if ! command -v markdownlint-cli2 &>/dev/null; then
    skip "markdownlint-cli2 not installed locally"
  fi
  cd "$REPO_ROOT"
  run markdownlint-cli2
  [ "$status" -eq 0 ]
}

# Behavioural sanity check: a deliberately malformed Markdown file must
# fail markdownlint-cli2 when it is in scope. Confirms the gate is wired
# up rather than silently passing.
@test "markdownlint-cli2 rejects a known-bad Markdown file" {
  if ! command -v markdownlint-cli2 &>/dev/null; then
    skip "markdownlint-cli2 not installed locally"
  fi
  TMP="$(mktemp -d)"
  cp "$CONFIG" "$TMP/.markdownlint-cli2.jsonc"
  # MD009 trailing-space + MD047 missing final newline + MD022 missing
  # blank line below heading. These are not disabled in the config.
  printf '# Title\nbody with trailing space   \n## Sub\nbody' > "$TMP/bad.md"
  cd "$TMP"
  run markdownlint-cli2 "bad.md"
  rm -rf "$TMP"
  [ "$status" -ne 0 ]
}
