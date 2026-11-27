# Unused-choice baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_unused.json` freezes what `compactUnused`
chooses at NEAT-AI revision `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`: the
removed neuron, the removal branch and every RNG draw. It holds 14 cases from
`test/compact/`. `scripts/legacy_compaction_unused.ts` calls the frozen
`compactUnused` and does not copy the algorithm. No caller is switched.

## What is captured

Per case: `traced`, `aligned` (what `compactUnused` iterates), `graceRemaining`,
`plankConstant`, `shuffled`, `removalDraws` (draws after the shuffle), `minted`,
`result`, and `chosenUUID` (read from the `unused` tag). `runtimeGraph` is
recorded only when its order differs from `aligned`.

## Deviations

- Data comes from `createSeededRng(seed)`, not `Math.random()`. Seeds 778011 to
  778014 are the original cases; 778021, 778031 and 778041 are the new ones.
- The global RNG is seeded in every case too, so the shuffle is reproducible.
- Every case stops at attempt 1. The orphan-cleanup test runs all 20 attempts;
  the capture stops at one.
- The second call of `CompactConstant` and `CompactKeepOrder` runs on the
  creature the test builds from its first call. That call runs unrecorded under
  the same seed and pins its constant as `first-constant-N`, so it cannot clash
  with the recorded `unity-constant-N`.

## Captured cases

| Case | Seed | Chosen | Branch | Draws |
| --- | --- | --- | --- | --- |
| `newborn_no_grace` | 778001 | `hidden-quiet` | bias fold | 0 |
| `newborn_grace_1` | 778002 | `hidden-loud` | bias fold | 0 |
| `newborn_grace_2` | 778003 | `hidden-loud` | bias fold | 0 |
| `newborn_grace_0_spent` | 778004 | `hidden-quiet` | bias fold | 0 |
| `newborn_nothing_to_compact` | 778005 | none (`result` null) | n/a | 0 |
| `unused_clipped` | 778011 | `hidden-3` | bias fold | 1 |
| `cascade_cosine_clipped` | 778012 | `hidden-4` | bias fold | 1 |
| `orphan_cleanup_feeder` | 778013 | `hidden-A` | bias fold | 1 |
| `orphan_cleanup_cascade` | 778014 | `hidden-B` | bias fold | 1 |
| `fix_if_compact` | 778021 | `hidden-3` | constant (minted 1) | 0 |
| `compact_constant_first` | 778031 | `hidden-3b` | constant (minted 1) | 0 |
| `compact_constant_second` | 778031 | `hidden-3` | constant (minted 2, replays natively) | 0 |
| `compact_keep_order_first` | 778041 | `hidden-3` | constant (minted 1) | 0 |
| `compact_keep_order_second` | 778041 | `hidden-3b` | constant (minted 2, replays natively) | 0 |

## Findings

- **Branch.** The nine original cases mint nothing and take the bias fold. The
  five new cases take the constant branch, minting 1, or 2 on a second call,
  and draw nothing.
- **Draws.** Each original data case draws once, for the target with one inward
  edge. The draw picks the zero-weight reconnect source as `floor(draw * input)`:
  `input-2`, `input-1`, `input-0`, `input-0`. The NewbornGrace cases draw nothing.
- **Reconnect kept.** The weight-0 synapse stays in `result.after`, e.g.
  `input-2 -> output-0` in `unused_clipped`.
- **Bias fold.** `newborn_grace_1`: `output-0` bias 0 becomes 0.45 (0.9 × 0.5).
  `unused_clipped`: `output-0` 1 becomes 1.6, `output-1` 0 becomes 0.7.
- **Grace.** The `newborn_grace_2` survivor keeps `newborn-grace` = "1"; the
  `newborn_grace_1` survivor's tag is removed.
- **Orphans.** Removing `hidden-B` also drops `hidden-A`, left with no outward edge.
- **Slot tags.** Each new constant carries `compact-unity-slot`. On a second
  call the input holds `first-constant-0` at slot `"0"`, so the new constant
  takes `"1"`. `unity-constant-0` is minted there but absent from `result.after`.
- **Replay.** `compact_legacy_unused.rs` replays all fourteen cases bit for bit.
  Both second calls pick the captured neuron, and `remove_neuron` then takes the
  sound reuse of `first-constant-0` (its bias is set to 1, as TypeScript does).
  The port mints the survivor once, where TypeScript minted `unity-constant-0`
  as well and discarded it. The survivor is `unity-constant-1` at slot `"1"`,
  and the replay takes its uuid from the capture.
- **Order and squash.** `runtimeGraphMatchesAligned` is true in all fourteen.
  CLIPPED is recorded as `HARD_TANH`.

## Reproduction

`<checkout>` is a clean NEAT-AI checkout at that revision.

```sh
deno run -A --config <checkout>/deno.json \
  scripts/capture_legacy_compaction.ts <checkout> \
  neat-core/tests/golden/compact_legacy_unused.json unused < /dev/null
```
