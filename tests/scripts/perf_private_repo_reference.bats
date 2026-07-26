#!/usr/bin/env bats
# Regression assertion for Issue #375 (BP-36e39327d964) — the tests/perf/
# acceptance-model files named the private `stSoftwareAU/GRQ` repository, its
# internal scripts, and private issue numbers throughout their comments.
# NEAT-AI-core is public, so those references pointed public readers at material
# they cannot open and leaked the structure of a private production system. The
# tests themselves are self-contained and unchanged; only the prose moves to
# concept level ("the downstream production training system", "the learn
# launcher script").
#
# These are "what" tests over the published artefact — they read the committed
# perf sources and assert on the observable outcome (no private-repo token, no
# private issue slug, no internal script path), the same artefact-content style
# as tests/scripts/private_repo_reference.bats. They are not a source grep of
# implementation detail: the artefact's public-safety is the contract under test.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  PERF="${REPO_ROOT}/tests/perf"
  PERF_FILES=(
    "${PERF}/learn_flags_wiring.ts"
    "${PERF}/learn_flags_wiring_test.ts"
    "${PERF}/learn_oome_repro.ts"
    "${PERF}/learn_oome_repro_test.ts"
  )
}

@test "all four perf acceptance-model files exist" {
  for f in "${PERF_FILES[@]}"; do
    [ -f "$f" ]
  done
}

# --- No private repository name ---------------------------------------------

@test "perf sources name no private repository" {
  # The private repo name is an uppercase token; match on word boundaries so
  # unrelated substrings do not trip the check.
  run grep -nEiw 'GRQ|GRQ-logs' "${PERF_FILES[@]}"
  [ "$status" -ne 0 ]
}

@test "perf sources reference no private repository path or issue slug" {
  run grep -nE 'stSoftwareAU/GRQ|GRQ#[0-9]|GRQ-[0-9]' "${PERF_FILES[@]}"
  [ "$status" -ne 0 ]
}

# --- No private internal script / test paths --------------------------------

@test "perf sources name none of the private trainer's internal scripts" {
  run grep -nE 'worker/learn\.sh|worker/shared/|memory_calc\.sh|stage_fail_marker\.sh|node\.sh' \
    "${PERF_FILES[@]}"
  [ "$status" -ne 0 ]
}

@test "perf sources name none of the private trainer's CI test files" {
  run grep -nE 'MemoryCalcHeapSize|MemoryCalcHostFloor|test/worker/' "${PERF_FILES[@]}"
  [ "$status" -ne 0 ]
}

# --- In-repo doc links still resolve ----------------------------------------

@test "perf sources link only research docs that exist" {
  for doc in $(grep -ohE 'docs/research/[A-Za-z0-9._-]+\.md' "${PERF_FILES[@]}" | sort -u); do
    [ -f "${REPO_ROOT}/${doc}" ]
  done
}
