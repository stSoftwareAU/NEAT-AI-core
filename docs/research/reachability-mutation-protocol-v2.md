# Reachability mutation protocol v2 — predeclaration

This protocol is fixed before running the extended comparison for issue #759.
It tests an independently defined bridge policy, not the paper's guided DST.
No production policy changes depend on these results. The original 40-seed
result remains an inconclusive, small synthetic control.

## Workloads and pairing

Run 40 independent paired seeds (`0..40`) for each of these generated families:

| Family | Hidden structure | Purpose |
| --- | --- | --- |
| `legacy` | Original four-hidden teacher and small cleaned student | Historical low-opportunity control |
| `sparse` | 24 TANH hidden neurons | Sustained reconnection opportunities |
| `mixed` | 48 hidden neurons cycling TANH, LOGISTIC, MEAN, MAXIMUM | Aggregate semantics and larger candidate space |
| `conditional` | 24 TANH hidden neurons plus a grafted IF node | Typed positive/negative branches |

Extended teachers have four inputs, one IDENTITY output, one bias-1 support
constant, two distinct uniformly chosen input sources per hidden neuron,
biases and constant-edge weights uniform in [-0.3, 0.3], and other weights
uniform in [-1, 1]. Each hidden neuron feeds the output. The conditional family
also grafts an IF using the canonical constructor: input 0 is the condition,
even/odd hidden neurons feed its positive/negative branches, and it feeds the
output. The student keeps teacher parameters but loses the input edges of
three quarters of the numbered hidden neurons (seed rotates the retained
quarter). Cleanup owns the resulting canonicalisation; the remaining constant
inputs preserve the disconnected hidden neurons as useful BiasDriven structure.

Both arms share teacher, initial student, calibration and validation records,
and proposal-indexed weight/selection RNG streams. SplitMix64 stream IDs are
fixed in the harness. Seeds are independent across trials; families are
analysed separately, never pooled. Alternate arm execution order by seed to
reduce systematic order bias. There is no hyperparameter tuning or pilot
selection using these seeds or validation records.

## Search, validation and limits

Each trial generates 256 calibration and 256 validation records with independent
uniform inputs in [-1, 1]. Targets are scalar teacher activations. Selection
sees calibration only; validation is evaluated on the initial and final models.
Each arm attempts at most 600 proposals and 150 valid offspring evaluations,
plus one initial calibration and two validation evaluations. An empty candidate
pool is a supported terminal state. Hitting the proposal cap before the
evaluation budget, a non-finite score, or an unexpected error is an incomplete
run: persist failures and available arm results, exit non-zero, and withhold
the overall verdict.

Unbiased selection samples uniformly from `scan_available_connections`.
Bridge selection uses its subset with an input-reached source and an
output-reaching target, excluding Live-to-Live pairs; if empty, sample the
unbiased pool. Add one untyped edge to a clone. Validate topology, then call
cleanup for canonicalisation and full authoritative validation before scoring.
Accept only a finite, strictly lower full-calibration MSE. Parents are immutable
until acceptance. No reachability-based deletion is introduced.

## Costs and outputs

Record teacher construction, data generation, initial cleanup and shared
preparation time; arm search time includes cloning, proposals, validation,
candidate cleanup and scoring. Time calibration and validation scoring
(including compilation) separately. Charge half the shared preparation to each
arm's end-to-end cost, but retain the once-per-pair actual cost. Record the
experiment process duration and build duration separately from search (external execution evidence).
Report calibration improvement per evaluation, per calibration-evaluation hour,
and per allocated end-to-end hour. Wall-clock values are indicative on a
shared host and never a pass/fail criterion.

Report initial/raw/cleaned/final structure, DeadEnd+Isolated separately from
BiasDriven, offspring means, bridge-selected/fallback counts, proposal and
evaluation counts, acceptance and rejection causes, stopping reasons, per-seed
pairs, coverage and failures. Source provenance is the committed Git tree and
full commit, embedded harness-source equality check, lockfile blob, compiler
and platform; reject dirty tracked sources or a stale harness binary. The
machine manifest is written before execution and must match the executable's
protocol and provenance. New results use schema version 2; retain the old
results file without reinterpreting its fields.

## Uncertainty and decision

Report paired bridge-minus-unbiased distributions for each family. Use 12,000
paired bootstrap resamples with seed `0x00B0_0757_12AB`; percentile intervals
use sorted indices 75 and 11,924 (98.75%, Bonferroni for four families). Report
sample counts and wins/losses/ties; an empty summary is unknown, not zero.

A family favours bridge only when its final validation-MSE interval is wholly
below zero, mean invalid-offspring difference is non-positive, and at least
20% of bridge-arm proposals actually use the bridge subset. An interval wholly
above zero favours unbiased; otherwise the result is inconclusive. Adoption
would require all three extended families to favour bridge with complete
coverage. The legacy family is a control. Even that outcome establishes only a
generated-workload result; representative product benefit remains unmeasured.
Dead-structure reduction and invalid-rate reduction may be structurally absent
in this addition-only, already-cleaned design and must be reported as such.

## Reproduction

From clean committed sources, export
`NEAT_EXPERIMENT_SOURCE_TREE="$(git rev-parse HEAD^{tree})"`, build the release
example with that environment value embedded, then run it with
`--declare <manifest.json>` before running
`--manifest <manifest.json> --out <results.json>`. Keep research execution out
of CI. No private models, corpora or results are needed or published here.
