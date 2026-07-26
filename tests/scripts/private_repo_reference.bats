#!/usr/bin/env bats
# Regression assertion for Issue #374 (BP-1a729aa65c12) — the wasm64 research
# docs named the private `stSoftwareAU/GRQ` repository (and its private data
# sibling `GRQ-logs`) throughout, and linked private issue URLs. NEAT-AI-core is
# public, so a reader following those references hits material they cannot see
# and learns the structure of a private production system. These tests pin the
# docs at concept level: no private-repo name, no private issue link, and the
# lane (d) filename no longer carries the private repo name.
#
# Issue #375 (BP-36e39327d964) extends the same guard to the acceptance-model
# sources under `tests/perf/`, whose comments named the private repo, its
# private issue numbers, its internal script paths and its `GRQ_`-prefixed
# environment overrides.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  RESEARCH="${REPO_ROOT}/docs/research"
  LANE_A="${RESEARCH}/wasm64-lane-a-4gb-ceiling-attribution.md"
  LANE_D="${RESEARCH}/wasm64-lane-d-learn-wiring-verification.md"
  LANE_D_OLD="${RESEARCH}/wasm64-lane-d-grq-learn-wiring-verification.md"
  README="${REPO_ROOT}/README.md"
  PERF="${REPO_ROOT}/tests/perf"
  PERF_SOURCES=(
    "${PERF}/learn_flags_wiring.ts"
    "${PERF}/learn_flags_wiring_test.ts"
    "${PERF}/learn_oome_repro.ts"
    "${PERF}/learn_oome_repro_test.ts"
  )
}

# --- Filename no longer carries the private repo name -----------------------

@test "lane (d) doc is renamed to drop the private repo name" {
  [ -f "$LANE_D" ]
}

@test "the old grq-named lane (d) filename is gone" {
  [ ! -f "$LANE_D_OLD" ]
}

# --- No private repo names in the published research docs --------------------

@test "lane (a) doc names no private repository" {
  run grep -Eiw 'GRQ|GRQ-logs' "$LANE_A"
  [ "$status" -ne 0 ]
}

@test "lane (d) doc names no private repository" {
  run grep -Eiw 'GRQ|GRQ-logs' "$LANE_D"
  [ "$status" -ne 0 ]
}

@test "neither research doc links a private issue slug" {
  run grep -rE 'stSoftwareAU/GRQ|GRQ#[0-9]' "$LANE_A" "$LANE_D"
  [ "$status" -ne 0 ]
}

# --- README link resolves to the renamed file -------------------------------

@test "README links the renamed lane (d) doc, not the old name" {
  run grep -q 'wasm64-lane-d-learn-wiring-verification.md' "$README"
  [ "$status" -eq 0 ]
  run grep -q 'wasm64-lane-d-grq-learn-wiring-verification.md' "$README"
  [ "$status" -ne 0 ]
}

# --- tests/perf acceptance models stay at concept level (Issue #375) ----------

@test "perf acceptance models name no private repository" {
  # Any GRQ occurrence — bare name, GRQ-logs, GRQ#NNNN, GRQ_ env override.
  run grep -nE 'GRQ' "${PERF_SOURCES[@]}"
  [ "$status" -ne 0 ]
}

@test "perf acceptance models reference no private internal script paths" {
  run grep -nE 'worker/(learn|node)\.sh|memory_calc\.sh|stage_fail_marker\.sh|MemoryCalc[A-Za-z]*\.ts' \
    "${PERF_SOURCES[@]}"
  [ "$status" -ne 0 ]
}

@test "perf acceptance models link the renamed lane (d) doc, not the old name" {
  run grep -n 'wasm64-lane-d-grq-learn-wiring-verification.md' "${PERF_SOURCES[@]}"
  [ "$status" -ne 0 ]
  run grep -q 'wasm64-lane-d-learn-wiring-verification.md' "${PERF}/learn_flags_wiring.ts"
  [ "$status" -eq 0 ]
}
