# Public synthetic legacy compaction measurements (Issue #776)

This is a **TypeScript-host baseline**, not completion of #776 or native
compaction parity. It invokes the frozen legacy `compactCreatureVariants`
entry point with feedback disabled and MCMC temperature omitted. The legacy
`compactCreature` wrapper invokes this same two-variant pipeline and returns
only `safe`; its wall time must not be described as safe-pass-only cost.

The [frozen native scorer comparison](compaction-native-scorer.md) uses the same
graph bytes and synthetic rows. It matches the CLI's different growth cost and
records both dependency revisions and observed differences; it is not a new
acceptance threshold.

## Source and measurement boundary

- Legacy revision: `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`.
- Generator: the public [productionScaleCreature](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/bench/_productionScaleCreature.ts), seed `1910`, unchanged.
- Source graph: 5,300 neurons including eight inputs, four outputs, and 87,096
  synapses. This is synthetic data, not an exported private creature.
- One warmup excluded from the summary, then three measured repetitions. Every
  repetition loads a fresh copy of the same exported graph; global RNG is reset
  to the same seed. Graph generation and WASM initialization are outside the
  operation timings. Generator time is recorded separately.
- Parsing, `Creature.fromJSON`, export and JSON encoding are timed separately
  from the whole candidate pipeline. The compaction call's own cloning,
  serialization, normalization and candidate construction remain inside it.
- Activation runs two batches of 32 fixed eight-input probes per graph. The
  first batch may include lazy graph setup; neither batch is a cold-process
  benchmark. Score timing measures the subsequent legacy `calculate` call with
  the measured MSE and growth cost `0.0001`, with whatever cache state that
  pipeline produced.
- The process uses the legacy vendored WASM activation and scoring primitives.
  `build.sh --verify-only` must pass before measuring. The report retains the
  declared core revision/asset SHA and memory model. This is not a benchmark of
  a native Rust whole-creature compactor or the external scorer service.

The [raw report](compaction-synthetic-performance.json) retains all samples,
including the warmup, environment, graph hashes, outputs, target rows, timings
and memory snapshots. Source graph SHA-256 (UTF-8 exported JSON bytes):

`97bf48c0f288468d8099f158414dc418206a69ec48cedcfbf031cfdb2b286eb8`

Input JSON size: 7497202 bytes. The frozen generator and graph hash make the
input reconstructible without checking in a multi-megabyte creature export.

## Observed results

Measured on 2026-11-26T16:17:34.306Z with Deno 2.9.8 / V8 15.0.245.2-rusty, Linux x86_64,
AMD EPYC 9V74, five reported logical CPUs. This is a single shared cloud host;
three repetitions characterize this run, not a confidence interval, latency
promise or regression threshold. Compare implementations on the same host and
inputs before drawing speedup conclusions.

| Operation | Median ms | Min–max ms |
|---|---:|---:|
| JSON parse | 22.402 | 22.078–26.764 |
| Creature load | 98.835 | 83.596–103.170 |
| Export | 7.345 | 5.112–8.332 |
| JSON encode | 27.035 | 21.531–47.300 |
| Both compaction variants | 9113.616 | 8404.694–9789.003 |

The following scores use **public synthetic targets**, generated independently
of the graph outputs by the formula in the script. MSE averages all 128 scalar
output errors (32 rows × four outputs). These targets are not representative
training/evaluation data. The zero-error score reports the legacy score with
prediction error set to zero. Negative scores are retained as
returned by the legacy implementation.

| Graph | Neurons incl. inputs | Synapses | Synthetic MSE | Legacy score | Zero-error score | Max output delta from original |
|---|---:|---:|---:|---:|---:|---:|
| original | 5300 | 87096 | 15.374102312786 | -15.773862312786 | -0.399760000000 | 0 |
| safe | 4341 | 69834 | 15.374102995178 | -15.505343011240 | -0.131240016062 | 1.90734863281e-06 |
| aggressive | 4341 | 69621 | 15.374206307167 | -15.503316323275 | -0.129110016108 | 0.00145363807678 |

Both variants reduce graph size; neither graph is declared equivalent or
accepted by this experiment. The observed output/error differences remain in
the report. A higher synthetic score is not authority to adopt the aggressive
candidate, change numerical tolerances, or cut over a consumer.

Two independent process runs reproduced the input/candidate hashes, output
arrays and scores exactly; timing and memory samples naturally differ. The
published run also requires equal graph/output/score fingerprints across its
warmup and measured repetitions, and checks that compaction leaves its source
export unchanged. MSE was independently recomputed within `1e-12` from every recorded output
and target; `score = zeroErrorScore - MSE` was checked within `1e-12`.

## Memory and remaining gates

`Deno.memoryUsage()` snapshots bracket candidate construction. They are process
RSS/heap/external memory observations, **not allocated bytes, peak RSS or an
allocation profile**. Garbage collection, retained prior samples and allocator
reuse affect them, including negative deltas. No forced GC is used. Allocation
and peak-memory instrumentation remain open work.

Also outstanding: native and WASM compaction-path measurements once the agreed
APIs exist, external authoritative-scorer comparison, more public graph shapes
(including refusal/no-op, aggregate and recurrent cases), deadline behavior and
representative consumer acceptance. This run neither changes production code
nor satisfies the exhaustive fixture/matrix or release gates in the
[baseline](compaction-baseline.md).

## Reproduce

From the core checkout, with `deno` on `PATH` and a clean legacy checkout at the
revision above:

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/benchmark_legacy_compaction.ts ../NEAT-AI \
  /tmp/compaction-synthetic-performance.json 3
```

The script rejects fewer than three measured repetitions. It shares the exact
revision/clean-checkout guard with fixture capture and verifies the legacy WASM
bundle before executing. In validation, a clean frozen worktree was accepted,
a tracked modification and a wrong revision were rejected, and existing
whole-creature fixtures regenerated byte-identically after guard extraction.
No elapsed-time assertion is used as a unit test.
