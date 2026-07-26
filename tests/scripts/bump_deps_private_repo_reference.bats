#!/usr/bin/env bats
# Regression assertion for Issue #377 (BP-285dade59c14) — the bump-deps.sh
# header comment named the private `stSoftwareAU/VibeCoding` repository twice, as
# issue slugs ("per the contract in stSoftwareAU/VibeCoding#1613" and "see
# stSoftwareAU/VibeCoding#1614"). NEAT-AI-core is public; VibeCoding is not, so a
# public reader following those pointers hits material they cannot open and
# learns of a private orchestration repository. This guard pins the header at
# concept level: it states the contract inline and names no private repo.
#
# These are "what" tests over the committed artefact — they read bump-deps.sh
# and assert on the observable outcome (no private-repo token, no private issue
# slug), the same artefact-content style as
# tests/scripts/private_repo_reference.bats. They also confirm the concept-level
# contract wording survives, so the header stays informative after the rewrite.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/bump-deps.sh"
}

@test "bump-deps.sh exists" {
  [ -f "$SCRIPT" ]
}

# --- No private repository name ---------------------------------------------

@test "bump-deps.sh names no private repository" {
  # The private repo name is a token; match on word boundaries so unrelated
  # substrings do not trip the check.
  run grep -niw 'VibeCoding' "$SCRIPT"
  [ "$status" -ne 0 ]
}

@test "bump-deps.sh references no private repository path or issue slug" {
  run grep -nE 'stSoftwareAU/VibeCoding|VibeCoding#[0-9]' "$SCRIPT"
  [ "$status" -ne 0 ]
}

# --- Concept-level contract wording survives --------------------------------

@test "bump-deps.sh still describes the quarantine-honouring refresh contract" {
  run grep -niE 'quarantine' "$SCRIPT"
  [ "$status" -eq 0 ]
}

@test "bump-deps.sh still notes NEAT-AI-core has no internal pins to refresh" {
  run grep -niE 'no internal pins to refresh' "$SCRIPT"
  [ "$status" -eq 0 ]
}
