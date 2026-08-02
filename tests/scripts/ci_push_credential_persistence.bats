#!/usr/bin/env bats
# Tests for the `ACTIONS_PUSH` PAT handling in the pushing CI jobs (Issue #483).
#
# Contract under test: the `version-increment` and `auto-format` jobs in
# .github/workflows/ci.yml check out the *PR head* and then execute code from
# that head (`./bump-deps.sh`, `cargo fmt`). A default checkout with
# `token: secrets.ACTIONS_PUSH` writes the PAT into .git/config, so PR-authored
# code runs for the whole job lifetime with the credential readable on disk.
# Only the final push needs it, so the PAT must be supplied just-in-time to that
# one step and never persisted.
#
# Asserts on the observable CI contract (which step can read the secret), not on
# private implementation detail.

setup() {
  load helpers
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
}

@test "version-increment keeps the push PAT off disk and out of PR-head steps" {
  require_python3
  run assert_just_in_time_push_credential "$WORKFLOW" version-increment ACTIONS_PUSH
  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
}

@test "auto-format keeps the push PAT off disk and out of PR-head steps" {
  require_python3
  run assert_just_in_time_push_credential "$WORKFLOW" auto-format ACTIONS_PUSH
  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
}

@test "no checkout anywhere in ci.yml persists the ACTIONS_PUSH PAT" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import re
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
ref = re.compile(r"\$\{\{[^}]*\bsecrets\.ACTIONS_PUSH\b")
offenders = []
for name, job in (data.get("jobs") or {}).items():
    for step in job.get("steps") or []:
        if not str(step.get("uses", "")).startswith("actions/checkout@"):
            continue
        with_ = step.get("with") or {}
        persisted = with_.get("persist-credentials") is not False
        if persisted and ref.search(yaml.safe_dump(with_)):
            offenders.append(name)
assert not offenders, f"jobs persisting ACTIONS_PUSH in .git/config: {offenders}"
PY
  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
}

@test "ci.yml documents the real scope of the ACTIONS_PUSH PAT" {
  # Issue #483: the header claimed a repository-scoped PAT while
  # upgrade-dependencies.yml called the same secret org-level. The repository
  # holds no Actions secrets, so ACTIONS_PUSH resolves from the organisation —
  # the header must not understate that blast radius.
  run grep -Eq '^#.*ACTIONS_PUSH.*org' "$WORKFLOW"
  [ "$status" -eq 0 ] || {
    echo "ci.yml header does not describe ACTIONS_PUSH as an org-level PAT" >&2
    return 1
  }
}
