# neat-core benchmark baseline (Issue #228)

Documented baseline for the production-sized hot-path and scoring benchmarks.
Every optimisation under the #227 milestone (per-record allocation, 8-record
batched scoring, gather locality, prefetch, rayon granularity) is gated against
these numbers: an optimisation PR must cite a before/after comparison for the
affected group against this file, with matching host/toolchain metadata.

> **Bench-only baseline.** The benchmarks are `harness = false` and excluded
> from CI, so nothing here affects library behaviour or CI runtime. The
> fixtures are synthesised from a fixed-seed PRNG — the 3 MB production
> `network.json` and the real training corpus are **not** committed.

## Fixture caveat — squash homogeneity (Issue #261)

Every neuron in the `production` / `production_2x` / `production_exact` fixtures
is uniformly `SquashType::Tanh` (`benches/common/mod.rs`), asserted by
`tests/bench_fixtures.rs::production_fixture_squash_is_homogeneous_tanh`. Real
production creatures also run `Gelu`/`Mish` (scalar `libm`), so read every
`scoring`/`production` A/B below with two corrections in mind:

- **Squash-vectorisation deltas are a lower bound.** Varied-squash creatures
  also convert scalar `libm` `Gelu`/`Mish` to the vectorised path, gaining at
  least as much as the all-`Tanh` fixture shows.
- **Branch-prediction levers are unmeasurable here.** A homogeneous squash means
  the predictor already nails the one-arm `match`, so a branch-misprediction
  optimisation shows no delta on this fixture — that is a fixture artefact, not
  evidence the lever is worthless on real creatures.

The real production `network.json` is not on the build host, so it cannot be
A/B'd here directly; re-run against the pinned creature to confirm on the real
varied-squash topology.

## Host / toolchain

| Field | Value |
| --- | --- |
| Host class | Apple Silicon (Apple M4 Pro class) |
| CPU | Apple M4 Pro — 12 cores (8 performance + 4 efficiency) |
| Logical cores (`available_parallelism`) | 12 |
| RAM | 24 GB |
| OS | macOS 26.5.2 (arm64) |
| Toolchain | rustc 1.96.0 (ac68faa20 2026-05-25) |
| Criterion | 0.8.2 |
| Build | `--release` (bench profile, optimised) |

Re-running the seeded synthesis on the same host reproduces the same fixtures;
numbers should stay within Criterion's normal noise band (~±5%). A larger
deviation on the same host indicates the fixed-seed setup is broken.

## Production record-count calibration

`parallel_scoring` and the `hot_paths` `scoring` group score
`PRODUCTION_SCORING_RECORDS = 4096` records per iteration (defined in
`benches/common/mod.rs`), replacing the prior uncalibrated 2048-record token
batch. Derivation, from committed production-run telemetry
(a 32-generation production run whose totals
match the run summary: `generations = 32`, `total_time_ms = 3,042,879`,
`fitnessMs = 2,901,473` — ~95% of wall clock):

| Quantity | Value | Source |
| --- | --- | --- |
| Training corpus size | 22,097,375,712 bytes | `training_data_size_bytes` |
| Training shards | 520 | `training_data_files` |
| Per-record width | 2461 × 4 = 9844 bytes | `num_inputs` × `size_of::<f32>()` |
| Whole-corpus records | ≈ 2.24 million | 22,097,375,712 / 9844 |
| Records per shard | ≈ 4,317 | 2,244,755 / 520 |
| **Bench batch** | **4096** | one shard, rounded down to 2¹² |

A single creature forward-passes the whole ~2.24 M-record corpus per generation
to compute its fitness. Materialising all of it (~21 GiB at production width) is
infeasible for a micro-benchmark, so the harness scores **one production
shard's worth** (~40 MiB at ~9.8 KiB/record). That batch already far exceeds any
CPU cache, so its memory-traffic behaviour is production-representative, and
records/sec extrapolates directly to the full corpus pass (throughput is
size-invariant once pool/allocation overhead is amortised).

## `hot_paths` — single-thread (`cargo bench -p neat-core --bench hot_paths -- production`)

Times are Criterion's `[lower estimate upper]`; the median estimate is the
comparison point.

| Group / benchmark | production | production_2x |
| --- | --- | --- |
| `forward_pass` | 32.76 µs `[32.18, 33.34]` | 67.26 µs `[65.53, 69.10]` |
| `batched_scoring/trace_batch_4way` | 80.17 µs `[78.46, 81.99]` | 199.18 µs `[194.54, 204.15]` |
| `batched_scoring/mse_sum_8records` | 223.55 µs `[213.63, 234.10]` | 656.06 µs `[640.07, 672.17]` |
| `backprop` | 153.87 µs `[150.97, 156.84]` | 368.19 µs `[361.57, 375.62]` |
| `scoring` (4096 records) | 242.79 ms `[240.03, 245.76]` | 491.36 ms `[487.31, 495.73]` |

