# Behaviour-guard captures (Issue #776)

Three records in `neat-core/tests/golden/compact_legacy_behaviour.json` capture
all three original
[`CompactBehaviourGuard.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactBehaviourGuard.ts)
tests. The recorder calls the frozen `compactCreature(creature, false)` and,
where the original does, `measureBehaviourDeviation` followed by
`isExactBehaviourPreserved`. It does not implement a new acceptance policy.

| Record | Captured outcome and original boundary |
|---|---|
| `logistic_chain` | The two-LOGISTIC chain returns no candidate. The original conditional measurement does not run; candidate and guard are null. This does not demonstrate that a guard rejected a candidate. |
| `identity_chain` | One pass removes `id-b`, retains `id-a`, and combines weights `2 * 0.5 = 1`. The remaining input weight is `1.5`, output bias `0.25`. The original guard returns true. |
| `provenance` | The same IDENTITY graph is loaded, then the original `addTag` calls attach `trainID=T-3840` and `error=0.6310`. Both survive compaction with `approach=compact`. This original checks tags only, so guard is null. |

Raw inputs are cloned before loading can add numeric IDs. Complete wire graphs
are retained before and after compaction, including the one-pass remaining
hidden neuron and the recorded `old-neurons=2` tag. The recorder uses the tags
package declared by the frozen checkout. It does not replace that API with
hand-written tag manipulation.

## Original guard measurements

Only `identity_chain` records a measurement. The recorder calls the original
helper first, then separately records the helper's default probe rows and
outputs as evidence for its summary. The frozen
[`BehaviourGuard.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/architecture/BehaviourGuard.ts)
defines 24 rows, seed `1234567`, cycling scales `1`, `3`, `0.25`, and Float32Array
rounding. The original exactness bound is
`worstDelta <= max(1e-5, 1e-4 * scale)`, where scale is the largest absolute
original output, floored at `1e-3`. These are recorded legacy constants, not new
native acceptance thresholds.

The observed 24 outputs match exactly: worst and mean delta are zero, with
output scale `4.5653886795043945`. The Rust integrity test independently
reconstructs the probe generator's f64 operations and f32 rounding, derives
both output sets from the original affine chain, and recomputes the deviation
summary from the stored outputs. It also checks complete before/candidate
wire graphs, candidate absence, tag multiplicity and the unmeasured boundaries.

These are capture-integrity checks, not native compaction or guard parity.
A finite probe set does not prove equality over every possible input or
representative consumer fitness. In particular, the absent LOGISTIC candidate
must not be reported as a measured guard rejection. The wrapper still computes
both variants internally; this capture does not time a safe-only algorithm.

## Validation and reproduction

All three originals passed unchanged. Every raw input and test name was
independently matched to the original declarations. The integrity test failed
before the capture existed, then passed. Fifteen independent corruptions were
rejected and restored: invented LOGISTIC candidate, altered fold weight,
lost/changed provenance tags, a measurement added to the tag-only test, changed
probe, either output set, worst/mean delta, scale, count, non-finite flag,
verdict and relative threshold. Existing capture files remain unchanged.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_behaviour.json behaviour
cargo test -p neat-core --test compact_legacy_behaviour
```

The shared source guard requires a clean checkout at the recorded revision.
From that checkout, run `deno test -A test/compact/CompactBehaviourGuard.ts` to
repeat the unchanged originals. Broader guard, trace and consumer acceptance
work remains open in the [overall baseline](compaction-baseline.md).
