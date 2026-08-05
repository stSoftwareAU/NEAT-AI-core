#!/usr/bin/env bats
# Regression assertion for Issue #501 (BP-99c924a96121) — eleven PR summaries
# practise a test-oracle integrity and mutation-evidence protocol that no main
# document stated, so an agent reading AGENTS.md could ship a green-but-blind
# parity test again. These tests pin that the protocol now lives in AGENTS.md:
# the five oracle rules, and the characterisation-test exception to TDD.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS_MD="${REPO_ROOT}/AGENTS.md"
  # Slice each `##` section into its own file, so a rule cannot be satisfied by
  # wording that happens to sit somewhere else in AGENTS.md.
  ORACLE_SECTION="${BATS_TEST_TMPDIR}/oracles.md"
  TDD_SECTION="${BATS_TEST_TMPDIR}/tdd.md"
  section "## Oracles and mutation evidence" >"$ORACLE_SECTION"
  section "## TDD (required)" >"$TDD_SECTION"
}

# section <heading> — print the body between <heading> and the next `##`.
section() {
  awk -v heading="$1" '
    index($0, heading) == 1 && !inside { inside = 1; next }
    inside && /^## / { exit }
    inside { print }
  ' "$AGENTS_MD"
}

# heading_line <substring> — line number of the first `##` heading containing it.
heading_line() {
  grep -n '^## ' "$AGENTS_MD" | grep -F "$1" | head -1 | cut -d: -f1
}

@test "AGENTS.md has an Oracles and mutation evidence section" {
  run grep -q '^## Oracles and mutation evidence' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  [ -s "$ORACLE_SECTION" ]
}

@test "the oracle section follows the what-not-how testing section" {
  what_not_how="$(heading_line 'Testing: ')"
  oracles="$(heading_line 'Oracles and mutation evidence')"
  [ -n "$what_not_how" ]
  [ -n "$oracles" ]
  [ "$oracles" -gt "$what_not_how" ]
}

@test "rule 1: an oracle must not share the code path under test" {
  run grep -qi 'must not share' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  # An independent reference honestly costs bit-exactness — say so.
  run grep -qi 'toleran' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "rule 2: a collapse of N copies shows every former site dies under mutation" {
  run grep -qi 'mutat' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  run grep -qi 'every former' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "rule 3: no vacuous oracles — derive the expected value" {
  run grep -qi 'vacuous' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  run grep -qF 'is_finite()' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  run grep -qi 'derivation' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "rule 4: a gate self-test compiles the live pattern, not a private copy" {
  run grep -qi 'live pattern' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  # The quoted-heredoc rule that makes one shared definition workable in bats.
  run grep -qF "<<'PY'" "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "rule 5: test the oracle directly when production cannot reach the edge case" {
  run grep -qi 'synthetic' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  run grep -qi 'test the oracle' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "the differential-oracle pattern keeps the pre-change implementation as reference" {
  run grep -qi 'differential' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
  run grep -qi 'pre-change' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "the oracle section draws the blind-vs-independent oracle as a mermaid diagram" {
  run grep -q '^```mermaid' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "the oracle section names the archived PR summaries it absorbs" {
  run grep -qE '#(409|476|478|479)' "$ORACLE_SECTION"
  [ "$status" -eq 0 ]
}

@test "TDD section carries the characterisation-test exception for pure extractions" {
  run grep -qi 'characterisation' "$TDD_SECTION"
  [ "$status" -eq 0 ]
  run grep -qi 'extraction' "$TDD_SECTION"
  [ "$status" -eq 0 ]
}
