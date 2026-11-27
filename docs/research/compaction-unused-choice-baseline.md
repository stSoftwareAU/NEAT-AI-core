# Unused-choice baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_unused.json` freezes what `compactUnused`
chooses at NEAT-AI revision `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`: the
removed neuron, the removal branch and every RNG draw. It holds 9 cases from
`test/compact/`. `scripts/legacy_compaction_unused.ts` calls the frozen
`compactUnused` and does not copy the algorithm. No caller is switched.

## What is captured

Per case: `traced`, `aligned` (what `compactUnused` iterates), `graceRemaining`,
`plankConstant`, `shuffled`, `removalDraws` (draws after the shuffle), `minted`,
`result`, and `chosenUUID` (read from the `unused` tag). `runtimeGraph` is
recorded only when its order differs from `aligned`.

## Deviations

- Data comes from `createSeededRng(seed)`, not `Math.random()` (seeds 778011 to 778014).
- The recorded global RNG is seeded in every case, so the shuffle is reproducible
  too; the originals leave it unseeded.
- Each data case records its first compacting attempt; all four compact on
  attempt 1. The orphan-cleanup test runs all 20 attempts, the capture stops at one.

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

## Findings

- **Branch.** Every outward target is IDENTITY, with no `propagate`, and `minted`
  is 0 in all nine cases. The constant branch is not exercised.
- **Draws.** Each data case draws once, for the target with one inward edge. The
  draw picks the zero-weight reconnect source as `floor(draw * input)`:
  `input-2`, `input-1`, `input-0`, `input-0`. The NewbornGrace cases draw nothing.
- **Reconnect kept.** The weight-0 synapse stays in `result.after`, e.g.
  `input-2 -> output-0` in `unused_clipped`.
- **Bias fold.** `newborn_grace_1`: `output-0` bias 0 becomes 0.45 (0.9 × 0.5).
  `unused_clipped`: `output-0` 1 becomes 1.6, `output-1` 0 becomes 0.7.
- **Grace.** The `newborn_grace_2` survivor keeps `newborn-grace` = "1"; the
  `newborn_grace_1` survivor's tag is removed.
- **Orphans.** Removing `hidden-B` also drops `hidden-A`, left with no outward edge.
- **Order and squash.** `runtimeGraphMatchesAligned` is true in all nine. CLIPPED is recorded as `HARD_TANH`.

## Reproduction

```sh
deno run -A --config /tmp/ts778c/neat/deno.json \
  scripts/capture_legacy_compaction.ts /tmp/ts778c/neat \
  neat-core/tests/golden/compact_legacy_unused.json unused < /dev/null
```
