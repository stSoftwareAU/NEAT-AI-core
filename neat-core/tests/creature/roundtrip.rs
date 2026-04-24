//! Round-trip tests for `CreatureExport` / `NeuronExport` / `SynapseExport`
//! serialisation — issue #30.
//!
//! These tests exercise the public `parse_creature_json` / `creature_to_json`
//! contract: parsing a fixture, serialising it back, and re-parsing the
//! result must preserve every field. They also verify deterministic output
//! and the 1:1 inverse mapping for every `SquashType` / `SynapseType` variant.

use neat_core::{
    CreatureExport, NeuronExport, SynapseExport, creature_to_json, creature_to_json_pretty,
    parse_creature_json, parse_squash_name, parse_synapse_type, squash_name_from,
    synapse_type_name_from,
};
use neat_core::{SquashType, SynapseType};

fn all_squash_variants() -> &'static [SquashType] {
    &[
        SquashType::Identity,
        SquashType::Relu,
        SquashType::Relu6,
        SquashType::LeakyRelu,
        SquashType::Selu,
        SquashType::Elu,
        SquashType::Logistic,
        SquashType::Tanh,
        SquashType::HardTanh,
        SquashType::Softsign,
        SquashType::Softplus,
        SquashType::Swish,
        SquashType::Mish,
        SquashType::Gelu,
        SquashType::Sine,
        SquashType::Cosine,
        SquashType::Tan,
        SquashType::ArcTan,
        SquashType::Gaussian,
        SquashType::BentIdentity,
        SquashType::BipolarSigmoid,
        SquashType::Bipolar,
        SquashType::Step,
        SquashType::Complement,
        SquashType::Absolute,
        SquashType::Square,
        SquashType::Cube,
        SquashType::Sqrt,
        SquashType::StdInverse,
        SquashType::Exponential,
        SquashType::LogSigmoid,
        SquashType::Isru,
        SquashType::Minimum,
        SquashType::Maximum,
        SquashType::If,
        SquashType::Hypotenuse,
        SquashType::HypotenuseV2,
        SquashType::Mean,
    ]
}

fn all_synapse_variants() -> &'static [SynapseType] {
    &[
        SynapseType::Standard,
        SynapseType::Condition,
        SynapseType::Negative,
        SynapseType::Positive,
    ]
}

fn assert_creature_equal(a: &CreatureExport, b: &CreatureExport) {
    assert_eq!(a.input, b.input, "input mismatch");
    assert_eq!(a.output, b.output, "output mismatch");
    assert_eq!(
        a.semantic_version, b.semantic_version,
        "semantic_version mismatch"
    );
    assert_eq!(a.forward_only, b.forward_only, "forward_only mismatch");
    assert_eq!(a.neurons.len(), b.neurons.len(), "neuron count mismatch");
    for (i, (na, nb)) in a.neurons.iter().zip(b.neurons.iter()).enumerate() {
        assert_eq!(na.neuron_type, nb.neuron_type, "neuron {i} type");
        assert_eq!(na.uuid, nb.uuid, "neuron {i} uuid");
        assert_eq!(na.bias, nb.bias, "neuron {i} bias");
        assert_eq!(na.squash, nb.squash, "neuron {i} squash");
    }
    assert_eq!(a.synapses.len(), b.synapses.len(), "synapse count mismatch");
    for (i, (sa, sb)) in a.synapses.iter().zip(b.synapses.iter()).enumerate() {
        assert_eq!(sa.from_uuid, sb.from_uuid, "synapse {i} fromUUID");
        assert_eq!(sa.to_uuid, sb.to_uuid, "synapse {i} toUUID");
        assert_eq!(sa.weight, sb.weight, "synapse {i} weight");
        assert_eq!(sa.synapse_type, sb.synapse_type, "synapse {i} type");
    }
}

#[test]
fn parse_serialise_parse_preserves_all_fields() {
    let creature = parse_creature_json(crate::common::minimal_creature_json()).expect("parse");
    let serialised = creature_to_json(&creature).expect("serialise");
    let reparsed = parse_creature_json(&serialised).expect("re-parse");
    assert_creature_equal(&creature, &reparsed);
}

