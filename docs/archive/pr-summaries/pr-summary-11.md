## Summary
Lifted the `Clone` derive on `CompiledNetwork` and the batch-4-way regression tests from the unmerged `makefaster` branch (PR #2) into the milestone branch. Cloning the compiled network lets native tools (e.g. the NEAT-AI scorer) pin one network per worker thread for forward-only batch scoring, so each thread owns its own activation/hint/trace buffers. Closes #11.

## Changes
- Added a brief doc comment and `#[derive(Clone)]` to `CompiledNetwork` in `neat-core/src/network.rs`.
- Added a `#[cfg(test)] mod tests` block at the end of `neat-core/src/network.rs` with seven scenarios validating `activate_and_trace_batch_4way` against the single-record `activate_and_trace`.

## Evidence
Backend/library change only — no web interface to screenshot. Verified via:
- `cargo test --workspace` (all tests pass, including the seven new ones)
- `./quality.sh` (rustfmt, clippy, cargo-deny, tests, docs, release build all pass)

## Test Plan
New tests added to `neat-core/src/network.rs`:
- `test_batch_4way_matches_single_relu` — Standard squash (ReLU hidden + Identity output).
- `test_batch_4way_matches_single_tanh_logistic` — TANH + LOGISTIC squash.
- `test_batch_4way_minimum_aggregate` — MINIMUM aggregate (`squash_type = 32`).
- `test_batch_4way_maximum_aggregate` — MAXIMUM aggregate (`squash_type = 33`).
- `test_batch_4way_if_aggregate` — IF aggregate (`squash_type = 34`) with condition / positive / negative synapse types.
- `test_batch_4way_constant_neuron` — Constant neuron (`is_constant: true`) passed through identity.
- `test_batch_4way_multi_layer` — 2-hidden-layer ReLU network with an identity output.

Each test runs four inputs through both the single-record `activate_and_trace` and the batch `activate_and_trace_batch_4way`, then asserts every f32 in the batch result lies within `1e-5` of the corresponding single-record result. Tests call the public API and assert on observable outputs — no source-text inspection.
