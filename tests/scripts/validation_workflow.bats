#!/usr/bin/env bats
# Tests for the CI `validation` job checkout hardening (Issue #320).
#
# The contract under test IS the CI wiring: actions/checkout writes the
# workflow GITHUB_TOKEN into .git/config by default, leaving a usable
# credential on disk for every later step in the job. The `validation` job only
# checks out the repo and runs read-only manifest/README/rustdoc checks — it
# never pushes back and fetches no private submodule — so the persisted
# credential is pure blast radius.
#
# Asserts on observable outcomes:
#   - ci.yml parses as YAML and defines a `validation` job,
#   - every actions/checkout step in that job sets persist-credentials: false.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "ci workflow defines a validation job" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
assert "validation" in data["jobs"], sorted(data["jobs"])
PY
  [ "$status" -eq 0 ]
}

@test "ci validation checkout does not persist credentials on disk" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
checkouts = [
    s for s in data["jobs"]["validation"]["steps"]
    if str(s.get("uses", "")).startswith("actions/checkout@")
]
assert checkouts, "no actions/checkout step found in validation"
for step in checkouts:
    with_ = step.get("with") or {}
    assert with_.get("persist-credentials") is False, (
        f"checkout persists credentials: {step}"
    )
PY
  [ "$status" -eq 0 ]
}
