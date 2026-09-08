# neat-core benchmarks

Criterion harness for the core hot paths (Issue #152). These benchmarks are
**opt-in only** — `harness = false` plus the `[[bench]]` target in
`neat-core/Cargo.toml` keeps them out of `cargo test` and the `quality.sh`
gate, so the CI runtime is unaffected.

There are three bench targets:

- `hot_paths` — the per-record hot paths (always available).
- `parallel_scoring` — data-parallel record scoring throughput (Issue #179),
  **requires `--features parallel`**. Without the feature its `main` is a no-op
  shim, so the target still builds on every configuration.
- `aggregate_frequency` — forward, traced and batched-scoring throughput on the
  `production_exact` creature across a sweep of aggregate-squash frequency
  (0 / 10 / 50 / 100% of neurons on `Minimum`/`Maximum`/`If`, Issue #510). The
  `production*` fixtures are homogeneous `Tanh`, so this is the only committed
  fixture with aggregate neurons in a production-sized creature. Its `agg0pct`
  point contains no aggregate neuron at all, which makes it a **null control**:
  its session-to-session spread is this host's noise floor.

## What is measured

`hot_paths.rs` covers the hottest paths in the crate:

| Group | Function(s) under test | Sizes |
| --- | --- | --- |
| `forward_pass` | `CompiledNetwork::activate` | small ~50, medium ~500, large ~5000, `production`, `production_2x` |
| `batched_scoring` | `activate_and_trace_batch_4way`, 8-record `mse_sum_batch_packed` (all shapes), production-sized `mse_sum_batch_packed` (`mse_sum_production`, Issue #384) | 8-record: same five shapes; `mse_sum_production`: `production`, `production_2x`, `production_exact` |
| `backprop` | one `propagate_topological_loop` step | same five shapes |
| `reverse_topological_order` | `compute_reverse_topological_order` over a creature's full synapse list (Issue #388) | all six shapes |
| `scoring` | `CompiledNetwork::score_records_flat` over a production-sized record batch | `production`, `production_2x`, `production_exact` |
| `topology_ops` | `scan_available_connections` — the mutation-time availability scan (Issue #387); `compute_reverse_topological_order` — the per-creature backprop-ordering setup (Issue #388) | `n1666_21513`, `n4127_21513` |
| `weighted_sum_simd` | `weighted_sum_simd` family (single / no-bias / squares / 4- and 8-record). Measures the `*_unchecked` kernels, which are what the forward pass runs once `CompiledNetwork::new` has discharged the index precondition (Issue #613); the safe entry points of the same name add an `O(end - start)` bounds pre-pass for callers holding no loaded network | 64-synapse block |
| `squash` | `apply_squash` / `apply_unsquash` over a spread of `SquashType`s | scalar |
| `squash_x4` | `squash_x4` — the lane-parallel 4-record approximation (Issue #180) against a baseline of four scalar `apply_squash` calls, over the hot transcendental squashes `Tanh`, `Logistic`, `Gelu`, `Mish`. The `scalar_x4` id is the four-call baseline; `simd_x4` is the vectorised kernel | one fixed 4-lane block, `[0.42, -1.3, 2.7, -0.05]` |

The `scoring` group (Issue #228) pushes a full production-sized record batch
through one creature via `score_records_flat`, so `hot_paths` reports
single-core scoring throughput at production record volume alongside the
parallel harness. It covers only the gather-bound `production` shapes and is
reachable with the `production` filter (`--bench hot_paths -- production`).
Issue #408 retired the separate `scoring_flat` A/B group: with the per-record
entry point deprecated there is no second input layout left to compare against,
so `scoring` now *is* the flat measurement.

The `topology_ops` group (Issue #387) covers the mutation-time helpers, which
are not per-record hot paths but are paid repeatedly across the population every
generation. Its topologies are built locally in `hot_paths.rs` (they are edge
lists, not `CompiledNetwork`s): `n1666_21513` is the anchor named in #387 —
1,666 neurons carrying 21,513 synapses, a fill factor under 0.8% — and
`n4127_21513` puts the same synapse count on the full `production_exact` neuron
count. Throughput is reported per candidate slot (`n²`), so a reintroduced dense
`n × n` existence matrix or result-vector realloc chain shows up directly.

`compute_reverse_topological_order` (Issue #388) shares those two topologies but
reports throughput per graph element (`neurons + synapses`), the work its Kahn
walk actually does. Its inward adjacency is CSR, so a regression back to a
per-neuron `Vec<Vec<u32>>` — n + 1 allocations and a pointer chase per neuron —
shows up as a throughput drop here. `cargo run --release --example
reverse_topo_order_alloc_ab` measures the same function's allocation count
directly against the pre-#388 shape.

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
| `production_exact` | 2461 | 1666 | 4127 | 1 | ~12.9 (exact) | **21,513** |

`production_exact` (Issue #286) pins the fixture to the committed production
creature topology — 1,666 non-input neurons, **exactly**
21,513 synapses, 2,461 inputs — so the Criterion baseline is anchored to the
real production model rather than `production`'s ~13-average approximation.
Unlike the `VariedAround` shapes it uses `FanIn::ExactTotal`, which distributes
the 21,513 synapses across the 1,666 neurons as evenly as possible (a Bresenham
stride, so the two fan-in values — 12 and 13 — are interleaved, not clustered).
Its label starts with `production`, so the `production` regex filter and the
`scoring` group pick it up automatically.

The `production` shape is synthesised from the seeded PRNG to match the real
creature's dimensions — the 3 MB `network.json` is **not** committed. Fan-in for
the production shapes is drawn per neuron (uniform around the ~13 average)
rather than held constant, so the gather pattern matches production sparsity.
`production_2x` doubles the neuron and synapse counts to cover #175's "or larger
creatures" requirement. The deterministic builders live in
`benches/common/mod.rs` and are exercised by the `bench_fixtures` integration
test.

> **Fixture caveat — squash is uniformly `Tanh` (Issue #261).** The `squash`
> column above is not varied: every neuron in the `production` / `production_2x`
> / `production_exact` shapes is built with `SquashType::Tanh`
> (`benches/common/mod.rs`), locked
> by `tests/bench_fixtures.rs::production_fixture_squash_is_homogeneous_tanh`.
> Real production creatures also run `Gelu`/`Mish` (scalar `libm`), so on this fixture
> squash-vectorisation deltas are a **lower bound** and branch-prediction levers
> are **unmeasurable** (a homogeneous squash lets the predictor nail the
> one-arm `match`). See [`BASELINE.md`](BASELINE.md) for the full caveat.

## Optimisation levers learned (Issue #261)

Durable negative/counter-intuitive results from the #227 perf campaign, kept
here so the levers are **not re-attempted** and the numbers are read correctly.

- **The per-record squash cost was the *range lookup*, not branch
  misprediction (PR #245, 6–8% single-core win).** The #245 issue hypothesised
  the win would come from eliminating a mispredicted squash `match`. It did not:
  the fixture is homogeneous `Tanh`, so the predictor already nails that branch
  (see the fixture caveat above). The real lever was LLVM **failing to CSE**
  eight identical `apply_get_range(squash)` range-lookups per neuron — the
  intervening `NaN`/`±Inf` branches blocked the merge, so 7 of every 8 range
  `match`es per batch were redundant work. **Hoisting** the per-record range
  lookup removed it. Rule: hoist per-record range lookups; do **not** attribute
  a perf delta to an unverified cause (here, branch misprediction) — verify the
  actual lever before recording it.
- **Corollary for future squash/scoring work.** On a *varied*-squash creature
  the eliminated misprediction would stack on top of the range-hoist gain, so
  the committed homogeneous-`Tanh` numbers are a lower bound for production.
  Chasing branch-misprediction *on this fixture* will measure nothing — validate
  such levers against the real varied-squash creature, not the synthetic shape.
- **Unchecked indexing in the aggregate kernels buys nothing (Issue #510,
  negative result — do not re-attempt).** Removing the per-synapse bounds check
  from the `Minimum`/`Maximum`/`If` loops measured **−1.3% to −4.8%** end-to-end
  at the production-realistic ~10% aggregate frequency, against a **19–40%**
  session noise floor on this host. The 15–38% wins appear only at 50–100%
  aggregate — synthetic-only. An isolated A/B against *safe* attribution
  controls showed the little that was there comes from **slice iteration, not
  bounds-check removal**: rewriting `synapses[start + i]` as a span-slice walk
  recovers most of it with no `unsafe`. Full record:
  [`docs/research/aggregate-unchecked-kernels-2026-08-05.md`](../../docs/research/aggregate-unchecked-kernels-2026-08-05.md).
- **Aggregate frequency, not aggregate kernel cost, is the lever (Issue #510).**
  `score_batch_into` drops to the per-lane path when **any** neuron is an
  aggregate, so a 10%-aggregate creature loses the record-interleaved fast path
  for all of its neurons: 35.8 ms → 85.9 ms per 4,096-record shard, a **2.4×**
  cost. That is where scoring effort on aggregate creatures belongs — tracked as
  [#514](https://github.com/stSoftwareAU/NEAT-AI-core/issues/514).

## Running

```bash
# Run every group.
cargo bench -p neat-core --bench hot_paths

# Run a single group (regex over benchmark ids).
cargo bench -p neat-core --bench hot_paths -- forward_pass
cargo bench -p neat-core --bench hot_paths -- backprop

# Run only the production-scale shapes across every hot path.
cargo bench -p neat-core --bench hot_paths -- production

# Sweep aggregate-squash frequency on the production_exact creature.
cargo bench -p neat-core --bench aggregate_frequency
```

> Use `--bench hot_paths` so Criterion's CLI flags are not handed to the
> default libtest harness on the library target.

### Data-parallel scoring (Issue #179)

`parallel_scoring.rs` reports records/sec for scoring a batch through one
creature on **a single core versus all available cores**, using the
`production`/`production_2x`/`production_exact` shapes. It scores inside a rayon
pool of a fixed
size, so the 1-core and all-core measurements share one code path and differ
only in pool size.

The batch is `PRODUCTION_SCORING_RECORDS` records (defined in
`benches/common/mod.rs` and shared with the `hot_paths` `scoring` group). That
count is **calibrated to production record volume** — one production training
shard, ~4.3k records — rather than an arbitrary token batch. The derivation is
documented in [`BASELINE.md`](BASELINE.md).

```bash
# Requires the `parallel` feature (rayon, native targets only).
cargo bench -p neat-core --features parallel --bench parallel_scoring
```

The `production` filter is a regex over benchmark ids, so it matches both the
`production` and `production_2x` shapes in `forward_pass`, `batched_scoring`,
`backprop` and `scoring`.

## Documented baseline (Issue #228)

[`BASELINE.md`](BASELINE.md) records the committed production/production_2x
numbers for every hot-path group, with host/CPU/toolchain metadata. Every
optimisation under the #227 milestone must cite a before/after comparison
against it for the affected group.

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
