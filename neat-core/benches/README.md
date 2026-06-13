# neat-core benchmarks

Criterion harness for the core hot paths (Issue #152). These benchmarks are
**opt-in only** — `harness = false` plus the `[[bench]]` target in
`neat-core/Cargo.toml` keeps them out of `cargo test` and the `quality.sh`
gate, so the CI runtime is unaffected.

## What is measured

`hot_paths.rs` covers the four hottest paths in the crate:

| Group | Function(s) under test | Sizes |
| --- | --- | --- |
| `forward_pass` | `CompiledNetwork::activate` | small ~50, medium ~500, large ~5000 neurons |
| `batched_scoring` | `activate_and_trace_batch_4way`, 8-record `mse_sum_batch_packed` | same three sizes |
| `backprop` | one `propagate_topological_loop` step | same three sizes |
| `weighted_sum_simd` | `weighted_sum_simd` family (single / no-bias / squares / 4- and 8-record) | 64-synapse block |
| `squash` | `apply_squash` / `apply_unsquash` over a spread of `SquashType`s | scalar |

Networks and inputs are built **once** outside the timed closure from a
fixed-seed PRNG with fixed topologies, and `criterion::black_box` guards inputs
and outputs. The harness is therefore deterministic, so before/after
comparisons across a code change are meaningful.

## Running

```bash
# Run every group.
cargo bench -p neat-core --bench hot_paths

# Run a single group (regex over benchmark ids).
cargo bench -p neat-core --bench hot_paths -- forward_pass
cargo bench -p neat-core --bench hot_paths -- backprop
```

> Use `--bench hot_paths` so Criterion's CLI flags are not handed to the
> default libtest harness on the library target.

## Comparing before vs after a change

Criterion stores a baseline under `target/criterion/` and reports the delta on
the next run automatically:

```bash
# 1. On the unchanged code, record a named baseline.
cargo bench -p neat-core --bench hot_paths -- --save-baseline before

# 2. Apply your change, then compare against it.
cargo bench -p neat-core --bench hot_paths -- --baseline before
```

Criterion prints the percentage change and whether it is statistically
significant. HTML reports (enabled via the `html_reports` feature) are written
to `target/criterion/report/index.html`.
