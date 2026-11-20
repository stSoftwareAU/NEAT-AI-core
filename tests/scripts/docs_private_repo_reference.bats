#!/usr/bin/env bats
# Regression assertion for PR #774 review — the "Family writer audit
# (Issue #767)" section of docs/CREATURE_JSON_PRESENTATION.md and its archived
# PR summary named the private production trainer and its private sample
# repository, with their commit SHAs, internal script paths with line numbers,
# production sample file names and host names. NEAT-AI-core is public, so those
# pointed readers at material they cannot open and disclosed the layout of a
# private production system — check 3 of the private-repo-reference audit.
#
# The earlier guards cover the README, neat-core sources, research docs, perf
# models and the archived PR summaries by repository *name* only. None of them
# read `docs/*.md`, and the archive guard cannot see an artefact name that
# carries no repository name. This guard closes both gaps.
#
# The guarded set is **discovered** (every top-level `docs/*.md`, plus every
# archived PR summary for the artefact patterns) rather than enumerated, so a
# document added after this file was written is covered too (Issue #722).
#
# Each pattern has exactly one definition, exported from setup() and compiled
# by both the sweep over the real files and the good/bad literal check, so
# gutting a pattern fails a test (AGENTS.md oracle rule 4).
#
# Scope note: the repository name is matched on word boundaries, as in
# tests/scripts/archive_pr_summaries_private_repo_reference.bats, so this
# repository's own snake_case test identifier is not flagged.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  DOCS="${REPO_ROOT}/docs"
  ARCHIVE="${DOCS}/archive/pr-summaries"
  DOC_FILES=()
  for doc in "${DOCS}"/*.md; do
    if [ -f "$doc" ]; then
      DOC_FILES+=("$doc")
    fi
  done
  ARCHIVE_FILES=()
  for summary in "${ARCHIVE}"/*.md; do
    if [ -f "$summary" ]; then
      ARCHIVE_FILES+=("$summary")
    fi
  done
  # Private repository names (word-boundary, case-insensitive).
  export NAME_PATTERN='GRQ|GRQ-sampler|GRQ-logs'
  # Private artefact names: the trainer's worker scripts and saver, the sample
  # check-in script, and the `samples/` layout of the private sample repository.
  export ARTEFACT_PATTERN='format_creature_json|creature_provenance_guard|cluster_creature_provenance|patch_creature_tags|saveValidatedCreatureExport|checkin-sample|train-extensions|worker/(Backprop|Lamarck|Forests|Ockham|Rebase)/|samples/<host>|samples/host|Enceladus'
  # A 40-hex commit SHA.
  export SHA_PATTERN='\b[0-9a-f]{40}\b'
}

@test "discovery covers every top-level docs markdown file and archived summary" {
  [ "${#DOC_FILES[@]}" -gt 0 ]
  [ "${#ARCHIVE_FILES[@]}" -gt 0 ]
  docs_present=$(find "${DOCS}" -maxdepth 1 -type f -name '*.md' | wc -l | tr -d '[:space:]')
  [ "${#DOC_FILES[@]}" -eq "$docs_present" ]
  # The document that carried the audit must be among the guarded files.
  found=0
  for doc in "${DOC_FILES[@]}"; do
    if [ "$(basename "$doc")" = "CREATURE_JSON_PRESENTATION.md" ]; then
      found=1
    fi
  done
  [ "$found" -eq 1 ]
}

@test "top-level docs name no private repository" {
  run grep -Eniw "$NAME_PATTERN" "${DOC_FILES[@]}"
  [ "$status" -ne 0 ]
}

@test "top-level docs and archived PR summaries name no private artefact" {
  run grep -Eni "$ARTEFACT_PATTERN" "${DOC_FILES[@]}" "${ARCHIVE_FILES[@]}"
  [ "$status" -ne 0 ]
}

@test "a full commit SHA in top-level docs sits on a public NEAT-AI repository row" {
  # Every 40-hex SHA the docs carry must be a public sibling's audited revision,
  # so a private repository's commit cannot be recorded here.
  run bash -c 'grep -HnE "$SHA_PATTERN" "$@" | grep -v "stSoftwareAU/NEAT-AI"' _ "${DOC_FILES[@]}"
  [ "$status" -ne 0 ]
}

# --- The patterns themselves can fail ----------------------------------------

@test "the name pattern matches private names and spares this repository's identifier" {
  run grep -Eiw "$NAME_PATTERN" <<<'the GRQ-sampler holds samples'
  [ "$status" -eq 0 ]
  run grep -Eiw "$NAME_PATTERN" <<<'GRQ trainer'
  [ "$status" -eq 0 ]
  run grep -Eiw "$NAME_PATTERN" <<<'production_exact_matches_committed_grq_topology'
  [ "$status" -ne 0 ]
}

@test "the artefact pattern matches private artefact names and spares public paths" {
  run grep -Ei "$ARTEFACT_PATTERN" <<<'worker/Lamarck/run.sh:363'
  [ "$status" -eq 0 ]
  run grep -Ei "$ARTEFACT_PATTERN" <<<'format_creature_json.sh:55'
  [ "$status" -eq 0 ]
  run grep -Ei "$ARTEFACT_PATTERN" <<<'`lamarck/src/run.rs:3116` writes best.json'
  [ "$status" -ne 0 ]
}

@test "the SHA pattern matches a full SHA and spares an eight-character one" {
  run grep -E "$SHA_PATTERN" <<<'`6639854db73450ce05d78e436d092f69df297285`'
  [ "$status" -eq 0 ]
  run grep -E "$SHA_PATTERN" <<<'NEAT-AI commit `221facd8`'
  [ "$status" -ne 0 ]
}
