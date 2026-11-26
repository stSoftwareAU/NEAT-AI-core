# Legacy compaction test inventory and rule boundaries (Issue #776)

This inventory freezes the two original TS suites at
`60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`. All **58 selected files** ran
successfully on 2026-11-26: **232 distinct test identities, 238 executions**.
The [machine-readable index](compaction-legacy-tests.json) records every test's
reported name, source line/column, execution count and result, plus each source
file's SHA-256. Runtime reporting captures generated tests that a source grep
would miss. The original tests, inputs and tolerances were not edited.

A separate run covers nine explicitly selected related files outside those
directories: **78 top-level tests and two nested steps**, all passed unchanged
on 2026-11-26. See [related caller and invariant tests](#related-caller-and-invariant-tests)
and their [separate runtime index](compaction-related-tests.json).

This is complete for the explicitly selected `test/compact/*.ts` and
`test/optimize/simplify/*.ts` files at that revision. It is **not** the complete
family test-to-rule/caller-to-API matrix required to accept #776. Tests elsewhere,
Ockham/Rebase/core suites, individual assertion-to-primitive mapping and fixture
replay coverage still need reconciliation. A passing legacy test is not native
compaction parity. See the [baseline](compaction-baseline.md) and
[family source inventory](compaction-family-inventory.md).

## Why declarations and executions differ

`CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts` has one registration
helper and **eight** runtime tests. Conversely, `ABSOLUTE.ts` declares one test
but executes **seven** times: Constant, SINE, Cosine, TAN, IDENTITY and COMPLEMENT
import its `makeData` helper and register the test again. The index preserves
that multiplicity without presenting it as seven independent regression cases.
A test identity here is its reported file, name, line and column; this is not a
count of assertions or a proof that every branch is exercised.

## Rule and ownership boundaries

Paths in this table are relative to the pinned NEAT-AI checkout. Rule IDs below
are inventory labels, not proposed API names. `CompactUtils.ts` re-exports O,
P, D, M and U; it does not own another copy of those algorithms. Exactness and
repair classifications belong to individual guarded operations, not filenames.

| ID | Source | Boundary to preserve |
|---|---|---|
| C | [compact/CompactCreature.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/CompactCreature.ts) | Whole-creature candidate pipeline; includes algebraic, cleanup, repair and rescaling paths. Its `safe` name alone is not an exactness proof. |
| T | [compact/CompactUnused.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/CompactUnused.ts) | Trace/statistics-dependent neuron selection and mean-activation replacement; clipping, finite-value refusal and subsequent cleanup need their own contract. |
| O | [compact/OrphanedNeuronCleanup.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/OrphanedNeuronCleanup.ts) | Topology cleanup and hidden-to-constant conversion; also computational constant construction and hidden removal. Preserve constant-support, ordering and UUID policies. |
| P | [compact/SynapsePruning.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/SynapsePruning.ts) | Duplicate-edge summation and exact-zero pruning with typed/IF/last-output-edge guards. The same helper also removes non-finite weights: classify that branch as repair. |
| D | [compact/DeadSubgraphPruning.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/DeadSubgraphPruning.ts) | Remove structure with no output path; preserve outputs and metadata invariants. |
| B | [compact/ParallelBridgeMerge.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/ParallelBridgeMerge.ts) | Algebraic IDENTITY/COMPLEMENT bridge candidates, constrained by type, aggregate target, duplicate-source and load-order guards. |
| K | [compact/ConstantFold.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/ConstantFold.ts) | Constant propagation/folding; `compact/ConstantMerge.ts` owns role/slot-aware canonical constant merging. These are different primitives. |
| I | [compact/IfCollapse.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/IfCollapse.ts) | Constant-selector IF collapse and role stripping; varying-selector and typed-routing refusals matter. |
| W | [compact/SimplifyLargeWeights.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/SimplifyLargeWeights.ts) | Homogeneous rescaling candidates with finite-value and magnitude-penalty acceptance. Optional MCMC temperature can admit a worse penalty; do not infer unconditional score improvement. |
| A | [compact/AggressivePrune.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/AggressivePrune.ts) | Approximate threshold pruning. Frozen, typed and IF-target edges are pinned; an output retains an inbound edge. Caller scoring decides acceptance. |
| V | [compact/CompactVariants.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/CompactVariants.ts) | Caller selection policy: deduplicate by UUID, prefer a distinct aggressive candidate only for a strictly better finite score. Not a graph rewrite primitive. |
| S | [optimize/Simplify.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/optimize/Simplify.ts) | Whole-creature simplification with squash-specific helpers, ordering, random eligible-neuron selection, its own memetic invalidation and behaviour guards. Requires a separate entrypoint contract. |
| M | [compact/MemeticCleanup.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/MemeticCleanup.ts) | Metadata invalidation/repair for removed neurons/synapses and orphan references; not activation algebra. |
| G | [architecture/BehaviourGuard.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/architecture/BehaviourGuard.ts) | Deterministic behavioural probes and typed-IF routing guard. Finite probes are acceptance checks, not a proof for every input. |
| U | [compact/CloneCreatureExport.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/CloneCreatureExport.ts) | Copy isolation and wire metadata, not graph simplification. |
| Q | [methods/activations/SquashUtils.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/methods/activations/SquashUtils.ts) | Aggregation classification used by rewrite guards. |
| R | [compact/RemoveBackwardSynapses.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/compact/RemoveBackwardSynapses.ts) | Repair-only deletion by neuron order for the forward-only pipeline; cannot be treated as recurrent equivalence. |
| X | [architecture/ErrorGuidedStructuralEvolution/DiscoveryNeuronRemoval.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/architecture/ErrorGuidedStructuralEvolution/DiscoveryNeuronRemoval.ts) | Discovery removal caller delegates graph pruning through `corePruneNeuron`; compensation inputs and bounded/IF behaviour acceptance remain distinct from whole-creature compaction. |
| N | [architecture/NewbornGrace.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/architecture/NewbornGrace.ts) | Caller/training policy that protects newborns and spends grace on successful compaction; separate from algebraic rewrite ownership. |

## Selected suite matrix

Each row maps a test file to the boundaries its tests touch. The JSON index
supplies the individual test identities for that row. Multiple IDs do not mean
that every assertion in the file covers every ID; that finer mapping remains
open. Counts are distinct identities / executions. Captured inputs and native
replay status are tracked separately in the baseline, not inferred from these
passing TS results.

| Original test file | Boundaries | Tests / executions |
|---|---|---|
| [test/compact/Cascade.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/Cascade.ts) | T, O | 1 / 1 |
| [test/compact/CleanupOrphanedNeurons.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CleanupOrphanedNeurons.ts) | O | 9 / 9 |
| [test/compact/CloneCreatureExport.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CloneCreatureExport.ts) | U | 2 / 2 |
| [test/compact/Compact.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/Compact.ts) | C, R, O, D | 5 / 5 |
| [test/compact/CompactBehaviourGuard.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactBehaviourGuard.ts) | C, G | 3 / 3 |
| [test/compact/CompactConstant.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactConstant.ts) | T, O | 1 / 1 |
| [test/compact/CompactCreature.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreature.ts) | C, P, D, B | 6 / 6 |
| [test/compact/CompactCreatureAggressivePrune.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureAggressivePrune.ts) | A, C, V | 7 / 7 |
| [test/compact/CompactCreatureCloneOptimisation.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureCloneOptimisation.ts) | C, U | 13 / 13 |
| [test/compact/CompactCreatureComplementBypass.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureComplementBypass.ts) | C | 4 / 4 |
| [test/compact/CompactCreatureConstantFold.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureConstantFold.ts) | C, K | 2 / 2 |
| [test/compact/CompactCreatureConstantMerge.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureConstantMerge.ts) | C, K | 4 / 4 |
| [test/compact/CompactCreatureConstantTransitiveFold.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureConstantTransitiveFold.ts) | C, K | 2 / 2 |
| [test/compact/CompactCreatureDisconnectedConstant.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureDisconnectedConstant.ts) | C, K, O | 1 / 1 |
| [test/compact/CompactCreatureIfCollapse.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureIfCollapse.ts) | C, I | 3 / 3 |
| [test/compact/CompactCreatureIntegrity.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureIntegrity.ts) | C, O, R, P | 6 / 6 |
| [test/compact/CompactCreatureNoOrphans.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureNoOrphans.ts) | C, O, R | 2 / 2 |
| [test/compact/CompactCreatureSimplifyLargeWeights.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureSimplifyLargeWeights.ts) | C, W | 1 / 1 |
| [test/compact/CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts) | C, W | 8 / 8 |
| [test/compact/CompactCreatureVariants.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureVariants.ts) | C, V | 8 / 8 |
| [test/compact/CompactKeepOrder.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactKeepOrder.ts) | T, O | 1 / 1 |
| [test/compact/CompactTagPreservation.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactTagPreservation.ts) | C, O, B | 8 / 8 |
| [test/compact/CompactUnusedFiniteGuard.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactUnusedFiniteGuard.ts) | T | 4 / 4 |
| [test/compact/CompactUnusedOrphanCleanup.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactUnusedOrphanCleanup.ts) | T, O | 2 / 2 |
| [test/compact/CompactUtils.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactUtils.ts) | P, D, M | 19 / 19 |
| [test/compact/ConstantMerge.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ConstantMerge.ts) | K | 6 / 6 |
| [test/compact/ConvertedConstantKeepsUuid_test.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ConvertedConstantKeepsUuid_test.ts) | O | 2 / 2 |
| [test/compact/DeadSubgraphPruning.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/DeadSubgraphPruning.ts) | D | 1 / 1 |
| [test/compact/FixIF.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/FixIF.ts) | T, O | 1 / 1 |
| [test/compact/IfRoutingScoreInvariant.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/IfRoutingScoreInvariant.ts) | C, S, G, X | 9 / 9 |
| [test/compact/IsAggregationSquash.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/IsAggregationSquash.ts) | Q | 3 / 3 |
| [test/compact/MemeticDanglingReferences.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/MemeticDanglingReferences.ts) | O, P, D, A, K, I, C, S, M | 3 / 3 |
| [test/compact/MergeDuplicateSynapses.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/MergeDuplicateSynapses.ts) | P | 2 / 2 |
| [test/compact/MergeDuplicateSynapsesTagDedup.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/MergeDuplicateSynapsesTagDedup.ts) | P | 1 / 1 |
| [test/compact/NewbornGrace.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/NewbornGrace.ts) | T, N | 5 / 5 |
| [test/compact/OrphanedNeuronCornerCases.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/OrphanedNeuronCornerCases.ts) | O | 6 / 6 |
| [test/compact/ParallelBridgeMerge.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMerge.ts) | B, C | 11 / 11 |
| [test/compact/ParallelBridgeMergeExactness.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMergeExactness.ts) | B | 5 / 5 |
| [test/compact/ParallelBridgeMergeOrdering.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMergeOrdering.ts) | B, K | 5 / 5 |
| [test/compact/ParallelIdentityBridgeMerge.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelIdentityBridgeMerge.ts) | C, B | 8 / 8 |
| [test/compact/ParallelIdentityBridgeUnification.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelIdentityBridgeUnification.ts) | C, B | 2 / 2 |
| [test/compact/ParallelMergeCornerCases.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelMergeCornerCases.ts) | B, P | 7 / 7 |
| [test/compact/PruneOrphanMemeticReferences.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/PruneOrphanMemeticReferences.ts) | M | 6 / 6 |
| [test/compact/RemoveBackwardSynapses.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/RemoveBackwardSynapses.ts) | R | 2 / 2 |
| [test/compact/RemoveHiddenNeuronCascade.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/RemoveHiddenNeuronCascade.ts) | O | 2 / 2 |
| [test/compact/SimplifyLargeWeights.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/SimplifyLargeWeights.ts) | W | 4 / 4 |
| [test/compact/UnusedClipped.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/UnusedClipped.ts) | T | 1 / 1 |
| [test/compact/ZeroWeightSynapsePruning.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ZeroWeightSynapsePruning.ts) | C, P, O | 1 / 1 |
| [test/compact/ZeroWeightSynapsePruningTyped.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ZeroWeightSynapsePruningTyped.ts) | P | 1 / 1 |
| [test/optimize/simplify/ABSOLUTE.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/ABSOLUTE.ts) | S | 1 / 7 |
| [test/optimize/simplify/BehaviourGuard.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/BehaviourGuard.ts) | S, G | 3 / 3 |
| [test/optimize/simplify/COMPLEMENT.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/COMPLEMENT.ts) | S | 1 / 1 |
| [test/optimize/simplify/Constant.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/Constant.ts) | S | 4 / 4 |
| [test/optimize/simplify/Cosine.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/Cosine.ts) | S | 1 / 1 |
| [test/optimize/simplify/IDENTITY.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/IDENTITY.ts) | S | 3 / 3 |
| [test/optimize/simplify/MemeticCleanup.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/MemeticCleanup.ts) | S | 1 / 1 |
| [test/optimize/simplify/SINE.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/SINE.ts) | S | 1 / 1 |
| [test/optimize/simplify/TAN.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/optimize/simplify/TAN.ts) | S | 1 / 1 |

## Reproduce the source run

From a clean NEAT-AI checkout at the revision above, with its declared vendored
WASM bundle verified, explicitly select every file. Deno's default filename
matching misses many of them. The commands write reports outside the checkout:

```sh
git rev-parse HEAD
git status --porcelain --untracked-files=no
./build.sh --verify-only
python3 - <<'PYRUN'
from pathlib import Path
import subprocess
files = sorted(str(p) for directory in ("test/compact", "test/optimize/simplify")
               for p in Path(directory).glob("*.ts"))
subprocess.run(["deno", "test", "-A", "--reporter=junit",
                "--junit-path=/tmp/compaction-tests.xml", *files], check=True)
PYRUN
```

The committed index drops elapsed timings and machine paths from the JUnit
report. It groups equal reported file/name/line/column identities and counts
their occurrences. Source hashes are over the unmodified UTF-8 file bytes.
Check the exit status, all reported failures/errors/skips, the selected file set,
and counts before accepting a regenerated index; do not silently overwrite it
when a newer checkout changes tests or their meaning.

## Related caller and invariant tests

These nine files are an explicit additional selection, not an exhaustive scan of
all tests outside the two directories. The separate index preserves every JUnit
name, source location, result and source hash, with `kind` distinguishing a test
from a nested step and `parent` identifying the enclosing test. All 78 tests and
two steps executed once. The two steps belong to one `ShallowClone.ts` test;
Deno includes them in its JUnit total of 80. They are not two additional
independent top-level tests. Across the two runs there are **310 distinct
top-level identities, 316 top-level executions and two nested-step executions**.
These counts do not measure assertions or migration coverage.

| Original file | Top-level tests / nested steps | Boundary and evidence limits |
|---|---|---|
| [test/architecture/NewbornGrace.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/architecture/NewbornGrace.ts) | 6 / 0 | N: tag budget validation, ageing, serialization and genome-identity invariance. These are metadata policy tests; trace-based grace behaviour is covered separately in `test/compact/NewbornGrace.ts`. |
| [test/architecture/StaleUuidAfterStructuralChange.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/architecture/StaleUuidAfterStructuralChange.ts) | 16 / 0 | C, T, O, S, M plus repair, backpropagation, restore-source and fitness callers. Covers genome-cache invalidation, preservation of neuron IDs, tag/memetic-only invariance and stale fitness/deduplication; not all cases rewrite topology. A three-helper registration loop contributes three distinct runtime tests. |
| [test/architecture/training/CompactResultValidation.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/architecture/training/CompactResultValidation.ts) | 4 / 0 | Training producer validation: reproduce orphan-constant rejection, repair the same object and validate its round trip, preserve a valid creature and pass through `undefined`. Repair policy is not an exact algebraic rewrite. |
| [test/creature/ShallowClone.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/creature/ShallowClone.ts) | 16 / 2 | Creature serialization clone ownership, mutable-state isolation, activation/trace equivalence, flags and identity; includes mutation/breeding isolation and compaction preserving the original. Sixteen top-level tests plus two nested mutation/structural-change steps. This is not U (export-object cloning). |
| [test/docs/CompactionInvariantAbsorbed.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/docs/CompactionInvariantAbsorbed.ts) | 2 / 0 | Two documentation-retention assertions: training documentation contains the absorbed invariants and obsolete summary files are absent. Passing these tests is not runtime compaction or scoring evidence. |
| [test/mutate/AddSkipConnection.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/mutate/AddSkipConnection.ts) | 14 / 0 | Mutation operator configuration, topology guards and bypass creation. One compaction comparison requires candidates in both arms, verifies that the control loses a relay, and compares surviving hidden IDs. The other cases are mutation coverage, not compaction parity. |
| [test/propagate/SyntheticSynapsesAggregateNeurons.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/propagate/SyntheticSynapsesAggregateNeurons.ts) | 10 / 0 | Synthetic-edge generation excludes IF/MAX/MIN targets; removal preserves IF roles; training covers aggregate and IDENTITY creatures. Checks guarded by `if (result.compact)` validate a returned candidate only when one exists. |
| [test/propagate/SyntheticSynapsesIntegration.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/propagate/SyntheticSynapsesIntegration.ts) | 8 / 0 | Training integration and call-scoped synthetic options. The compact-output validity check is conditional on `result.compact`; a passing run alone does not prove that a compact candidate was produced. |
| [test/propagate/Traced.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/propagate/Traced.ts) | 2 / 0 | T: trace-based removal of unused neurons; separately, applying learned trace data changes weights. The learning case is a caller boundary, not an algebraic compaction rule. |

The additional callers reach these source boundaries at the same frozen revision:

- [`validateAndRepairCompact`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/architecture/training/TrainingTeardown.ts) invokes `validateOrDiagnose` before the compact result is serialized; the helper returns the original object or `undefined`.
- [`validateOrDiagnose`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/utils/Diagnostics.ts) validates, attempts `fix()` after failure and validates again; this is repair with a failure path.
- [`shallowClone`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/creature/CreatureSerialization.ts) owns the creature clone used by `Creature.shallowClone()`; distinct from cloning exported JSON in U.
- [`AddSkipConnection`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/mutate/AddSkipConnection.ts) owns bypass mutation; the selected test compares its effect on subsequent C compaction.
- [`generateSyntheticSynapses`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/propagate/SyntheticSynapses.ts) owns candidate synthetic edges and aggregate-target exclusions.
- [`removeSyntheticSynapses`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/propagate/RemoveSyntheticSynapses.ts) zeros near-zero untyped synthetic weights (default threshold `1e-7`), selects from `creature.compactVariants(false)` and loads the selected candidate if present. Its `removed` result counts zeroed edges, not proven final graph removals.

To reproduce this additional run, use the same clean, verified legacy checkout
as above. Pass the core checkout's absolute path to this command; the selected
files come from the committed index, not Deno's default filename discovery:

```sh
python3 - /absolute/path/to/NEAT-AI-core <<'PYRUN'
import json
from pathlib import Path
import subprocess
import sys
index = json.loads((Path(sys.argv[1]) /
                    "docs/research/compaction-related-tests.json").read_text())
assert subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() == index["revision"]
files = [entry["path"] for entry in index["files"]]
subprocess.run(["deno", "test", "-A", "--reporter=junit",
                "--junit-path=/tmp/compaction-related-tests.xml", *files], check=True)
PYRUN
```

The same report/status/source-hash verification rules apply. Treat nested steps
separately when comparing identities and totals. Neither this selection nor the
directory selection changes original test inputs or acceptance tolerances.

## Remaining inventory work

- Continue tracing tests outside these two directories beyond the nine related
  files above, including discovery removal and indirect callers. The selected
  files and a preliminary lexical candidate list are not exhaustive.
- Map each individual assertion and numerical tolerance to its primitive or
  caller policy, including tests with multiple boundaries and generated cases.
- Reconcile raw helper inputs with whole-creature captures: typed zero-weight
  tests deliberately include duplicate edges and mixed IDs. A load/normalise
  round trip could change the helper's input before it runs.
- Map every original case to a frozen output/refusal fixture and to native/WASM
  replay. Inventory rows and passing originals do not discharge that step.
- Complete Ockham, Rebase and core rule/consumer mappings, performance/scorer
  baselines and release/private-consumer acceptance recorded in the baseline.

No production implementation or dependency pin changes here. The later issue
acceptance gates remain in force.
