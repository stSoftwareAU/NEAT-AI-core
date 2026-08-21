//! GRQ #4257 — `memetic.weights` arrives in **two** valid forms, and both must
//! parse, round trip and validate.
//!
//! NEAT-AI's wire exporter (`src/creature/MemeticWireExport.ts`) writes the
//! UUID-keyed row array `[{ fromUUID, toUUID, weight }, …]`; its in-memory
//! runtime record is the id-keyed map `{ "<fromId>": [{ toId, weight }, …] }`.
//! Neither is legacy. Modelling only the map made
//! `neat_ai_backpropagation` exit 1 on the GRQ-10 sampler fittest creature with
//! `Creature JSON error: invalid type: sequence, expected a map`, so the whole
//! Backprop stage failed on a creature both stacks consider valid.
//!
//! The rows carry the same wire UUIDs the synapses do — `input-N` for an
//! implicit input neuron, `output-N` for an output, the stable `uuid`
//! otherwise — so rule 31 resolves them exactly as
//! `resolve_synapse_endpoints` resolves a synapse endpoint.

use neat_core::creature::{MemeticWeightRowExport, MemeticWeights};
use neat_core::creature_validate::{ValidateOptions, creature_validate, reason};
use neat_core::{CreatureExport, creature_to_json, parse_creature_json};

// ---------------------------------------------------------------------------
// Fixtures — `input-0 -> h -> output-0`, the shape the sampler creature uses.
// ---------------------------------------------------------------------------

/// A creature whose `memetic.weights` is written as `{WEIGHTS}` and whose
/// `memetic.biases` is written as `{BIASES}`.
const TEMPLATE: &str = r#"{
  "input": 2,
  "output": 1,
  "neurons": [
    { "type": "hidden", "uuid": "h", "bias": 0.25, "squash": "TANH" },
    { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
  ],
  "synapses": [
    { "fromUUID": "input-0", "toUUID": "h", "weight": 1.0 },
    { "fromUUID": "input-1", "toUUID": "h", "weight": -0.5 },
    { "fromUUID": "h", "toUUID": "output-0", "weight": 2.0 }
  ],
  "memetic": {
    "generation": 7,
    "score": -0.25,
    "biases": {BIASES},
    "weights": {WEIGHTS}
  }
}"#;

fn creature_json(biases: &str, weights: &str) -> String {
    TEMPLATE
        .replace("{BIASES}", biases)
        .replace("{WEIGHTS}", weights)
}

/// The wire form the sampler writes: UUID rows and UUID-keyed biases.
fn wire_form(weights: &str) -> String {
    creature_json(r#"{ "h": 0.125 }"#, weights)
}

fn parse(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("the creature parses")
}

fn validate_err(json: &str) -> String {
    let creature = parse(json);
    let failure = creature_validate(&creature, &ValidateOptions::default())
        .expect_err("this creature breaks rule 31");
    assert_eq!(failure.reason, reason::MEMETIC);
    failure.message
}

// ---------------------------------------------------------------------------
// 1. Parsing — the reported exit 1.
// ---------------------------------------------------------------------------

/// The exact failure from `GRQ-10-sloth.log`: a UUID row array where the
/// id-keyed map was the only modelled form.
#[test]
fn the_uuid_row_array_parses_into_rows() {
    let json = wire_form(
        r#"[
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.5 },
      { "fromUUID": "h", "toUUID": "output-0", "weight": -0.25 }
    ]"#,
    );

    let creature = parse(&json);
    let weights = &creature.memetic.as_ref().expect("memetic block").weights;

    assert_eq!(
        weights.rows(),
        Some(
            [
                MemeticWeightRowExport {
                    from_uuid: Some("input-0".to_string()),
                    to_uuid: Some("h".to_string()),
                    weight: Some(0.5),
                },
                MemeticWeightRowExport {
                    from_uuid: Some("h".to_string()),
                    to_uuid: Some("output-0".to_string()),
                    weight: Some(-0.25),
                },
            ]
            .as_slice()
        )
    );
    assert!(weights.by_id().is_none(), "the map form was not written");
}

/// The id-keyed map still parses into the map form — the other valid shape is
/// not traded away for the new one.
#[test]
fn the_id_keyed_map_still_parses_into_the_map() {
    let json = creature_json(
        r#"{ "-1": 0.75 }"#,
        r#"{ "0": [{ "toId": 2, "weight": 0.5 }] }"#,
    );

    let creature = parse(&json);
    let weights = &creature.memetic.as_ref().expect("memetic block").weights;

    let by_id = weights.by_id().expect("the map form was written");
    assert_eq!(by_id["0"][0].to_id, Some(2));
    assert_eq!(by_id["0"][0].weight, Some(0.5));
    assert!(weights.rows().is_none());
}

