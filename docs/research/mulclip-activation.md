# MulClip activation experiment (Issue #760)

## Status

**Completed: do not expose MulClip to consumers.** Corrected protocol v2 found
no qualifying candidate on ARM Linux, x86 Linux or Apple Silicon macOS. The
optional harness, example and integration test were retired from routine Cargo
targets after the negative result. Their tested source is preserved as the
[frozen replay artifact](mulclip/replay.md), with raw v1/v2 results and code
generation evidence. Existing activations, serialisation, defaults and public
interfaces remain unchanged.

Protocol v1's `candidate-all-platforms` classification for d2 is historical,
not an adoption claim. It used reconstructed arithmetic, saturated parity6
and MulClip lane loops that were scalar in the inspected ARM build. Consumer
exposure is conditional on evidence and is not an outstanding implementation
step after rejection.

```mermaid
flowchart LR
    P[Verify published method] --> F[Freeze protocol v2]
    F --> R[Paired runs on three platforms]
    R --> N[No qualifying candidate]
    N --> A[Archive source and raw evidence]
    N --> C[Keep consumer activations unchanged]
```

## Source verification and Float32 contract

The full [paper, arXiv:2610.39343v1](https://arxiv.org/html/2610.39343v1#S2)
was read on 14 November 2026. The earlier note that it remained unreachable is
withdrawn. Section II confirms the coefficients; Appendix A specifies the
operation order. This experiment uses input clipping followed by the published
subtraction, rather than v1's factored polynomial and exact-zero shortcut.

For d2, alpha = 1/4, threshold = 4, peak position = 2. For d3, alpha = 4/27,
threshold = 3√3/2, peak position = 1.5. Coefficients are rounded once to f32.
At the rounded positive d3 threshold, separate Float32 products and subtraction
leave `2^-22` (bits `0x34800000`), rather than zero. Positive infinity clips
to that same value; negative infinity produces the negative residual. NaN
propagates. The paper does not specify these non-finite cases.

The archived 25-test suite checked independent literal bit expectations through
scalar/lane evaluation, including finite and infinite inputs. An independent
f64 closed-form oracle covered the grid, allowing f32 rounding without sharing
the tested polynomial. Both reported defects were observed red before repair:
the d3 endpoint returned zero, and assessment accepted an incomplete run as
ordinary evidence. The repaired suite also checks missing/duplicate cells,
truncated distributions and recorded failures.

## Why v1 was insufficient

The original manifest is `mulclip/manifest.json`. Raw results remain in
`results-aarch64-linux.json`, `results-x86_64-linux.json` and
`results-aarch64-macos.json`; `execution-ci-43568741230.json` records the hosted
run. Each JSON contains 98 completed cells and no recorded failures. These
counts describe those records, not verification that the original assessment
could detect a missing cell. The revised assessor checks the manifest's full
Cartesian product before interpreting summaries.

- The one positive d2 fitness interval was smooth4 × uniform_k4, paired median
  advantage +0.0193, 95% interval [+0.0008, +0.0491] on ARM Linux. Other smooth4
  distributions did not establish an improvement. Repeated fitness on other
  platforms used the same seeds and adds no independent sampling evidence.
- Parity6 had only 64 possible patterns with 128 features. Accuracy saturated
  around one. This did not test the claimed folding advantage. The paper's own
  Appendix E also discusses saturation at small parity widths.
- The monotonic d2 ablation had similar fitness. Existing hard-tanh and ReLU
  were cheaper than MulClip on the v1 timing cells. A tanh-only speed advantage
  therefore did not establish a useful new capability.
- d3 failed the predeclared v1 fitness rule on every platform. That negative
  finding is retained, without treating a Float32 repair as evidence of benefit.
- The Apple Silicon record came from a virtualised macOS guest; hosted-runner
  and container timings are indicative measurements on shared hosts.

## Comparability and the micro/end-to-end discrepancy

The v1 ARM Linux assembly audit is recorded in
`mulclip/codegen-v1-aarch64-linux.txt`. `Prepared::apply_lanes::<8>` executed a
runtime power loop separately for each MulClip lane (`fmul s...`). Tanh's arm
used packed `fmul/fadd/fdiv v*.4s` and `v*.2s`. The graph dot-product loop used
packed arithmetic, but that did not make its activation evaluation SIMD.

V1's micro input was normal with standard deviation 3. Its graph inputs were
uniform on [-1,1], with degree-normalised weights and small biases. The micro
path consequently exercised clipping and folding much more frequently, while
most graph nodes remained near the origin. Branch behaviour, runtime power
loops and graph memory traffic differed; dividing each by tanh did not make
them equivalent workloads. Assembly proves the scalar-lane confound; the
relative contribution of branch prediction to the old timing reversal was not
measured with hardware counters.

V2 specialised the two published polynomial orders and timed matched wide and
narrow micro inputs. Release assembly was inspected on all three measured
architectures: d2/d3 use packed four-lane and two-lane polynomial arithmetic,
plus two scalar lanes. x86 uses `mulps/subps`; ARM uses packed `fmul/fsub`.
The `mulclip/codegen-v2-*.txt` excerpts record that partial vectorisation.
The timed graph paths reuse core's scalar and interleaved dot kernels after
load-time validation, rather than maintaining another timed dot-product loop.
The independent hand graph remains the correctness oracle.

The existing common `production` fixture adds a wide observation layer and
varied sparse fan-in to the uniform and heavy-tail controls. Public
`CompiledNetwork::activate`, `score_records_flat` and independent f64 graph
outputs checked the kernel proxy in the archived integration suite. The proxy still omits aggregate dispatch,
recurrent state, range bookkeeping, MSE reduction and orchestration; it must
not be presented as full production scoring throughput. Fitness is static
random-feature ridge regression, not the paper's temporal ring reservoir or
NEAT topology evolution.

## Controlled revision

`mulclip/manifest-v2.json` and `mulclip/protocol-v2.md` freeze the revised
experiment before validation is measured. They specify fresh calibration and
validation seeds, parity10, the wide sparse fixture, compute limits,
preparation accounting, paired raw distributions, stricter intervals and
complete-cell assessment. V1 results must not be relabelled as v2 results.

The archived example refuses historical v1 execution, missing provenance on
measured runs and failed results. It writes a failed scientific run's record
before exiting non-zero. Quick runs are unassessable. The temporary dispatch
lane was removed after collection; routine CI has no MulClip target.

## Training limitations

Inside the clipping interval the mathematical derivative is
`1 - d * alpha * |x|^(d-1)`. It changes sign beyond the peak and jumps from
`1-d` to zero at the threshold. Saturated inputs have no useful gradient.
The rounded endpoint residual does not restore that gradient.

MulClip is not injective: d2 maps both 1 and 3 to 0.75, so recovering a
pre-activation from the output needs a regime choice. A monotonic ablation
still cannot invert a saturated output uniquely. No `apply_unsquash` regime,
training derivative, consumer mutation or serialisation code is added.
Ridge readout fitness does not validate backpropagation, derivative handling,
memetic training or evolutionary search. These are limits on transferring the
paper's result to core, not evidence that a new activation must be exposed.

## V2 results and decision

Sources: `mulclip/results-v2-*.json` and
`mulclip/cross-platform-assessment-v2.json`. Each platform completed 140/140
cells, 840 calibration fits and 560 validation fits with no scientific-run
failures. Across platforms that is 420 cells and 4,200 fits. Intervals use the
predeclared 99.6875% bootstrap level; timings are **indicative on shared,
virtualised hosts**, not a performance gate.

| Platform | Wall seconds | d2 speed | d3 speed | Fitness decision |
| --- | ---: | --- | --- | --- |
| aarch64-linux | 6.33 | pass | pass | neither qualifies |
| x86_64-linux | 11.40 | pass | pass | neither qualifies |
| aarch64-macos | 13.91 | fail | fail | neither qualifies |

D2 has no task × degree cell with an interval lower bound above zero. D3's
smooth4 median regressions exceed the 0.02 margin on every degree distribution.
Apple Silicon also fails the strict speed rule: d2 uniform_k32 lanes8 has upper
ratio 1.058, and d3 heavy_tail lanes8 has upper ratio 1.093. Those noisy intervals
were kept rather than rerun until favourable.

Wide sparse fixture, median time/tanh (full intervals and 21 raw paired samples
per cell are in the JSON):

| Platform | Path | hard-tanh | ReLU | d2 | d3 |
| --- | --- | ---: | ---: | ---: | ---: |
| aarch64-linux | scalar | 0.772 | 0.745 | 0.800 | 0.799 |
| aarch64-linux | lanes8 | 0.667 | 0.657 | 0.742 | 0.744 |
| x86_64-linux | scalar | 0.278 | 0.278 | 0.290 | 0.291 |
| x86_64-linux | lanes8 | 0.647 | 0.615 | 0.714 | 0.719 |
| aarch64-macos | scalar | 0.434 | 0.376 | 0.414 | 0.471 |
| aarch64-macos | lanes8 | 0.626 | 0.555 | 0.668 | 0.804 |

ARM Linux smooth4 paired NMSE advantage over tanh, positive favouring MulClip:

| Distribution | d2 median [interval] | d3 median [interval] |
| --- | --- | --- |
| uniform_k4 | +0.0041 [-0.0280, +0.0644] | -0.0337 [-0.0817, -0.0089] |
| uniform_k32 | -0.0034 [-0.0191, +0.0408] | -0.0458 [-0.0698, -0.0137] |
| heavy_tail | +0.0082 [-0.0217, +0.0346] | -0.0561 [-0.1113, -0.0350] |
| wide_varied | +0.0162 [-0.0154, +0.0549] | -0.0481 [-0.0651, -0.0328] |

Parity10 mean accuracy ranges from 0.502 to 0.531 across these cells:
non-saturating, but near chance. It supplies no evidence that this static
random-feature model captures the paper's temporal folding advantage.
Repeated platform fitness uses the same seeds, so it is replication, not
additional independent trials. The d2 monotonic ablation again gives similar
smooth-task outcomes; folding has not earned a consumer implementation here.

The revised ARM Linux wide/narrow lane micro ratios are 0.447/0.441 for d2 and
0.404/0.409 for d3. The earlier reversal against lane tanh disappears after the
arithmetic and code-generation repairs. Because both changed together, this
is diagnostic evidence of a v1 confound, not an isolated attribution of its
cost. The v1 branch-prediction contribution remains unmeasured.

## Execution costs and failures

`execution-v2-aarch64-linux.json` and `execution-ci-43572780033.json` record
source, host/CPU, Rust/LLVM, manifest digest, outcomes and preparation.
In-process timing preparation/timing-and-statistics/fitness seconds are:

| Platform | Preparation | Timing/statistics | Fitness |
| --- | ---: | ---: | ---: |
| aarch64-linux | 0.0017 | 1.631 | 4.693 |
| x86_64-linux | 0.0056 | 3.403 | 7.987 |
| aarch64-macos | 0.0028 | 4.041 | 9.867 |

Fitness time includes dataset/graph preparation, feature computation and ridge
fits. Build and test preparation is outside these totals. Two hosted attempts
failed during assembly collection before any measurements or fits; all four
failed host preparations remain recorded in `execution-ci-43572175098.json`
and `execution-ci-43572393572.json`, with their elapsed job windows and causes.
The successful run used an explicit assembly output filename. No validation
seeds or decision thresholds changed between attempts.

The manifest bounded fits at 1,400 and checked a 300-second wall budget between
fits/passes. Hosted jobs had a 30-minute outer bound including installation and
build, rather than the replay command's separate 600-second process wrapper;
actual scientific runs took 6–14 seconds. Timing noise, small seed count and
unmeasured real training remain limits on the conclusion.

The reported manual pre-push failure was reproduced: worker-generated metadata
requested `cargo +29eef336d9b2848a0b548edc03f92a220660cdb8 --version`, treating the
installer action SHA as a toolchain. The pinned action's `action.yml` defaults
to `stable`. No hook or gate metadata was changed. The worker checkpoint path
published the Rust revision; normal metadata-only pushes passed their hooks.
This infrastructure defect was diagnosed, not disguised as a core build error.

## Retention and replay

No consumer activation, derivative/inverse API or migration is justified by
this experiment. Maintaining an optional executable implementation after that
negative result adds no supported capability, so the live harness was removed.
The frozen source patch preserves the corrected numerical implementation,
independent tests and reproducible driver without adding routine CI or public
library maintenance. Its isolated replay passed all 25 tests.

See [replay.md](mulclip/replay.md) for the base revision, patch digest and
commands. The [frozen protocol](mulclip/protocol-v2.md) describes the measured
source, not a current Cargo example. Existing deterministic workspace tests
remain the regression guard for ordinary core behaviour.
