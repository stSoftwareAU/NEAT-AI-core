# Unity-constant removal baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_unity.json` freezes the constant
("unity slot") branch of `removeNeuron` from NEAT-AI at revision
`60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`. It holds 7 cases. The branch is
taken when any outward target's squash defines `propagate`: IF, MINIMUM,
MAXIMUM, MEAN, HYPOT and HYPOTv2. The constant's uuid is pinned to
`unity-constant-N` by stubbing `crypto.randomUUID`.

`scripts/legacy_compaction_unity.ts` calls the frozen implementation. It does
not copy the algorithm. The TypeScript stays in NEAT-AI; no caller is switched
by this change. The native port is `neat-core/src/compact_unity_constant.rs`,
added in the same change.

| Case | Meaning | Outcome |
|---|---|---|
| `unity_if` | Hidden IDENTITY neuron with one IF output target | removed |
| `unity_mixed_targets` | IF and IDENTITY targets; the IDENTITY target's bias is unchanged | removed |
| `unity_two_roles` | Two edges from the removed neuron to one IF target, in condition and positive roles | removed |
| `unity_after_existing_constants` | Two untagged constants already present; the new constant lands after them | removed |
| `unity_feeder_cascade` | Hidden feeder upstream of the removed neuron, with a MEAN output target | removed |
| `unity_overflow` | HYPOT target with activation `1e308`, so the rescaled weight `10 * 1e308` is not finite | throws `weight must be a number` |
| `unity_reuse_tagged_slot` | Existing constant tagged `compact-unity-slot` = `"0"` | removed, defect (see b) |

## Findings

**(a) Fresh slot path.** With no constant tagged slot `"0"`, a new constant
neuron is inserted after the inputs and existing constants. It has bias 1, no
squash, and the tag `compact-unity-slot` = `"0"`. Every outward synapse of the
removed neuron is re-sourced from it with weight `w * activation` and the same
role. No target bias changes, and no zero-weight reconnect is added.

**(b) Reuse path defect.** With an existing constant tagged slot `"0"`, the
frozen code returns the constant it has just removed, with a stale index, so
removal rewires through another neuron. `unity_reuse_tagged_slot` ends with both
constants gone and `hidden-0` carrying the condition weight 4.5 (the condition
weight 3 times the activation 1.5). The native port refuses this path.

**(c) Overflow.** TypeScript throws `weight must be a number` after it has
inserted the constant. The `after` block keeps that constant with no outward
edge. The native port refuses before changing anything.

## Reproduction

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_unity.json unity
```
