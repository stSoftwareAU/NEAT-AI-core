# Creature metadata repair and finite-guard captures (Issue #776)

The first two of eight records in `neat-core/tests/golden/compact_legacy_metadata.json` capture
the first two original tests in
[`PruneOrphanMemeticReferences.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/PruneOrphanMemeticReferences.ts).
All six tests in that file passed unchanged. The remaining four tests already
have five [raw helper records](compaction-helper-baseline.md), because the
exact-count test supplies both dirty and clean inputs. Together the two modes
cover this file's six tests; they do not complete #776 or its metadata coverage.
Four additional records capture every original test in
[`CompactUnusedFiniteGuard.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactUnusedFiniteGuard.ts).
All ten original tests across the two files passed unchanged.

## Runtime preparation matters

Both original tests load the same two-input graph with
`Creature.fromJSON(json, true)`, then attach stale memetic data. Attaching that
data before loading would let load-time cleanup remove the failure being
tested. The capture preserves the raw graph before the loader adds IDs and
records the actual runtime hidden-neuron ID, `1775329602`, derived by the
legacy loader. Synapse endpoints in the runtime snapshots are neuron indices,
not those IDs. Memetic bias keys are IDs; valid exported keys are UUID labels.

The recorder takes an explicit runtime projection of neurons, synapses,
memetic data and the forward-only flag. It does not export the invalid creature
first: `exportJSON()` validates DEBUG creatures and converts memetic references
to wire labels. Input-neuron bias sentinels are explicitly encoded as
`"Infinity"`; a normal JSON encoding would silently turn them into `null`.
No private fields or executable methods are serialized.

| Capture | Original operation | Frozen observable outcome |
|---|---|---|
| `creature_memetic_prune` | `pruneOrphanMemeticReferences(creature)` | Validation first fails with `ValidationError` naming absent ID `1998066541`. Pruning returns one removal, retains the valid hidden bias `0.1`, and validation succeeds. The valid wire export maps that bias to `hidden-a` and represents empty weights as `[]`. |
| `creature_memetic_fix` | `fix(creature, {forwardOnly: true})` | The original test only validates after repair, so `beforeValidation` is `null`. `fix` returns `undefined` (recorded as `null`), removes the sole stale bias, retains empty memetic metadata, and sets `forwardOnly: true`. Validation with that option succeeds. |

Both captured cases preserve all graph neurons and synapses. That is an
observation about these inputs, not a claim that `fix()` generally preserves
topology, metadata or fitness. The recorder invokes the original functions;
it does not duplicate the repair algorithm or use a whole-creature compactor.

## Non-constant removal and finite guards

The four `removeNeuron` records retain original raw `input`, a `loaded`
runtime projection, and the actual `before` projection after any injections.
The overflow tests inject extreme weights and biases **after** loading, just
as the original tests do, to bypass load-time clamping. Runtime neuron IDs
identify the hidden neuron; synapse endpoints are indices. The NaN activation
argument is encoded explicitly as `"NaN"`. Finite `Number.MAX_VALUE` values
remain numbers, with the repository's round-trip float parser preserving them.

| Capture | Original call and observable contract |
|---|---|
| `finite_normal` | Activation `0.5` removes the IDENTITY hidden neuron and changes output bias to `0.5 + 0.3 * 0.5 = 0.65`, within the original `1e-12` bound. |
| `finite_overflow` | Output bias `MAX_VALUE / 2`, outward weight `MAX_VALUE`, activation `1`: the addition would overflow, so return false without changing the captured topology or biases. |
| `finite_nan` | Activation NaN with outward weight `0.5`: return false and preserve the runtime projection. |
| `finite_atomic` | Two outputs have outward weights `0.5` and `MAX_VALUE`, with activation `MAX_VALUE`. The second product overflows; reject without changing either output bias, including the first finite adjustment. |

Successful removal adds a temporary zero-weight edge from a randomly chosen
input to keep the output connected. The capture sets the legacy RNG to seed
`1910` immediately before that one call and restores the previous RNG in
`finally`. The original test remains unmodified and does not assert a
particular input source. The integrity test permits either original input;
the stored seeded record chooses input index `1`. Seeding is a reproducibility
choice for the recorder, not a new deterministic-selection contract.

No activation-equivalence claim is made: the supplied activation is an
argument to trace-based bias adjustment, not an algebraic identity for every
input. Refusal equality covers the recorded runtime projection only.
`removeNeuron` deletes cached UUID state on entry, including failed calls;
these original fixtures do not exercise a pre-existing cached UUID. Do not
describe false as a universal whole-object no-op. Extreme graphs are not
reloaded, exported, or repaired after injection.

## Constant-branch targets

