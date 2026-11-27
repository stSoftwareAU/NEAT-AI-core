//! Exact zero-weight pruning (Issue #777), graded against the frozen
//! `zero_weight_orphan` capture and against the TypeScript guards.
//!
//! The primitive removes synapses only. Stranded neurons are then handed to
//! [`cleanup_creature`](neat_core::cleanup_creature), which already owns the
//! dead-neuron cascade. Nothing here reimplements that walk.

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightRowExport, MemeticWeights};
use neat_core::{
    CreatureExport, NeuronExport, SynapseExport, ValidateOptions, cleanup_creature,
    compile_creature, creature_validate, prune_zero_weight_synapses,
};
use serde::Deserialize;

fn neuron(neuron_type: &str, uuid: &str, bias: f64, squash: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias,
        squash: Some(squash.to_string()),
    }
}

fn edge(from: &str, to: &str, weight: f64) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: None,
    }
}

fn typed(from: &str, to: &str, weight: f64, role: &str) -> SynapseExport {
    let mut synapse = edge(from, to, weight);
    synapse.synapse_type = Some(role.to_string());
    synapse
}

fn creature(neurons: Vec<NeuronExport>, synapses: Vec<SynapseExport>) -> CreatureExport {
    let output = neurons
        .iter()
        .filter(|neuron| neuron.neuron_type == "output")
        .count();
    CreatureExport {
        input: 1,
        output,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

fn output_with(synapses: Vec<SynapseExport>) -> CreatureExport {
    creature(
        vec![neuron("output", "output-0", 0.1, "IDENTITY")],
        synapses,
    )
}

#[test]
fn exact_zero_beside_a_real_edge_is_removed_and_the_rest_stays() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.25),
        edge("input-0", "output-0", 0.0),
    ]);
    let pruned = prune_zero_weight_synapses(&original);
    assert_eq!(pruned.removed_zero_synapses, 1);
    assert_eq!(pruned.repaired_non_finite, 0);
    assert!(pruned.changed());
    assert_eq!(
        pruned.creature.synapses,
        vec![edge("input-0", "output-0", 1.25)]
    );
    assert_eq!(pruned.creature.neurons, original.neurons);

    let again = prune_zero_weight_synapses(&pruned.creature);
    assert!(!again.changed());
    assert_eq!(again.creature, pruned.creature);
}

#[test]
fn negative_zero_is_exact_zero_and_a_tiny_weight_is_not() {
    let negative_zero = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-0", "output-0", -0.0),
    ]);
    assert_eq!(
        prune_zero_weight_synapses(&negative_zero).removed_zero_synapses,
        1
    );

    let tiny = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-0", "output-0", 1e-12),
    ]);
    let pruned = prune_zero_weight_synapses(&tiny);
    assert!(!pruned.changed());
    assert_eq!(pruned.creature, tiny);
}

#[test]
fn a_role_and_an_if_target_keep_an_exact_zero() {
    let guarded = creature(
        vec![
            neuron("hidden", "hidden-if", 0.0, "IF"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        vec![
            typed("input-0", "hidden-if", 0.0, "condition"),
            typed("input-0", "hidden-if", 1.0, "positive"),
            typed("input-0", "hidden-if", -1.0, "negative"),
            edge("input-0", "hidden-if", 0.0),
            typed("input-0", "output-0", 0.0, "positive"),
            edge("hidden-if", "output-0", 1.0),
        ],
    );
    let pruned = prune_zero_weight_synapses(&guarded);
    assert!(
        !pruned.changed(),
        "typed zeros and an untyped zero into IF stay"
    );
    assert_eq!(pruned.creature, guarded);
}

#[test]
fn an_empty_role_is_untyped_and_the_zero_goes() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: 0.0,
            synapse_type: Some(String::new()),
        },
    ]);
    assert_eq!(
        prune_zero_weight_synapses(&original).removed_zero_synapses,
        1
    );
}

#[test]
fn the_last_inbound_zero_of_an_output_stays_and_a_second_one_goes() {
    let only = output_with(vec![edge("input-0", "output-0", 0.0)]);
    let pruned = prune_zero_weight_synapses(&only);
    assert!(!pruned.changed(), "the sole output inbound is structural");
    assert_eq!(pruned.creature, only);

    let two = output_with(vec![
        edge("input-0", "output-0", 0.0),
        edge("input-0", "output-0", -0.0),
    ]);
    let pruned = prune_zero_weight_synapses(&two);
    assert_eq!(pruned.removed_zero_synapses, 1);
    assert_eq!(pruned.creature.synapses.len(), 1);
    assert_eq!(pruned.creature.synapses[0].weight, 0.0);
}

#[test]
fn a_typed_zero_counts_as_kept_so_the_untyped_zero_can_go() {
    let original = output_with(vec![
        typed("input-0", "output-0", 0.0, "positive"),
        edge("input-0", "output-0", 0.0),
    ]);
    let pruned = prune_zero_weight_synapses(&original);
    assert_eq!(pruned.removed_zero_synapses, 1);
    assert_eq!(pruned.creature.synapses.len(), 1);
    assert_eq!(
        pruned.creature.synapses[0].synapse_type.as_deref(),
        Some("positive")
    );
}

