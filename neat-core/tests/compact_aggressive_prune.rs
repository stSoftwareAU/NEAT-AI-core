//! Aggressive prune (Issue #778), graded against the frozen TypeScript captures
//! and against the TypeScript guards. The pass removes synapses only; neurons
//! stay for [`cleanup_creature`](neat_core::cleanup_creature).

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightRowExport, MemeticWeights};
use neat_core::{
    AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, AggressivePruneError, CreatureExport, NeuronExport,
    SynapseExport, aggressive_prune,
};

const T: f64 = AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD;

fn neuron(neuron_type: &str, uuid: &str, squash: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias: 0.1,
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
    let output = neurons.iter().filter(|n| n.neuron_type == "output").count();
    CreatureExport {
        input: 3,
        output,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

fn output_with(synapses: Vec<SynapseExport>) -> CreatureExport {
    creature(vec![neuron("output", "output-0", "IDENTITY")], synapses)
}

fn weights(c: &CreatureExport) -> Vec<f64> {
    c.synapses.iter().map(|s| s.weight).collect()
}

#[test]
fn frozen_captures_replay_with_the_same_survivors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy_variants.json"
    );
    let records: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut replayed = 0;
    for name in [
        "aggressive_maximum",
        "aggressive_frozen",
        "aggressive_last_input",
    ] {
        let record = records
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("capture {name}"));
        let input: CreatureExport = serde_json::from_value(record["input"].clone()).unwrap();
        let frozen: Vec<usize> = record["input"]["synapses"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, s)| s["frozen"] == true)
            .map(|(i, _)| i)
            .collect();
        let threshold = record["threshold"].as_f64().unwrap();

        let pruned = aggressive_prune(&input, threshold, &frozen).unwrap();

        assert_eq!(
            pruned.removed_synapses as u64,
            record["result"]["removedSynapses"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            pruned.changed(),
            record["result"]["changed"].as_bool().unwrap(),
            "{name}"
        );
        let expected: Vec<(String, String, f64)> = record["after"]["synapses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["fromUUID"].as_str().unwrap().to_string(),
                    s["toUUID"].as_str().unwrap().to_string(),
                    s["weight"].as_f64().unwrap(),
                )
            })
            .collect();
        let actual: Vec<(String, String, f64)> = pruned
            .creature
            .synapses
            .iter()
            .map(|s| (s.from_uuid.clone(), s.to_uuid.clone(), s.weight))
            .collect();
        assert_eq!(actual, expected, "{name}");
        let uuids: Vec<&str> = record["after"]["neurons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["uuid"].as_str().unwrap())
            .collect();
        let got: Vec<&str> = pruned
            .creature
            .neurons
            .iter()
            .map(|n| n.uuid.as_str())
            .collect();
        assert_eq!(got, uuids, "{name}");
        replayed += 1;
    }
    assert_eq!(replayed, 3);
}

#[test]
fn an_if_output_keeps_every_typed_tiny_edge() {
    let original = creature(
        vec![neuron("output", "output-0", "IF")],
        vec![
            typed("input-0", "output-0", 1e-9, "condition"),
            typed("input-1", "output-0", 1e-9, "positive"),
            typed("input-2", "output-0", 1e-9, "negative"),
        ],
    );
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert!(!pruned.changed());
    assert_eq!(pruned.creature, original);
}

#[test]
fn a_tiny_untyped_edge_into_an_if_target_stays() {
    let original = creature(
        vec![
            neuron("hidden", "hidden-if", "IF"),
            neuron("output", "output-0", "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-if", 1e-9),
            edge("hidden-if", "output-0", 1.0),
        ],
    );
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 0);
    assert_eq!(pruned.creature, original);
}

#[test]
fn an_empty_role_is_still_a_role_and_pins_the_edge() {
    let mut empty = edge("input-1", "output-0", 1e-9);
    empty.synapse_type = Some(String::new());
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        empty,
        edge("input-2", "output-0", 1e-9),
    ]);
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(weights(&pruned.creature), vec![1.0, 1e-9]);
    assert_eq!(
        pruned.creature.synapses[1].synapse_type.as_deref(),
        Some("")
    );
    assert_eq!(pruned.creature.synapses[1].from_uuid, "input-1");
}

#[test]
fn a_plain_tiny_edge_with_a_strong_sibling_goes() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-1", "output-0", 1e-9),
    ]);
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(weights(&pruned.creature), vec![1.0]);
}

#[test]
fn tiny_edges_into_aggregate_hidden_neurons_are_removed() {
    for squash in ["MINIMUM", "HYPOT"] {
        let original = creature(
            vec![
                neuron("hidden", "hidden-a", squash),
                neuron("output", "output-0", "IDENTITY"),
            ],
            vec![
                edge("input-0", "hidden-a", 0.8),
                edge("input-1", "hidden-a", 1e-9),
                edge("hidden-a", "output-0", 1.0),
            ],
        );
        let pruned = aggressive_prune(&original, T, &[]).unwrap();
        assert_eq!(pruned.removed_synapses, 1, "{squash}");
        assert_eq!(weights(&pruned.creature), vec![0.8, 1.0], "{squash}");
    }
}

