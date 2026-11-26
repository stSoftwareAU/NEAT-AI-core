# Legacy compaction variant and selection captures (Issue #776)

Nine records in `neat-core/tests/golden/compact_legacy_variants.json` cover all
eight original tests in
[`test/compact/CompactCreatureVariants.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/CompactCreatureVariants.ts).
All eight tests passed unchanged at that frozen revision. This supplements the
[whole-creature baseline](compaction-baseline.md); #776 remains incomplete.

## Preserve the variant boundary

`scripts/legacy_compaction_variants.ts` calls the frozen
`compactCreatureVariants`, `compactCreature` and `selectCompactVariant`
implementations. It does not duplicate their algorithms. The first three
original tests share the published `constant-fold-full-removal.json` fixture;
one capture records their candidate existence, wrapper identity and UUID
deduplication outcomes. The fourth test supplies a no-op input. The final four
tests supply seven separate selection calls, including three fallback calls
and separate lower-score and tied-score calls.

Raw `input` is retained before `Creature.fromJSON(input, false)` adds canonical
IDs. Pipeline records also retain the prepared `before` graph. Candidate
snapshots record the exported `graph`, computed `uuid`, and runtime `score`
separately: an absent score is recorded as `null`. Selection records additionally
retain each candidate's original input. The selected role comes from identity
comparison with the actual object returned by the legacy selector; it is not
inferred from scores or a reimplemented selection rule. UUID snapshots are
computed after selection and may fill the candidate's cached identity.

| Record | Original test(s) | Recorded contract |
|---|---|---|
| `fold_variants` | Candidate existence; wrapper compatibility; deduplication | Safe and aggressive both exist and share a UUID. The wrapper returns the same genome as safe. There is one unique candidate and selection retains safe. |
| `noop_variants` | Empty result | Bare IDENTITY input/output graph with bias `0.1` produces neither variant. |
| `select_safe_only` | Present-variant fallback, first call | Return the sole safe candidate even though its score is absent. |
| `select_aggressive_only` | Present-variant fallback, second call | Return the sole aggressive candidate even though its score is absent. |
| `select_empty` | Present-variant fallback, third call | Return no candidate. |
| `select_duplicate` | Identical distinct instances | Bias `0.1` candidates have the same genome and separate object identities. Safe score `5` wins even against duplicate score `999`. |
| `select_higher` | Strict improvement | Distinct aggressive bias `0.2`, score `6` replaces safe bias `0.1`, score `5`. |
| `select_lower` | No improvement, first call | Safe score `6` beats distinct aggressive score `5`. |
| `select_tie` | No improvement, second call | Two distinct candidates with score `5` retain safe. |

These are the original fixture and constructor arguments, without additional
synthetic policy cases. The original file's comment describing an aggressive
no-op applies to this fixture; it does not establish that aggressive compaction
is universally a no-op. The selector's finite-score guard needs additional
coverage beyond these eight tests. Scores here are the original tests' assigned
values, not measured fitness or new scorer acceptance thresholds.

## Integrity and reproduction

`neat-core/tests/compact_legacy_variants.rs` checks required records, selected
roles, original tiny graph/score inputs, distinct versus equal UUIDs, wrapper
compatibility and the fold fixture's constant removal. The folded bias check
uses `0.2 + 0.5 * 0.5 = 0.45` and the original constant-fold test's `1e-9` bound.
This is capture integrity, not a native compactor or selector parity test. It
does not establish activation equivalence, finite-score policy coverage or
consumer acceptance.

The test failed before the fixture existed. Nine independent record corruptions
were caught and restored: missing aggressive fold candidate, invented no-op
candidate, and an incorrect selected role in each of the seven selection calls.
Separate wrapper-identity and duplicate-instance corruptions were also caught
and restored. All three capture modes regenerate byte-for-byte; this mode
changes neither existing whole-creature nor raw-helper captures.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_variants.json variants
cargo test -p neat-core --test compact_legacy_variants
```

The shared source guard requires the exact frozen revision and clean tracked
sources. The original test can be rerun from that checkout with
`deno test -A test/compact/CompactCreatureVariants.ts`.
