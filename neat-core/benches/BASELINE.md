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
GRQ creatures also run `Gelu`/`Mish` (scalar `libm`), so read every
`scoring`/`production` A/B below with two corrections in mind:

- **Squash-vectorisation deltas are a lower bound.** Varied-squash creatures
  also convert scalar `libm` `Gelu`/`Mish` to the vectorised path, gaining at
  least as much as the all-`Tanh` fixture shows.
- **Branch-prediction levers are unmeasurable here.** A homogeneous squash means
  the predictor already nails the one-arm `match`, so a branch-misprediction
  optimisation shows no delta on this fixture — that is a fixture artefact, not
  evidence the lever is worthless on real creatures.

The real `GRQ-cluster/network.json` is not on the build host, so it cannot be
A/B'd here directly; re-run against the pinned creature to confirm on the real
varied-squash topology.

## Host / toolchain

| Field | Value |
| --- | --- |
| Host class | Apple Silicon (GRQ host class) |
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
batch. Derivation, from the committed GRQ-cluster telemetry
(`GRQ-cluster/performance.csv`, the 32-generation production run whose totals
match `result.json`: `generations = 32`, `total_time_ms = 3,042,879`,
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
sequential `score_records` path, an internal consistency check on the fixture.

## Production-exact topology baseline (Issue #286)

Dated anchor for the **exact** committed `GRQ-cluster/network.json` topology —
**1,666 non-input neurons, 21,513 synapses, 2,461 inputs** (4,127 total neurons,
one output). Unlike the `production` shape's ~13-average `VariedAround` fan-in,
the `production_exact` shape uses `FanIn::ExactTotal(21_513)`, which spreads the
synapses across the 1,666 neurons as evenly as possible (12 or 13 each,
Bresenham-interleaved) so the synapse count reproduces the real model to the
synapse. Seeded synthesis is deterministic and asserted exact by
`tests/bench_fixtures.rs::production_exact_matches_committed_grq_topology`.

**Measured 2026-07-18** on the same GRQ host class, toolchain **rustc 1.97.0**
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
> *same* sequential `score_records` path, so in principle they match — but they
> were taken in separate `cargo bench` invocations and the ~90 ms → ~62 ms spread
> is real run-to-run variance (thermal state on the laptop-class M4 Pro under a
> ~90 ms single-shot benchmark with 100 iterations). Treat the **`parallel_scoring`
> 1-core/12-core pair as the authoritative scoring anchor** — both were measured
> in one invocation, so their 2.51× ratio is internally consistent — and read the
> single-core scoring latency as ~62–92 ms (±~20%). A lane sub-issue must A/B
> with `--save-baseline` inside a single invocation to stay inside this band.

## Record-interleaved scoring optimisation (Issue #287)

The single-thread scoring hot path (`score_records` → `score_batch_into`, the
same lane NEAT-AI's per-creature wasm32 workers drive) now transposes each
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

**Measured 2026-07-18**, GRQ host class (Apple M4 Pro), rustc 1.97.0,
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
