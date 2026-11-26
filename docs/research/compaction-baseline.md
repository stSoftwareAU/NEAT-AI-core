# Legacy compaction baseline (Issue #776)

This is the first capture slice of #776, not its acceptance gate. Production
implementations remain unchanged. Do not start #777/#778 on the strength of
these nineteen captures: the remaining coverage and family checks below are open.

## Revisions and original suites

| Repository | Inspected revision | Verification |
|---|---|---|
| NEAT-AI-core | `3759355634b1cabc8034cd76df5dbd44a867d18c` | `cargo test --workspace --locked` passed |
| NEAT-AI | `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab` | 238 executions passed; 232 distinct compaction/simplification tests |
| NEAT-AI-Ockham | `febc59d836bcb165e99aaa149a13f52bdcad431e` | `cargo test --workspace --locked` passed |

The TypeScript baseline was run with every `.ts` file explicitly selected from
`test/compact` and `test/optimize/simplify`. Deno's default directory discovery
does **not** select most of these filenames. The original assertions and
tolerances were not changed. This is not a claim that the entire family suite
has passed.

The [selected-suite test matrix](compaction-test-matrix.md) records all 58
selected files, their rule boundaries and individual runtime test identities.
It explains the imported and generated tests behind those counts. A separate
selection adds nine related caller/invariant files: 78 top-level tests and two
nested steps passed unchanged. Their identities and source hashes are recorded
separately, with conditional candidate and documentation-only evidence labelled.
The matrix lists the remaining work before the family inventory is exhaustive.

## Captures

`neat-core/tests/golden/compact_legacy.json` stores the original input after its
legacy wire round trip, the actual candidate or `null`, and original activation
outputs at five deterministic probes. `complement_bypass` uses the input from
`test/compact/CompactCreatureComplementBypass.ts`. The other three squash variants are explicitly
synthetic variations of its squashes, not captures of three additional tests. Two further inputs are copied from
`test/compact/CompactCreatureConstantTransitiveFold.ts`, including its
forward-only flag and feedback-loop argument. The three original IF inputs are
read directly from the pinned checkout's `test/data/if-collapse-*.json` files
used by `test/compact/CompactCreatureIfCollapse.ts`. The fourth IF input changes
only the negative fixture's condition weight to zero and is explicitly synthetic;
its `fixture` field records the source of that variation.

Nine further inputs are copied unchanged from `ZeroWeightSynapsePruning.ts`
and all eight tests in `ParallelIdentityBridgeMerge.ts`, both in `test/compact`.
These original tests pass without changes. Three-input cases use five
three-dimensional probes; the earlier ten captures remain unchanged.

| Capture | Observable legacy contract |
|---|---|
| `complement_bypass` | Remove the hidden COMPLEMENT; bias becomes `0.1 + 1.5 * (1 - 0.25) = 1.225`; weights become `-0.75` and `3.25` |
| `identity_fanin_noop` | This two-input IDENTITY fan-in does not meet the legacy chain-fold guard; no candidate |
| `aggregate_guard` | COMPLEMENT feeding MAXIMUM is not bypassed; no candidate |
| `nonlinear_noop` | TANH feeding TANH is not algebraically folded; no candidate |
| `constant_transitive_fold` | Fold the constant-fed LOGISTIC hidden neuron and its constant away, retaining the varying output input |
| `constant_with_varying_input` | Fold the constant input but retain the LOGISTIC hidden neuron because another input varies |
| `if_positive` | A positive constant selector retains only the positive arm, stripped of its role; stranded constants disappear |
| `if_negative` | A negative constant selector retains only the negative arm, stripped of its role |
| `if_zero` | Synthetic zero-selector boundary variation of the negative fixture still selects the negative arm |
| `if_varying` | A varying selector retains the IF neuron and its typed condition input |
| `zero_weight_orphan` | Remove the zero edge and stranded hidden neuron; retain the direct input weight `1.25` and output bias `0.1` |
| `parallel_two` | Merge two IDENTITY bridges into retained `h1` with input weights `1` and `0.45`, and a unit output edge |
| `parallel_weights` | Preserve those products and merge bias to `2 * 0.1 + 1.5 * 0.2 = 0.5` |
| `parallel_bias` | Merge three biases to `2 * 0.5 + 3 * -0.3 + 0.5 * 0.1 = 0.15`; keep output bias `0.2` |
| `parallel_duplicate_sources` | The original duplicate-source group returns no candidate |
| `parallel_nonlinear` | The original LOGISTIC bridges return no candidate |
| `parallel_fanin` | A two-input bridge plus a single-input bridge returns no candidate |
| `parallel_forward_only` | Merge the two bridges while retaining `forwardOnly: true` |
| `parallel_three` | Retain `h2` with weights `0.5`, `1.6`, `-1.2` and merged bias `0.25` |