/// A row missing `toUUID` or `weight` must still *parse*, for the same reason
/// `MemeticWeightExport` keeps its fields optional: rule 31 has to be able to
/// report it, and a serde error at the parse boundary never reaches a rule.
#[test]
fn a_row_missing_its_fields_still_parses_so_the_rule_can_report_it() {
    let json = wire_form(r#"[{ "fromUUID": "h" }]"#);

    let creature = parse(&json);
    let rows = creature.memetic.as_ref().expect("memetic").weights.rows();

    assert_eq!(
        rows,
        Some(
            [MemeticWeightRowExport {
                from_uuid: Some("h".to_string()),
                to_uuid: None,
                weight: None,
            }]
            .as_slice()
        )
    );
}

/// The map form's values are still typed arrays — a bare object under a key is
/// a serde error, which is the divergence
/// `creature_validate_conformance.rs` declares for `memetic-weights-not-an-array`.
#[test]
fn a_map_value_that_is_not_an_array_is_still_a_parse_error() {
    let json = creature_json(r#"{}"#, r#"{ "0": { "toId": 2, "weight": 0.5 } }"#);

    let error = parse_creature_json(&json).expect_err("a bare object is not a weight list");
    assert!(
        error.to_string().contains("Creature JSON error"),
        "reported as a JSON error: {error}"
    );
}

/// Neither form is `weights: 3` — a scalar names the two shapes it could have
/// been rather than failing with serde's bare "invalid type" text.
#[test]
fn a_scalar_weights_value_fails_loud_naming_both_forms() {
    let json = wire_form("3");

    let error = parse_creature_json(&json).expect_err("a number is neither form");
    let text = error.to_string();
    assert!(text.contains("fromUUID"), "names the row form: {text}");
    assert!(text.contains("map"), "names the map form: {text}");
}

// ---------------------------------------------------------------------------
// 2. Round trip — whichever form was read is the form written back.
// ---------------------------------------------------------------------------

#[test]
fn the_row_form_round_trips_as_rows() {
    let json = wire_form(
        r#"[
      { "fromUUID": "input-1", "toUUID": "h", "weight": 0.5 }
    ]"#,
    );

    let creature = parse(&json);
    let written = creature_to_json(&creature).expect("serialises");

    assert!(
        written.contains(r#""weights":[{"fromUUID":"input-1","toUUID":"h","weight":0.5}]"#),
        "the array form is written back as an array: {written}"
    );
    assert_eq!(parse(&written), creature, "and re-parses unchanged");
}

#[test]
fn the_map_form_round_trips_as_a_map() {
    let json = creature_json(r#"{}"#, r#"{ "0": [{ "toId": 2, "weight": 0.5 }] }"#);

    let creature = parse(&json);
    let written = creature_to_json(&creature).expect("serialises");

    assert!(
        written.contains(r#""weights":{"0":[{"toId":2,"weight":0.5}]}"#),
        "the map form is written back as a map: {written}"
    );
    assert_eq!(parse(&written), creature, "and re-parses unchanged");
}

/// `ancestry` — the key added alongside the row form — is carried verbatim by
/// `MemeticExport::extra` rather than dropped.
#[test]
fn the_ancestry_key_survives_the_row_form_round_trip() {
    let json = wire_form(r#"[{ "fromUUID": "h", "toUUID": "output-0", "weight": 0.5 }]"#).replace(
        "\"score\": -0.25",
        "\"score\": -0.25, \"ancestry\": [{\"generation\": 6}]",
    );

    let creature = parse(&json);
    let written = creature_to_json(&creature).expect("serialises");

    assert!(written.contains("\"ancestry\""), "kept: {written}");
    assert_eq!(parse(&written), creature);
}

// ---------------------------------------------------------------------------
// 3. Rule 31 — the row form resolves against the creature's wire UUIDs.
// ---------------------------------------------------------------------------

#[test]
fn a_row_form_memetic_block_that_resolves_passes() {
    let json = wire_form(
        r#"[
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.5 },
      { "fromUUID": "h", "toUUID": "output-0", "weight": -0.25 }
    ]"#,
    );

    let stats = creature_validate(&parse(&json), &ValidateOptions::default())
        .expect("every row names a real synapse");
    assert_eq!(
        (stats.input, stats.hidden, stats.output, stats.connections),
        (2, 1, 1, 3)
    );
}

/// A bias keyed by a wire UUID resolves too — the wire form keys biases the
/// same way it keys the rows.
#[test]
fn a_bias_keyed_by_a_wire_uuid_resolves() {
    let json = creature_json(
        r#"{ "output-0": 0.5, "input-1": -0.25 }"#,
        r#"[{ "fromUUID": "h", "toUUID": "output-0", "weight": 0.5 }]"#,
    );

    creature_validate(&parse(&json), &ValidateOptions::default())
        .expect("both bias keys name a real neuron");
}

/// A bias key that is neither a neuron id nor a wire UUID is still rejected —
/// accepting both forms must not accept a reference to nothing.
#[test]
fn a_bias_key_naming_nothing_is_still_rejected() {
    let json = creature_json(
        r#"{ "ghost": 0.5 }"#,
        r#"[{ "fromUUID": "h", "toUUID": "output-0", "weight": 0.5 }]"#,
    );

    assert_eq!(
        validate_err(&json),
        "Neuron with id ghost not found in the creature."
    );
}

#[test]
fn a_row_whose_from_uuid_names_no_neuron_is_rejected() {
    let json = wire_form(r#"[{ "fromUUID": "ghost", "toUUID": "h", "weight": 0.5 }]"#);

    assert_eq!(
        validate_err(&json),
        "Synapse with id ghost not found in the creature."
    );
}

#[test]
fn a_row_whose_to_uuid_names_no_neuron_is_rejected() {
    let json = wire_form(r#"[{ "fromUUID": "h", "toUUID": "ghost", "weight": 0.5 }]"#);

    assert_eq!(
        validate_err(&json),
        "Memetic from id h has no valid neuron."
    );
}

#[test]
fn a_row_without_a_to_uuid_is_rejected() {
    let json = wire_form(r#"[{ "fromUUID": "h", "weight": 0.5 }]"#);

    assert_eq!(
        validate_err(&json),
        "Memetic from id h to id undefined is invalid at index 0."
    );
}

#[test]
fn a_row_without_a_from_uuid_is_rejected_with_its_index() {
    let json = wire_form(
        r#"[
      { "fromUUID": "h", "toUUID": "output-0", "weight": 0.5 },
      { "toUUID": "output-0", "weight": 0.5 }
    ]"#,
    );

    assert_eq!(
        validate_err(&json),
        "Memetic from id undefined to id output-0 is invalid at index 1."
    );
}

#[test]
fn a_row_without_a_weight_is_rejected_with_its_index() {
    let json = wire_form(
        r#"[
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.5 },
      { "fromUUID": "h", "toUUID": "output-0" }
    ]"#,
    );

    assert_eq!(
        validate_err(&json),
        "Memetic from id h to id output-0 has invalid weight at index 1."
    );
}

/// Both endpoints exist, but nothing connects them — the same rule the map
/// form enforces by id.
#[test]
fn a_row_naming_two_real_neurons_with_no_synapse_between_them_is_rejected() {
    let json = wire_form(r#"[{ "fromUUID": "input-0", "toUUID": "output-0", "weight": 0.5 }]"#);

    assert_eq!(
        validate_err(&json),
        "Memetic from id input-0 to id output-0 has no matching synapses."
    );
}

/// The map form keeps validating by runtime id — the derived hidden id, not
/// the UUID — so the two forms coexist without either weakening the other.
#[test]
fn the_map_form_still_validates_by_runtime_id() {
    let json = creature_json(
        r#"{ "-1": 0.5 }"#,
        r#"{ "99": [{ "toId": -1, "weight": 0.5 }] }"#,
    );

    assert_eq!(
        validate_err(&json),
        "Synapse with id 99 not found in the creature."
    );
}

/// A hand-built creature reaches the row form without going through JSON.
#[test]
fn a_hand_built_creature_can_carry_the_row_form() {
    let json = wire_form(r#"[{ "fromUUID": "h", "toUUID": "output-0", "weight": 0.5 }]"#);
    let mut creature = parse(&json);

    creature.memetic.as_mut().expect("memetic").weights =
        MemeticWeights::Rows(vec![MemeticWeightRowExport {
            from_uuid: Some("input-1".to_string()),
            to_uuid: Some("h".to_string()),
            weight: Some(-1.5),
        }]);

    creature_validate(&creature, &ValidateOptions::default())
        .expect("input-1 -> h is a real synapse");
}
