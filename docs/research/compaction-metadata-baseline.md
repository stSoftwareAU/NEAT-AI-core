# Creature metadata repair captures (Issue #776)

Two records in `neat-core/tests/golden/compact_legacy_metadata.json` capture
the first two original tests in
[`PruneOrphanMemeticReferences.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/PruneOrphanMemeticReferences.ts).
All six tests in that file passed unchanged. The remaining four tests already
have five [raw helper records](compaction-helper-baseline.md), because the
exact-count test supplies both dirty and clean inputs. Together the two modes
cover this file's six tests; they do not complete #776 or its metadata coverage.

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

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_metadata.json metadata
cargo test -p neat-core --test compact_legacy_metadata
```

The shared source guard requires clean tracked sources at the frozen revision.
To reproduce the original tests from that checkout, run
`deno test -A test/compact/PruneOrphanMemeticReferences.ts`. DEBUG validation
may write its normal diagnostic file; this is not a fixture or private input.
Other metadata, UUID, ordering, trace and repair contracts remain outstanding
in the [overall baseline](compaction-baseline.md).