`neat-core/tests/compact_legacy_capture.rs` checks the changed/no-op contracts
and native activation of both halves against recorded legacy outputs at the
original regressions' activation tolerances: `1e-5` for the initial squash
variants, `1e-6` for the constant-fold and IF cases, and `1e-7` for zero-weight
cleanup. The parallel source tests assert graph arithmetic at `1e-12`, which
the native replay preserves; they specify no activation tolerance. An additional
`1e-6` bound checks their native activation against the frozen TS outputs.
The retained bridge, its UUID, edge roles, output bias and forward-only flag
are checked separately: one legacy pass does not fully flatten these networks.
The duplicate-source and fan-in source tests allow other passes to fire; their
`null` captures describe the actual frozen result, not a new universal refusal rule.
This harness does not compare a future
Rust compactor yet, and five probes do not prove equivalence for every input.
The recorded scores use the legacy `Score.calculate` with error `0` and growth
cost `0.0001`, matching the constant-chain and IF regressions. Both constant cases and all
three fixed-selector IF cases must
retain or improve that score. These are complexity-score baselines at zero
error, not full-corpus scorer acceptance or native scorer parity.
The unmodified legacy implementation remains the oracle. Mutation evidence:
adding `0.1` to the compacted COMPLEMENT output bias fails at `[0, 0]` with
`1.325 != 1.225`; the mutation was reverted. Adding `0.1` to either constant-case output bias
also fails replay; both mutations were reverted. IF mutations independently change the positive
arm source, retain a negative role after collapse, perturb the zero-selector
output bias, and strip the varying condition role; each fails its structural or
activation assertion, and all mutations were reverted.
All nine zero/parallel cases also reject independent mutations: perturbed
zero-edge survivor/parallel weights, incorrect merged biases, a candidate in
each no-op case, or a lost forward-only flag. All mutations were reverted.

Regenerate only from a clean checkout at the recorded revision:

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy.json
cargo test -p neat-core --test compact_legacy_capture
```

The capture script refuses another revision or tracked modifications. Outputs
are records for review, never automatically updated by the native test.

## Initial caller and ownership inventory

Paths below are relative to the named repository. This is the inspected subset,
not an exhaustive family caller-to-API matrix.

The [family source and dependency inventory](compaction-family-inventory.md)
records pinned source revisions, declared dependencies, lockfile resolutions
and additional implementation boundaries across the 14 public repositories.
Its remaining verification gaps are explicit; it does not complete this gate.

| Repository / callers | Current API / ownership |
|---|---|
| NEAT-AI `src/Creature.ts` | `compact` and `compactVariants` delegate to `src/compact/CompactCreature.ts` |
| NEAT-AI `src/blackbox/FineTune.ts`, `src/propagate/RemoveSyntheticSynapses.ts` | Candidate variants and caller-side selection |
| NEAT-AI `src/architecture/training/TrainingTeardown.ts`, `TrainingPredictiveCoding.ts` | Trace-dependent `compactUnused`, then variant generation; training deadlines stay at the caller (these TS callers are distinct from the core engines removed in #414) |
| NEAT-AI `src/NEAT/NeatEvolution.ts` | `src/optimize/Simplify.ts::simplify` |
| Ockham `ockham/src/canonical.rs` | Exact fixed-point zero-edge and identity cleanup, deterministic report |
| Ockham `ockham/src/collapse.rs` | Identity candidate; typed/aggregate/self-loop and cost guards |
| Ockham `ockham/src/cascade.rs` | Removal **estimate**, not a graph rewrite; do not migrate it as a second cleanup engine |
| Core `prune_cleanup`, `prune_rewrite`, `prune_json` | Existing shared rewrite owner and wire boundary; reuse the pruning parity matrix |

The existing [pruning matrix](pruning-parity-matrix.md) documents deliberately
different constant-support, IF-repair and memetic policies. Whole-creature
compaction must not silently inherit one caller's policy for another. Preserve
those distinctions in explicit contracts before moving algorithms.

## Outstanding before accepting #776

The [raw helper baseline](compaction-helper-baseline.md) separately captures
typed zero-weight pruning, duplicate-role policies and backward-edge repair.
Those intermediate inputs must not be normalized through a whole-creature
loader before invoking their original helper.

- Freeze remaining original fixtures: identity chains/parallel bridges, typed
  remaining typed-IF guards, remaining transitive-constant cases, zero weights, recurrent refusal, ordering, metadata,
  UUID/memetic invalidation, clipping, non-finite traces and deadlines.
- Classify large-weight rescaling, aggressive pruning, trace-based pruning and
  repair-only transforms from their actual guards; candidate acceptance and
  authoritative scoring remain outside the graph engine.
- Record Ockham outputs and original suites, all family callers and dependency
  pins, and any additional engine/packaging consumers.
- Extend replay to native and WASM compaction, with independent legacy oracles;
  do not label activation-only checks as compaction parity.
- Publish reproducible large public/synthetic creature benchmarks, allocation
  and serialization costs, and original-versus-candidate scorer baselines.
- Run every required family suite and release/artifact check. Private consumer
  verification stays in its own environment.

Later migration issues retain their explicit dependency gates. The final
family acceptance issue #780 also depends on consumer cutovers and TypeScript
retirement in other repositories; this baseline does not close those gates.
