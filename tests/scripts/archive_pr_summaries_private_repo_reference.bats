#!/usr/bin/env bats
# Regression assertion for Issue #378 (BP-90cd524b1b9b) — thirteen archived PR
# summaries under `docs/archive/pr-summaries/` named the private
# `stSoftwareAU/GRQ` or `stSoftwareAU/VibeCoding` repositories by name or slug
# (e.g. the `stSoftwareAU/GRQ` slug, `GRQ#3508`, `GRQ-cluster/network.json`,
# `stSoftwareAU/VibeCoding#1613`). NEAT-AI-core is public and both of those
# repositories are private, so a reader following those pointers hits material
# they cannot open and learns the layout of a private production system —
# check 3 of the private-repo-reference audit.
#
# This guard pins the enumerated archives at concept level: no private-repo
# token, no private path or issue slug, while the historical narrative survives.
#
# These are "what" tests over the committed artefacts — they read the archived
# files and assert on the observable outcome, the same artefact-content style as
# tests/scripts/private_repo_reference.bats and
# tests/scripts/bump_deps_private_repo_reference.bats.
#
# Scope note: the guard matches the private repo names on **word boundaries**
# (`-w`), so snake_case identifiers that merely embed the letters are not
# flagged. That is deliberate — `production_exact_matches_committed_grq_topology`
# in pr-summary-286.md is this repository's own test-function name, and renaming
# it in an archive would dangle against the live test suite (see the scope note
# in pr-summary-376.md).

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  ARCHIVE="${REPO_ROOT}/docs/archive/pr-summaries"
  SUMMARIES=(
    "${ARCHIVE}/pr-summary-5.md"
    "${ARCHIVE}/pr-summary-6.md"
    "${ARCHIVE}/pr-summary-38.md"
    "${ARCHIVE}/pr-summary-228.md"
    "${ARCHIVE}/pr-summary-243.md"
    "${ARCHIVE}/pr-summary-246.md"
    "${ARCHIVE}/pr-summary-261.md"
    "${ARCHIVE}/pr-summary-286.md"
    "${ARCHIVE}/pr-summary-287.md"
    "${ARCHIVE}/pr-summary-288.md"
    "${ARCHIVE}/pr-summary-296.md"
    "${ARCHIVE}/pr-summary-298.md"
    "${ARCHIVE}/pr-summary-299.md"
  )
}

@test "every enumerated archived PR summary is present" {
  for summary in "${SUMMARIES[@]}"; do
    [ -f "$summary" ] || {
      echo "missing archive: $summary"
      return 1
    }
  done
}

# --- No private repository names --------------------------------------------

@test "archived PR summaries name no private production-trainer repository" {
  # Word-boundary match: `GRQ`, `GRQ-cluster`, `GRQ-23`, `GRQ#3508` all trip;
  # snake_case identifiers embedding the letters do not (see scope note above).
  run grep -niw 'GRQ' "${SUMMARIES[@]}"
  [ "$status" -ne 0 ]
}

@test "archived PR summaries name no private worker-orchestration repository" {
  run grep -niw 'VibeCoding' "${SUMMARIES[@]}"
  [ "$status" -ne 0 ]
}

@test "archived PR summaries reference no private repository path or issue slug" {
  run grep -nE 'stSoftwareAU/(GRQ|VibeCoding)|GRQ ?#[0-9]|VibeCoding#[0-9]' \
    "${SUMMARIES[@]}"
  [ "$status" -ne 0 ]
}

@test "pr-summary-299 carries no grq-prefixed private launcher identifier" {
  # The one snake_case case that is *not* covered by the word-boundary rule
  # above: `grq_fail_loud_exit_trap` is the private launcher's own shell
  # function, not an identifier this repository owns.
  run grep -niE 'grq_[a-z_]+' "${ARCHIVE}/pr-summary-299.md"
  [ "$status" -ne 0 ]
}

# --- The historical narrative survives the reword ---------------------------

@test "pr-summary-38 still states the pre-quality-gate bump contract" {
  run grep -niE 'worker-orchestration contract' "${ARCHIVE}/pr-summary-38.md"
  [ "$status" -eq 0 ]
}

@test "pr-summary-299 still records the learn-invocation wiring verification" {
  run grep -niE 'learn-invocation wiring' "${ARCHIVE}/pr-summary-299.md"
  [ "$status" -eq 0 ]
}

@test "pr-summary-296 still records the exit-133 heap-limit signature" {
  run grep -nE 'exit 133' "${ARCHIVE}/pr-summary-296.md"
  [ "$status" -eq 0 ]
}

@test "pr-summary-286 still records the production creature topology numbers" {
  run grep -nE '1,666 non-input' "${ARCHIVE}/pr-summary-286.md"
  [ "$status" -eq 0 ]
}
