# Unchecked WASM `gather4` activation loads — result record (Issue #509)

**Decision: KEEP.** The unchecked gather is repeatably faster on the isolated
kernel (**−20% to −23%** median, every session, both production shapes) and
repeatably — if modestly — faster on production-sized end-to-end inference
(**−2%** on a quiet host, −4% to −11% on a loaded one, never slower in any
session). Numerics are **bit-identical**, the batched scoring path is
**unchanged**, and native targets are untouched by construction.

## Hypothesis

`CompiledNetwork::new` validates at load time that every
`SynapseData::from_index` is `< num_neurons`, and the activation buffer is sized
to exactly `num_neurons` (`AGENTS.md`, "Unsafe & SIMD invariants"). The
`scalar::tail_*` remainder helpers and all native SIMD kernels already index
with `get_unchecked` on the strength of that invariant; the `wasm32` `gather4`
scaffold helper did not. Its four `SynapseData` reads are sequential and might
be provable by LLVM, but the four **indirect** activation reads are
data-dependent on `from_index` and cannot be — so a production forward pass was
expected to be paying four unprovable bounds checks per four synapses.

Hypothesis: eliding them speeds up the gather-bound production forward pass.

## Implementation tested

One helper changed — `gather4` in `neat-core/src/simd.rs`, commit
`14f8d020c00e02928f923ce85275a609d86565d7`:

- All `unsafe` lives in that single helper, with an explicit `# Safety` contract
  naming both obligations (`base + 4 <= synapses.len()`; every `from_index` in
  the four lanes indexes `activations`) and a `// SAFETY:` note discharging them
  from the load-time validation.
- `gather4_products`, `reduce4`, the four single-record kernels and the fold in
  each of them are **unchanged** — lane order, FMA order and the seed-taking
  remainder are all untouched, which is why the result is bit-identical rather
  than merely close.
- The bounds-checked implementation is retained verbatim behind the
  `checked-gather4` feature: it is the A/B control below and the one-flag way
  back out.
- No SIMD, squash, layout or numerical change of any other kind.

## Benchmark setup

`wasm-bench/` (out of the workspace; see its README). The decisive design point
is that the driver holds **both wasm modules in one process** and alternates
between them sample by sample, flipping the order every sample. The host is a
shared build machine whose load average ranged from 3.5 to 16 during this work,
and un-paired cross-process timings on it were useless (±3× spreads); paired
in-process ratios stayed stable across every run.

| Item | Value |
| --- | --- |
| Commit | `14f8d02` (`issue-509-prototype-unchecked-wasm-gather4-activation-loads`) |
| Build flags | `--release` (`opt-level = 3`, `lto = true`, `codegen-units = 1`), `RUSTFLAGS="-C target-feature=+simd128,+relaxed-simd"` |
| Control build | `cargo build --release --target wasm32-unknown-unknown --features checked-gather4` |
| Experimental build | same, default features |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Runtime | Node `v25.6.1` (V8 `14.9.207.2`), `WebAssembly.instantiate`, relaxed-SIMD enabled |
| Machine | Apple M4, 10 cores, 24 GiB, macOS 26.6 — **shared, loaded**; load average recorded per run below |
| Timer | `process.hrtime.bigint()` in the driver, around each exported call |
| Warm-up | 3 full passes of all three benchmarks per variant, discarded (V8 tiers wasm Liftoff → TurboFan in the background) |
| Samples | 25 per variant per session × 5 sessions = 125 paired samples (`production_exact`); 20 × 4 = 80 (`production_2x`) |
| Statistic | paired ratio `unchecked / control` per sample; median + IQR, plus per-session medians for repeatability |

### Creature and data

Fixtures are `neat-core/benches/common/mod.rs` **verbatim** — the same
deterministic builders the committed Criterion baseline uses.

| Shape | Inputs | Neurons | Outputs | Synapses | Records |
| --- | ---: | ---: | ---: | ---: | ---: |
| `production_exact` | 2,461 | 4,127 | 1 | 21,513 | 4,096 |
| `production_2x` | 4,922 | 8,268 | 2 | ~43.5k | 2,048 |

