#!/usr/bin/env bats
# Regression assertion for Issue #501 (BP-99c924a96121) — the test-oracle
# integrity and mutation-evidence protocol was practised and justified in nine
# archived PR summaries (#387, #388, #409, #442, #443, #446, #476, #478, #479,
# #480) but stated in no main document, so an agent reading AGENTS.md could
# still ship a green-but-blind parity test. These tests pin the protocol to
# AGENTS.md: the section exists, sits with the rest of the testing guidance, and
# states each of the five rules plus the characterisation-test exception to TDD.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS="${REPO_ROOT}/AGENTS.md"
}

# Body of the H2 named $1, up to the next H2. Fails loud on an empty body so a
# renamed heading cannot make every assertion below pass vacuously.
section_body() {
  local heading="$1" body
  body="$(awk -v h="$heading" '
    index($0, "## ") == 1 { f = (substr($0, 4) == h) ? 1 : 0; next }
    f' "$AGENTS")"
  if [[ -z "${body//[[:space:]]/}" ]]; then
    echo "AGENTS.md has no body under '## ${heading}'" >&2
    return 1
  fi
  printf '%s\n' "$body"
}

# assert_states <text> <ERE> <what the rule must say> — non-zero, with a named
# cause, when the documented rule is missing.
assert_states() {
  local text="$1" pattern="$2" what="$3"
  if [[ ! "$text" =~ $pattern ]]; then
    echo "AGENTS.md does not state ${what} (no /${pattern}/)" >&2
    return 1
  fi
}

@test "AGENTS.md carries an oracle/mutation-evidence section" {
  run grep -c '^## Oracles and mutation evidence$' "$AGENTS"
  [ "$status" -eq 0 ]
  [ "$output" -eq 1 ]
}

@test "the oracle section follows the testing guidance it extends" {
  # It must sit immediately after `Testing: "what" not "how"` — the two are one
  # body of testing rules, and a reader stopping at the first must not miss the
  # second.
  run bash -c "grep -n '^## ' '$AGENTS' | grep -A1 'Testing: ' | tail -1"
  [ "$status" -eq 0 ]
  [[ "$output" == *"Oracles and mutation evidence"* ]]
}

@test "rule: an oracle must not share the code path under test" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" 'score_records_flat' \
    'the flat-scoring parity case (#409)' || failed=1
  assert_states "$section" 'score_batch_into' \
    'the shared kernel that made the old oracle blind' || failed=1
  assert_states "$section" '[Ii]ndependent' \
    'that the reference must be independent of the code under test' || failed=1
  assert_states "$section" '1e-3' \
    'the honest tolerance an independent oracle forces' || failed=1
  return "$failed"
}

@test "rule: a refactor collapsing N copies proves each former site dies" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" '[Mm]utat' 'that the evidence is mutation' || failed=1
  assert_states "$section" '(every|each) former (copy|site)' \
    'that every former copy must die under mutation, per site' || failed=1
  assert_states "$section" 'eleven|11' \
    'the #443 sweep across all eleven former copies' || failed=1
  return "$failed"
}

@test "rule: no vacuous oracles — derive the expected value" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" 'is_finite\(\)' \
    'the banned is_finite() oracle shape (#479)' || failed=1
  assert_states "$section" 'vacuous' 'the vacuous-oracle rule by name' || failed=1
  assert_states "$section" '[Dd]eriv' \
    'that the expected value is derived, with the derivation written down' \
    || failed=1
  return "$failed"
}

@test "rule: a gate self-test compiles the live pattern, not a copy" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" 'live (pattern|regex|gate)' \
    'that a gate self-test must compile the live pattern (#478)' || failed=1
  assert_states "$section" "<<'PY'" \
    'the quoted-heredoc rule for a shared pattern' || failed=1
  return "$failed"
}

@test "rule: test the oracle directly when production cannot reach an edge case" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" 'synthetic|directly' \
    'that the oracle itself is tested with synthetic input (#476)' || failed=1
  assert_states "$section" 'split_batch_records' \
    'the latent len0-for-len2 slip that motivates it' || failed=1
  return "$failed"
}

@test "the differential-oracle pattern is stated" {
  local section failed=0
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  assert_states "$section" '[Dd]ifferential' \
    'the differential-oracle pattern (#387/#388)' || failed=1
  assert_states "$section" 'verbatim' \
    'that the pre-change implementation is kept verbatim as the reference' \
    || failed=1
  return "$failed"
}

@test "the section cites the archived summaries it absorbs" {
  local section failed=0 pr
  section="$(section_body 'Oracles and mutation evidence')" || return 1
  for pr in 387 388 409 443 476 478 479; do
    assert_states "$section" "#${pr}" "the source of the rule it absorbs (#${pr})" \
      || failed=1
  done
  return "$failed"
}

@test "TDD section records the characterisation-test exception" {
  local section failed=0
  section="$(section_body 'TDD (required)')" || return 1
  assert_states "$section" '[Cc]haracterisation' \
    'the characterisation-test exception for pure extractions (#442)' || failed=1
  assert_states "$section" 'extraction' \
    'that the exception is scoped to a pure extraction' || failed=1
  # Australian spelling throughout.
  if grep -qi 'characterization' "$AGENTS"; then
    echo "AGENTS.md uses US spelling 'characterization'" >&2
    failed=1
  fi
  return "$failed"
}
