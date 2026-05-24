## Summary
Adds symmetric `Serialize` support to `CreatureExport`, `NeuronExport`, and
`SynapseExport` so parsed networks (or those constructed in Rust) can be
written back out to the canonical TypeScript `CreatureExport` JSON shape.
The existing `#[serde(rename = "...")]` attributes (`semanticVersion`,
`forwardOnly`, `fromUUID`, `toUUID`, `type`) apply symmetrically on output.

Adds 1:1 inverse helpers:
- `squash_name_from(SquashType) -> &'static str` — inverse of `parse_squash_name`.
- `synapse_type_name_from(SynapseType) -> Option<&'static str>` — inverse of `parse_synapse_type`.

Also adds convenience wrappers `creature_to_json` / `creature_to_json_pretty`
for the canonical serialisation path, and derives `Clone` / `PartialEq` on
the three structs to support round-trip equality assertions.

Unblocks snapshot diffing, cache priming, and the topology-export work in #22.

Closes #30.

## Evidence
Backend/library change — no UI to screenshot. Verified by unit/integration
tests (see Test Plan below) and a clean `./quality.sh` run covering fmt,
clippy `-D warnings`, workspace tests, `cargo deny`, and `cargo doc -D warnings`.

## Test Plan
Added `neat-core/tests/creature/roundtrip.rs` with 7 new tests:

- `parse_serialise_parse_preserves_all_fields` — parse the shared minimal
  fixture, serialise, re-parse, and assert field-by-field equality.
- `serialisation_is_deterministic_byte_identical` — two serialisations of
  the same `CreatureExport` produce byte-identical JSON (compact and pretty).
- `minimal_one_input_one_output_one_synapse_roundtrips` — edge case:
  1 input, 1 output, 1 synapse, no hidden, no optional fields.
- `camelcase_field_names_are_symmetric_on_output` — asserts the serialised
  output uses `semanticVersion`, `forwardOnly`, `fromUUID`, `toUUID`, and
  `type`, and that the result re-parses to an equal value.
- `every_squash_variant_roundtrips_through_name_helpers` — for every
  `SquashType` variant, `parse_squash_name(squash_name_from(v)) == Ok(v)`.
- `every_synapse_variant_roundtrips_through_name_helpers` — for every
  `SynapseType` variant, `parse_synapse_type(synapse_type_name_from(v)) == v`.
- `optional_fields_absent_when_none` — `None` optional fields are omitted
  from the JSON (matching the TypeScript "optional field" convention) and
  still round-trip cleanly.

All existing tests continue to pass; `./quality.sh` passes cleanly.
