#!/usr/bin/env bats
# Tests for the advisory-triggered security-update channel (Issue #121).
#
# Rationale: the weekly `upgrade-dependencies.yml` cron is the only mechanism
# that *raises* a dependency-bump PR. An advisory disclosed mid-week therefore
# sits undefended-by-automation for up to six days. A `.github/dependabot.yml`
# with the Cargo ecosystem enabled gives Dependabot a security-updates channel
# that opens a fix PR the moment a RustSec/OSV advisory lands against a crate
# already in the tree — decoupling the urgent patch path from the routine
# weekly bump.
#
# These are "what" tests: they parse the YAML and assert on observable
# configuration outcomes (version, ecosystem, directory, schedule), not on
# source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  DEPENDABOT_FILE="${REPO_ROOT}/.github/dependabot.yml"
}

@test "dependabot config file exists" {
  [ -f "$DEPENDABOT_FILE" ]
}

@test "dependabot config is valid YAML using schema version 2" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml
with open("$DEPENDABOT_FILE") as fh:
    data = yaml.safe_load(fh)
assert isinstance(data, dict), "top level must be a mapping"
assert data.get("version") == 2, f"expected version 2, got {data.get('version')!r}"
PY
  [ "$status" -eq 0 ]
}

@test "dependabot enables the cargo ecosystem at the workspace root" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml
with open("$DEPENDABOT_FILE") as fh:
    data = yaml.safe_load(fh)
updates = data.get("updates") or []
cargo = [u for u in updates if u.get("package-ecosystem") == "cargo"]
assert cargo, "no cargo package-ecosystem entry found"
roots = [u for u in cargo if u.get("directory") == "/"]
assert roots, "cargo ecosystem must target the workspace root '/'"
PY
  [ "$status" -eq 0 ]
}

@test "cargo ecosystem entry declares an update schedule" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml
with open("$DEPENDABOT_FILE") as fh:
    data = yaml.safe_load(fh)
cargo = [u for u in (data.get("updates") or []) if u.get("package-ecosystem") == "cargo"]
assert cargo, "no cargo package-ecosystem entry found"
for u in cargo:
    schedule = u.get("schedule") or {}
    assert schedule.get("interval"), "cargo ecosystem entry must declare schedule.interval"
PY
  [ "$status" -eq 0 ]
}

# Security-update PRs are raised independently of the version-update schedule,
# but a bounded open-pull-requests-limit keeps the advisory fast-lane from
# being throttled to the default of 5. Assert the cargo entry sets one.
@test "cargo ecosystem entry sets an open-pull-requests-limit" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import sys, yaml
with open("$DEPENDABOT_FILE") as fh:
    data = yaml.safe_load(fh)
cargo = [u for u in (data.get("updates") or []) if u.get("package-ecosystem") == "cargo"]
assert cargo, "no cargo package-ecosystem entry found"
for u in cargo:
    limit = u.get("open-pull-requests-limit")
    assert isinstance(limit, int) and limit > 0, \
        f"cargo ecosystem entry must set a positive open-pull-requests-limit, got {limit!r}"
PY
  [ "$status" -eq 0 ]
}
