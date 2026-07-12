#!/usr/bin/env bats
# Regression assertion for Issue #259 (BP-bc92894e647a) — the SIMD / `unsafe` /
# buffer-reuse engineering campaign left durable invariants stranded in the
# PR-summary archive. These tests pin the invariants down in the agent
# instruction files (AGENTS.md) and the load-time memory-safety note in
# SECURITY.md, so they are visible at the moment an agent edits simd_native.rs
# rather than only inside per-PR summaries.
#
# Following the security_runbook.bats precedent, these assert on documentation
# CONTENT — the content is the deliverable, so its presence is the observable
# outcome under test.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS_MD="${REPO_ROOT}/AGENTS.md"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
}

@test "AGENTS.md has an Unsafe & SIMD invariants section" {
  [ -f "$AGENTS_MD" ]
  run grep -qiE '^##+ .*Unsafe .*SIMD invariants' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the unsafe_op_in_unsafe_fn deny floor" {
  run grep -q 'unsafe_op_in_unsafe_fn' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the do-not-wrap-already-enabled-intrinsic rule" {
  # An already-enabled compute intrinsic inside #[target_feature] is safe and
  # must not be wrapped (trips unused_unsafe).
  run grep -qiE 'unused_unsafe' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qiE 'target_feature' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the // SAFETY: names-the-guard convention" {
  run grep -q 'SAFETY:' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qE 'is_.*_feature_detected' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the load-time from_index / get_unchecked soundness chain" {
  run grep -q 'get_unchecked' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q 'from_index' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q 'InvalidSynapseIndex' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the buffer-reuse rule (per-call reset + state-leak test + one-network-per-thread)" {
  run grep -qiE 'buffer[- ]reuse|reuse.*buffer|scratch buffer' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qiE 'state[- ]leak' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qiE 'per[- ]thread|per thread|thread owns' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md includes a Mermaid diagram for the invariant chain" {
  run grep -q '```mermaid' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md records the load-time index-validation memory-safety invariant" {
  [ -f "$SECURITY_MD" ]
  run grep -q 'InvalidSynapseIndex' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q 'get_unchecked' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q 'from_index' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md states the load-time check must never be removed" {
  run grep -qiE 'never be removed|must not be removed|never remove' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}
