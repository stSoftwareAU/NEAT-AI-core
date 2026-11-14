# MulClip protocol v2 — frozen before measurement

Historical protocol: the live harness has been retired after a negative
result. The commands below refer to its frozen source; use [replay.md](replay.md)
to restore it in an isolated worktree. The scientific protocol below is unchanged.

Issue #760. The accompanying `manifest-v2.json` defines the complete run.
Protocol v1, its manifest, three platform results and cross-platform assessment
remain historical evidence; they do not establish a v2 candidate. Replay v1 at
commit `9e506f9`. The current example refuses to label a new run as v1.

## Method and controls

Use [arXiv:2610.39343v1, Section II and Appendix A](https://arxiv.org/html/2610.39343v1#S2).
Round coefficients and thresholds once to Float32. Clip the input before
polynomial evaluation. For d2 subtract `(alpha * abs(xc)) * xc`; for d3
subtract `(alpha * (xc * xc)) * xc`. Keep multiply/subtract separate, without
FMA contraction. NaN propagates; infinities reach the clipped endpoint.
These are harness conventions for non-finite inputs, which the paper does not
specify. Do not force the rounded endpoint to zero or the ablation to one.

Compare tanh, hard-tanh, ReLU, d2/d3 and both monotonic-clip ablations. Scalar
controls use `apply_squash`; lane controls use `squash_xn`, with the existing
ReLU fallback. No squash code or consumer API is added.

## Paths and fixtures

- Scalar and eight-record interleaved forward passes call core's existing
  unchecked weighted-sum kernels after `CompiledNetwork::from_parts` validates
  their spans and indices. Allocate scratch once and reset it per call. Include
  input loading, neuron processing and output consumption; exclude preparation.
- Reuse `benches/common::build_network` for the `production` fixture:
  2,461 inputs, 1,673 computed neurons, varied fan-in around 13. Retain the
  smaller uniform fan-in 4/32 and truncated heavy-tail graphs as contrasting
  workloads. These are homogeneous standard activations, without aggregates,
  recurrent state, range-limiting bookkeeping, MSE reduction or host scheduling;
  their timing is a forward-kernel proxy, not full application throughput.
- Fitness uses 128 computed neurons. The wide fixture is scaled to the task's
  input width and 128 neurons through the same common builder and its varied
  fan-in rule; it is not a production-scale training experiment.
- Activation-only timing uses the same seeded normal draws at standard
  deviations 3 (`wide`) and 0.3 (`narrow`). Both controls and candidates receive
  identical inputs. This distinguishes the old wide micro workload from the
  mostly unsaturated graph pre-activations without changing observed v1 data.
- Inspect emitted release assembly on each measured architecture. An array of
  eight values alone is not evidence of SIMD. Preserve instruction excerpts
  and build provenance; partial vectorisation and scalar lanes must be named.

## Search, validation and decision

Calibration seeds 7602011–7602013 select one of five input scales per
activation/task/degree distribution. Validation uses untouched seeds
7602101–7602110. The graph and dataset are paired across activations at each
seed. No seed is selected by its validation score.

Use parity10 (1,024 possible patterns versus 128 features) and smooth4, with
1,024 train and 512 test records and the existing ridge readout. Random sampling
can repeat parity patterns across train/test; this is a bounded capacity probe,
not a claim about unseen-pattern generalisation or temporal parity reservoirs.
Report per-seed metrics, not only their mean. Do not replace or retune a task
once validation is read, even if it is uninformative.

Each speed cell contains 21 paired, shuffled repetitions following three
warm-ups, over 512 records or 4,096 micro inputs. The percentile bootstrap uses
10,000 resamples and central level 0.996875 (Bonferroni 0.05/16 for two
candidates × two tasks × four graph distributions). This is a conservative
exploratory screen, not a powered equivalence trial.

A local candidate must have median time/tanh <= 0.95 and interval upper < 1
on **both** paths of **every** graph distribution; its median paired fitness
advantage must exceed -0.02 everywhere, with interval lower > 0 in at least one
cell. Report cheaper controls and folding ablations alongside it. Full
cross-platform candidacy requires a valid run on aarch64 Linux, x86_64 Linux
and Apple Silicon macOS. Missing platforms are not passing platforms.
Consumer exposure is conditional on convincing evidence, not a remaining
implementation obligation when the experiment rejects the candidate.

## Compute and records

One complete run plans 140 cells, 840 calibration fits and 560 validation fits
(1,400 total). No additional searches are permitted. The manifest limits fits
to 1,400 and wall time to 300 seconds. The harness checks wall time between
fits and timing passes; bound the whole command with a 600-second external
timeout as well. Preparation and statistical analysis count in total wall time;
report timing preparation, timing/statistics and fitness wall time separately.
Build/test and code-generation work is recorded in the execution note outside
those in-process totals.

Run each platform once after freezing this protocol. Do not discard slow or
failed runs: retain the JSON, process exit and failure cause. Assessment refuses
recorded failures, missing/duplicate cells, incomplete distributions and
coverage that disagrees with the manifest. Quick runs are never assessable.
Keep research execution separate from routine CI. Timings on this shared
container and hosted runners are indicative, never a build gate.

```bash
timeout 600 cargo run --release -p neat-core --example mulclip_experiment -- \
  --manifest docs/research/mulclip/manifest-v2.json \
  --out results-v2-<arch>-<os>.json \
  --code-rev "$(git rev-parse HEAD)" --rustc "$(rustc -vV)" < /dev/null
```

Use `gtimeout` where macOS supplies GNU coreutils under that name. Record host,
CPU, Rust/LLVM, source revision, manifest digest, process status and actual
coverage with the results. Keep derivative/inverse and training limitations in
the research note. Decide retention from the evidence; do not extend the public
activation enum to justify keeping experimental code.
