# Creature presentation corpus

Golden persisted-creature JSON for the presentation contract of Issue #765:
the one byte layout a creature file is written in, so a diff between two
engines' output is a real difference and never formatting noise. The contract
itself is described in
[`docs/CREATURE_JSON_PRESENTATION.md`](../../../../docs/CREATURE_JSON_PRESENTATION.md).

## Never hand-reformat these files

Regenerate them with the canonical writer below. An editor, `jq`, a
pretty-printer or a trailing newline changes bytes the gate pins, and the
gate will go red.

## Provenance

The creatures are **synthetic**: authored in NEAT-AI-core for Issue #765 and
vendored outward to the siblings. None of the data comes from a real run. The
key order is taken from NEAT-AI's TypeScript exporter, as of NEAT-AI commit
`221facd8`:

- `src/utils/CreatureExportBuilder.ts`
- `src/neuron/NeuronSerialization.ts`
- `src/architecture/Synapse.ts`
- the golden `test/fixtures/golden/creature-memetic.json`

## How the bytes were produced

Canonical files are the output of this writer over the authored content,
run as a scratch Deno script outside the repository (Deno 2.9.8):

```ts
const negativeZeroReplacer = (_key, value) =>
  Object.is(value, -0) ? JSON.rawJSON("-0") : value;
const bytes = JSON.stringify(JSON.parse(text), negativeZeroReplacer, 1);
```

```sh
deno run --no-config --no-lock --allow-write=canonical gen.ts canonical
```

The bytes are one-space indented, use ECMAScript number text (`1`, `1e-7`,
`1e+21`), keep `-0`, and carry no trailing newline.

The variants are the same documents as the real tools write them:

- `variants/serde_typed_pretty/memetic_by_id.json` is
  `creature_to_json_pretty(&parse_creature_json(canonical)?)?` written as-is
  (serde_json 1.0.154, from `Cargo.lock`).
- `variants/serde_value_pretty/number_forms.json` is the canonical file parsed as
  a `serde_json::Value`, every number rebuilt with
  `Number::from_f64(n.as_f64())` except values under the keys `input`, `output`,
  `generation`, `id` and `toId`, which stay as parsed (a Rust consumer holds them
  as integers and never writes `2.0`), then `serde_json::to_string_pretty` plus one
  trailing newline.
- `variants/jq_indent_1/number_forms.json` is
  `jq --indent 1 . variants/serde_value_pretty/number_forms.json` (jq 1.8.2).

## Coverage

| File | Covers |
| --- | --- |
| `canonical/number_forms.json` | Every number form: `0.000001`, `1e-7`, `1e+21`, `100000000000000000000`, `123456789012345680000`, `5e-324`, `1.7976931348623157e+308`, `2.2985736498644322e-8`, `-1.5e-9`, integer-valued `bias` and `weight`, one `-0`; creature tags; RFC 4122, `input-N` and `output-N` uuids; unsorted neurons and synapses; row-form memetic with ancestry |
| `canonical/if_forest_tagged.json` | IF node `forest-7c1d-if0` with condition, positive and negative synapses; creature, neuron and synapse tags; a tag value needing `\"`, `\\` and `\n` escapes; `frozen`; empty `[]` and `{}` in ancestry; row-form memetic |
| `canonical/memetic_by_id.json` | Id-keyed memetic weights and biases with `"2"` and `"10"` (index keys sort numerically, `"-1"` after them); `id` as the last neuron key; no tags and no `frozen`, so the typed serde variant is lossless; one `-0`; integer-valued bias |
| `variants/serde_typed_pretty/` | Typed serde output: two-space indent, `1.0`, struct field order, `"10"` before `"2"` |
| `variants/serde_value_pretty/` | `Value` output: alphabetical keys, two-space indent, `1.0`, trailing newline |
| `variants/jq_indent_1/` | `jq` output: one-space indent, uppercase `E`, `1.0`, trailing newline |

## Gates

- `tests/creature_presentation_test.ts` (Deno) pins the bytes, the key order,
  the feature coverage and variant equivalence.
- `neat-core/tests/creature_presentation.rs` pins the Rust writer
  (`neat_core::creature_presentation`) to the canonical bytes: every canonical
  file is byte-identical through `creature_presentation_json` (the tagless
  `memetic_by_id.json` also through the typed entry), and every variant
  normalises to its canonical bytes. It also checks every canonical file parses
  with `parse_creature_json` and compiles with `compile_creature`, and that
  `variants/serde_typed_pretty/memetic_by_id.json` is byte-identical to
  `creature_to_json_pretty`'s output for the canonical file.
