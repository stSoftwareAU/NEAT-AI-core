## Summary
Adds a deterministic, pure-data topology export for `CompiledNetwork` — DOT (Graphviz) and a minimal topology JSON — so networks can be inspected and visualised externally without pulling a GUI stack into `neat-core`. Closes #22.

New module `neat-core/src/topology_export.rs` provides:

- `CompiledNetwork::to_dot(num_outputs) -> String` — emits a `digraph CompiledNetwork { ... }` with one node per neuron (labelled with index, kind, squash function and bias) and one edge per synapse (labelled with weight and synapse type). Shapes encode node kind (`ellipse` for inputs, `box` for hidden, `doublecircle` for outputs, `diamond` for constants).
- `CompiledNetwork::to_topology_json(num_outputs) -> String` — emits pretty-printed JSON with `num_inputs`, `num_outputs`, `num_neurons`, and ordered `nodes`/`synapses` arrays.
- `NodeKind` enum, `squash_name`, `synapse_type_name`, and the free `to_dot`/`to_topology_json` functions, all re-exported from `lib.rs`.

Output is deterministic: nodes are emitted in ascending index order, synapses in storage order (grouped by target neuron then by per-target compilation order), and weights are formatted with fixed six-decimal-place precision.

`num_outputs` is passed in because `CompiledNetwork` does not itself record the output count — outputs are the last `num_outputs` neurons by construction.

## Evidence
This is a CLI/library change with no web interface. Evidence is test-based:

- `cargo test --workspace --lib --tests --all-features` — all tests pass (13 new + 397 existing).
- `./quality.sh` — passes cleanly (fmt, clippy `-D warnings`, check, tests, rustdoc `-D warnings`, release build).
- Module-level rustdoc includes a runnable usage example (verified by the doc-test build).

## Test Plan
New integration tests in `neat-core/tests/topology_export.rs`:

- **Happy path (DOT)** — valid `digraph` header/footer, one node per neuron, input/hidden/output kinds labelled, activation names labelled, one edge per synapse, weights and synapse types in edge labels.
- **Happy path (JSON)** — round-trips through `serde_json::from_str`, reports correct `num_inputs`/`num_outputs`/`num_neurons`, node kinds and squash names match, bias present for non-inputs and absent for inputs, synapse from/to/type correct.
- **Edge case** — minimal single-input → single-output network exports cleanly to both formats.
- **Determinism** — two calls on the same network return byte-identical output for both DOT and JSON; re-compiling the same creature JSON and exporting produces byte-identical output across both formats.
