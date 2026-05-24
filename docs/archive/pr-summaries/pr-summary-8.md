## Summary

Lifts the six pure-computation topology helpers from NEAT-AI's
`wasm_activation/src/topology_ops.rs` into `neat-core` as ordinary
`pub fn` items, gated with `#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]`
so the same source builds for native CLI tools and WASM. Closes #8.

Functions and constants moved:

- `validate_topology`, `validate_topology_batch`
- `scan_available_connections`
- `compute_reverse_topological_order`
- `validate_structural_integrity`
- `detect_cycles`
- Topology error codes: `VALID`, `SELF_CONNECTION`, `BACKWARD_CONNECTION`,
  `SORT_ERROR_FROM`, `SORT_ERROR_TO`, `DUPLICATE_CONNECTION`
- Structural error codes: `STRUCTURAL_VALID`,
  `STRUCTURAL_SYNAPSE_TARGETS_INPUT`, `STRUCTURAL_CONSTANT_HAS_INWARD`,
  `STRUCTURAL_HIDDEN_NO_INWARD`, `STRUCTURAL_HIDDEN_NO_OUTWARD`,
  `STRUCTURAL_BIAS_NOT_FINITE`, `STRUCTURAL_IF_TOO_FEW_INWARD`,
  `STRUCTURAL_IF_MISSING_CONDITION`, `STRUCTURAL_IF_MISSING_POSITIVE`,
  `STRUCTURAL_IF_MISSING_NEGATIVE`

Duplicated numeric markers (`IF_SQUASH`, `SYN_STANDARD`, `SYN_CONDITION`,
`SYN_NEGATIVE`, `SYN_POSITIVE`) now resolve from the existing
`neat_core::SquashType` and `neat_core::SynapseType` enums rather than
hardcoded `u8` literals, preventing drift. A guard test asserts
`IF_SQUASH == SquashType::If as u8` and similar for synapse types.

The upstream `wasm_activation/src/topology_ops.rs` can now be reduced to
`pub use neat_core::{…}` re-exports when NEAT-AI bumps its
`neat-core` dependency.

## Evidence

Backend / CLI change — no web interface to screenshot. Verified via:

- `./quality.sh` passes cleanly (fmt, clippy `-D warnings`, deny, doc
  `-D warnings`, tests, release build).
- Native: `cargo test --lib -p neat-core topology_ops` — 32 tests pass.
- WASM: `cargo check --target wasm32-unknown-unknown -p neat-core`
  succeeds, confirming the `cfg_attr(target_arch = "wasm32", wasm_bindgen)`
  exports compile for both targets.

## Test Plan

All 32 unit tests ported from `wasm_activation/src/topology_ops.rs` live
in `neat-core/src/topology_ops.rs::tests`:

- `validate_topology`: valid, self-connection, backward, sort error
  (from/to), duplicate, empty, mismatched lengths.
- `scan_available_connections`: simple forward-only scan, skips-constant.
- `compute_reverse_topological_order`: simple chain, larger DAG.
- `validate_topology_batch`: multi-valid, mixed valid/invalid, empty.
- `validate_structural_integrity`: valid, synapse-targets-input,
  constant-has-inward, hidden-no-inward, hidden-no-outward,
  bias-not-finite, bias-NaN, IF-too-few-inward, IF-missing-negative,
  IF-valid.
- `detect_cycles`: acyclic, 2-node cycle, self-loop, 3-node cycle, empty.

Plus two new drift-guard tests: `if_squash_matches_squash_type` and
`synapse_type_constants_match_enum`.
