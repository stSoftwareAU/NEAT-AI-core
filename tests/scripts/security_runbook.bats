#!/usr/bin/env bats
# Regression assertion for Issue #123 (SCR-RUNBOOK) — the repo had a SECURITY.md
# covering the dependency-bump quarantine override, but no named disclosure
# contact and no written emergency-bump *response* procedure a responder could
# follow. These tests pin down that SECURITY.md now carries both the
# vulnerability-reporting channel and the emergency dependency-bump runbook,
# so disclosure no longer defaults to a public issue and the response steps no
# longer live only in maintainers' heads.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
}

@test "SECURITY.md exists" {
  [ -f "$SECURITY_MD" ]
}

@test "SECURITY.md has a vulnerability-reporting section" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'Reporting a vulnerability' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md names a disclosure contact address" {
  [ -f "$SECURITY_MD" ]
  run grep -q 'security@stsoftware.com.au' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md offers a private GitHub Security Advisory channel" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'Security Advisor' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md warns against public issues for embargoed advisories" {
  [ -f "$SECURITY_MD" ]
  # The disclosure guidance must explicitly steer reporters away from public
  # issues so embargoed advisories are not leaked.
  run grep -qi 'public issue' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md documents an emergency dependency-bump procedure" {
  [ -f "$SECURITY_MD" ]
  run grep -qi 'Emergency dependency bump' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "the emergency-bump runbook names the workflow and the bump-deps.sh fallback" {
  [ -f "$SECURITY_MD" ]
  # A responder must be able to find both levers: the dispatchable workflow and
  # the local script fallback.
  run grep -qi 'Upgrade Cargo Dependencies' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q 'bump-deps.sh' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}
