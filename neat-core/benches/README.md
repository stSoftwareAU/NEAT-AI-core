# neat-core benchmarks

Criterion harness for the core hot paths (Issue #152). These benchmarks are
**opt-in only** — `harness = false` plus the `[[bench]]` target in
`neat-core/Cargo.toml` keeps them out of `cargo test` and the `quality.sh`
gate, so the CI runtime is unaffected.

## What is measured

`hot_paths.rs` covers the four hottest paths in the crate:

| Group | Function(s) under test | Sizes |
| --- | --- | --- |
| `forward_pass` | `CompiledNetwork::activate` | small ~50, medium ~500, large ~5000, `production`, `production_2x` |
| `batched_scoring` | `activate_and_trace_batch_4way`, 8-record `mse_sum_batch_packed` | same five shapes |
| `backprop` | one `propagate_topological_loop` step | same five shapes |
| `weighted_sum_simd` | `weighted_sum_simd` family (single / no-bias / squares / 4- and 8-record) | 64-synapse block |
| `squash` | `apply_squash` / `apply_unsquash` over a spread of `SquashType`s | scalar |

Networks and inputs are built **once** outside the timed closure from a
fixed-seed PRNG with fixed topologies, and `criterion::black_box` guards inputs
and outputs. The harness is therefore deterministic, so before/after
comparisons across a code change are meaningful.

### Network shapes

The three synthetic shapes are dense feedforward nets with a small input layer
and a constant fan-in. The two `production` shapes (Issue #176) mirror the real
production creature — a **wide, shallow** topology with a huge input layer, a
modest neuron count and a sparse, *varied* fan-in — which is gather-bound in a
way the dense shapes are not, so deltas measured only on the synthetic shapes
can be misleading.

| Shape | Inputs | Non-input neurons | Total neurons | Outputs | Avg fan-in | ~Synapses |
| --- | --- | --- | --- | --- | --- | --- |
| `small_50` | 8 | 42 | 50 | 4 | 12 (fixed) | ~0.5k |
| `medium_500` | 16 | 484 | 500 | 8 | 16 (fixed) | ~7.7k |
| `large_5000` | 32 | 4968 | 5000 | 16 | 24 (fixed) | ~119k |
| `production` | 2461 | 1673 | 4134 | 1 | ~13 (varied) | ~21.7k |
| `production_2x` | 4922 | 3346 | 8268 | 2 | ~13 (varied) | ~43.5k |

The `production` shape is synthesised from the seeded PRNG to match the real
creature's dimensions — the 3 MB `network.json` is **not** committed. Fan-in for
the production shapes is drawn per neuron (uniform around the ~13 average)
rather than held constant, so the gather pattern matches production sparsity.
`production_2x` doubles the neuron and synapse counts to cover #175's "or larger
creatures" requirement. The deterministic builders live in
`benches/common/mod.rs` and are exercised by the `bench_fixtures` integration
test.

## Running

```bash
# Run every group.
cargo bench -p neat-core --bench hot_paths

# Run a single group (regex over benchmark ids).
cargo bench -p neat-core --bench hot_paths -- forward_pass
cargo bench -p neat-core --bench hot_paths -- backprop

# Run only the production-scale shapes across every hot path.
cargo bench -p neat-core --bench hot_paths -- production
```

> Use `--bench hot_paths` so Criterion's CLI flags are not handed to the
> default libtest harness on the library target.

The `production` filter is a regex over benchmark ids, so it matches both the
`production` and `production_2x` shapes in `forward_pass`, `batched_scoring` and
`backprop`.

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
