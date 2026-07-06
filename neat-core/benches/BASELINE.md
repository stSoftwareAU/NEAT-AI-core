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
