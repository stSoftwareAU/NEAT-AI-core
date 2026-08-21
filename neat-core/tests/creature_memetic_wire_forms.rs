//! `memetic.weights` parses in **either** valid wire form — GRQ#4257.
//!
//! NEAT-AI writes the record two ways and both are current (neither is
//! legacy): `src/creature/MemeticWireExport.ts` serialises weights as a flat
//! array of `{fromUUID, toUUID, weight}` rows for any JSON that leaves the
//! process, while the in-memory / id-keyed form
//! (`{"<fromId>": [{toId, weight}, …]}`) is what
//! `src/architecture/NormaliseCreatureExport.ts` normalises that array back
//! into. Modelling only the map made every sampler creature carrying the row
//! form fail to parse — `invalid type: sequence, expected a map` — which
//! exited the GRQ Backprop stage 1.
//!
//! These tests pin three things: both forms parse, a parsed creature is
//! serialised back in the form it was read, and the `MEMETIC` rules resolve
//! whichever form was read.

use std::collections::BTreeMap;

use neat_core::creature::{
    MemeticExport, MemeticWeightExport, MemeticWeightRowExport, MemeticWeights,
};
use neat_core::creature_validate::{
    FailureClass, ValidateOptions, ValidationFailure, ValidationStats, reason,
    validate_synapse_and_memetic_rules,
};
use neat_core::{
    CreatureExport, NeuronExport, SynapseExport, creature_to_json, parse_creature_json,
};

// ---------------------------------------------------------------------------
// Fixtures — `input-0 -> h -> o`, the hidden neuron carrying id 7 so that a
// resolved id is never also an index.
// ---------------------------------------------------------------------------

/// The row (UUID) form, as the sampler writes it, `ancestry` and all.
const ROW_FORM: &str = r#"{
  "input": 1,
  "output": 1,
  "neurons": [
    { "id": 7, "type": "hidden", "uuid": "h", "bias": 0.5, "squash": "IDENTITY" },
    { "id": -1, "type": "output", "uuid": "o", "bias": 0.25, "squash": "IDENTITY" }
  ],
  "synapses": [
    { "fromUUID": "input-0", "toUUID": "h", "weight": 1.0 },
    { "fromUUID": "h", "toUUID": "o", "weight": -0.5 }
  ],
  "memetic": {
    "generation": 3,
    "score": 0.35,
    "biases": { "h": 0.125 },
    "weights": [
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.0000024 },
      { "fromUUID": "h", "toUUID": "o", "weight": -0.5 }
    ],
    "ancestry": [
      {
        "generation": 2,
        "weights": [{ "fromUUID": "input-0", "toUUID": "h", "weight": 0.0000024 }],
        "biases": {},
        "score": 0.35
      }
    ]
  }
}"#;

/// The id-keyed map form — the shape this crate has always modelled.
const MAP_FORM: &str = r#"{
  "input": 1,
  "output": 1,
  "neurons": [
    { "id": 7, "type": "hidden", "uuid": "h", "bias": 0.5, "squash": "IDENTITY" },
    { "id": -1, "type": "output", "uuid": "o", "bias": 0.25, "squash": "IDENTITY" }
  ],
  "synapses": [
    { "fromUUID": "input-0", "toUUID": "h", "weight": 1.0 },
    { "fromUUID": "h", "toUUID": "o", "weight": -0.5 }
  ],
  "memetic": {
    "generation": 3,
    "score": 0.35,
    "biases": { "7": 0.125 },
    "weights": { "0": [{ "toId": 7, "weight": 0.0000024 }] }
  }
}"#;

fn memetic_of(json: &str) -> MemeticExport {
    parse_creature_json(json)
        .expect("creature parses")
        .memetic
        .expect("the fixture carries a memetic record")
}

fn neuron(neuron_type: &str, uuid: &str, id: i64) -> NeuronExport {
    NeuronExport {
        id: Some(id),
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias: 0.5,
        squash: Some("IDENTITY".to_string()),
    }
}

fn edge(from: &str, to: &str) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight: 1.0,
        synapse_type: None,
    }
}

/// `input-0 -> h -> o`, ids `0`, `7`, `-1`.
fn creature_with(memetic: MemeticExport) -> CreatureExport {
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![neuron("hidden", "h", 7), neuron("output", "o", -1)],
        synapses: vec![edge("input-0", "h"), edge("h", "o")],
        semantic_version: None,
        forward_only: false,
        memetic: Some(memetic),
    }
}