#[test]
fn non_finite_weights_are_the_repair_branch_and_skip_the_output_guard() {
    let original = output_with(vec![
        edge("input-0", "output-0", f64::INFINITY),
        typed("input-0", "output-0", f64::NAN, "condition"),
    ]);
    let pruned = prune_zero_weight_synapses(&original);
    assert_eq!(pruned.removed_zero_synapses, 0);
    assert_eq!(pruned.repaired_non_finite, 2);
    assert!(pruned.creature.synapses.is_empty());
}

#[test]
fn a_zero_into_a_hidden_is_removed_without_deleting_the_hidden() {
    let original = creature(
        vec![
            neuron("hidden", "hidden-h", 0.2, "TANH"),
            neuron("output", "output-0", 0.1, "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-h", -0.5),
            edge("input-0", "output-0", 1.25),
            edge("hidden-h", "output-0", 0.0),
        ],
    );
    let pruned = prune_zero_weight_synapses(&original);
    assert_eq!(pruned.removed_zero_synapses, 1);
    assert_eq!(
        pruned.creature.neurons.len(),
        2,
        "dead cascade is cleanup's"
    );
    assert!(
        pruned
            .creature
            .synapses
            .iter()
            .all(|synapse| synapse.weight != 0.0)
    );
}

#[test]
fn memetic_rows_for_a_removed_edge_go_and_the_record_stays() {
    let mut original = creature(
        vec![
            neuron("hidden", "hidden-h", 0.2, "TANH"),
            neuron("output", "output-0", 0.1, "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-h", 1.0),
            edge("hidden-h", "output-0", 0.0),
            edge("input-0", "output-0", 1.25),
        ],
    );
    let mut extra = serde_json::Map::new();
    extra.insert("generation".into(), serde_json::json!(4));
    original.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("output-0".to_string(), 0.01)]),
        weights: MemeticWeights::Rows(vec![
            MemeticWeightRowExport {
                from_uuid: Some("input-0".to_string()),
                to_uuid: Some("output-0".to_string()),
                weight: Some(0.02),
            },
            MemeticWeightRowExport {
                from_uuid: Some("hidden-h".to_string()),
                to_uuid: Some("output-0".to_string()),
                weight: Some(0.0),
            },
        ]),
        extra,
    });

    let pruned = prune_zero_weight_synapses(&original);
    let memetic = pruned
        .creature
        .memetic
        .as_ref()
        .expect("the record stays; only dangling rows go");
    assert_eq!(memetic.biases.get("output-0"), Some(&0.01));
    assert_eq!(
        memetic.biases.get("hidden-h"),
        None,
        "no bias was recorded for the hidden neuron"
    );
    assert_eq!(memetic.extra.get("generation"), Some(&serde_json::json!(4)));
    match &memetic.weights {
        MemeticWeights::Rows(rows) => {
            assert_eq!(rows.len(), 1, "the removed hidden→output row goes");
            assert_eq!(rows[0].from_uuid.as_deref(), Some("input-0"));
            assert_eq!(rows[0].weight, Some(0.02));
        }
        MemeticWeights::ById(_) => panic!("fixture is the row form"),
    }

    let mut no_op = output_with(vec![edge("input-0", "output-0", 1.0)]);
    no_op.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("output-0".to_string(), 0.01)]),
        weights: MemeticWeights::default(),
        extra: serde_json::Map::new(),
    });
    let pruned = prune_zero_weight_synapses(&no_op);
    assert!(!pruned.changed());
    assert_eq!(pruned.creature, no_op);
}

#[derive(Deserialize)]
struct Capture {
    name: String,
    before: CreatureExport,
    probes: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
}

#[test]
fn frozen_zero_weight_orphan_matches_after_cleanup_removes_the_stranded_hidden() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy.json"
    );
    let captures: Vec<Capture> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let case = captures
        .into_iter()
        .find(|capture| capture.name == "zero_weight_orphan")
        .expect("frozen capture");

    let pruned = prune_zero_weight_synapses(&case.before);
    assert_eq!(pruned.removed_zero_synapses, 1);
    assert!(
        pruned
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == "hidden-unused"),
        "the primitive leaves the stranded hidden for cleanup"
    );

    let cleaned = cleanup_creature(&pruned.creature).expect("cleanup accepts the prune");
    assert_eq!(cleaned.creature.neurons.len(), 1);
    assert_eq!(cleaned.creature.neurons[0].uuid, "output-0");
    assert_eq!(cleaned.creature.neurons[0].bias, 0.1);
    assert_eq!(cleaned.creature.synapses.len(), 1);
    assert_eq!(cleaned.creature.synapses[0].from_uuid, "input-0");
    assert_eq!(cleaned.creature.synapses[0].to_uuid, "output-0");
    assert_eq!(cleaned.creature.synapses[0].weight, 1.25);
    assert!(
        cleaned
            .removed_neurons
            .iter()
            .any(|uuid| uuid == "hidden-unused")
    );

    creature_validate(
        &cleaned.creature,
        &ValidateOptions {
            neurons: None,
            connections: None,
            feedback_loop: None,
            forward_only: true,
        },
    )
    .expect("cleaned creature validates");

    let mut net = compile_creature(&cleaned.creature).unwrap();
    for (probe, expected) in case.probes.iter().zip(&case.outputs) {
        let actual = net.activate(probe, cleaned.creature.output);
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 1e-6,
                "{probe:?}: {actual} != {expected}"
            );
        }
    }
}