`production_exact` is the committed production creature topology to the synapse
(Issue #286). The record count is one production training shard's worth
(`PRODUCTION_SCORING_RECORDS`, derived in `benches/BASELINE.md` from a
32-generation production run: 2.24 M records per creature per generation across
520 shards). At production width that batch is ~40 MiB — far beyond any cache,
so its memory-traffic behaviour is production-representative.

### Benchmarks

- `kernel` — isolated: one `weighted_sum_simd_unchecked` call per non-input
  neuron over the creature's real synapse spans, ×200 (the `gather4` consumer).
  Issue #613 split the kernel in two; this harness measures the `*_unchecked`
  form, which is the one the forward pass reaches.
- `activate` — end-to-end **single-record inference**: `activate_into` over
  every record. This is the production forward pass that calls `gather4`.
- `score` — end-to-end **batched scoring**: `score_records_flat` over the whole
  flat batch. This path runs the 8-record kernels, which do *not* use `gather4`,
  so it is the "no material regression on another production path" check.

## Raw results

Paired median ratio (`unchecked / control`); **< 1 means the unchecked gather is
faster**.

| Run | Shape | Host load | `kernel` | `activate` | `score` |
| --- | --- | ---: | ---: | ---: | ---: |
| A (quiet) | `production_exact` | 11.5 → 3.6 | **0.7893** | **0.9807** | 0.9998 |
| B (quiet) | `production_exact` | 3.6 | **0.7691** | **0.9802** | 1.0002 |
| C | `production_exact` | 13.8 | 0.7969 | 0.8931 | 1.0009 |
| D | `production_exact` | 16.4 | 0.7844 | 0.9113 | 0.9831 |
| E (quiet) | `production_2x` | ~4 | **0.8072** | **0.9692** | 1.0061 |
| F | `production_2x` | 11.9 | 0.7976 | 0.9627 | 0.9772 |

Per-session medians within the two quiet runs (the repeatability check):

| Run A session | 0 | 1 | 2 | 3 | 4 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `kernel` | 0.7939 | 0.7817 | 0.7900 | 0.7321 | 0.8202 |
| `activate` | 0.9777 | 0.9799 | 0.9804 | 0.9796 | 0.9864 |
| `score` | 0.9971 | 1.0002 | 1.0012 | 1.0001 | 1.0008 |

Absolute numbers from run A (125 samples per variant; median, IQR):

| Bench | Control median | Control IQR | Unchecked median | Unchecked IQR | Pairs faster |
| --- | ---: | ---: | ---: | ---: | ---: |
| `kernel` | 2.022 ms | 1.897–2.038 | 1.551 ms | 1.498–1.599 | 125/125 |
| `activate` | 166.738 ms | 166.297–167.422 | 163.480 ms | 163.238–164.110 | 115/125 |
| `score` | 36.037 ms | 35.809–36.341 | 36.058 ms | 35.812–36.248 | 64/125 |

### Production-scale end-to-end result

Records/second on `production_exact`, 4,096-record batch, run A medians:

| Path | Control | Unchecked | Change |
| --- | ---: | ---: | ---: |
| Single-record inference (`activate_into`) | 24,566 rec/s | 25,055 rec/s | **+2.0%** |
| Batched scoring (`score_records_flat`) | 113,663 rec/s | 113,597 rec/s | −0.06% (neutral) |

`production_2x`, 2,048-record batch, run E medians: single-record inference
10,687 → 10,971 rec/s (**+2.7%**); batched scoring 49,220 → 48,864 rec/s
(−0.7%, inside that run's session spread of 0.90–1.04).

The +2% end-to-end is small next to the −21% kernel gain, and that is the
expected shape: on this topology a forward pass also zero-fills and copies 2,461
inputs per record and runs a `tanh` squash per neuron, so the weighted sum is
only a fraction of the per-record cost. The gain is real, though — it survives
every session of every run and the two quiet runs agree to within 0.05%.

### Binary size

`neat-core` `cdylib`, `wasm32-unknown-unknown`, same flags: 605,582 bytes
(checked) → 608,927 bytes (unchecked), **+3,345 bytes (+0.55%)** — unstripped
and before `wasm-opt`. Immaterial, and in the opposite direction to the naive
expectation: removing the checks removes their shared trap blocks but lets LLVM
inline the gather more aggressively.

## Parity and safety results

- **Numerical parity: bit-identical.** Every one of the 660 paired samples per
  benchmark across all six runs (1,980 in total) produced the same `f64` checksum bits for both variants,
  on all three benchmarks. Expected by construction — only bounds checks were
  removed; lane order, FMA order and the seed-taking remainder are untouched.
- **Parity tests executed on a real wasm runtime.** `neat-core`'s own test
  targets cannot be built for wasm (its `criterion` dev-dependency refuses to
  compile for wasi), so `wasm-bench` re-points them and runs them under Node's
  WASI on `wasm32-wasip1` — the route PR #448 established. **39 tests pass in
  both variants**: `simd_weighted_sums` (23), `simd_scalar_layer` (10),
  `simd_chunk_walk_scaffold` (3), `unchecked_gather_invariant` (3).
- **Safety contract tested.** `neat-core/tests/unchecked_gather_invariant.rs`
  pins the load-time invariant the `unsafe` rests on, including the coverage a
  4-wide gather specifically needs: an out-of-range `from_index` is rejected
  from **every** lane position across a 21-synapse span (not just the first
  synapse, which `network.rs`'s existing unit tests already covered), a loaded
  network's every `from_index` indexes an activation buffer of exactly
  `num_neurons`, and the boundary index `num_neurons - 1` still loads. It runs
  in the native CI suite *and* on wasm.
- **No new unsoundness class.** The obligation is exactly the one
  `scalar::tail_*` and every native SIMD kernel already carry. `CompiledNetwork`
  has public fields, so a caller can still hand-build a network without going
  through `new` — that exposure predates this change and is unchanged by it.
- **Other production targets.** The change is `#[cfg(target_arch = "wasm32")]`,
  so x86_64/aarch64 native builds are untouched; `./quality.sh` (fmt, clippy,
  deny, doc, full native suite) passes.

## Decision and reasoning

**Keep**, with the checked implementation retained behind `checked-gather4`.

Against the acceptance gate:

| Gate | Result |
| --- | --- |
| Production-sized end-to-end inference improves above noise | Yes — +2.0% / +2.7% single-record inference; IQR entirely below 1.0; 115/125 and 76/80 paired samples faster |
| Repeatable across runs | Yes — six runs, two shapes, four or five sessions each; never slower in any session |
| Numerical parity | Bit-identical across 1,980 paired samples |
| Existing tests and CI pass | Yes — native `quality.sh`, plus 39 tests executed on wasm in both variants |
| Safety invariant explicit, tested, auditable | Yes — one helper, `# Safety` contract, dedicated per-lane-position test |
| No material regression elsewhere | Batched scoring neutral (−0.06% / −0.7%, inside session spread); native untouched; +0.55% wasm binary |

**Likely cause of the gain, and of its size.** The four indirect activation
reads are data-dependent on `from_index`, so the bounds checks cannot be hoisted
or proven away — each was a compare-and-branch per lane inside the innermost
loop, i.e. eight checks per eight-synapse dual-accumulator iteration. Removing
them shortens the loop body and lets the gather inline, which is why the
isolated kernel moves ~21%. It moves end-to-end inference only ~2% because the
weighted sum is a minority of a forward pass on this creature: 2,461 input
copies and a per-neuron `tanh` dominate. The batched scoring path is flat
because it never calls `gather4` at all — it runs `weighted_sum_simd_8records`.

**Corollary for future work.** Do not re-attempt this as a "big win" — the
kernel-level 21% is *not* what production sees, and any future gather-level
optimisation should be judged on `activate`, not on the isolated kernel. The
larger end-to-end deltas visible in the loaded-host runs (C, D, F) are noise
amplification, not a better result; the quiet-host numbers are the honest ones.
