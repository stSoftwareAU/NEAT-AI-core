# [perf] Reuse per-neuron scratch buffers in topological backprop

## Summary

The reverse-topological backprop loop in
`neat-core/src/topological_backprop.rs` allocated **nine fresh `Vec`s for
every neuron, on every backward pass** (`Vec::with_capacity(list_length)` ×9
inside the per-neuron loop). For a network with N neurons that is `9 × N`
allocations per backward pass — pure allocator churn, since the contents are
rebuilt from scratch each iteration.

This change hoists the nine scratch buffers to `let mut … = Vec::new()`
declarations **outside** the reverse-topo loop and `.clear()`s them at the top
of each neuron iteration. `Vec::clear()` retains capacity, so after the first
few neurons no further allocation occurs; the push-based fill logic is
unchanged. This is an **allocation-only** change — backprop output is
**bit-for-bit identical** (verified by an identical benchmark checksum, see
Evidence).

Closes #154.

## Change

```mermaid
flowchart TB
    subgraph Before["Before — 9 allocs per neuron"]
        B1[reverse-topo loop] --> B2[neuron i]
        B2 --> B3["9× Vec::with_capacity"]
        B3 --> B4[fill + use]
        B4 --> B5[drop all 9 Vecs]
        B5 --> B2
    end
    subgraph After["After — buffers reused"]
        A0["9× Vec::new (hoisted, once)"] --> A1[reverse-topo loop]
        A1 --> A2[neuron i]
        A2 --> A3["9× .clear (retain capacity)"]
        A3 --> A4[fill + use]
        A4 --> A2
    end
```

The nine reused buffers: `fused_squash_types`, `fused_hint_values`,
`fused_activations`, `fused_weights`, `from_activation_cache`,
`from_weight_cache`, `from_value_cache`, `synapse_idx_cache`,
`is_self_loop_cache`.

## Evidence (performance)

Benchmark: `neat-core/examples/backprop_scratch_bench.rs` — a dense
fully-connected MLP (144 neurons, 4608 synapses), 50 000 backward passes,
`cargo run --release`.

| Metric | Before (with_capacity ×9/neuron) | After (reused buffers) | Improvement |
|---|---|---|---|
| Per pass | 169.45 µs | 144.59 µs | **−14.7 %** |
| Throughput | 5 901 passes/s | 6 916 passes/s | **+17.2 %** |
| Output checksum | 235008000 | 235008000 | identical |

The identical checksum confirms the change is allocation-only with no
numerical effect. The gain is most visible on dense networks, exactly as the
issue predicted.

Reproduce:

```bash
cargo run --release --example backprop_scratch_bench
```

This is a backend/library change — there is no web UI to screenshot.

## Test Plan

- Added `topological_backprop::tests::scratch_buffer_reuse_does_not_leak_across_neurons`
  — a regression test for the key risk of buffer reuse: stale entries from a
  larger neuron processed earlier leaking into a smaller neuron processed later
  in the same pass. It computes an identical 2-inward output neuron in two
  contexts — (a) alone, and (b) immediately after a 3-inward neuron — and
  asserts the neuron outcome and both synapse deltas are bit-for-bit identical.
  This invariant holds both before and after the change, guarding the refactor.
- All existing backprop/training tests pass unchanged:
  `cargo test --workspace` → 145 lib tests + all integration tests green.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -D warnings` clean.
- `./quality.sh` — the Rust gates (fmt, clippy, tests, doc) pass. Four
  pre-existing `bats` failures in `ci_workflow_quarantine.bats` (tests 31/32/33/37,
  about `ci.yml` invoking `bump-deps.sh`) are unrelated to this change and fail
  identically on a clean checkout of the base branch.
