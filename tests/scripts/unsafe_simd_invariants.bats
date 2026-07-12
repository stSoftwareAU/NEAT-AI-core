#!/usr/bin/env bats
# Regression assertion for Issue #259 (BP-bc92894e647a) — the SIMD / unsafe /
# buffer-reuse engineering campaign left durable invariants in the PR-summary
# archive that never reached the agent instruction files. These tests pin down
# that the invariants now live in AGENTS.md (unsafe/SIMD contract, buffer-reuse
# hazard) and that the load-time index-validation memory-safety note lives in
# SECURITY.md, so a future refactor cannot silently regress them.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS_MD="${REPO_ROOT}/AGENTS.md"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
}

# --- AGENTS.md: Unsafe & SIMD invariants section --------------------------

@test "AGENTS.md exists" {
  [ -f "$AGENTS_MD" ]
}

@test "AGENTS.md has an Unsafe & SIMD invariants section" {
  run grep -qi 'Unsafe .* SIMD invariants' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the unsafe_op_in_unsafe_fn deny floor" {
  run grep -q 'unsafe_op_in_unsafe_fn' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md states already-enabled intrinsics must not be wrapped in unsafe" {
  # The "don't wrap a safe intrinsic" rule must name the lint it trips.
  run grep -q 'unused_unsafe' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md documents the SAFETY-names-the-guard convention" {
  run grep -q '// SAFETY:' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q '_feature_detected!' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md documents the buffer-reuse reset + state-leak test rule" {
  run grep -qi 'buffer reuse' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qi 'state.leak' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md names get_unchecked and its load-time validation guard" {
  run grep -q 'get_unchecked' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q 'InvalidSynapseIndex' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md draws the invariant chain as a mermaid diagram" {
  run grep -q '```mermaid' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

# --- SECURITY.md: load-time memory-safety invariant -----------------------

@test "SECURITY.md records the load-time memory-safety invariant" {
  run grep -q 'InvalidSynapseIndex' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q 'get_unchecked' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md states the from_index check must never be removed" {
  run grep -qi 'from_index' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -qi 'never be removed' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}