#[test]
fn serialisation_is_deterministic_byte_identical() {
    let creature = parse_creature_json(crate::common::minimal_creature_json()).expect("parse");
    let a = creature_to_json(&creature).expect("serialise a");
    let b = creature_to_json(&creature).expect("serialise b");
    assert_eq!(a, b, "two serialisations must be byte-identical");

    let pretty_a = creature_to_json_pretty(&creature).expect("pretty a");
    let pretty_b = creature_to_json_pretty(&creature).expect("pretty b");
    assert_eq!(
        pretty_a, pretty_b,
        "pretty serialisations must also be byte-identical"
    );
}

#[test]
fn minimal_one_input_one_output_one_synapse_roundtrips() {
    let json = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.25, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.5}
        ]
    }"#;
    let creature = parse_creature_json(json).expect("parse minimal");
    assert_eq!(creature.input, 1);
    assert_eq!(creature.output, 1);
    assert_eq!(creature.neurons.len(), 1);
    assert_eq!(creature.synapses.len(), 1);
    assert!(creature.synapses[0].synapse_type.is_none());

    let serialised = creature_to_json(&creature).expect("serialise");
    let reparsed = parse_creature_json(&serialised).expect("re-parse");
    assert_creature_equal(&creature, &reparsed);
}

#[test]
fn camelcase_field_names_are_symmetric_on_output() {
    let creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![NeuronExport {
            neuron_type: "output".to_string(),
            uuid: "output-0".to_string(),
            bias: 0.0,
            squash: Some("IDENTITY".to_string()),
        }],
        synapses: vec![SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: 1.0,
            synapse_type: Some("condition".to_string()),
        }],
        semantic_version: Some("1.2.3".to_string()),
        forward_only: true,
    };

    let serialised = creature_to_json(&creature).expect("serialise");
    // Assert on the canonical TypeScript camelCase field names.
    assert!(serialised.contains("\"semanticVersion\":\"1.2.3\""));
    assert!(serialised.contains("\"forwardOnly\":true"));
    assert!(serialised.contains("\"fromUUID\":\"input-0\""));
    assert!(serialised.contains("\"toUUID\":\"output-0\""));
    assert!(serialised.contains("\"type\":\"condition\""));
    assert!(serialised.contains("\"type\":\"output\""));

    let reparsed = parse_creature_json(&serialised).expect("re-parse");
    assert_creature_equal(&creature, &reparsed);
}

#[test]
fn every_squash_variant_roundtrips_through_name_helpers() {
    for &variant in all_squash_variants() {
        let name = squash_name_from(variant);
        let parsed = parse_squash_name(name)
            .unwrap_or_else(|e| panic!("parse_squash_name({name:?}) failed for {variant:?}: {e}"));
        assert_eq!(
            parsed, variant,
            "parse_squash_name(squash_name_from({variant:?})) != {variant:?}"
        );
    }
}

#[test]
fn every_synapse_variant_roundtrips_through_name_helpers() {
    for &variant in all_synapse_variants() {
        let name = synapse_type_name_from(variant);
        let parsed = parse_synapse_type(name);
        assert_eq!(
            parsed, variant,
            "parse_synapse_type(synapse_type_name_from({variant:?})) != {variant:?}"
        );
    }
}

#[test]
fn optional_fields_absent_when_none() {
    // A creature with no semanticVersion and no synapse types should not
    // emit those keys at all — matches the TypeScript "optional field"
    // convention and avoids explicit `null` values in the canonical JSON.
    let creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![NeuronExport {
            neuron_type: "output".to_string(),
            uuid: "output-0".to_string(),
            bias: 0.0,
            squash: None,
        }],
        synapses: vec![SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: 1.0,
            synapse_type: None,
        }],
        semantic_version: None,
        forward_only: false,
    };

    let serialised = creature_to_json(&creature).expect("serialise");
    assert!(!serialised.contains("semanticVersion"));
    assert!(!serialised.contains("\"squash\""));
    // The only "type" field that remains is the neuron type.
    assert_eq!(serialised.matches("\"type\"").count(), 1);

    let reparsed = parse_creature_json(&serialised).expect("re-parse");
    assert_creature_equal(&creature, &reparsed);
}
