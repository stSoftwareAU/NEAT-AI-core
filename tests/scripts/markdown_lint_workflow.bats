#!/usr/bin/env bats
# Tests for the Markdown Lint GitHub Actions workflow (Issue #56).
# Asserts on observable outcomes:
#   - the workflow YAML parses,
#   - the markdownlint-cli2 config file is valid JSONC and applies,
#   - markdownlint-cli2 actually passes against the current tree.

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

@test "markdown-lint workflow gates Mermaid validation on a Deno worker module" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["markdownlint"]["steps"]
mermaid = [s for s in steps if s.get("name") == "Validate Mermaid blocks"]
assert mermaid, "Validate Mermaid blocks step missing"
guard = mermaid[0].get("if", "")
assert "detect-deno" in guard and "present" in guard, guard
PY
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