fn row(from_uuid: &str, to_uuid: &str, weight: f64) -> MemeticWeightRowExport {
    MemeticWeightRowExport {
        from_uuid: Some(from_uuid.to_string()),
        to_uuid: Some(to_uuid.to_string()),
        weight: Some(weight),
    }
}

fn rows(rows: Vec<MemeticWeightRowExport>) -> MemeticExport {
    MemeticExport {
        weights: MemeticWeights::Rows(rows),
        ..MemeticExport::default()
    }
}

fn by_id(key: &str, entries: Vec<MemeticWeightExport>) -> MemeticExport {
    let mut map = BTreeMap::new();
    map.insert(key.to_string(), entries);
    MemeticExport {
        weights: MemeticWeights::ById(map),
        ..MemeticExport::default()
    }
}

fn run(creature: &CreatureExport) -> ValidationStats {
    let mut stats = ValidationStats::default();
    validate_synapse_and_memetic_rules(creature, &ValidateOptions::default(), &mut stats)
        .unwrap_or_else(|failure| panic!("expected a valid creature, was: {failure}"));
    stats
}

fn run_err(creature: &CreatureExport) -> ValidationFailure {
    let mut stats = ValidationStats::default();
    validate_synapse_and_memetic_rules(creature, &ValidateOptions::default(), &mut stats)
        .expect_err("expected a validation failure")
}

// ---------------------------------------------------------------------------
// Parsing — both forms, and everything either carries alongside.
// ---------------------------------------------------------------------------

#[test]
fn the_row_form_parses_into_uuid_rows() {
    let memetic = memetic_of(ROW_FORM);

    let rows = match &memetic.weights {
        MemeticWeights::Rows(rows) => rows,
        MemeticWeights::ById(_) => panic!("the row form must not be read as a map"),
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].from_uuid.as_deref(), Some("input-0"));
    assert_eq!(rows[0].to_uuid.as_deref(), Some("h"));
    assert_eq!(rows[0].weight, Some(0.0000024));
    assert_eq!(rows[1].to_uuid.as_deref(), Some("o"));
}

#[test]
fn the_map_form_still_parses_into_the_id_keyed_map() {
    let memetic = memetic_of(MAP_FORM);

    let map = match &memetic.weights {
        MemeticWeights::ById(map) => map,
        MemeticWeights::Rows(_) => panic!("the map form must not be read as rows"),
    };
    assert_eq!(map.len(), 1);
    assert_eq!(map["0"][0].to_id, Some(7));
    assert_eq!(map["0"][0].weight, Some(0.0000024));
}

#[test]
fn the_row_form_keeps_its_biases_and_the_keys_the_exporter_writes() {
    let memetic = memetic_of(ROW_FORM);

    assert_eq!(memetic.biases.get("h"), Some(&0.125));
    assert_eq!(
        memetic.extra.get("generation").and_then(|v| v.as_i64()),
        Some(3),
        "`generation` is preserved verbatim"
    );
    assert!(
        memetic.extra.contains_key("ancestry"),
        "`ancestry` is preserved verbatim: {:?}",
        memetic.extra.keys().collect::<Vec<_>>()
    );
}

#[test]
fn an_empty_weights_array_is_read_as_rows_not_as_a_map() {
    let json = ROW_FORM.replace(
        r#""weights": [
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.0000024 },
      { "fromUUID": "h", "toUUID": "o", "weight": -0.5 }
    ],"#,
        r#""weights": [],"#,
    );

    let memetic = memetic_of(&json);

    assert_eq!(memetic.weights, MemeticWeights::Rows(Vec::new()));
    assert!(memetic.weights.is_empty());
}

#[test]
fn weights_that_are_neither_an_array_nor_a_map_still_fail_loudly() {
    let json = ROW_FORM.replace(
        r#""weights": [
      { "fromUUID": "input-0", "toUUID": "h", "weight": 0.0000024 },
      { "fromUUID": "h", "toUUID": "o", "weight": -0.5 }
    ],"#,
        r#""weights": 7,"#,
    );

    let error = parse_creature_json(&json).expect_err("a scalar is not a weights record");
    assert!(
        error.to_string().contains("Creature JSON error"),
        "unexpected error: {error}"
    );
}

// ---------------------------------------------------------------------------
// Serialisation — the form a creature was read in is the form it is written
// back in, and output stays deterministic.
// ---------------------------------------------------------------------------

