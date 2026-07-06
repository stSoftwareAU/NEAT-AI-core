# Establish production-sized benchmark baseline and scoring fixtures (Issue #228)

## Summary

Establishes the documented, production-calibrated benchmark baseline that every
optimisation under the #227 milestone is gated against. Closes #228.

Production timings show fitness evaluation dominates (~95% of wall clock:
2,901,473 ms of 3,042,879 ms over 32 generations). This is a **bench-only**
change — no library behaviour is touched — that does three things:

1. **Calibrates the scoring record count to production volume.**
   `parallel_scoring.rs` scored an uncalibrated 2048-record token batch. It now
   scores `PRODUCTION_SCORING_RECORDS = 4096` records — one GRQ-cluster training
   shard's worth — with the derivation from committed telemetry
   (`GRQ-cluster/performance.csv` / `result.json`) documented in `BASELINE.md`
   and `benches/common/mod.rs`.
2. **Adds a `scoring` group to `hot_paths.rs`** measuring single-core
   `score_records` throughput at production record volume for `production` /
   `production_2x`, reachable via `--bench hot_paths -- production`.
3. **Commits `neat-core/benches/BASELINE.md`** with the production/production_2x
   numbers for every hot-path group, plus host/CPU/toolchain metadata.

The record count lives in `benches/common/mod.rs` as a single source of truth
(`PRODUCTION_SCORING_RECORDS` + `build_records`), shared by both bench targets
and the `bench_fixtures` test.

### Record-count derivation (cross-checkable against #227)

| Quantity | Value | Source |
| --- | --- | --- |
| Training corpus size | 22,097,375,712 bytes | `training_data_size_bytes` |
| Training shards | 520 | `training_data_files` |
| Per-record width | 2461 × 4 = 9844 bytes | `num_inputs` × `size_of::<f32>()` |
| Whole-corpus records | ≈ 2.24 M | corpus / record width |
| Records per shard | ≈ 4,317 | 2.24 M / 520 |
| **Bench batch** | **4096** | one shard, rounded to 2¹² |

Materialising the full ~2.24 M-record corpus (~21 GiB) is infeasible for a
micro-benchmark, so the harness scores one shard (~40 MiB) — already far larger
than any CPU cache, so records/sec extrapolates to the full corpus pass.

## Evidence

Backend/bench-only change — no web interface to screenshot. Evidence is the
committed baseline numbers, captured on the Apple Silicon host class.

**Host:** Apple M4 Pro (8P+4E, 12 logical), macOS 26.5.2 arm64, rustc 1.96.0,
Criterion 0.8.2, `--release`.

`hot_paths -- production` (single-thread):

| Group / benchmark | production | production_2x |
| --- | --- | --- |
| `forward_pass` | 32.76 µs | 67.26 µs |
| `batched_scoring/trace_batch_4way` | 80.17 µs | 199.18 µs |
| `batched_scoring/mse_sum_8records` | 223.55 µs | 656.06 µs |
| `backprop` | 153.87 µs | 368.19 µs |
| `scoring` (4096 records) | 242.79 ms | 491.36 ms |

`parallel_scoring --features parallel` (4096 records, 1 vs 12 cores):

| Shape | 1 core | 12 cores | records/s (1 → 12) | Speed-up |
| --- | --- | --- | --- | --- |
| `production` | 242.97 ms | 75.44 ms | 16.86 K → 54.30 K | 3.22× |
| `production_2x` | 487.26 ms | 137.78 ms | 8.41 K → 29.73 K | 3.54× |

Internal consistency check: single-core `parallel_scoring` (16.86 Krecords/s,
`production`) matches the `hot_paths` `scoring` group (16.87 Krecords/s) — both
drive the same sequential `score_records` path.

```mermaid
flowchart LR
    T["GRQ-cluster telemetry<br/>performance.csv / result.json"] --> D["Derive record count<br/>corpus / 520 shards ≈ 4.3k"]
    D --> C["PRODUCTION_SCORING_RECORDS = 4096<br/>(common/mod.rs)"]
    C --> H["hot_paths: scoring group"]
    C --> P["parallel_scoring: 1 vs all cores"]
    H --> B["BASELINE.md"]
    P --> B
    B --> O["Gates every #227 optimisation PR<br/>(before/after comparison)"]
```

## Test Plan

Added behavioural ("what") tests to `neat-core/tests/bench_fixtures.rs`
(exercising the shared fixtures via `#[path]`):

- `build_records_produces_deterministic_distinct_batch_sized_to_inputs` — the
  helper yields `count` records each of `num_inputs` length, reproducible under
  a fixed seed (non-determinism guard) and distinct across the batch.
- `production_scoring_record_count_is_production_representative` — the built
  batch exceeds the prior 2048 token batch and stays memory-feasible (< 1 GiB)
  at the widest shape.
- `score_records_on_production_batch_yields_finite_ordered_outputs` — scoring a
  production-shaped batch produces finite, correctly-shaped outputs.

Verification run (all green):

- `cargo test --workspace --lib --tests --all-features` — all pass, including
  the 9 `bench_fixtures` tests and 7 `parallel_scoring` tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- `cargo bench -p neat-core --no-run --features parallel` — the required bench
  compile smoke test (benches are `harness = false` / excluded from CI, so this
  is the automated gate).
- `cargo fmt --all --check`, `cargo doc`, `codespell` — clean.

### Pre-existing failure noted

`./quality.sh` exits early on 4 pre-existing `ci.yml` bats failures (tests
48/49/50/54 about `bump-deps.sh` wiring) that are present on the clean base
branch and unrelated to this change (which touches only `neat-core/benches/*`,
`neat-core/tests/bench_fixtures.rs` and `docs/`). Every Rust stage `quality.sh`
would run past that point was executed directly and passed.
