## Summary

Lift the reverse-topological backpropagation loop from NEAT-AI's
`wasm_activation/src/topological_backprop.rs` into `neat-core` as a plain
`pub fn propagate_topological_loop` over decoded Rust structs. The WASM
byte-buffer ABI stays in `wasm_activation` (separate repo); this crate now
exposes the generic algorithm so native consumers (CLI, scorer, discovery)
can run backprop without going through a packed binary buffer.

The new module:

- Publishes `NeuronType` (enum) and matching `NEURON_TYPE_*` `pub const`s
  so the TS↔WASM contract can share a single source of truth.
- Returns typed `PropagateOutcome` per neuron (`Skipped` / `NoChange` /
  `Special` / `Standard`) — native callers do not need the
  `±Infinity` sentinel encoding. The WASM shim in `wasm_activation`
  translates these variants back to the wire sentinels.
- Accumulates per-synapse deltas (`SynapseDelta`) matching the existing
  `accumulate_weight_single` 7-tuple layout.
- Reuses the existing helpers that already live in `neat-core`:
  `apply_fused_error_distribution`, `apply_distribute_elastic_error`,
  `apply_squash`, `apply_unsquash`, `accumulate_weight_single`,
  `accumulate_bias_single`.

Closes #9.

## Evidence

Backend-only change, no UI. Verified by `cargo test --workspace --lib`
passing (including 9 new tests in `topological_backprop::tests`) and the
full `./quality.sh` gate (fmt, clippy with `-D warnings`, deny, doc
with `-D warnings`, release build).

## Test Plan

New tests in `neat-core/src/topological_backprop.rs`:

- `neuron_type_round_trips_constants` — enum ↔ `NEURON_TYPE_*` const mapping.
- `neuron_type_is_terminal_upstream_matches_semantics` — input/constant
  never act as backprop sources.
- `no_expected_error_yields_no_change_for_output` — exact match returns
  `PropagateOutcome::NoChange`.
- `non_zero_error_produces_standard_outcome_and_upstream_delta` — output
  error propagates to the hidden layer and accumulates synapse deltas.
- `special_squash_yields_special_outcome` — IF neurons return
  `PropagateOutcome::Special { target_activation }`.
- `self_loop_synapse_is_blocked_from_upstream_feedback` — self-loop
  synapses accumulate zero deltas.
- `skipped_neurons_when_no_error_seeded` — zero seeded error ⇒ NoChange
  branch.
- `propagate_needed_false_on_output_skips_neuron` — sparseConfig mask
  suppresses output seeding.
- `normalise_gradients_reduces_multi_path_delta` — sqrt-scaling never
  *increases* per-hidden error magnitude.
