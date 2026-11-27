# Constant-merge helper captures (Issue #776)

Six records in `neat-core/tests/golden/compact_legacy_constants.json` capture
all six original tests in
[`ConstantMerge.ts`](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/test/compact/ConstantMerge.ts).
All originals passed unchanged. These are direct `mergeRedundantConstants`
calls; they complement the whole-creature constant captures in the
[overall baseline](compaction-baseline.md). They do not complete #776.

| Capture | Original observable contract |
|---|---|
| `if_roles` | Three original constants retire in favor of three bias-one canonical sources. Condition, positive and negative roles remain distinct, with weights `0.4 * 0.5`, `0.6 * -0.25` and `0.3 * 2`. No endpoint pair is duplicated. |
| `fixpoint` | The same constructor first changes, then a second call returns unchanged with zero removals. `JSON.stringify` of the graph is byte-for-byte unchanged on that second call. |
| `same_role` | Two positive-role contributions combine into one `constant-0` edge of weight `0.6 * 0.5 + 0.25 * 4 = 1.3`; two original neurons and one net edge retire. |
| `maximum_slots` | MAXIMUM keeps independent contributions. Three canonical sources replace three originals, while surplus `const-d` retains bias `0.25` and weight `0.9`. |
| `frozen` | A frozen constant and a constant with a frozen outgoing edge remain untouched in wire topology, bias, weight and frozen flags. The helper reports no change. |
| `reuse_slot` | An existing `constant-0` and its positive edge of weight `0.6` survive. The extra constant uses `constant-1` for the negative role at `0.3 * 0.5`, without duplicating endpoint pairs. |

## Preparation and identity

The original first five tests pass raw exports directly. Only `reuse_slot`
explicitly calls `normaliseCreatureExport` before the helper. The recorder
keeps the unmodified literal in `input`, the prepared helper input in `before`,
and the actual return counts and mutated graph in `result` and `after`.
It invokes the frozen implementation without a creature loader, whole-creature
compactor or activation call.

The helper itself normalizes IDs, even when frozen inputs cause it to return
`changed: false`. That refusal is not byte-for-byte equality with the raw
input. By contrast, the second call in `fixpoint` receives an already-canonical
graph and preserves its actual JSON string, including member order. Its
`second` record stores both strings and both graph snapshots; the integrity
test verifies their agreement and the unchanged result.

New integer IDs use the legacy allocator's process state. The two equal raw
inputs in `if_roles` and `fixpoint` therefore receive different newly allocated
constant IDs within the capture process. Their wire identities are still
`constant-0`, `constant-1` and `constant-2`. Reproduce the CLI in a fresh process
with the recorded case order. No fixed cross-process or cross-language numeric
ID policy is inferred from these observations. The checks require unique IDs,
consistent UUID/integer endpoints, and retention of the existing canonical
constant's ID in `reuse_slot`.

## Verification and limits

The integrity test derives the expected weights from the original biases and
weights at the original `1e-12` bound. It checks all expected edges and roles,
canonical slot membership and bias-one values, retained noncanonical neurons,
frozen flags, helper counts, and absence of duplicate endpoint pairs. The
MAXIMUM test checks the surplus input structurally; activation samples alone
could miss loss of a dominated contribution.

The new test failed before fixture generation. Raw constructors/literals and
test names were independently compared with the source. Deliberate corruption
of every record was rejected and restored, including role weights, the second
pass, summed contributions, a lost surplus MAXIMUM edge, frozen state and
canonical reuse. Additional checks rejected duplicate endpoints, stale integer
handles, changed canonical bias and a changed serialized second pass. Existing
capture modes and this new mode regenerate byte-for-byte.

These are **capture integrity checks, not native rewrite or activation parity**.
The original helper tests do not measure activation error or fitness; no new
tolerance or acceptance claim is added. The public large forest helper fixture
and the remaining family requirements stay open in the overall baseline.

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_constants.json constants
cargo test -p neat-core --test compact_legacy_constants
```

The shared source guard requires clean tracked sources at the frozen revision.
Run the unchanged source suite there with `deno test -A test/compact/ConstantMerge.ts`.