#[test]
fn a_weight_equal_to_the_threshold_stays_and_one_just_below_goes() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-1", "output-0", 1e-3),
        edge("input-2", "output-0", 0.000_999_999),
    ]);
    let pruned = aggressive_prune(&original, 1e-3, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(weights(&pruned.creature), vec![1.0, 1e-3]);
}

#[test]
fn negative_weights_are_judged_by_magnitude() {
    let original = output_with(vec![
        edge("input-0", "output-0", -1.0),
        edge("input-1", "output-0", -1e-9),
        edge("input-2", "output-0", -0.5),
    ]);
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(weights(&pruned.creature), vec![-1.0, -0.5]);
}

#[test]
fn non_finite_weights_are_kept() {
    let original = output_with(vec![
        edge("input-0", "output-0", f64::NAN),
        edge("input-1", "output-0", f64::INFINITY),
        edge("input-2", "output-0", f64::NEG_INFINITY),
    ]);
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 0);
    assert!(!pruned.changed());
    assert_eq!(pruned.creature.synapses.len(), 3);
    assert!(pruned.creature.synapses[0].weight.is_nan());
    assert_eq!(pruned.creature.synapses[1].weight, f64::INFINITY);
    assert_eq!(pruned.creature.synapses[2].weight, f64::NEG_INFINITY);
}

#[test]
fn a_nan_edge_does_not_count_as_the_outputs_kept_inbound() {
    let original = output_with(vec![
        edge("input-0", "output-0", f64::NAN),
        edge("input-1", "output-0", 1e-9),
        edge("input-2", "output-0", 2e-9),
    ]);
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(pruned.creature.synapses.len(), 2);
    assert!(pruned.creature.synapses[0].weight.is_nan());
    assert_eq!(pruned.creature.synapses[1].weight, 1e-9);
}

#[test]
fn each_output_with_only_tiny_edges_keeps_exactly_its_first() {
    let original = creature(
        vec![
            neuron("output", "output-0", "IDENTITY"),
            neuron("output", "output-1", "IDENTITY"),
        ],
        vec![
            edge("input-0", "output-0", 1e-9),
            edge("input-0", "output-1", 3e-9),
            edge("input-1", "output-0", 2e-9),
            edge("input-1", "output-1", 4e-9),
        ],
    );
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 2);
    assert_eq!(weights(&pruned.creature), vec![1e-9, 3e-9]);
}

#[test]
fn a_frozen_index_out_of_range_is_refused_and_in_range_keeps_the_edge() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-1", "output-0", 1e-9),
    ]);
    assert_eq!(
        aggressive_prune(&original, T, &[0, 2]),
        Err(AggressivePruneError::FrozenIndexOutOfRange {
            index: 2,
            synapses: 2
        })
    );
    let pruned = aggressive_prune(&original, T, &[1, 1]).unwrap();
    assert!(!pruned.changed());
    assert_eq!(pruned.creature, original);
}

#[test]
fn a_nan_threshold_is_refused() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-1", "output-0", 1e-9),
    ]);
    assert_eq!(
        aggressive_prune(&original, f64::NAN, &[]),
        Err(AggressivePruneError::NanThreshold)
    );
    assert_eq!(
        aggressive_prune(&original, 1e-3, &[])
            .unwrap()
            .removed_synapses,
        1
    );
}

#[test]
fn memetic_rows_for_a_removed_edge_go_and_the_record_stays() {
    let mut original = creature(
        vec![
            neuron("hidden", "hidden-h", "TANH"),
            neuron("output", "output-0", "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-h", 1.0),
            edge("hidden-h", "output-0", 1e-9),
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
                weight: Some(0.03),
            },
        ]),
        extra,
    });

    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    let memetic = pruned.creature.memetic.as_ref().expect("record stays");
    assert_eq!(memetic.biases.get("output-0"), Some(&0.01));
    assert_eq!(memetic.extra.get("generation"), Some(&serde_json::json!(4)));
    match &memetic.weights {
        MemeticWeights::Rows(rows) => {
            assert_eq!(rows.len(), 1);
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
    let pruned = aggressive_prune(&no_op, T, &[]).unwrap();
    assert!(!pruned.changed());
    assert_eq!(pruned.creature, no_op);
}

#[test]
fn a_hidden_neuron_loses_its_tiny_outward_edge_but_stays_listed() {
    let original = creature(
        vec![
            neuron("hidden", "hidden-h", "TANH"),
            neuron("output", "output-0", "IDENTITY"),
        ],
        vec![
            edge("input-0", "hidden-h", 0.5),
            edge("hidden-h", "output-0", 1e-9),
            edge("input-1", "output-0", 1.0),
        ],
    );
    let pruned = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(pruned.removed_synapses, 1);
    assert_eq!(pruned.creature.neurons, original.neurons);
    assert!(
        pruned
            .creature
            .synapses
            .iter()
            .all(|s| s.from_uuid != "hidden-h")
    );
}

#[test]
fn the_input_creature_is_left_unchanged() {
    let original = output_with(vec![
        edge("input-0", "output-0", 1.0),
        edge("input-1", "output-0", 1e-9),
    ]);
    let snapshot = original.clone();
    let _ = aggressive_prune(&original, T, &[]).unwrap();
    assert_eq!(original, snapshot);
}
