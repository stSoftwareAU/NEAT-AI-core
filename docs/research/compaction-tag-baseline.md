# Tag-preservation captures (Issue #776)

Eight records in `neat-core/tests/golden/compact_legacy_tags.json` capture all
eight original tests in
[`CompactTagPreservation.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactTagPreservation.ts).
All originals passed unchanged. The recorder preserves each test name, raw
input, preparation, operation, result and resulting graph. This completes
capture of this file, not #776 or the wider metadata baseline.

| Capture | Original operation | Observed tag contract |
|---|---|---|
| `complement_new` | Whole-creature compaction | The new direct synapse carries both `source=training-data` and `layer=output-link`. |
| `complement_existing` | Whole-creature compaction | The existing direct synapse retains both `path=direct` and `path=complement`; equal names with different values are distinct tags. |
| `identity_chain` | Whole-creature compaction | The replacement `hidden-0` to output edge retains `chain=bridge` and `layer=to-output`. |
| `orphan_constant` | Raw orphan cleanup | One hidden neuron becomes a constant, preserving `orphan-0`, bias `0.5` and `purpose=bias-source`. No neuron is removed. |
| `bridge_identity_neurons` | Raw parallel bridge merge | The kept bridge carries both `role=primary` and `role=secondary`. |
| `bridge_complement_neurons` | Raw parallel bridge merge | The kept bridge carries both `variant=alpha` and `variant=beta` while becoming IDENTITY. |
| `bridge_synapses` | Raw parallel bridge merge | The two inbound edges retain their respective `channel=A` and `channel=B` tags after redirection. |
| `complement_output` | Whole-creature compaction, then export normalization | The surviving output keeps `role=final-output` and resolves to output ID `-1`. |

## Original boundaries

Four pipeline inputs are loaded with `Creature.fromJSON` and passed to
`compactCreature(creature, false)`. `result: "candidate"` records that the
returned creature exists; `after` contains its wire export. Only the final
test explicitly calls `normaliseCreatureExport` on that export, matching its
original `exportNormalised` helper. The other four inputs go directly to
`cleanupOrphanedNeurons` or `mergeParallelBridges`, without a loader or an
extra normalization pass. Their results retain the helpers' actual counts.

The raw helper calls assign runtime IDs themselves. Input is cloned first so
the original literals remain intact; `before` records the actual operation
input. The orphan conversion preserves the existing UUID rather than creating
a random constant identity. Bridge cases keep `bridge-B`, the later neuron,
and update both UUID and integer endpoint handles.

The record includes the complete returned graph, including pipeline-level
`approach` and `old-neurons` tags. The integrity test checks the original
neuron/synapse tag contracts and their surrounding graph arithmetic. It does
not make those additional pipeline tags a new acceptance requirement.

## Integrity checks and limits

The Rust integrity test checks complete neuron and synapse records after
excluding added integer handles; bridge handles and the final output ID have
separate assertions. It compares tag values with multiplicity preserved,
without imposing tag-array or graph-array order that these originals do not
assert. The separate [ordering baseline](compaction-bridge-baseline.md) checks
activation order where it is part of the original contract.

For the small literal graphs, expected arithmetic is derived independently:
COMPLEMENT bypass gives bias `0.8` and weight `-0.4`, or
`0.3 - 0.8 * 0.5 = -0.10000000000000003` when a direct edge already exists.
The two COMPLEMENT bridges give bias `2` and input weights `-0.5` and `-0.3`.
The test checks these frozen values alongside the tags so a preserved tag
cannot conceal a broken graph. No activation or measured-fitness assertion
is added to the original tag tests, and no numerical tolerance is widened.
This is capture integrity, **not native rewrite or activation parity**.

The new test failed before fixture generation, then passed. All eight raw
inputs and test names were independently compared with the original source.
Deliberate loss or corruption of tags in every record was rejected and
restored, together with changed COMPLEMENT arithmetic, stale redirected
integer handles, a duplicated tag and a replaced orphan UUID. All six capture modes regenerate
byte-for-byte; the earlier five modes are unchanged.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_tags.json tags
cargo test -p neat-core --test compact_legacy_tags
```

The source guard requires clean tracked sources at the frozen revision. Run
the original file from that checkout with
`deno test -A test/compact/CompactTagPreservation.ts`.
Other metadata, trace and family acceptance gaps remain listed in the
[overall baseline](compaction-baseline.md).
