#!/usr/bin/env bats
# Regression assertion for Issue #661 (BP-1e702c568704) — the live rustdoc and
# comment text of `neat-core`'s primary sources named the private
# `stSoftwareAU/GRQ` repository's issue numbers ("GRQ #4257", "GRQ #4261",
# "GRQ #4277") and its internal artefact paths ("GRQ-10 sampler",
# "GRQ-sampler/samples/…", "GRQ-10-sloth.log") directly.
#
# NEAT-AI-core is public, and these are the module- and function-level docs a
# public reader (or docs.rs) surfaces first, so those references pointed readers
# at material they cannot open and disclosed the layout of a private production
# system — check 3 of the private-repo-reference audit. The technical
# explanations (float round-trip exactness, the two memetic-weight wire forms,
# the same-role fan-in rule) survive at concept level; only the private names go.
#
# These are "what" tests over the committed artefacts — they read the published
# sources and assert on the observable outcome (no private-repo token, no
# private issue slug, no private artefact path), the same artefact-content style
# as tests/scripts/perf_private_repo_reference.bats and
# tests/scripts/archive_pr_summaries_private_repo_reference.bats.
#
# Scope note: the private repo name is matched on **word boundaries** (`-w`), so
# a snake_case identifier that merely embeds the letters is not flagged. That is
# deliberate and matches the guard for the archived summaries —
# `production_exact_matches_committed_grq_topology` is this repository's own
# live test-function name, referenced by `benches/BASELINE.md` and by two
# archived PR summaries; renaming it would dangle those records (see the scope
# notes in pr-summary-376.md and pr-summary-378.md).

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  CORE="${REPO_ROOT}/neat-core"
  CORE_FILES=(
    "${CORE}/src/creature.rs"
    "${CORE}/src/creature_validate.rs"
    "${CORE}/tests/creature_float_roundtrip.rs"
    "${CORE}/tests/creature_memetic_weight_forms.rs"
    "${CORE}/tests/creature_validate_synapse_rules.rs"
    "${CORE}/Cargo.toml"
    "${CORE}/benches/BASELINE.md"
  )
}

@test "every enumerated neat-core source is present" {
  for f in "${CORE_FILES[@]}"; do
    [ -f "$f" ] || {
      echo "missing source: $f"
      return 1
    }
  done
}

# --- No private repository name ---------------------------------------------

@test "neat-core sources name no private production-trainer repository" {
  # Word-boundary match: `GRQ`, `GRQ-10`, `GRQ's`, `GRQ #4257` all trip;
  # snake_case identifiers embedding the letters do not (see scope note above).
  run grep -niw 'GRQ' "${CORE_FILES[@]}"
  [ "$status" -ne 0 ]
}

# --- No private issue slug ---------------------------------------------------

@test "neat-core sources reference no private repository path or issue slug" {
  run grep -nE 'stSoftwareAU/GRQ|GRQ ?#[0-9]|GRQ-[0-9]' "${CORE_FILES[@]}"
  [ "$status" -ne 0 ]
}

# --- No private internal artefact paths --------------------------------------

@test "neat-core sources name none of the private trainer's internal artefacts" {
  run grep -nE 'GRQ-sampler|GRQ-[0-9]+-[a-z]+\.(json|log)|-sloth\.log' \
    "${CORE_FILES[@]}"
  [ "$status" -ne 0 ]
}
