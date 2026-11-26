# Frozen native scorer comparison (Issue #776)

This extends the [public synthetic TS baseline](compaction-synthetic-performance.md)
with the actual NEAT-AI-scorer CLI. It is an observation of three public graphs
and 32 synthetic rows, not full family acceptance or a new parity tolerance.
The [raw report](compaction-native-scorer.json) preserves every native result,
reference score, graph/corpus/binary hash, compiler and build configuration.

## Same graphs, corpus and scoring cost

The optional fourth argument to `benchmark_legacy_compaction.ts` writes a new
artifact directory containing the original, safe and aggressive graph exports,
a `reference.json` and one `.bin` corpus. Existing directories are rejected.
Graph bytes must match the SHA-256 values from the published TS report. The
native runner also compares every corpus byte against the published probes and
targets encoded as little-endian f32: eight inputs followed by four targets,
32 records, 1,536 bytes. TS already consumes Float32Array inputs, and targets
are exact multiples of 1/8. No graph flag is changed to select a faster path.

**Growth cost differs between the first experiment and the CLI.** The original
TS timing report used `0.0001`; this frozen CLI fixes it at `0.0000001` with no
option to override it. Artifact generation therefore records a fresh legacy TS
`calculate` result at `0.0000001`. The table below compares those matching-cost
scores. It does not compare native scores directly with the earlier table.

The graph exports have `forwardOnly: false`; the native results accordingly
report `record_iterator`. This experiment does not measure the native fused
forward-only pipeline. GPU is explicitly disabled (`--gpu off --cost MSE`);
the scorer's existing CPU label is `cpu-fallback`. Scorer environment overrides
and `RAYON_NUM_THREADS` are removed for these invocations; other host defaults
remain those of the frozen scorer.

## Frozen build

- Scorer [revision a164969](https://github.com/stSoftwareAU/NEAT-AI-scorer/tree/a164969ac4d96ff39eace7086ad22c0538d95705), version `1.1.110`, clean tracked sources.
- Its committed lockfile resolves core tag `v0.23.7` to `3759355634b1cabc8034cd76df5dbd44a867d18c`.
- Build: `cargo build --release --locked --bin rust_scorer`, Rust `1.99.0`.
  The scorer's committed configuration sets `target-cpu=native`; no environment
  Rust flags override it. The runner builds the CLI itself and hashes its bytes.
- The TS baseline remains at `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`; its
  verified WASM bundle declares core `1d44889e405e4e0cfb553321b44cc39e7d771f5f`.
  These core revisions are **different**. No pin was upgraded to manufacture
  agreement, and this is not an isolation of native versus WASM code generation
  at a common revision.

## Observations

Each graph ran once for warmup and three more times in a fresh CLI process.
Repeated errors and scores were identical. Elapsed process times include
startup, graph parsing/compilation, file I/O and scoring, so they cannot be
compared directly with TS activation-only timings or the two-variant rewrite
latency. These short runs are not throughput estimates for production corpora.

| Graph | TS MSE | Native MSE | Native − TS error | TS score at 1e-7 | Native score | Median process ms (min–max) |
|---|---:|---:|---:|---:|---:|---:|
| original | 15.374102312786 | 15.374102922876 | 6.10089299968e-07 | -14.375502072786 | -14.375502682876 | 70.728 (68.528–391.801) |
| safe | 15.374102995178 | 15.374102034265 | -9.60912636572e-07 | -14.375234235194 | -14.375233274281 | 57.446 (54.202–70.073) |
| aggressive | 15.374206307167 | 15.374205759376 | -5.47791087158e-07 | -14.375335417183 | -14.375334869392 | 57.973 (56.998–58.878) |

Every run consumed 32 records and preserved graph mode. The MSE differences
are recorded rather than rounded to zero or accepted by a newly chosen
threshold. On these synthetic rows, both runtimes rank the safe candidate
above aggressive at the CLI's growth cost. That does not establish a consumer
acceptance decision: the cost change from the first experiment matters, and
these targets are not representative training/evaluation data.

## Reproduce

Use clean checkouts at the revisions above, with `deno`, the scorer's declared
Rust toolchain and Python 3.11+ available. From the core checkout:

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/benchmark_legacy_compaction.ts ../NEAT-AI \
  /tmp/compaction-artifact-run.json 3 /tmp/compaction-artifacts
python3 scripts/benchmark_compaction_scorer.py ../NEAT-AI-scorer \
  /tmp/compaction-artifacts /tmp/compaction-native-scorer.json 3
```

Choose a new artifact directory each time. The native runner requires the
frozen scorer revision/core lock resolution, builds with `--locked`, checks
that the lockfile remains unchanged, verifies graph/corpus bytes, and requires
finite repeatable results, the expected record count, MSE cost and CPU path.
It refuses changed graph hashes or corpus bytes before building/scoring.
Full local core quality checks and all 21 original scorer CLI smoke tests
passed. Deliberately changed graph hashes and corpus bytes were rejected before
scoring and restored. Artifact generation preserved every original TS graph
hash, output and score. This is not a claim that every family suite has passed.

Remaining work includes a common-revision native/WASM comparison, more public
graphs and datasets, recurrent/fused/GPU paths, allocation/peak profiling,
exhaustive compaction fixture and caller coverage, and private-consumer release
acceptance. Neither the scorer, its core pin nor production implementations
are changed here.
