# Unity-constant removal baseline (Issue #778)

`neat-core/tests/golden/compact_legacy_unity.json` freezes the constant
("unity slot") branch of `removeNeuron` from NEAT-AI at revision
`60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`. It holds 14 cases. The branch is
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
| `unity_reuse_slot_one_held` | Leading constants tagged `"0"` and `"1"`, removed neuron at the insertion point | removed, new constant in slot 2 (see b) |
| `unity_reuse_slot_two_held` | Leading constants tagged `"0"`, `"1"` and `"2"` | returns `false`, holder biases set to 1, defect (see b) |
| `unity_reuse_holder_behind` | Constant tagged `"0"` listed behind the removed neuron | loader moves it to lead; removed, new constant in slot 1 (see b) |
| `unity_reuse_second_slot_zero_holder` | Two leading constants tagged `"0"` | second merged into first; removed, new constant in slot 0 (see b) |
| `unity_reuse_third_slot_zero_holder` | Three leading constants tagged `"0"` | second merged into first, third left; removed, new constant in slot 0 (see b) |
| `unity_reuse_second_slot_one_holder` | One leading constant tagged `"0"`, two tagged `"1"` | second slot-1 holder merged into the first; removed, new constant in slot 1 (see b) |
| `unity_reuse_second_slot_two_holder` | Leading constants tagged `"0"` and `"1"`, two tagged `"2"` | second slot-2 holder merged into the first; removed, new constant in slot 2 (see b) |

## Findings

**(a) Fresh slot path.** With no constant tagged slot `"0"`, a new constant
neuron is inserted after the inputs and existing constants. It has bias 1, no
squash, and the tag `compact-unity-slot` = `"0"`. Every outward synapse of the
removed neuron is re-sourced from it with weight `w * activation` and the same
role. No target bias changes, and no zero-weight reconnect is added.

**(b) Reuse path.** With an existing constant tagged slot `"0"`, the frozen code
returns the constant it has just removed, with a stale index, so removal rewires
through whatever neuron sits at that index. `unity_reuse_tagged_slot` ends with
both constants gone and `hidden-0` carrying the condition weight 4.5 (the
condition weight 3 times the activation 1.5). The reuse is sound only when that
index lands on the removed neuron, which needs the holder to lead the constants
and the removed neuron to sit directly after them. The second call of
`CompactConstant` and `CompactKeepOrder` has that shape. Its replay is captured
as `compact_constant_second` and `compact_keep_order_second` in
`compact_legacy_unused.json`, and the native port takes it: the holder's bias is
set to 1 as TypeScript does, the survivor takes slot 1, and only the survivor is
minted. The defective shape, `unity_reuse_tagged_slot`, stays refused.

A held slot 1 is reused the same way. When a second leading constant holds it,
`createConstantOne(creature, 1)` also returns a stale index naming the removed
neuron, so `removeNeuron` asks for slot 2 and mints a fresh constant there.
`unity_reuse_slot_one_held` captures this: both holders' biases become 1, the
survivor takes slot 2, and TypeScript mints three UUIDs for it. The native port
takes this shape too, and it mints only the survivor. With slot 2 held as well
(`unity_reuse_slot_two_held`) by one constant, every call returns the removed
neuron. The frozen code then treats that neuron as the constant it is already
wired to, and returns `false` after it has set every holder's bias to 1.
`compactUnused` still returns that changed creature. For an untyped edge it would
instead fold the weight into the edge it removes. The port refuses this shape.
With two tagged leading slot-2 holders the count-2 call of
`createConstantOne(creature, 2)` runs the same dedupe merge as slots 0 and 1, and
`removeNeuron` succeeds (`unity_reuse_second_slot_two_holder`). The capture shows
`k2b` merged into `k2` with its synapse re-sourced from `k2`, `k0`, `k1` and `k2`
set to bias 1, and a new constant tagged `"2"` carrying the condition weight 4.5.
TypeScript mints two UUIDs plus one per outward synapse for it, three here
with one outward synapse. The port takes this shape, and mints only the
survivor.

A holder listed behind the removed neuron never reaches `removeNeuron`:
`Creature.fromJSON` puts the constants first, so `unity_reuse_holder_behind`
loads with its holder leading and takes the slot-1 reuse above. The native port
reads the order it is handed, so it refuses that order and replays the capture
once the constants lead.

A second leading constant tagged with the same slot is merged
(`unity_reuse_second_slot_zero_holder`, and for slots 1 and 2
`unity_reuse_second_slot_one_holder` and
`unity_reuse_second_slot_two_holder`). The dedupe loop meets both holders ahead
of the temporary constant. It re-sources the second holder's outward synapses
onto the first, removes the second, re-sorts the synapses and stops. The
temporary constant survives in that slot, so TypeScript leaves two constants
with the same tag. A third holder is left as it is
(`unity_reuse_third_slot_zero_holder`), and so is a holder matched by legacy id
alone or a tagged holder behind the insertion point, since the loop matches tags
only and stops at the temporary constant. The native port takes all of these. It
refuses a merge that would duplicate one of the first holder's synapses, or that
would remove a holder with an inward synapse. It also refuses one constant
holding slots 0 and 1, whose slot-0 tag TypeScript would overwrite, and one
constant holding slots 1 and 2 or slots 0 and 2 (for example slot 1 by legacy
id -1001 and slot 2 by tag), whose earlier tag it would overwrite and for which
it returns `false`; that refusal comes before any slot-2 merge. Those refusals are read from the frozen source, not captured. It also refuses a merge
when the removed constant itself leads the list, since it may be one of the
holders.

**(c) Overflow.** TypeScript throws `weight must be a number` after it has
inserted the constant. The `after` block keeps that constant with no outward
edge. The native port refuses before changing anything.

## Reproduction

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_unity.json unity
```