#[test]
fn a_row_form_creature_is_written_back_as_rows() {
    let creature = parse_creature_json(ROW_FORM).expect("creature parses");

    let json = creature_to_json(&creature).expect("creature serialises");

    assert!(
        json.contains(r#""weights":[{"fromUUID":"input-0","toUUID":"h""#),
        "weights must stay an array of UUID rows: {json}"
    );
    let reparsed = parse_creature_json(&json).expect("the serialised creature re-parses");
    assert_eq!(reparsed, creature, "round trip preserves every field");
    assert_eq!(
        creature_to_json(&reparsed).expect("creature serialises"),
        json,
        "serialisation stays deterministic"
    );
}

#[test]
fn a_map_form_creature_is_written_back_as_a_map() {
    let creature = parse_creature_json(MAP_FORM).expect("creature parses");

    let json = creature_to_json(&creature).expect("creature serialises");

    assert!(
        json.contains(r#""weights":{"0":[{"toId":7"#),
        "weights must stay an id-keyed map: {json}"
    );
    let reparsed = parse_creature_json(&json).expect("the serialised creature re-parses");
    assert_eq!(reparsed, creature, "round trip preserves every field");
}

// ---------------------------------------------------------------------------
// Validation — the `MEMETIC` rules resolve whichever form was read.
// ---------------------------------------------------------------------------

#[test]
fn a_row_block_that_resolves_passes() {
    let mut memetic = rows(vec![row("input-0", "h", 0.25), row("h", "o", -0.5)]);
    memetic.biases.insert("h".to_string(), 0.5);

    let stats = run(&creature_with(memetic));

    assert_eq!(stats.connections, 2);
}

#[test]
fn a_bias_keyed_by_its_wire_uuid_resolves() {
    let mut memetic = MemeticExport::default();
    memetic.biases.insert("o".to_string(), 0.5);

    run(&creature_with(memetic));
}

#[test]
fn a_bias_keyed_by_an_unknown_uuid_is_rejected() {
    let mut memetic = MemeticExport::default();
    memetic.biases.insert("ghost".to_string(), 0.5);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Neuron with id ghost not found in the creature."
    );
}

#[test]
fn a_map_key_that_is_a_wire_uuid_resolves() {
    let memetic = by_id(
        "h",
        vec![MemeticWeightExport {
            to_id: Some(-1),
            weight: Some(0.25),
        }],
    );

    run(&creature_with(memetic));
}

#[test]
fn a_row_naming_an_unknown_source_is_rejected() {
    let memetic = rows(vec![row("ghost", "h", 0.25)]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Synapse with id ghost not found in the creature."
    );
}

#[test]
fn a_row_without_a_source_is_rejected() {
    let memetic = rows(vec![MemeticWeightRowExport {
        from_uuid: None,
        to_uuid: Some("h".to_string()),
        weight: Some(0.25),
    }]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Synapse with id undefined not found in the creature."
    );
}

#[test]
fn a_row_without_a_destination_is_rejected() {
    let memetic = rows(vec![MemeticWeightRowExport {
        from_uuid: Some("input-0".to_string()),
        to_uuid: None,
        weight: Some(0.25),
    }]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 0 to id undefined is invalid."
    );
}

#[test]
fn a_row_without_a_weight_is_rejected_with_its_index() {
    let memetic = rows(vec![
        row("input-0", "h", 0.25),
        MemeticWeightRowExport {
            from_uuid: Some("h".to_string()),
            to_uuid: Some("o".to_string()),
            weight: None,
        },
    ]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 7 to id -1 has invalid weight at index 1."
    );
}

#[test]
fn a_row_naming_an_unknown_destination_is_rejected() {
    let memetic = rows(vec![row("input-0", "ghost", 0.25)]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(failure.message, "Memetic from id 0 has no valid neuron.");
}

#[test]
fn a_row_with_no_matching_synapse_is_rejected() {
    // Both endpoints resolve (input id 0 and output id -1) but `input-0` feeds
    // the hidden neuron, not the output, so no `0 -> -1` synapse exists.
    let memetic = rows(vec![row("input-0", "o", 0.25)]);

    let failure = run_err(&creature_with(memetic));

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 0 to id -1 has no matching synapses."
    );
}

#[test]
fn the_row_form_fixture_parses_and_validates_end_to_end() {
    let creature = parse_creature_json(ROW_FORM).expect("creature parses");

    let stats = run(&creature);

    assert_eq!(stats.connections, 2);
}
