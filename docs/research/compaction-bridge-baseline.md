# Parallel bridge ordering and refusal captures (Issue #776)

Four records in `neat-core/tests/golden/compact_legacy_bridges.json` preserve
the first four original tests in
[`ParallelBridgeMergeOrdering.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ParallelBridgeMergeOrdering.ts).
All five tests in that file passed unchanged, including its public forest
regression. The large forest fixture's constant-merge preparation and bridge
call are **not captured here**. This is another partial baseline for #776;
no production implementation or consumer dependency changes.

| Capture | Original boundary | Recorded outcome |
|---|---|---|
| `late_source` | Load the original `lateSourceBridges()` graph and call the helper on `exportJSONUnchecked` | Remove `bridge-early`, retain `source-late` before `bridge-late`, and keep all four remaining edges forward. Reload strips no edge. Original 25 sine-derived float32 rows differ by at most `1e-6`. |
| `backward_refusal` | Pass the raw recurrent literal directly to the helper | Remove no neuron; retain the backward `bridge-b` to `target` edge and all other topology. No loader or activation is added to this original test. |
| `maximum_refusal` | Pass original `bridgesIntoAggregate("MAXIMUM", false, 6)` directly | Remove no neuron; preserve the six independent bridge contributions and MAXIMUM target. No loader or activation is added. |
| `if_refusal` | Load original `bridgesIntoAggregate("IF", true, 12)` and export unchecked | Decline every role, retaining four condition, four positive and four negative edges. Reload strips no edge. All 25 original dyadic rows have exactly equal outputs. |

## Preparation and numerical boundaries

The recorder copies the original constructors and row builders, and calls the
frozen `mergeParallelBridges` implementation. It does not reproduce the merge
algorithm. The raw recurrent and MAXIMUM inputs bypass the loader. The other
two cases retain their original load/export/reload sequence. Raw input is
cloned before loading because the legacy loader assigns IDs into its argument.

`before` is the exact helper input; `after` is captured before reloading can
mutate it. The helper itself adds integer neuron and endpoint IDs, including
on refused calls. Consequently, refusal preserves wire topology and metadata
but is not byte-for-byte object equality. Integrity checks resolve both UUID
and integer endpoint handles, including the redirected edge's target. The
reloaded wire export must match the helper output after removing these IDs.

For the successful merge the original contributions give bias
`1.5 * 0.2 + 2 * 0.1 = 0.5`, redirected input weight `2 * 0.5 = 1`, retained
input weight `1.5 * 0.3`, and output weight `1`. The retained input product is
stored as the actual JavaScript value `0.44999999999999996`. The kept bridge
follows the late source in activation order; choosing the early bridge would
create a backward edge. The test checks the complete expected wire graph as
well as forward ordering, so sampled activation agreement cannot hide a
structural change.

The first and fourth records retain all original probe rows and both output
arrays. The original late-source bound is `1e-6`; the original IF bound is
exact equality. Neither is widened. These are frozen observations and capture
integrity checks, **not native bridge rewrite or activation parity**. The raw
recurrent case makes no claim about recurrent activation equivalence.

`merge_parallel_bridges` (Issue #777) now replays the `mergeParallelBridges`
helper captures natively, in `neat-core/tests/compact_parallel_bridge.rs`; the
`compactCreature` whole-pipeline captures stay TypeScript-only.

## Verification and reproduction

The integrity test failed before the new fixture existed. All four raw inputs
and both 25-row corpora were independently compared with the original test's
declarations. All existing capture modes remain byte-identical. Deliberate
corruptions of retained order, merge weights, backward topology, refusal
count, IF roles, reloaded weights, integer handles and recorded outputs were
rejected individually and restored.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_bridges.json bridges
cargo test -p neat-core --test compact_legacy_bridges
```

The shared source guard requires clean tracked sources at the frozen revision.
Run the unchanged original suite from that checkout with
`deno test -A test/compact/ParallelBridgeMergeOrdering.ts`.
The small COMPLEMENT, refusal and corner-case contracts are recorded separately
in the [bridge case captures](compaction-bridge-cases-baseline.md). The public
forest regressions are summarised in the
[forest baseline](compaction-forest-baseline.md). Remaining family acceptance
work stays open in the [overall baseline](compaction-baseline.md).