Throughput highlights: `forward_pass` ≈ 126 Melem/s (production) / 123 Melem/s
(production_2x); `backprop` ≈ 26.9 Melem/s / 22.5 Melem/s; single-core
`scoring` ≈ 16.9 Krecords/s / 8.34 Krecords/s.

## `parallel_scoring` — 1 core vs all cores (`cargo bench -p neat-core --bench parallel_scoring --features parallel`)

4096 records scored through one creature inside a fixed-size rayon pool.

| Shape | 1 core | 12 cores | records/s (1 → 12) | Speed-up |
| --- | --- | --- | --- | --- |
| `production` | 242.97 ms `[240.75, 245.23]` | 75.44 ms `[73.05, 78.03]` | 16.86 K → 54.30 K | 3.22× |
| `production_2x` | 487.26 ms `[484.49, 490.09]` | 137.78 ms `[133.86, 141.99]` | 8.41 K → 29.73 K | 3.54× |

The single-core `parallel_scoring` figure (16.86 Krecords/s for `production`)
matches the `hot_paths` `scoring` group (16.87 Krecords/s) — both drive the same
sequential `score_records_flat` path, an internal consistency check on the
fixture.

## Production-exact topology baseline (Issue #286)

Dated anchor for the **exact** committed production creature topology —
**1,666 non-input neurons, 21,513 synapses, 2,461 inputs** (4,127 total neurons,
one output). Unlike the `production` shape's ~13-average `VariedAround` fan-in,
the `production_exact` shape uses `FanIn::ExactTotal(21_513)`, which spreads the
synapses across the 1,666 neurons as evenly as possible (12 or 13 each,
Bresenham-interleaved) so the synapse count reproduces the real model to the
synapse. Seeded synthesis is deterministic and asserted exact by
`tests/bench_fixtures.rs::production_exact_matches_committed_grq_topology`.

**Measured 2026-07-18** on the same Apple M4 Pro host class, toolchain **rustc 1.97.0**
(the earlier `production`/`production_2x` rows above were taken on rustc 1.96.0,
so compare across sections only within a lane, not across toolchains).

| Field | Value |
| --- | --- |
| Host | Apple M4 Pro — 12 cores (8P + 4E), 24 GB, macOS (arm64) |
| Logical cores (`available_parallelism`) | 12 |
| Toolchain | rustc 1.97.0 |
| Criterion | 0.8.2 |
| Build | `--release` (bench profile) |

### Single-thread lane (`cargo bench -p neat-core --bench hot_paths -- production_exact`)

| Group / benchmark | production_exact | Throughput |
| --- | --- | --- |
| `forward_pass` | 30.76 µs `[30.19, 31.35]` | ~134 Melem/s |
| `batched_scoring/trace_batch_4way` | 122.65 µs `[120.98, 124.33]` | — |
| `batched_scoring/mse_sum_8records` | 242.08 µs `[239.09, 244.99]` | — |
| `backprop` | 214.14 µs `[210.46, 218.05]` | ~19.3 Melem/s |
| `scoring` (4096 records) | 91.50 ms `[90.05, 92.96]` | 44.77 Krecords/s |

### Parallel lane (`cargo bench -p neat-core --features parallel --bench parallel_scoring -- production_exact`)

4096 records scored through one creature inside a fixed-size rayon pool.

| Shape | 1 core | 12 cores | records/s (1 → 12) | Speed-up |
| --- | --- | --- | --- | --- |
| `production_exact` | 61.84 ms `[60.36, 63.38]` | 24.62 ms `[22.35, 26.98]` | 66.23 K → 166.4 K | 2.51× |

### Per-creature scoring latency → the project metric

The project metric is **score improvement per wall-clock hour**, and per
generation a single creature forward-passes the whole ~2.24 M-record corpus to
compute its fitness (see the record-count calibration above). At the measured
`production_exact` throughput that per-creature scoring pass costs:

| Lane | records/s | Per-creature corpus pass (~2.24 M records) |
| --- | --- | --- |
| Single core | 66.2 K | ≈ **33.9 s** |
| 12 cores | 166.4 K | ≈ **13.5 s** |

That per-creature latency is the denominator of the metric: halving it doubles
the creatures a fixed wall-clock budget can score, so it is the figure every
lane sub-issue's optimisation is measured against.

