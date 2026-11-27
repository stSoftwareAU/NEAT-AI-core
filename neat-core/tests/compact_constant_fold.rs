//! Exact constant folding (Issue #777), graded against the frozen captures.
//!
//! A disconnected constant is not this pass's to remove: it has no outbound
//! edge, and [`cleanup_creature`](neat_core::cleanup_creature) already owns
//! that cascade. A typed edge into an IF is left in place.

use neat_core::{CreatureExport, compile_creature, fold_constants};
use serde::Deserialize;

#[derive(Deserialize)]
struct Capture {
    name: String,
    before: CreatureExport,
    after: Option<CreatureExport>,
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

fn case(name: &str) -> Capture {
    captures()
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing capture {name}"))
}

fn bias_of(creature: &CreatureExport, uuid: &str) -> f64 {
    creature
        .neurons
        .iter()
        .find(|neuron| neuron.uuid == uuid)
        .unwrap_or_else(|| panic!("missing {uuid}"))
        .bias
}

fn assert_outputs(creature: &CreatureExport, case: &Capture) {
    let mut network = compile_creature(creature).unwrap();
    for (probe, expected) in case.probes.iter().zip(&case.outputs) {
        let actual = network.activate(probe, creature.output);
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= case.activation_tolerance,
                "{}: {actual} != {expected}",
                case.name
            );
        }
    }
}

#[test]
fn a_constant_chain_folds_into_the_output_bias() {
    let case = case("constant_transitive_fold");
    let folded = fold_constants(&case.before);
    assert_eq!(folded.removed_neurons, 2);
    assert_eq!(folded.creature.neurons.len(), 1);
    assert_eq!(folded.creature.neurons[0].uuid, "output-0");
    assert_eq!(folded.creature.synapses.len(), 1);
    assert_eq!(folded.creature.synapses[0].weight, 0.6);
    // logistic(0.2 + 0.5 * 0.5) * 0.5 + 0.1, at the forward pass's f32 squash.
    let expected = case.after.as_ref().expect("the capture folded the chain");
    assert!((bias_of(&folded.creature, "output-0") - expected.neurons[0].bias).abs() < 1e-6);
    assert_outputs(&folded.creature, &case);
}

#[test]
fn a_varying_input_keeps_the_hidden_neuron_and_absorbs_the_constant() {
    let case = case("constant_with_varying_input");
    let folded = fold_constants(&case.before);
    assert_eq!(folded.removed_neurons, 1);
    assert!(
        folded
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == "hidden-h")
    );
    assert!(
        !folded
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == "const-c")
    );
    // 0.2 + 0.5 * 0.4 = 0.4
    assert_eq!(bias_of(&folded.creature, "hidden-h"), 0.4);
    assert_eq!(bias_of(&folded.creature, "output-0"), 0.1);
    assert_eq!(folded.creature.synapses.len(), 2);
    assert_outputs(&folded.creature, &case);
}

#[test]
fn a_constant_into_a_varying_hard_tanh_adds_its_product_to_the_bias() {
    let case = case("constant_fold_full");
    let folded = fold_constants(&case.before);
    assert_eq!(folded.removed_neurons, 1);
    let hidden = "533d8616-037c-4278-b95c-3a2a1ce15ee6";
    // 0.2 + 0.5 * 0.5 = 0.45
    assert_eq!(bias_of(&folded.creature, hidden), 0.45);
    assert!(
        !folded
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.neuron_type == "constant")
    );
    assert_outputs(&folded.creature, &case);
}

#[test]
fn a_typed_edge_into_an_if_stays_and_the_plain_edge_folds() {
    let case = case("constant_fold_partial_if");
    let folded = fold_constants(&case.before);
    assert_eq!(
        folded.removed_neurons, 0,
        "the typed IF edge still reads it"
    );
    // 0.1 + 0.4 * 0.5, which is the f64 value the frozen capture stores.
    assert_eq!(bias_of(&folded.creature, "nonif-consumer"), 0.1 + 0.4 * 0.5);
    assert_eq!(bias_of(&folded.creature, "neuron-132866057"), 0.5);
    let positive = folded
        .creature
        .synapses
        .iter()
        .find(|synapse| synapse.synapse_type.as_deref() == Some("positive"))
        .expect("the IF role stays");
    assert_eq!(positive.weight, 0.3);
    assert_eq!(positive.from_uuid, "neuron-132866057");
}

#[test]
fn a_disconnected_constant_is_left_for_cleanup() {
    let case = case("constant_disconnected");
    let folded = fold_constants(&case.before);
    assert!(!folded.changed());
    assert!(
        folded
            .creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == "unused-const")
    );
    assert_eq!(folded.creature, case.before);
}

#[test]
fn an_output_keeps_one_inbound_when_the_constant_is_its_only_source() {
    let mut original = case("constant_disconnected").before;
    original.neurons.insert(
        0,
        neat_core::NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: "only-source".to_string(),
            bias: 0.5,
            squash: None,
        },
    );
    original.synapses = vec![neat_core::SynapseExport {
        from_uuid: "only-source".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 2.0,
        synapse_type: None,
    }];
    let folded = fold_constants(&original);
    assert!(!folded.changed());
    assert_eq!(folded.creature.synapses.len(), 1);
    assert_eq!(bias_of(&folded.creature, "output-0"), 0.0);
}
