# Large-weight simplification baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_simplify.json` freezes
`simplifyLargeWeights` from NEAT-AI at revision
`60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`. It holds 23 cases, a
`penalty_empty` record, and the shared `magnitude-penalty-corpus.json` with the
penalties the legacy code computed for it. Numbers are stored as JS
`String(value)`, so `NaN` and `Infinity` survive JSON.

`scripts/legacy_compaction_simplify.ts` calls the frozen implementation. It
does not copy the algorithm. The TypeScript stays in NEAT-AI; no caller is
switched by this change.

| Case | Meaning | Outcome |
|---|---|---|
| `absolute` | ABSOLUTE neuron, huge inbound, tiny outbound | changed |
| `relu_negative` | ReLU with a negative bias and outbound weight | changed |
| `tiny_inbound` | IDENTITY with tiny inbound, huge outbound | changed |
| `typed_if` | IF neuron with condition, positive and negative inbound roles | changed |
| `mean_hypot` | MEAN feeding HYPOTv2 in a chain | changed |
| `self_loop` | ABSOLUTE neuron with a recurrent self-edge | changed |
| `worse_mcmc_accept` | Worsening rescale accepted: fixed draw 0 is below exp(-delta/T) | changed |
| `tanh_left` | TANH is not a candidate squash | unchanged |
| `balanced` | Scales within the 1000 ratio | unchanged |
| `output_skipped` | Output neurons are never rescaled | unchanged |
| `nan_bias` | NaN bias makes the inbound maximum NaN (JavaScript `Math.max` propagates it), so the imbalance test fails and the neuron is skipped | unchanged |
| `nan_inbound_first` | A NaN first inbound weight makes the inbound maximum NaN even though a later weight is 1e6, so the neuron is skipped | unchanged |
| `infinite_inbound` | An infinite inbound weight gives an infinite scale factor, which is refused before any rescale | unchanged |
| `worse_greedy` | Rescale would worsen the penalty and there is no temperature | unchanged |
| `worse_mcmc_reject` | Same worsening rescale, draw 0.999999 rejects it | unchanged |
| `supported_ReLU`, `supported_LeakyReLU`, `supported_MAXIMUM`, `supported_MINIMUM`, `supported_IF`, `supported_HYPOT`, `supported_HYPOTv2`, `supported_MEAN` | The eight original per-squash fixtures of `test/compact/CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts`, unedited | changed |

## Exactness classification

- The rescale is exact in real arithmetic for positively homogeneous squashes.
  It is not bit-exact in floating point, so outputs agree to rounding only.
- Acceptance is the magnitude-penalty heuristic, not the authoritative score.
- MCMC can accept a worse penalty. The caller supplies the uniform draw, so the
  result is reproducible.
- The memetic record is cleared when anything is rescaled, as TypeScript
  deletes it. It is kept when nothing changes.

## Magnitude penalty

`magnitude_penalty` in `neat-core/src/magnitude_penalty.rs` restates the rule
in NEAT-AI-scorer's `rust_scorer/src/scoring.rs`. Both are pinned to the same
corpus. The scorer is not switched by this change.

## Reproduction

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_simplify.json simplify
cargo test -p neat-core --test compact_simplify_large_weights
```
