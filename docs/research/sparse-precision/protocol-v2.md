# Sparse precision protocol v2 — frozen before validation

Part of issue #758. The v1 manifest and Linux samples remain historical evidence;
this revision does not overwrite or pool them. The v2 validation seeds have not
been used for tuning. Freeze this document, manifest-v2.json and the source in a
commit before running validation. Any later behavioural revision needs a new
protocol and new validation seeds.

## Unchanged controls

Six shapes, inactive fractions 0/0.5/0.9, widths 1/8/32, 64 records per sample,
15 paired repetitions, five conversion repetitions, 2000 bootstrap resamples,
calibration seed 1001 and eight calibration records. The speed threshold stays
0.90; absolute tolerances stay 0.001 (f32) and 0.02 (reduced storage). Required
platforms stay aarch64 Linux, x86_64 Linux and Apple Silicon macOS. No search or
hyperparameter selection is performed. Timings on shared hosts are indicative.

## Revisions and reasons

- Validation network seed 12002 and record seed 13003 replace the already viewed
  v1 validation. A shape/fraction uses the same network and records at every
  width; width no longer changes the topology seed. Calibration remains separate.
- Keep `current` as the original chunked public production baseline. Add
  `current_whole_scan`, which makes one `score_records_flat_into` call over the
  corpus at widths above one. Both use `activate_into` at width one. The public
  batched wrapper allocates BatchScratch per call; its reusable entry point is
  private. This control exposes caller allocation overhead without widening core.
- Keep warm execution samples. Also pair a second measurement that owns typed
  parts, constructs CompiledNetwork, converts to the variant, initialises scratch
  and scans the corpus once. Current uses that constructed network directly;
  sparse variants pay for constructing the canonical network before conversion.
  This measures the existing conversion entry path; it does not measure a future
  direct sparse compiler. Output allocation is shared and outside these samples.
- Record raw constructor and conversion samples and first-run cost separately.
  Runtime compilation means `CompiledNetwork::from_parts`, including owned parts,
  validation, hot views and initial buffers. JSON parsing, disk I/O, directory
  orchestration and GPU upload are outside this experiment. Rust build time is
  recorded separately, including whether the build used a warm cache.
- Record sparse scratch from actual capacities. Report current scratch as a
  clearly labelled length model of `CompiledNetwork::assemble` and
  `BatchScratch::new`; it excludes allocator rounding and Vec headers. Static
  weight/index/neuron payload remains length-based. These are not RSS or peak
  process memory figures; conversion temporaries and simultaneous live variants
  contribute to process memory but are not claimed as measured peak memory.
- Keep the original 30-second cell and 1500-second run budgets. Include preparation
  in the cell budget and limit subsequent cells by remaining run budget. Limits
  are soft at paired-round boundaries: compilation, one complete round and
  bootstrap summarisation can overrun. Preserve raw samples and mark unfinished
  cells `incomplete`; they never receive a speed verdict. The CLI saves results,
  emits an EXPERIMENT_FAILURE marker and exits non-zero for incomplete or failed
  combinations. Expected numerical failures therefore also return non-zero.
- Require both warm and end-to-end speed criteria for adoption. This strengthens
  the v1 warm-only rule prospectively; the 75% denominator is planned cells,
  including accuracy failures. Missing platforms prevent a cross-platform verdict.
- Results schema is v2, in a separate filename. Echo requested and effective
  manifests, classify quick runs as smoke, enumerate missing required platforms
  and mark Metal explicitly unrun. A current-whole-scan ratio accompanies each
  variant so a win over repeated allocation cannot be presented as a layout win.

## Semantics and identity

The sparse interfaces accept finite biases, weights and records. Reject a weight
that encodes to infinity before a saturating squash can conceal it. Non-constant
aggregate neurons and non-forward edges remain refused. Constants produce their
bias; zero-incoming neurons still squash the bias; zero-weight removal happens
before encoding. Scratch must be fully initialised on every call. Parent networks
remain immutable. The independent scalar f64 oracle calls no production kernel
or squash; generated workloads use Tanh only.

CSR is a *family* of layouts. Production includes AoS synapses and a redundant
SoA hot view, specialised SIMD, and fixed internal eight-lane batching. The
experimental CSR has u32 row offsets and u16 columns with a plain loop. Thus this
comparison measures concrete layout/kernel combinations; it cannot identify a
pure layout effect or prove that every possible CSR/COO/blocked kernel loses.
The blocked layout pads each row to four edges; it is not matrix BSR or 2:4.
Software bf16/f16 storage is decoded to f32. No hardware reduced-precision
arithmetic or conversion is measured.

Record git HEAD, source/manifest/lockfile git blob IDs, full compiler version,
Cargo version, flags, kernel, CPU identification and process parallelism. Refuse
tracked source changes before measurement. Per-cell network topology and
little-endian record bytes have FNV-1a fingerprints (reproducibility checksums,
not security hashes), seeds, actual edge counts and zero-weight counts. Generated
shapes mirror production dimensions; they are not a production model or corpus.
