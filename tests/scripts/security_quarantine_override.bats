#!/usr/bin/env bats
# Regression assertion for Issue #124 (SCR-QUARANTINE-OVERRIDE) — the repo
# enforces a release-age quarantine on dependency bumps
# (VIBE_BUMP_QUARANTINE_HOURS, Issue #76) but had no documented emergency
# fast-lane for bypassing that window when an actively-exploited advisory's
# fix is newer than the quarantine. These tests pin down that the documented
# override path (SECURITY.md) and the self-documenting workflow_dispatch input
# (emergency_bypass) both exist and stay wired.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
  UPGRADE_WF="${REPO_ROOT}/.github/workflows/upgrade-dependencies.yml"
}

@test "SECURITY.md exists" {
  [ -f "$SECURITY_MD" ]
}

@test "SECURITY.md documents the emergency quarantine override section" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'Emergency quarantine override' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md spells out the zero-hour bypass mechanism" {
  [ -f "$SECURITY_MD" ]
  # Both documented levers must appear: the env var set to 0 and the
  # equivalent bump-deps.sh flag.
  run grep -q 'VIBE_BUMP_QUARANTINE_HOURS=0' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q -- '--quarantine-hours 0' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md requires a clean cargo audit before merge on the fast lane" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'cargo audit' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md scopes the override to approver / actively-exploited cases" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'exploited' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "upgrade-dependencies.yml exposes an emergency_bypass workflow_dispatch input" {
  [ -f "$UPGRADE_WF" ]
  run grep -q 'emergency_bypass' "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}

@test "upgrade-dependencies.yml wires emergency_bypass to a zero-hour window" {
  [ -f "$UPGRADE_WF" ]
  # When the bypass input is set, the effective quarantine window must collapse
  # to 0 hours; otherwise the input would be cosmetic.
  run grep -Eq "emergency_bypass.*==.*'?true'?" "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}
