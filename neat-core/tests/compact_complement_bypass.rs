//! Exact COMPLEMENT bypass (Issue #777).
//!
//! The oracle is the frozen `complement_bypass` capture plus the formula
//! `bias += w_out * (1 - bias)` and `w' = -w_out * w_in`. Aggregate targets,
//! a second call, a refused backward fold, and role matching are graded
//! against that same formula.

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightRowExport, MemeticWeights};
use neat_core::{CreatureExport, NeuronExport, SynapseExport, bypass_complement, compile_creature};
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

fn creature(
    input: usize,
    neurons: Vec<NeuronExport>,
    synapses: Vec<SynapseExport>,
) -> CreatureExport {
    let output = neurons
        .iter()
        .filter(|neuron| neuron.neuron_type == "output")
        .count();
    CreatureExport {
        input,
        output,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: false,
        memetic: None,
    }
}

#[derive(Deserialize)]
struct Capture {
    name: String,
    before: CreatureExport,
    after: Option<CreatureExport>,
    #[serde(rename = "feedbackLoop", default)]
    feedback_loop: bool,
    probes: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
    #[serde(rename = "activationTolerance")]
    activation_tolerance: f32,
}

fn captures() -> Vec<Capture> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn complement_bypass_matches_the_frozen_fold_and_its_outputs() {
    let case = captures()
        .into_iter()
        .find(|case| case.name == "complement_bypass")
        .expect("frozen complement case");
    let folded = bypass_complement(&case.before, case.feedback_loop);
    assert_eq!(folded.removed_neuron.as_deref(), Some("hidden-0"));
    let expected = case.after.expect("the capture produced a creature");
    assert_eq!(folded.creature.neurons.len(), 1);
    assert_eq!(folded.creature.neurons[0].uuid, "output-0");
    assert_eq!(folded.creature.neurons[0].bias, expected.neurons[0].bias);
    // 0.1 + 1.5 * (1 - 0.25) = 1.225
    assert_eq!(folded.creature.neurons[0].bias, 1.225);
    let mut weights: Vec<(&str, f64)> = folded
        .creature
        .synapses
        .iter()
        .map(|synapse| (synapse.from_uuid.as_str(), synapse.weight))
        .collect();
    weights.sort_by(|left, right| left.0.cmp(right.0));
    // input-0: -(1.5 * 0.5) = -0.75
    // input-1: 0.25 + -(1.5 * -2) = 3.25
    assert_eq!(weights, vec![("input-0", -0.75), ("input-1", 3.25)]);

    let mut network = compile_creature(&folded.creature).unwrap();
    for (probe, expected_output) in case.probes.iter().zip(&case.outputs) {
        let actual = network.activate(probe, folded.creature.output);
        for (actual, expected_output) in actual.iter().zip(expected_output) {
            assert!((actual - expected_output).abs() <= case.activation_tolerance);
        }
    }

    let again = bypass_complement(&folded.creature, case.feedback_loop);
    assert!(!again.changed());
    assert_eq!(again.creature, folded.creature);
}

#[test]
fn an_aggregate_target_keeps_the_complement() {
    let case = captures()
        .into_iter()
        .find(|case| case.name == "aggregate_guard")
        .expect("frozen aggregate guard");
    assert!(case.after.is_none());
    let folded = bypass_complement(&case.before, case.feedback_loop);
    assert!(!folded.changed());
    assert_eq!(folded.creature, case.before);
}

#[test]
fn a_typed_inbound_merges_only_into_the_same_role() {
    let original = creature(
        1,
        vec![
            neuron("hidden", "hidden-0", 0.0, "COMPLEMENT"),
            neuron("output", "output-0", 0.5, "IDENTITY"),
        ],
        vec![
            typed("input-0", "hidden-0", 2.0, "positive"),
            typed("input-0", "output-0", 0.5, "positive"),
            edge("input-0", "output-0", 0.25),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    let folded = bypass_complement(&original, false);
    assert!(folded.changed());
    // bias: 0.5 + 1 * (1 - 0) = 1.5
    assert_eq!(folded.creature.neurons[0].bias, 1.5);
    let positive = folded
        .creature
        .synapses
        .iter()
        .find(|synapse| synapse.synapse_type.as_deref() == Some("positive"))
        .expect("the positive edge stays");
    // 0.5 + -(1 * 2) = -1.5, role kept
    assert_eq!(positive.weight, -1.5);
    let untyped = folded
        .creature
        .synapses
        .iter()
        .find(|synapse| synapse.synapse_type.is_none())
        .expect("the untyped edge is a different role");
    assert_eq!(untyped.weight, 0.25);
}

#[test]
fn a_forward_only_backward_edge_is_not_folded_and_the_neuron_still_goes() {
    let mut original = creature(
        1,
        vec![
            neuron("output", "output-0", 0.4, "IDENTITY"),
            neuron("hidden", "hidden-c", 0.0, "COMPLEMENT"),
            neuron("hidden", "hidden-src", 0.0, "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-src", 1.0),
            edge("hidden-src", "hidden-c", 2.0),
            edge("hidden-c", "output-0", 3.0),
        ],
    );
    original.forward_only = true;
    let folded = bypass_complement(&original, false);
    assert_eq!(folded.removed_neuron.as_deref(), Some("hidden-c"));
    assert_eq!(folded.creature.neurons[0].bias, 0.4, "the fold was refused");
    assert!(
        folded
            .creature
            .synapses
            .iter()
            .all(|synapse| synapse.from_uuid != "hidden-c" && synapse.to_uuid != "hidden-c")
    );
    assert!(
        !folded
            .creature
            .synapses
            .iter()
            .any(|synapse| synapse.weight == -6.0),
        "2 * -3 was not written"
    );
}

#[test]
fn only_the_first_complement_is_removed() {
    let original = creature(
        1,
        vec![
            neuron("hidden", "hidden-a", 0.0, "COMPLEMENT"),
            neuron("hidden", "hidden-b", 0.0, "COMPLEMENT"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-a", 1.0),
            edge("hidden-a", "output-0", 1.0),
            edge("input-0", "hidden-b", 1.0),
            edge("hidden-b", "output-0", 1.0),
        ],
    );
    let folded = bypass_complement(&original, true);
    assert_eq!(folded.removed_neuron.as_deref(), Some("hidden-a"));
    assert!(
        folded
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == "hidden-b")
    );
}

#[test]
fn a_memetic_row_for_the_removed_edge_is_dropped() {
    let mut original = creature(
        1,
        vec![
            neuron("hidden", "hidden-0", 0.0, "COMPLEMENT"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    original.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("output-0".to_string(), 0.01)]),
        weights: MemeticWeights::Rows(vec![MemeticWeightRowExport {
            from_uuid: Some("hidden-0".to_string()),
            to_uuid: Some("output-0".to_string()),
            weight: Some(0.2),
        }]),
        extra: serde_json::Map::new(),
    });
    let folded = bypass_complement(&original, false);
    let memetic = folded.creature.memetic.expect("the record stays");
    assert_eq!(memetic.biases.get("output-0"), Some(&0.01));
    match memetic.weights {
        MemeticWeights::Rows(rows) => assert!(rows.is_empty()),
        other => panic!("unexpected weight form: {other:?}"),
    }
}
