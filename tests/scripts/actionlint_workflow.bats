#!/usr/bin/env bats
# Tests for the actionlint GitHub Actions linter workflow (Issue #96).
#
# Asserts on observable outcomes:
#   - the workflow YAML file exists and parses,
#   - it triggers on pull_request and on pushes to Develop,
#   - it actually runs the `actionlint` binary,
#   - third-party actions are SHA-pinned (consistent with Issue #77),
#   - the install step pins both version and SHA-256 (supply-chain hygiene
#     mirroring gitleaks.yml from Issue #99 and wasm-pack from Issue #78),
#   - if `actionlint` is installed locally, it passes against the current
#     workflows on disk (behavioural sanity check).

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/actionlint.yml"
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

@test "actionlint workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "actionlint workflow is valid YAML" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml,sys; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "actionlint workflow triggers on PRs and on pushes to Develop" {
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
push = triggers.get("push") or {}
branches = push.get("branches") or []
assert "Develop" in branches, branches
PY
  [ "$status" -eq 0 ]
}

@test "actionlint workflow exposes a job that runs the actionlint binary" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
jobs = data["jobs"]
# Single job with a recognisable name, e.g. "actionlint".
assert "actionlint" in jobs, list(jobs)
job = jobs["actionlint"]
assert job["runs-on"] == "ubuntu-latest", job
steps = job["steps"]
runs = [s.get("run", "") for s in steps]
# A step must actually invoke the actionlint binary (not just install it).
assert any(
    "actionlint" in r
    and "curl" not in r
    and "tar" not in r
    and "sha256sum" not in r
    and "install -m" not in r
    for r in runs
), runs
PY
  [ "$status" -eq 0 ]
}

@test "actionlint workflow pins third-party actions to commit SHAs" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
sha_re = re.compile(r"^[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
for step in data["jobs"]["actionlint"]["steps"]:
    uses = step.get("uses")
    if uses is None:
        continue
    assert sha_re.match(uses), f"action not SHA-pinned: {uses}"
PY
  [ "$status" -eq 0 ]
}

@test "actionlint workflow pins both version and SHA-256 for the CLI install" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["actionlint"]["steps"]
install = [
    s for s in steps
    if isinstance(s.get("name"), str) and "install" in s["name"].lower()
    and "actionlint" in s["name"].lower()
]
assert install, "Install actionlint step missing"
step = install[0]
env = step.get("env", {}) or {}
# A pinned version env var and a sha256 env var must both be present.
keys = {k.upper(): v for k, v in env.items()}
version = keys.get("ACTIONLINT_VERSION") or keys.get("VERSION")
sha256 = keys.get("ACTIONLINT_SHA256") or keys.get("SHA256")
assert version, f"version env var missing: {env}"
assert re.match(r"^\d+\.\d+\.\d+$", str(version)), version
assert sha256, f"sha256 env var missing: {env}"
assert re.match(r"^[0-9a-f]{64}$", str(sha256)), sha256
# The run script must invoke sha256sum -c to verify the downloaded asset.
run_script = step.get("run", "")
assert "sha256sum" in run_script and "-c" in run_script, run_script
PY
  [ "$status" -eq 0 ]
}

# Behavioural sanity check: if actionlint is installed locally, the workflows
# in this repo must actually pass it. Skipped on machines without actionlint
# so unit tests stay portable.
@test "actionlint passes against the current workflows on disk" {
  if ! command -v actionlint &>/dev/null; then
    skip "actionlint not installed locally"
  fi
  cd "$REPO_ROOT"
  # SC2016 is an intentional pattern in upgrade-dependencies.yml (literal echo
  # strings inside a heredoc); the workflow ignores it via -ignore, so mirror
  # that here for the local sanity check.
  run actionlint -no-color -ignore 'SC2016' .github/workflows/actionlint.yml .github/workflows/ci.yml .github/workflows/gitleaks.yml .github/workflows/markdown-lint.yml .github/workflows/security.yml .github/workflows/semgrep.yml .github/workflows/upgrade-dependencies.yml .github/workflows/wasm-bundle.yml
  echo "$output"
  [ "$status" -eq 0 ]
}
