# Raw legacy compaction helper captures (Issue #776)

These eleven records supplement the
[whole-creature baseline](compaction-baseline.md). They freeze the unmodified
NEAT-AI helpers at `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`, using inputs copied
unchanged from the original tests. All twelve original tests in the five
selected source files passed unchanged. The records cover ten of those tests:
one memetic test supplies both a dirty and a clean input; the two creature-based
memetic/`fix()` tests are not captured by this raw-helper mode.
This remains a partial #776 baseline, not its acceptance gate.

## Preserve the helper boundary

`neat-core/tests/golden/compact_legacy_helpers.json` records the raw `input`,
the helper's actual `before` input, its returned `result`, and mutated `after`.
The capture does **not** call `Creature.fromJSON`, compile the graph, run
activation or repair it into a valid whole creature. That would change what
these regressions test: some inputs deliberately contain duplicate edges,
mixed UUID/integer references, or an IF with only two roles.

The backward-edge tests explicitly call `normaliseCreatureExport` first.
Their records therefore name that preparation and distinguish raw `input`
from prepared `before`. The other helpers receive the original raw input;
any normalization visible in their `after` is performed by the helper itself.

| Capture | Original source in NEAT-AI `test/compact` | Frozen observable contract |
|---|---|---|
| `typed_zero` | `ZeroWeightSynapsePruningTyped.ts` | Remove one of seven edges. Keep the last zero-weight output input, all three typed IF inputs and the untyped zero edge to IF. Remove memetic data. |
| `duplicate_sum` | `MergeDuplicateSynapses.ts`, first test | Merge three of five edges. Output weight is `0.5 - 0.2 = 0.3`; the pointwise hidden target receives `0.1 + 0.2 + 0.3 = 0.6` with the first `condition` role and both tags. Remove memetic data. |
| `distinct_if_roles` | `MergeDuplicateSynapses.ts`, second test | Merge count is zero: condition and positive edges to IF remain separate even though source and target match. Do not invent the missing negative role. |
| `backward_repair` | `RemoveBackwardSynapses.ts`, first test | Remove only `hidden-1 -> hidden-0`; retain the three forward edges and neuron order. This is repair, not recurrent equivalence. |
| `forward_noop` | `RemoveBackwardSynapses.ts`, second test | Return zero removals and leave the prepared input unchanged. |
| `duplicate_tags` | `MergeDuplicateSynapsesTagDedup.ts` | Sum `0.25 + 0.75 = 1` and retain one tag for two distinct objects with equal `{name,value}`. Preserve the IDs added by the merge helper itself. |
| `memetic_count` | `PruneOrphanMemeticReferences.ts`, exact-count test, dirty input | Return four: one stale bias, one absent-edge row, one entire absent-source entry (despite its two rows), and one stale ancestry row. Preserve valid metadata and topology. |
| `memetic_clean` | Same test, clean input | Return zero and preserve the complete input, including valid bias and weight metadata. |
| `memetic_array_weights` | Same file, array-shaped weights test | Return one for the stale bias. Preserve all three wire-array slots unchanged; their indices are not runtime neuron IDs. |
| `memetic_absent` | Same file, no-memetic test | Return zero and preserve the absence of memetic data. |
| `memetic_no_weights` | Same file, no-weights snapshot test | Return two for stale root/ancestry biases. Preserve valid biases and do not introduce weights into either snapshot. |

The two duplicate tests intentionally distinguish pointwise targets from IF
targets. A universal endpoint-only key or a universal role-aware key would
lose one of these legacy policies. Capture the distinction before designing
shared primitives or choosing explicit options.

The five memetic records invoke `pruneOrphanMemeticReferences` on its original
`MemeticPruneTarget` objects: neuron IDs plus synapse indices, not creature
exports. Their numeric result counts metadata removals, not graph removals.
They do not exercise `pruneDanglingMemeticFromExport`, whose UUID wire boundary
is a separate contract. Neither `Creature.fromJSON` nor `fix()` is called for
these records. The tag record likewise retains two distinct equal-valued tag
objects until the original merge helper runs.

## Integrity checks and limits

`neat-core/tests/compact_legacy_helpers.rs` checks the recorded result counts,
surviving endpoints, roles, weights, tag union, memetic invalidation,
preparation boundary and no-op equality. Its `1e-12` weight bound checks the
simple recorded arithmetic; the original TS assertions also run unchanged.
The tag-deduplication case checks the exact sum of one. Memetic cases compare
the entire result against the original input with only the explicitly stale
entries removed; clean/no-memetic cases require whole-object equality.
The fixture test first failed because the helper capture file was absent.
Each case independently rejects a deliberately corrupted record: lost typed
zero edge, wrong duplicate sum, swapped IF role, restored backward edge, or
changed no-op weight. All five mutations were reverted.
The expanded required-case check first failed before the six records were added.
Seven further corruptions were caught and reverted: duplicated equal tag,
wrong removal count, restored stale ancestry row, changed valid weight, lost
wire-array slot, invented memetic object, and invented weights field. These
are fixture mutations, not evidence of native compaction implementation parity.

These are **capture integrity checks**, not native helper parity: no new Rust
rewriter is being tested here. Activation/score claims would be inappropriate
for the raw invalid/intermediate graphs. Future native helper replay must
consume the same boundary inputs and compare actual outcomes to these records;
passing the current fixture checks cannot discharge that migration gate.

## Regenerate

The existing capture command shares its exact-revision and clean-checkout
guards with a new optional `helpers` mode:

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_helpers.json helpers
cargo test -p neat-core --test compact_legacy_helpers
```

The default `creature` mode retains its previous output. Both modes were
regenerated byte-for-byte during validation. `scripts/legacy_compaction_helpers.ts`
contains the original input literals and invokes the legacy functions directly;
it is an oracle capture utility, not a production implementation or fallback.
Production code and dependency pins remain unchanged. The remaining original
helper cases, trace/metadata fixtures, family matrix and performance/scorer
baselines remain open under #776.
