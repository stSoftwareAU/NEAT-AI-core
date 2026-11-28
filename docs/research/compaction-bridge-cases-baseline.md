# Parallel bridge case captures (Issue #776)

Twenty-five records in
`neat-core/tests/golden/compact_legacy_bridge_cases.json` preserve the small
original tests in
[`ParallelBridgeMerge.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMerge.ts),
[`ParallelIdentityBridgeUnification.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelIdentityBridgeUnification.ts),
[`ParallelMergeCornerCases.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelMergeCornerCases.ts)
and the four deterministic tests in
[`ParallelBridgeMergeExactness.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMergeExactness.ts).
The first corner-case test supplies two helper calls, so twenty-four tests
become twenty-five records. This extends the
[ordering captures](compaction-bridge-baseline.md). It does not complete #776.

The public forest regression in the exactness file is summarised in the
[forest baseline](compaction-forest-baseline.md), not in this file. Its
creature is about 3.4 MB, and its activation sample draws unseeded
`Math.random`. The "loads last" test's fifty-row sample is unseeded for the
same reason. Both activation checks stay in the original suite. This capture
freezes the "loads last" graph, including its forward reload, without inventing
a probe corpus.

## What each record keeps

`scripts/legacy_compaction_bridge_cases.ts` copies the original literals and
calls the frozen `mergeParallelBridges` or `compactCreature`. It does not
reimplement either algorithm. Raw helper calls use preparation `none`. The
"loads last" record follows the original `Creature.fromJSON` then
`exportJSONUnchecked` sequence and reloads with the original `"issue-3809"`
label. The two whole-creature records call `compactCreature(creature, false)`
and `validate()` on the candidate.

| Record | Original contract frozen here |
|---|---|
| `complement_pair` | Two zero-bias COMPLEMENT bridges collapse to one IDENTITY neuron with bias `3.5`. |
| `complement_weights` | Bias `3`, inbound weights `-1` and `-0.45`, outbound weight `1`, within `1e-12`. |
| `mixed_squash` | IDENTITY and COMPLEMENT are not one merge group. Remove nothing. |
| `logistic_refusal`, `tanh_refusal`, `relu_refusal`, `absolute_refusal`, `maximum_neuron_refusal` | Those squashes are not merged. The helper returns the same object. |
| `complement_three` | Three COMPLEMENT bridges collapse to one IDENTITY neuron with bias `2.13`. |
| `identity_still_merges`, `identity_integrity`, `post_prune_shape` | IDENTITY bridges still collapse to one neuron. The biased cases keep bias `0.5`. |
| `uuid_only` | A UUID-only export merges without a prior id pass. Bias `0.5`, weights `1` and `0.45`. |
| `typed_condition`, `typed_positive`, `complement_typed` | An extra typed inward edge keeps both neurons. |
| `kept_source` | The non-bridge source id `5002` survives, including its direct output edge of weight `0.1`. |
| `duplicate_sources` | Shared inbound sources are not redirected into a duplicate edge. |
| `if_target_declined` | An IF target keeps both bridges and both condition edges, plus the positive and negative arms. Added integer ids are not a topology change. |
| `maximum_target_declined` | A MAXIMUM target is not pre-summed. |
| `backward_source_left` | Remove one neuron. `later` still feeds `back-fed`. The kept bridge bias is `-0.05`. |
| `keep_last_bridge` | Keep `bridge-late`, redirect `input-0` onto it, and reload the same forward graph. Bias `-0.25`. |
| `identity_compact` | Whole-creature compaction leaves one IDENTITY bridge. The original three samples stay within `1e-6`. |
| `complement_compact` | Whole-creature compaction leaves one hidden neuron. The pipeline folds `h0` into output bias `0.9` and weight `-0.4`, and leaves `h1` as COMPLEMENT. The original assertion is only "fewer than two". |

Refusal records that already carried integer ids compare byte-for-byte. UUID-only
refusals gain ids inside the helper; wire topology, roles and weights match
after those ids are removed. Successful merges are checked against the original
arithmetic, and every recorded synapse endpoint resolves to a neuron or input.
The `0.45` products are stored as the JavaScript value `0.44999999999999996`.
The kept backward-source bias is stored as `-0.04999999999999993`. Neither is
rounded to a new tolerance. The original bound remains `1e-12`.

These are capture-integrity checks. They are not a native bridge implementation
and not activation parity for the unseeded samples.

## Reproduce

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_bridge_cases.json bridge-cases
cargo test -p neat-core --test compact_legacy_bridge_cases
```

The shared source guard requires a clean checkout at the frozen revision. Run
the unchanged originals from that checkout:

```sh
deno test -A \
  test/compact/ParallelBridgeMerge.ts \
  test/compact/ParallelIdentityBridgeUnification.ts \
  test/compact/ParallelMergeCornerCases.ts \
  test/compact/ParallelBridgeMergeExactness.ts
```

`merge_parallel_bridges` (Issue #777) now replays the `mergeParallelBridges`
cases natively, in `neat-core/tests/compact_parallel_bridge.rs`; the
`compactCreature` whole-pipeline captures stay TypeScript-only.

Native replay of the `compactCreature` whole-pipeline captures stays open,
tracked in the [overall baseline](compaction-baseline.md). The forest summary is in the
[forest baseline](compaction-forest-baseline.md).
