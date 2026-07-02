#!/usr/bin/env bats
# Tests for the settings-as-code branch-protection ruleset (Issue #208).
#
# Rationale: the CODEOWNERS rules on privileged CI paths only enforce a
# review at merge time once the default-branch ruleset sets
# `require_code_owner_review: true`. This file mirrors the live GitHub
# ruleset (id 15236989) so the intended controls are reviewable in git and a
# repo admin can re-apply them. These tests parse the JSON and assert on the
# observable controls (which rules the ruleset enforces), not on source text.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  RULESET="${REPO_ROOT}/.github/rulesets/develop.json"
}

@test "the ruleset file exists and is valid JSON" {
  [ -f "$RULESET" ]
  run python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$RULESET"
  [ "$status" -eq 0 ]
}

@test "the ruleset targets the default branch with active enforcement" {
  run python3 - "$RULESET" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d.get("target") == "branch", d.get("target")
assert d.get("enforcement") == "active", d.get("enforcement")
assert "~DEFAULT_BRANCH" in d["conditions"]["ref_name"]["include"]
PY
  [ "$status" -eq 0 ]
}

@test "the ruleset requires code-owner review and at least one approval" {
  run python3 - "$RULESET" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
pr = next(r for r in d["rules"] if r["type"] == "pull_request")
p = pr["parameters"]
assert p["require_code_owner_review"] is True, p
assert p["required_approving_review_count"] >= 1, p
PY
  [ "$status" -eq 0 ]
}

@test "the ruleset blocks force-pushes and non-linear history" {
  run python3 - "$RULESET" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
types = {r["type"] for r in d["rules"]}
assert "non_fast_forward" in types, types      # blocks force-push
assert "required_linear_history" in types, types
PY
  [ "$status" -eq 0 ]
}

@test "the ruleset does not require signed commits (would reject CI bot pushes)" {
  # The Auto-format / Auto-increment CI jobs push unsigned commits with the
  # ACTIONS_PUSH PAT. A required_signatures rule would reject them, so the
  # settings-as-code file must not carry one.
  run python3 - "$RULESET" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
types = {r["type"] for r in d["rules"]}
assert "required_signatures" not in types, types
PY
  [ "$status" -eq 0 ]
}