> **Scoring-lane variance (be honest about it).** The ~90 ms `hot_paths`
> `scoring` figure and the ~62 ms `parallel_scoring` `1_core` figure exercise the
> *same* sequential `score_records_flat` path, so in principle they match — but
> they were taken in separate `cargo bench` invocations and the ~90 ms → ~62 ms
> spread
> is real run-to-run variance (thermal state on the laptop-class M4 Pro under a
> ~90 ms single-shot benchmark with 100 iterations). Treat the **`parallel_scoring`
> 1-core/12-core pair as the authoritative scoring anchor** — both were measured
> in one invocation, so their 2.51× ratio is internally consistent — and read the
> single-core scoring latency as ~62–92 ms (±~20%). A lane sub-issue must A/B
> with `--save-baseline` inside a single invocation to stay inside this band.

## Record-interleaved scoring optimisation (Issue #287)

The single-thread scoring hot path (`score_records_flat` → `score_batch_into`,
the same lane NEAT-AI's per-creature wasm32 workers drive) now transposes each
group of eight records into a **record-interleaved** activation buffer:
lane `l` of source neuron `n` lives at `inter[n * 8 + l]`, so all eight records
for a synapse's source are contiguous. Each gather in
`weighted_sum_interleaved_8` is then one cache-line read (two adjacent 4-wide
loads on NEON / one `_mm256_loadu_ps` on AVX2 / one contiguous `f32x4` pair on
wasm `simd128`) instead of eight scattered per-lane loads, and each neuron's
eight outputs are a single contiguous store. This extends the #230 batched-SIMD
approach to the gather itself; it is layout-driven, so the win is portable
across the native and `wasm32` builds. Networks containing aggregate squashes
(Minimum/Maximum/If/Hypotenuse/HypotenuseV2/Mean) keep the original per-lane
path; the all-standard-squash production topology takes the fast path.

**Methodology (controls for the laptop thermal band above).** Because separate
`cargo bench` invocations drift, the A/B was run as **alternating** old/new
rounds of the *same* prebuilt bench binaries, capturing the unchanged
`forward_pass` benchmark alongside `scoring` as a drift control. `forward_pass`
stayed flat across old/new (34.65 µs vs 34.71 µs mean), confirming the `scoring`
delta is the code change, not thermal drift.

**Measured 2026-07-18**, Apple M4 Pro host class, rustc 1.97.0,
`--release`, 4 alternating rounds (Criterion `--sample-size 60`):

| `production_exact` (median) | old | new | change |
| --- | --- | --- | --- |
| `scoring` (4096 records) mean of 4 rounds | 101.4 ms | 48.1 ms | **−52% (≈2.1×)** |
| `forward_pass` control (unchanged) mean | 34.65 µs | 34.71 µs | flat |

Every round showed the interleaved path far faster on `scoring` (old
83/104/109/109 ms → new 49/50/53/40 ms). At the ~48 ms new scoring figure the
per-creature ~2.24 M-record corpus pass (see the latency table above) drops from
≈33.9 s toward the low-20s-of-seconds on a single core — a direct win on the
score-per-hour metric. `forward_pass` (the single-record `activate` path) is
untouched and unaffected. Numerics: full 8-record groups are bit-identical to
the prior per-lane 8-record path (same FMA order); the `records.len() % 8` tail
runs the exact single-record kernel, so single-record scoring stays
bit-for-bit identical to `activate` (asserted by
`tests/interleaved_scoring_parity.rs` and the existing scoring parity suite).

## Fused MSE batch loss routed through the #287 interleaved gather (Issue #384)

The `#[wasm_bindgen]` fused activate + MSE entry point (`mse_sum_batch_packed`)
ran its **own** duplicate forward pass over the old eight-scattered-buffer
per-lane layout (`mse_sum_batch_8way` → `weighted_sum_simd_8records`), so the
locality win #287 landed for the then-current per-record `score_records` entry
point (removed in Issue #409; `score_records_flat` is its successor) never
reached the loss lane production scoring actually calls. #384 dispatches standard-squash networks
(the all-`Tanh` production topology) through the same record-interleaved gather
(`interleaved_forward_8` → `weighted_sum_interleaved_8`) and keeps aggregate
networks on the exact per-lane path. The full 8-record groups are **bit-identical**
to the prior per-lane path (the interleaved gather is proven bit-identical to
`weighted_sum_simd_8records`, asserted by
`tests/simd_weighted_sums.rs::interleaved_8_is_bit_identical_to_scattered_8records`);
the whole rerouted result is bit-identical before/after, asserted by
`loss::interleaved_mse_parity` and `tests/mse_batch_interleaved_parity.rs`.

The 8-record bench (`batched_scoring/mse_sum_8records`) over-weights per-call
buffer setup, so #384 adds `batched_scoring/mse_sum_production`
(`PRODUCTION_SCORING_RECORDS = 4096` per iteration) to measure the steady-state
gather cost.

**Measured 2026-07-26**, Apple M4 (this host — see below), rustc 1.97.1,
`--release`, Criterion `--sample-size 10 --measurement-time 6 --warm-up-time 1`,
`--save-baseline`/`--baseline` A/B on the *same* prebuilt bench binary (old =
`mse_sum_batch_8way` scattered, new = interleaved reroute):

| benchmark (median) | old | new | change |
| --- | --- | --- | --- |
| `mse_sum_8records/production` | 175.0 µs | 76.2 µs | **−56.6%** |
| `mse_sum_production/production` (4096) | 87.96 ms | 34.97 ms | **−60.2% (≈2.5×)** |
| `mse_sum_8records/production_2x` | 384.8 µs | 167.6 µs | **−56.8%** |
| `mse_sum_production/production_2x` (4096) | 191.0 ms | 77.87 ms | **−59.2% (≈2.5×)** |
| `mse_sum_8records/production_exact` | 205.7 µs | 76.2 µs | **−63.0%** |
| `mse_sum_production/production_exact` (4096) | 103.3 ms | 34.87 ms | **−66.5% (≈3.0×)** |

All deltas are `p = 0.00 < 0.05` (Criterion), far outside the ±5% noise band.
Per the squash-homogeneity caveat above, these all-`Tanh` figures are a **lower
bound** — varied-squash creatures also convert scalar `libm` `Gelu`/`Mish` to
the vectorised path, so they gain at least as much.

> **Host note.** This A/B ran on a plain **Apple M4** (10 cores), not the
> Apple M4 Pro host class the older sections above used; the numbers are only
> comparable within this section's own old/new pair, which is what the
> gain claim rests on.

## Fused-MSE record tile widened from 8 to a tunable `R` (Issue #530)

Issue #384 routed the fused MSE lane through the #287 interleaved gather but
kept the record group at **8**, so the network's whole synapse array was re-streamed once
per eight records. On `production_exact` that array is ~172 KB (21,513 × 8 B
`SynapseData`) and the 8-lane `mse_inter` is ~132 KB — together well past an
M4 performance core's 128 KB L1D, so every group re-fetched the synapses from
L2. #530 makes the tile a single constant, `loss::MSE_TILE_LANES`, gathered by
const-generic kernels (`weighted_sum_interleaved::<R>`: `R / 8` `__m256`
accumulators on AVX2, `R / 4` `float32x4` on NEON, contiguous `f32x4` quads on
wasm `simd128`). Per-record synapse traffic falls by `R / 8`.

Two changes are load-bearing beyond the width itself, and both are pinned by
`loss::interleaved_mse_parity`:

- The tile transpose walks **input-major** — a neuron's `R` lanes go to
  consecutive `mse_inter` slots — so the scratch buffer is filled by one linear
  sweep. The original lane-major order revisits the whole `num_inputs * R`
  region once per lane, which stops fitting in L1 as `R` grows and would have
  eaten the gain. This alone is most of the `R = 8` column below.
- `interleaved_tile_mse` is **seed-taking**: it takes the running `f64`
  `sum_error` and returns it. Returning a per-tile partial sum re-associates the
  reduction — caught in development as a 12-ULP parity failure at n = 4096.

**Measured 2026-08-06**, Apple M4 (10 cores, this host), rustc 1.97.1,
`--release`, `hot_paths -- mse_sum_production/production_exact` (4,096 records
per iteration). **5 interleaved A/B rounds**, each round rebuilding and running
every arm back to back so thermal drift hits all arms alike; `base` is the
pre-#530 code run from a separate `git worktree` at the parent commit.
**Medians** (`n` rounds per arm):

| arm | n | median | min | max | vs `base` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `base` (pre-#530, 8-record groups) | 6 | 21.343 ms | 19.877 | 21.819 | — |
| `MSE_TILE_LANES = 8` | 6 | 17.994 ms | 16.978 | 19.127 | **−15.7%** |
| `MSE_TILE_LANES = 16` | 6 | 14.468 ms | 13.659 | 15.281 | **−32.2%** |
| **`MSE_TILE_LANES = 32` (shipped)** | 5 | **11.558 ms** | 10.252 | 12.152 | **−45.8%** |
| `MSE_TILE_LANES = 64` | 5 | 12.728 ms | 12.130 | 13.496 | −40.4% |

The arms do not overlap at all — `R = 32`'s worst round (12.152 ms) beats
`base`'s best (19.877 ms) and `R = 8`'s best (16.978 ms) — so the ordering does
not rest on the medians alone. Against the same-code `R = 8` arm (the A/B the
issue asks for, one constant apart) `R = 32` is **−35.8%**.

**The curve turns between 32 and 64.** At `R = 64` `mse_inter` reaches ~1 MB on
this creature and the NEON kernel needs 16 live accumulators; both the extra
capacity pressure and the register pressure show up as a partial give-back. 32
is the shipped default.

**Memory.** `mse_inter` is `num_neurons * MSE_TILE_LANES * 4` bytes per compiled
network — ~132 KB at 8 lanes, **~528 KB at 32**, ~1 MB at 64 on the ~4,127-neuron
production creature. Directory scoring holds one compiled network per worker, so
at N = 50 workers that is ~26 MB of scratch at the shipped width. Budget it
against the scorer's worker-count RAM ceiling before raising the constant.

**Numerics.** Bit-identical at every width: each lane accumulates its own
`bias + Σ w·a` in synapse order, independently of the other lanes, and every
tier reduces into `sum_error` in strict record order. Asserted against the
independent scattered oracle for widths 8/16/32/64 across 15 record counts
straddling every tier boundary
(`loss::interleaved_mse_parity::every_tile_width_is_bit_identical_to_scattered`).

> Per the squash-homogeneity caveat above these all-`Tanh` figures isolate the
> memory-traffic effect; this change is layout-only, so unlike a
> squash-vectorisation lever it is neither a lower nor an upper bound on
> varied-squash creatures.

## Flat-slice record **input** for batched scoring (Issue #386)

Issue #229 flattened the scoring *output* to one contiguous buffer, but the
*input* stayed a `&[Vec<f32>]` — one heap allocation per record for the caller and a
`Vec` header to pointer-chase on every lane load, even though the kernels only
ever read a record as `&[f32]`. #386 adds the matching flat *input* contract
(`score_records_flat` / `_flat_into` / `score_records_parallel_flat`): record
`i`'s inputs are `inputs[i * stride .. i * stride + stride]`, mirroring the flat
output layout and the packed buffer the fused loss lane already takes
(`mse_sum_batch_packed`). The `&[Vec<f32>]` entry points are unchanged wrappers
over the same kernel, so both layouts are **bit-identical**
(`tests/flat_record_scoring_parity.rs`).

> **Historical — this lane no longer exists.** The `wasm_dataset` module, its
> `TrainingDataset::evaluate_mse` / `input_batch` API, and the
> `dataset_evaluate_mse` bench group were all removed as unconsumed dead code in
> Issue #415. The paragraph and tables below are retained only as the historical
> record of the #386 flat-batch change; none of them describes a current code
> path.

The biggest win at the time was `TrainingDataset::evaluate_mse` (the >4 GB
Memory64 offload lane from #298). It had stored inputs contiguously in SoA
layout yet scored **one record at a time** through `activate` —
re-bounds-checking per record, allocating a fresh `Vec<f32>` per record via
`to_vec()` (exactly the per-record allocation removed by #229), and never
touching the 8-record interleaved SIMD path from issues #230/#287. #386 changed
it to bounds-check once, take `input_batch(start, count)` as a single slice, and
drive it through the flat batched path in 1024-record chunks (a multiple of the
8-record SIMD group, so grouping and results were unchanged while the scratch
output buffer stayed a bounded constant).

**Methodology.** Same alternating-rounds protocol as #287 above, because
separate `cargo bench` invocations on this laptop drift by more than the effect
being measured on the smaller groups: old/new rounds of the *same* prebuilt
bench binaries, run back to back, with the untouched `forward_pass` benchmark
captured alongside as a drift control.

**Measured 2026-07-26**, Apple M4 (the #384 host — see the host note above),
rustc 1.97.0, `--release`, Criterion
`--sample-size 10 --measurement-time 5 --warm-up-time 1`.

`dataset_evaluate_mse` (4096 records/iteration), mean of the alternating rounds
(the since-removed group — see the Issue #415 note above):

| benchmark (mean of rounds) | old | new | change |
| --- | --- | --- | --- |
| `dataset_evaluate_mse/production` | 330.3 ms | 69.9 ms | **−78.8% (≈4.7×)** |
| `dataset_evaluate_mse/production_2x` | 686.7 ms | 149.7 ms | **−78.2% (≈4.6×)** |
| `dataset_evaluate_mse/production_exact` | 283.6 ms | 69.6 ms | **−75.5% (≈4.1×)** |

Drift controls — both must be flat, and are:

| control (mean of rounds) | old | new | change |
| --- | --- | --- | --- |
| `forward_pass/production_exact` | 48.10 µs | 49.42 µs | flat |
| `scoring/production` (4096) | 74.25 ms | 72.87 ms | flat |
| `scoring/production_2x` (4096) | 165.17 ms | 163.06 ms | flat |
| `scoring/production_exact` (4096) | 70.72 ms | 69.90 ms | flat |

At the time, the `scoring` group was the no-regression gate for the
`&[Vec<f32>]` wrapper: it routed through the same rewritten kernel and had to
not pay for the new input layout. It did not. That wrapper is also history —
Issue #409 deleted `score_records` / `score_records_parallel` outright — so the
group no longer gates anything about it; today it drives `score_records_flat`
directly.

> **Historical A/B — `scoring_flat` was retired by Issue #408, and the
> per-record entry point it was compared against was removed by Issue #409.**
> It was a second group added by #386 that scored the *same* shard through the
> flat input entry point, so the delta against `scoring` isolated the per-record
> `Vec` header indirection (both fixtures were built outside the timed loop, so
> the caller-side one-allocation-per-record the `&[Vec<f32>]` signature forced
> was *additional* saving not counted here). With only one input layout left
> there was nothing to compare against, and `scoring` *is* the flat measurement.
> The table below is the evidence for the #386 flat-input win, measured before
> that retirement; it cannot be reproduced against the current tree, where the
> group's Criterion filter has matched no benchmark since #408. The
> "The `scoring` group (Issue #228)" paragraph in
> `neat-core/benches/README.md` and the `bench_scoring` doc comment in
> `neat-core/benches/hot_paths.rs` tell the same story.

| benchmark (mean of rounds, 2026-07-26) | `scoring` | `scoring_flat` — retired by #408, entry point removed by #409 | change |
| --- | --- | --- | --- |
| `production` (4096) | 72.87 ms | 70.34 ms | −3.5% |
| `production_2x` (4096) | 163.06 ms | 149.70 ms | −8.2% |

`parallel_scoring` was A/B'd the same way (6 alternating rounds on
`production_exact`, prebuilt old/new binaries) and is flat — as expected, since
it drives the identical `score_batch_into`:

| `parallel_scoring/production_exact` | old (mean) | new (mean) | change |
| --- | --- | --- | --- |
| `1_core` | 71.5 ms | 69.4 ms | flat |
| `12_cores` | 41.4 ms | 43.3 ms | flat (within a ±25% run-to-run spread) |

> **12-core noise.** The `12_cores` lane builds a fresh `ThreadPoolBuilder` per
> iteration and is scheduler-sensitive; across six alternating rounds old spanned
> 38.0–47.2 ms and new spanned 36.4–51.4 ms, so the ~5% mean gap is well inside
> the spread. The clean `1_core` signal is flat-to-slightly-better.

## Native (`--features parallel`) vs wasm32 scoring lane — decision (Issue #288)

The `parallel` feature (rayon, #179) has always compiled a native rayon entry
point — `CompiledNetwork::score_records_parallel_flat` today, named
`score_records_parallel` before Issue #409 removed it — but it was never A/B'd
against the wasm32 lane at production scale, so production never routed
per-creature scoring to it. This section quantifies the trade-off and records
the decision.

**Verdict: route production per-creature scoring to the native rayon lane where
the native `rust_scorer` is built.** Native beats wasm32 on the production
fixture on both axes — per-core codegen (NEON + FMA vs simd128 + relaxed-madd)
and, decisively, by using idle cores the single-threaded wasm32 lane cannot.
This is a **positive result**; the core-side native path
(`score_records_parallel_flat` with its sequential/wasm32 fallback) is ready,
and the production wiring is raised cross-repo (a WorkerPool idle-tail change
and a host-flags change in the downstream production repos) per the issue's
one-root-cause-one-repo rule — this issue owns only the neat-core native path,
benchmark, and this decision.

### The two lanes being compared

Both lanes drive the **same** `score_records_flat` forward pass (the #287
record-interleaved batched-SIMD gather); they differ only in codegen and thread
count:

- **wasm32 single-thread** — the algorithm as compiled to `wasm32-unknown-unknown`
  with `simd128` + `relaxed-simd` (`f32x4_relaxed_madd`), `wasm-opt`-optimised,
  run in Node. This is the production wasm codegen the `wasm_activation` bundle
  ships. In NEAT-AI the WorkerPool runs one creature per worker with no wasm
  threads, so a single creature's scoring is single-threaded — this row is its
  ceiling.
- **native** — the same source compiled for `aarch64-apple-darwin` (AVX2/FMA on
  x86, NEON on ARM), scored through a fixed-size rayon pool of 1 or 12 workers
  via `score_records_parallel_flat`.

### Measured 2026-07-18 — Apple M4 Pro host class

Apple M4 Pro (8P + 4E, 12 logical cores), 24 GB, macOS (arm64), rustc 1.97.0,
`--release`. 4096 records (one production shard; see the record-count
calibration above) scored through one creature.

- **Native**: `cargo bench -p neat-core --features parallel --bench parallel_scoring`
  (Criterion, `--sample-size 30`), median estimate.
- **wasm32**: `wasm-pack build --target nodejs --release` of a throwaway harness
  that reuses the committed `benches/common` fixtures and calls the identical
  `score_records_flat`, driven by `node` timing `score_once()` (median of 20,
  after a 5-iteration warm-up). Harness source and commands under
  **Reproducing** below.

| Shape | wasm32 1-thread | native 1 core | native 12 cores |
| --- | --- | --- | --- |
| `production` | 125.4 ms · 32.7 K rec/s | 45.96 ms · 89.1 K rec/s | 15.47 ms · 264.8 K rec/s |
| `production_2x` | 207.8 ms · 19.7 K rec/s | 102.7 ms · 39.9 K rec/s | 32.52 ms · 126.0 K rec/s |
| `production_exact` | 83.40 ms · 49.1 K rec/s | 46.77 ms · 87.6 K rec/s | 17.56 ms · 233.3 K rec/s |

Native-over-wasm32 speed-ups on the exact committed topology
(`production_exact`, 1,666 neurons / 21,513 synapses / 2,461 inputs):

| Comparison | production_exact | Interpretation |
| --- | --- | --- |
| native 1 core ÷ wasm32 1-thread | **1.78×** | pure codegen: NEON + FMA vs simd128 + relaxed-madd |
| native 12 cores ÷ wasm32 1-thread | **4.75×** | the production reality — one creature on native uses cores wasm32 cannot |
| native 12 cores ÷ native 1 core | 2.66× | rayon per-creature scaling across 8P + 4E |

The codegen delta ranges 1.78–2.73× across the three shapes; the full native
12-core-vs-wasm32 delta ranges 4.75–8.10×.

### The per-creature parallelism win zone

The project metric is **score improvement per wall-clock hour**, whose
denominator is the per-creature ~2.24 M-record corpus pass. At the
`production_exact` throughputs above that pass costs:

| Lane | records/s | Per-creature corpus pass (~2.24 M records) |
| --- | --- | --- |
| wasm32 single-thread | 49.1 K | ≈ **45.6 s** |
| native single core | 87.6 K | ≈ **25.6 s** |
| native 12 cores | 233.3 K | ≈ **9.6 s** |

Two distinct wins stack:

1. **Codegen win (whole generation).** Native's ~1.8× per-core advantage applies
   to *every* creature regardless of core occupancy — it is not tail-specific.
2. **Idle-core win (the tail).** NEAT-AI's WorkerPool saturates cores while
   un-scored creatures outnumber cores, but at the **generation-end tail** fewer
   creatures than cores remain and cores go idle. wasm32 workers are
   single-threaded per creature, so that idle time is wasted; native
   `score_records_parallel_flat` lets each remaining creature spread its record
   batch across the idle cores. This tail is the **per-creature parallelism win
   zone** — where native rayon converts otherwise-idle cores into throughput.

### Honesty caveats

- **12-core variance.** The all-core medians were taken at `--sample-size 30` on
  a laptop-class 8P + 4E part; run-to-run spread is wide (Criterion flagged the
  all-core groups as noisy). The parallel scaling is sub-linear (2.66–3.16×, not
  12×) because the ~40 MiB batch is memory-bandwidth-bound and the 4 efficiency
  cores are slower than the 8 performance cores. The **single-thread codegen
  delta is stable and by itself justifies native**; the parallel scaling is the
  bonus that pays off most in the idle-tail.
- **Fixture squash homogeneity.** As above (Issue #261) the fixtures are
  all-`Tanh`; real `Gelu`/`Mish` creatures run scalar `libm` on wasm32, which
  the native SIMD/`libm` split widens further — so the native advantage here is
  a lower bound, not an upper one.
- **wasm32 lane is a codegen ceiling, not the production wasm scorer.** This
  measures the shared `score_records_flat` compiled to wasm32. NEAT-AI's
  production wasm path additionally pays JS↔wasm orchestration per record, so
  the real
  wasm32 scoring lane is no faster than this row — reinforcing the verdict.

## Backprop-setup adjacency: `Vec<Vec<u32>>` → CSR (Issue #388)

`compute_reverse_topological_order` built its inward adjacency as one `Vec` per
neuron — `n + 1` heap allocations per creature plus the geometric regrowth of
every inner `Vec` — and Kahn's walk then chased a separate pointer per neuron.
It now builds a **CSR** triple (`inward_starts` prefix sum + flat
`inward_indices`) in two passes, so the walk reads one contiguous array. The
returned order is unchanged, element for element
(`reverse_topological_order_matches_reference_on_random_dags`).

**Measured 2026-07-26**, Apple M4 Pro (12 cores, 24 GB, macOS 26.5.2 arm64),
rustc 1.97.0, Criterion 0.8.2, `--release` bench profile. New group
`reverse_topological_order`; times are Criterion's mean `[lower, upper]`, and
the change column is Criterion's own `--baseline` comparison median.

| Shape | Before (`Vec<Vec<u32>>`) | After (CSR) | Change |
| --- | --- | --- | --- |
| `small_50` | 15.60 µs `[13.33, 18.37]` | 5.88 µs `[5.21, 6.63]` | −62.3% |
| `medium_500` | 169.48 µs `[144.80, 199.02]` | 94.51 µs `[85.89, 104.21]` | −44.2% |
| `large_5000` | 2.361 ms `[2.071, 2.682]` | 1.799 ms `[1.679, 1.928]` | −23.8% |
| `production` | 588.97 µs `[533.27, 648.77]` | 231.09 µs `[204.74, 261.11]` | −60.8% |
| `production_2x` | 1.372 ms `[1.211, 1.550]` | 490.28 µs `[448.27, 536.87]` | −64.3% |
| **`production_exact`** (1,666 non-input neurons, 21,513 synapses) | **547.19 µs** `[503.07, 594.92]` | **234.34 µs** `[213.14, 257.97]` | **−57.2%** |

Allocation count for one call at production shape (4,127 neurons / 2,461 inputs
/ ~21.7 k synapses), counted with the same counting-allocator harness as
`neat-core/tests/reverse_topological_allocations.rs`:

| Shape | Before | After |
| --- | --- | --- |
| n = 128 | 239 | 7 |
| production (n = 4,127) | 5,021 | **7** |

The count is now independent of neuron count — asserted permanently by
`reverse_topological_order_allocation_count_does_not_scale_with_neurons`.

## Reproducing

```bash
# Single-thread hot paths at production scale (forward_pass, batched_scoring,
# backprop, scoring for production / production_2x).
cargo bench -p neat-core --bench hot_paths -- production

# Data-parallel scoring throughput (1 core vs all cores).
cargo bench -p neat-core --bench parallel_scoring --features parallel

# Bench compile smoke test (the only automated gate, since benches are
# harness = false and excluded from CI).
cargo bench -p neat-core --no-run --features parallel
```

To compare a candidate optimisation, save this state as a Criterion baseline
first, then re-run after the change:

```bash
cargo bench -p neat-core --bench hot_paths -- --save-baseline before
# … apply change …
cargo bench -p neat-core --bench hot_paths -- --baseline before
```

### wasm32 scoring anchor (Issue #288)

Criterion is native-only, so the wasm32 row in the native-vs-wasm32 decision is
measured with a throwaway `wasm-pack` harness that reuses the committed
`benches/common` fixtures and calls the identical
`CompiledNetwork::score_records_flat`. `score_records_flat` is a plain `pub`
method (not feature- or target-gated), so the
wasm32 build scores the same production topology through the same code path,
compiled with `simd128` + `relaxed-simd` and `wasm-opt`-optimised — the
production `wasm_activation` bundle's codegen. Create a scratch crate outside the
workspace:

```toml
# Cargo.toml
[package]
name = "wasmbench288"
version = "0.0.0"
edition = "2024"
[lib]
crate-type = ["cdylib"]
[dependencies]
neat-core = { path = "/abs/path/to/NEAT-AI-core/neat-core" }
wasm-bindgen = "0.2.126"
[profile.release]
opt-level = 3
lto = true
```

```rust
// src/lib.rs
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
#[path = "/abs/path/to/NEAT-AI-core/neat-core/benches/common/mod.rs"]
#[allow(dead_code)]
mod common;
use common::{NETWORKS, PRODUCTION_SCORING_RECORDS, build_network, build_records};
use neat_core::network::CompiledNetwork;
thread_local! {
    // Flat input layout (Issue #386): records * stride, contiguous.
    static ST: RefCell<Option<(CompiledNetwork, Vec<f32>, usize, usize)>> =
        const { RefCell::new(None) };
}
#[wasm_bindgen]
pub fn setup(code: u32) {
    let label = ["production", "production_2x", "production_exact"][code as usize];
    let s = NETWORKS.iter().find(|s| s.label == label).unwrap();
    let net = build_network(s, 0x5EED);
    let recs = build_records(net.num_inputs(), PRODUCTION_SCORING_RECORDS);
    let stride = net.num_inputs();
    let flat: Vec<f32> = recs.iter().flat_map(|r| r.iter().copied()).collect();
    ST.with(|c| *c.borrow_mut() = Some((net, flat, stride, s.num_outputs)));
}
#[wasm_bindgen]
pub fn score_once() -> f32 {
    ST.with(|c| {
        let b = c.borrow();
        let (net, flat, stride, no) = b.as_ref().unwrap();
        net.score_records_flat(flat, *stride, *no).iter().sum()
    })
}
```

```bash
wasm-pack build --target nodejs --release --out-dir pkg
# Node: setup(code) once, then time score_once() (median of ~20, warm up first),
# records/sec = PRODUCTION_SCORING_RECORDS / median_seconds.
node -e 'import("./pkg/wasmbench288.js").then(m=>{m.setup(2);
  for(let i=0;i<5;i++)m.score_once();
  const t=[];for(let i=0;i<20;i++){const a=performance.now();m.score_once();t.push(performance.now()-a);}
  t.sort((x,y)=>x-y);console.log("median ms",t[10].toFixed(2));})'
```
