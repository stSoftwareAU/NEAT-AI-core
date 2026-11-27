# Unused-choice baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_unused.json` freezes what `compactUnused`
chooses at NEAT-AI revision `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`: the
neuron it removes, the removal branch, and every RNG draw it makes. It holds 9
cases, taken from `test/compact/NewbornGrace.ts`, `UnusedClipped.ts`,
`Cascade.ts` and `CompactUnusedOrphanCleanup.ts`.

`scripts/legacy_compaction_unused.ts` calls the frozen `compactUnused` and does
not copy the algorithm. The TypeScript stays in NEAT-AI, and no caller is
switched by this change.

## What is captured

Per case: `traced` (the trace passed in), `aligned` (the neurons and synapses
`compactUnused` iterates), `runtimeGraphMatchesAligned` (with `runtimeGraph`
recorded only when the order differs), `graceRemaining`, `plankConstant`,
`shuffled` (after `CreatureUtil.shuffle`), `removalDraws` (draws made after the
shuffle returned), `minted` (`crypto.randomUUID` calls), `result`, and
`chosenUUID` (read from the `unused` tag, not computed).

## Deviations

- Data for the three data-driven test files comes from `createSeededRng(seed)`
  instead of `Math.random()`. Seeds: 778011 to 778014.
- The global RNG that the recorder wraps is seeded in every case, so the shuffle
  is reproducible too. The original tests leave it unseeded.
- Each data case records the first attempt that compacts. The original
  orphan-cleanup test runs all 20 attempts, so the capture covers only the first.

Nothing else differs from the frozen tests.

## Captured cases

| Case | Seed | Chosen | Removal branch | Draws |
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

- **Branch.** All nine removals took the bias-fold branch. Every outward target
  is IDENTITY, which has no `propagate`, and `minted` is 0 in every case, so no
  constant neuron was created. The constant branch is not exercised here.
- **Draws.** Each data case draws once, for the target with a single inward
  connection. The draw picks the zero-weight reconnect source as
  `floor(draw * input)`: `input-2` (unused_clipped), `input-1` (cascade),
  `input-0` (both orphan cases). The NewbornGrace cases draw nothing, because
  every target keeps two or more inward connections.
- **Reconnect kept.** The drawn weight-0 synapse stays in `result.after`, for
  example `input-2 -> output-0` with weight 0 in `unused_clipped`.
- **Bias fold.** In `newborn_grace_1`, `output-0` bias 0 becomes 0.45
  (0.9 × 0.5). In `unused_clipped`, `output-0` bias 1 becomes 1.6 and
  `output-1` bias 0 becomes 0.7.
- **Grace.** The `newborn_grace_2` survivor keeps `newborn-grace` = "1". The
  `newborn_grace_1` survivor has its tag removed (`tags` is `[]`).
- **Orphans.** Removing `hidden-B` also drops `hidden-A`, which is left with no
  outward connection, so the result keeps only `hidden-C` and `output-0`.
- **Order.** `runtimeGraphMatchesAligned` is true in all nine cases, so
  `runtimeGraph` is null throughout.
- **Attempts.** All four data cases compact on attempt 1.
- **Squash.** The CLIPPED fixture neurons are recorded as `HARD_TANH` in both
  `traced` and `aligned`.

## Reproduction

```sh
deno run -A --config /tmp/ts778c/neat/deno.json \
  scripts/capture_legacy_compaction.ts /tmp/ts778c/neat \
  neat-core/tests/golden/compact_legacy_unused.json unused < /dev/null
```