Six further `removeNeuronConstantBranch` records run the frozen `removeNeuron`
against an output whose squash is `IF`, `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT`
or `HYPOTv2`: every squash that defines `propagate` at the frozen revision. Each
record stores `definesPropagate` (read from `findSquash()`), the successful
result, a `constantNeuronAdded` flag and the target's bias, which stays at its
original `0.25`: TypeScript wires in a unity constant instead of folding into
the bias. The constant neuron's uuid is random, so no other state is kept.
`neat-core/tests/compact_remove_neuron.rs` asserts that `remove_neuron_by_bias_fold`
refuses all six with `ConstantBranchNotPorted`. The constant branch itself is
ported in `crate::compact_unity_constant::remove_neuron` (Issue #778).

## Converted constants retain their UUIDs

Two additional records capture both original
[`ConvertedConstantKeepsUuid_test.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ConvertedConstantKeepsUuid_test.ts)
tests for NEAT-AI #4071. Each runs with the original seed `4071`, restoring the
previous RNG in `finally`. Inputs are cloned before any loader or helper can
add identity fields.

- `converted_uuid_roundtrip` loads the original graph with DEBUG false, then
  calls `cleanupOrphanedNeuronsInCreature`. Hidden `U` (input ID `1777777`)
  becomes a constant with bias `0.3`; the existing constant
  `legacy-neuron-1777777` stays distinct with bias `0.5`. Export and DEBUG-true
  reload retain both UUIDs and their separate outward edges. Validation succeeds.
  The intermediate export is recorded without validating or repairing it first.
- `converted_uuid_raw` passes the original raw export directly to
  `cleanupOrphanedNeurons`. Hidden `hidden-no-inward` retains its UUID, becomes a
  constant and receives the f64 LOGISTIC value `1 / (1 + exp(-0.5))`. The output
  and both synapses remain. The recorder does not add a loader or validation
  step to this original raw-helper case.

Both results are `{ removed: 0, converted: 1 }`. Complete before/after wire
semantics are checked, including all neuron and edge records. Comparisons
ignore added numeric handles and array ordering; those are not new ordering
contracts. The raw graph's added handles must still agree with its UUID
endpoints, and reloaded runtime IDs must be unique. The converted constant's
wire UUID, not its original numeric ID, is the regression contract: wire export
omits numeric IDs, and reload derives `U`'s ID as `1000085`. Neither the supplied
`1777777` nor that derived value is imposed on a future native implementation.

The fixture test failed before the records were added. Both unchanged original
tests passed, both raw inputs and test names match the source declarations, and
the earlier six metadata records are unchanged. Ten independent corruptions
were rejected and restored: converted/reloaded UUID collisions, a missing
outward connection, wrong conversion count, failed validation, duplicate runtime
ID, lost raw UUID, untransformed LOGISTIC bias, wrong raw endpoint handle and a
lost reloaded edge. These are capture-integrity checks, not native cleanup or
activation parity. No activation samples or fitness policy were added.

## Integrity checks and reproduction

`neat-core/tests/compact_legacy_metadata.rs` checks the validation outcomes,
removal/void results, stale-key removal, valid-bias retention, unchanged runtime
topology, forward-only repair and the complete valid wire export. Expected
runtime memetic data is the original object with only the stale key removed;
expected wire data uses the original graph and the explicitly converted
memetic fields. These are capture integrity checks, not native repair,
validation or activation parity.

The fixture test failed before capture generation. Raw graphs, runtime hidden
IDs and attached memetic objects were independently compared against both
original declarations. Deliberate corruptions of validation results, removal
count, stale/valid bias entries, topology, forward-only state, sentinel encoding
and wire metadata were rejected and restored. Both existing modes and the new
metadata mode regenerate byte-for-byte; existing records are unchanged.
The finite-guard tests also failed before adding their four records. Their
raw inputs match the original source literals and the earlier two records
remain unchanged. Seven additional corruptions were rejected and restored:
wrong successful bias, false success on overflow, NaN encoded as null, a
partial change to either output bias, a clamped extreme weight, and a nonzero
replacement edge. All three capture modes regenerate byte-for-byte.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_metadata.json metadata
cargo test -p neat-core --test compact_legacy_metadata --test compact_legacy_converted_uuid
```

The shared source guard requires clean tracked sources at the frozen revision.
To reproduce the original tests from that checkout, run
`deno test -A test/compact/PruneOrphanMemeticReferences.ts test/compact/CompactUnusedFiniteGuard.ts test/compact/ConvertedConstantKeepsUuid_test.ts`. DEBUG validation
may write its normal diagnostic file; this is not a fixture or private input.
Other metadata, cached UUID invalidation, ordering, trace and repair contracts remain outstanding
in the [overall baseline](compaction-baseline.md).
