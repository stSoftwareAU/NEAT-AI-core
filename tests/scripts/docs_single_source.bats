#!/usr/bin/env bats
# Regression assertion for Issue #264 (BP-694fec042ea8) — several operational
# procedures were maintained verbatim in two or three docs at once (emergency
# quarantine override, version-bump policy, CODEOWNERS/ruleset governance and
# the ruleset-apply id). Duplicated copies drift when one is edited and the
# others are forgotten. These tests pin down that each procedure now has a
# single authoritative home and the other locations point at it with a link,
# so the values that must stay identical (a flag name, the ruleset id, the
# version policy) live in exactly one prose location.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
  AGENTS="${REPO_ROOT}/AGENTS.md"
  RELEASING="${REPO_ROOT}/RELEASING.md"
  CODEOWNERS="${REPO_ROOT}/.github/CODEOWNERS"
  RULESET="${REPO_ROOT}/.github/rulesets/develop.json"
}

# --- Emergency quarantine override: authoritative in SECURITY.md ------------

@test "SECURITY.md is the single home for the emergency override procedure" {
  # The full override mechanism (both levers) must live in SECURITY.md.
  run grep -q 'VIBE_BUMP_QUARANTINE_HOURS=0' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q -- '--quarantine-hours 0' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "README defers the override procedure to SECURITY.md via a link" {
  run grep -q 'SECURITY.md#emergency-quarantine-override' "$README"
  [ "$status" -eq 0 ]
}

@test "README does not restate the override bypass levers" {
  # The condensed restatement (the bypass flag + zero-hour window) must not be
  # duplicated into README — those levers live only in SECURITY.md now.
  run grep -q -- '--quarantine-hours 0' "$README"
  [ "$status" -ne 0 ]
  run grep -q 'emergency_bypass: true' "$README"
  [ "$status" -ne 0 ]
}

@test "SECURITY.md keeps a single merged emergency section, not two" {
  # The two near-duplicate H2 sections were merged; only the quarantine
  # override heading remains as an H2 (## ...), no separate H2 runbook.
  run grep -c '^## Emergency ' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  [ "$output" -eq 1 ]
}

# --- Version-bump policy: authoritative in RELEASING.md ---------------------

@test "RELEASING.md is the authoritative versioning policy home" {
  run grep -qi 'Versioning policy' "$RELEASING"
  [ "$status" -eq 0 ]
}

@test "README defers the version policy to RELEASING.md" {
  run grep -q 'RELEASING.md' "$README"
  [ "$status" -eq 0 ]
  # The detailed pre-1.0 minor-bump example must not be restated in README.
  run grep -q '0.1.x → 0.2.0' "$README"
  [ "$status" -ne 0 ]
}

@test "AGENTS.md defers the version policy to RELEASING.md" {
  run grep -q 'RELEASING.md' "$AGENTS"
  [ "$status" -eq 0 ]
  # The detailed pre-1.0 minor-bump example must not be restated in AGENTS.md.
  run grep -q '0.1.x → 0.2.0' "$AGENTS"
  [ "$status" -ne 0 ]
}

# --- CODEOWNERS / ruleset governance: authoritative in SECURITY.md ----------

@test "the ruleset-apply id lives in exactly one prose location" {
  # SECURITY.md is the single prose home for the hard-coded ruleset id.
  run grep -q '15236989' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  # It must not be duplicated into the ruleset JSON comment.
  run grep -q '15236989' "$RULESET"
  [ "$status" -ne 0 ]
}

@test "develop.json points at SECURITY.md for the governance rationale" {
  run grep -q 'SECURITY.md' "$RULESET"
  [ "$status" -eq 0 ]
}

@test "CODEOWNERS points at SECURITY.md for the governance rationale" {
  run grep -q 'SECURITY.md' "$CODEOWNERS"
  [ "$status" -eq 0 ]
}
