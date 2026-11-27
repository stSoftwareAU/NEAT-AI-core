//! Native replay of the frozen `mergeParallelBridges` captures (Issue #777),
//! plus hand-derived tests for the two deliberate, stricter departures
//! (`MEAN` and unparsable targets) and the failure and memetic contracts.
//!
//! The oracle is the TypeScript output frozen at revision
//! `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`, compared bit-exactly.

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightRowExport, MemeticWeights};
use neat_core::{
    CreatureExport, NeuronExport, ParallelBridgeMergeError, SynapseExport, merge_parallel_bridges,
    parse_creature_json,
};
use serde_json::Value;

fn golden(name: &str) -> Vec<Value> {
    let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).expect("golden file is a JSON array")
}

/// Convert a captured creature (which may carry only runtime ids, tags and
/// `frozen`) to the core wire form.
fn to_core(case: &str, side: &str, value: &Value) -> CreatureExport {
    let input = value["input"].as_i64().expect("input width");
    // Some captures identify neurons by runtime id only; core is UUID-keyed,
    // so give those a stable label derived from the id (same on both sides).
    let mut value = value.clone();
    for neuron in value["neurons"].as_array_mut().expect("neurons") {
        if neuron.get("uuid").is_none() {
            let id = neuron["id"].as_i64().expect("uuid or id");
            neuron["uuid"] = Value::String(format!("id-{id}"));
        }
    }
    let neurons = value["neurons"].as_array().expect("neurons");
    let uuid_of_id = |id: i64| -> String {
        if (0..input).contains(&id) {
            return format!("input-{id}");
        }
        neurons
            .iter()
            .find(|n| n["id"].as_i64() == Some(id))
            .unwrap_or_else(|| panic!("{case} {side}: no neuron with id {id}"))["uuid"]
            .as_str()
            .expect("uuid")
            .to_string()
    };
    let mut wire = value.clone();
    for synapse in wire["synapses"].as_array_mut().expect("synapses") {
        let obj = synapse.as_object_mut().expect("synapse object");
        for (id_key, uuid_key) in [("fromId", "fromUUID"), ("toId", "toUUID")] {
            let id = obj.remove(id_key);
            if !obj.contains_key(uuid_key) {
                let id = id.and_then(|v| v.as_i64()).expect("id or uuid");
                obj.insert(uuid_key.to_string(), Value::String(uuid_of_id(id)));
            }
        }
        obj.remove("tags");
        obj.remove("frozen");
    }
    for neuron in wire["neurons"].as_array_mut().expect("neurons") {
        let obj = neuron.as_object_mut().expect("neuron object");
        obj.remove("tags");
        obj.remove("frozen");
    }
    parse_creature_json(&wire.to_string())
        .unwrap_or_else(|e| panic!("{case} {side}: wire form rejected: {e}"))
}

fn without_ids(mut creature: CreatureExport) -> CreatureExport {
    for neuron in &mut creature.neurons {
        neuron.id = None;
    }
    creature
}

/// Replay one capture; returns nothing, panics naming the case.
fn replay(case: &Value) {
    let name = case["name"].as_str().expect("name");
    let before = to_core(name, "before", &case["before"]);
    let after = without_ids(to_core(name, "after", &case["after"]));
    let merged = merge_parallel_bridges(&before)
        .unwrap_or_else(|e| panic!("{name}: unexpected refusal to finish: {e}"));
    let expected_removed = case["result"]["removedNeurons"].as_u64().expect("removed") as usize;
    assert_eq!(merged.removed_neurons, expected_removed, "{name}: removed");
    let got = without_ids(merged.creature);

    assert_eq!(
        got.neurons.len(),
        after.neurons.len(),
        "{name}: neuron count"
    );
    for (i, (g, e)) in got.neurons.iter().zip(&after.neurons).enumerate() {
        assert_eq!(g.neuron_type, e.neuron_type, "{name}: neuron {i} type");
        assert_eq!(g.uuid, e.uuid, "{name}: neuron {i} uuid");
        assert_eq!(g.bias, e.bias, "{name}: neuron {i} bias");
        assert_eq!(g.squash, e.squash, "{name}: neuron {i} squash");
    }
    assert_eq!(
        got.synapses.len(),
        after.synapses.len(),
        "{name}: synapse count"
    );
    for (i, (g, e)) in got.synapses.iter().zip(&after.synapses).enumerate() {
        assert_eq!(g.from_uuid, e.from_uuid, "{name}: synapse {i} from");
        assert_eq!(g.to_uuid, e.to_uuid, "{name}: synapse {i} to");
        assert_eq!(g.weight, e.weight, "{name}: synapse {i} weight");
        assert_eq!(g.synapse_type, e.synapse_type, "{name}: synapse {i} type");
    }
}

fn merge_cases(file: &str) -> Vec<Value> {
    golden(file)
        .into_iter()
        .filter(|c| {
            c["operation"].as_str().unwrap_or("mergeParallelBridges") == "mergeParallelBridges"
        })
        .collect()
}

#[test]
fn legacy_bridge_captures_replay_bit_exactly() {
    let cases = golden("compact_legacy_bridges");
    assert_eq!(cases.len(), 4, "the capture holds four bridge records");
    for case in &cases {
        assert_eq!(case["source"], "src/compact/ParallelBridgeMerge.ts");
        replay(case);
    }
}

#[test]
fn bridge_case_captures_replay_bit_exactly() {
    let all = golden("compact_legacy_bridge_cases");
    let cases = merge_cases("compact_legacy_bridge_cases");
    let compact_only = all
        .iter()
        .filter(|c| c["operation"] == "compactCreature")
        .count();
    assert_eq!(
        cases.len(),
        all.len() - compact_only,
        "every non-compactCreature case is a mergeParallelBridges replay"
    );
    let names: Vec<&str> = cases.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(!names.is_empty());
    for expected in ["complement_three", "duplicate_sources"] {
        assert!(names.contains(&expected), "{expected} must be replayed");
    }
    for case in &cases {
        replay(case);
    }
}

#[test]
fn tag_captures_replay_topology_bit_exactly() {
    let all = golden("compact_legacy_tags");
    let cases = merge_cases("compact_legacy_tags");
    let expected = all
        .iter()
        .filter(|c| c["operation"] == "mergeParallelBridges")
        .count();
    assert_eq!(cases.len(), expected);
    assert!(expected > 0, "the tag capture holds merge cases");
    for case in &cases {
        replay(case);
    }
}

// ---- hand-written fixtures -------------------------------------------------

fn neuron(kind: &str, uuid: &str, bias: f64, squash: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
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

/// Two IDENTITY bridges (inputs 0 and 1) into one target with `target_squash`.
fn two_bridges(target_squash: &str, out_w: f64, in_w: f64) -> CreatureExport {
    CreatureExport {
        input: 2,
        output: 1,
        neurons: vec![
            neuron("hidden", "h0", 0.5, "IDENTITY"),
            neuron("hidden", "h1", 0.25, "IDENTITY"),
            neuron("output", "output-0", 0.0, target_squash),
        ],
        synapses: vec![
            edge("input-0", "h0", in_w),
            edge("h0", "output-0", out_w),
            edge("input-1", "h1", in_w),
            edge("h1", "output-0", out_w),
        ],
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

#[test]
fn a_mean_target_is_refused_but_an_identity_target_merges() {
    let mean = two_bridges("MEAN", 2.0, 3.0);
    let refused = merge_parallel_bridges(&mean).unwrap();
    assert_eq!(refused.removed_neurons, 0);
    assert_eq!(refused.creature, mean);

    // Positive control: only the target squash differs.
    let sum = two_bridges("IDENTITY", 2.0, 3.0);
    let merged = merge_parallel_bridges(&sum).unwrap();
    assert_eq!(merged.removed_neurons, 1);
    // Kept h1: bias = 2.0 * 0.25 + 2.0 * 0.5 = 1.5; redirected weight 2.0 * 3.0.
    assert_eq!(merged.creature.neurons[0].uuid, "h1");
    assert_eq!(merged.creature.neurons[0].bias, 1.5);
    assert_eq!(merged.creature.synapses[0].to_uuid, "h1");
    assert_eq!(merged.creature.synapses[0].weight, 6.0);
}

#[test]
fn an_unknown_squash_target_is_refused_but_a_known_one_merges() {
    let unknown = two_bridges("NOT_A_SQUASH", 2.0, 3.0);
    let refused = merge_parallel_bridges(&unknown).unwrap();
    assert_eq!(refused.removed_neurons, 0);
    assert_eq!(refused.creature, unknown);

    let known = two_bridges("TANH", 2.0, 3.0);
    assert_eq!(merge_parallel_bridges(&known).unwrap().removed_neurons, 1);
}

#[test]
fn an_overflowing_merge_names_the_kept_bridge_and_a_finite_one_succeeds() {
    // Kept h1: new inbound weight = 1e200 * 1e200 = inf.
    let overflow = two_bridges("IDENTITY", 1e200, 1e200);
    assert_eq!(
        merge_parallel_bridges(&overflow),
        Err(ParallelBridgeMergeError::NonFiniteWeight {
            neuron_uuid: "h1".to_string()
        })
    );
    let finite = two_bridges("IDENTITY", 1e100, 1e100);
    assert_eq!(merge_parallel_bridges(&finite).unwrap().removed_neurons, 1);
}

#[test]
fn an_overflowing_bias_is_reported_as_a_bias_error() {
    // Out weight 1e200 on bias 1e200: 1e200 * 1e200 = inf, but the inbound
    // weight (1e-200 * 1e200 = 1) stays finite, so the bias is what fails.
    let mut creature = two_bridges("IDENTITY", 1e200, 1e-200);
    creature.neurons[1].bias = 1e200;
    assert_eq!(
        merge_parallel_bridges(&creature),
        Err(ParallelBridgeMergeError::NonFiniteBias {
            neuron_uuid: "h1".to_string()
        })
    );
}

/// Two independent groups: (h0, h1) into o0 and (h2, h3) into o1.
fn two_groups() -> CreatureExport {
    CreatureExport {
        input: 4,
        output: 2,
        neurons: vec![
            neuron("hidden", "h0", 0.0, "IDENTITY"),
            neuron("hidden", "h1", 0.0, "IDENTITY"),
            neuron("hidden", "h2", 0.0, "IDENTITY"),
            neuron("hidden", "h3", 0.0, "IDENTITY"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
            neuron("output", "output-1", 0.0, "IDENTITY"),
        ],
        synapses: vec![
            edge("input-0", "h0", 1.0),
            edge("h0", "output-0", 1.0),
            edge("input-1", "h1", 1.0),
            edge("h1", "output-0", 1.0),
            edge("input-2", "h2", 1.0),
            edge("h2", "output-1", 1.0),
            edge("input-3", "h3", 1.0),
            edge("h3", "output-1", 1.0),
        ],
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

#[test]
fn only_one_group_is_merged_per_call() {
    let start = two_groups();
    let first = merge_parallel_bridges(&start).unwrap();
    assert_eq!(
        first.removed_neurons, 1,
        "one removal, from the first group"
    );
    let uuids: Vec<&str> = first
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids, ["h1", "h2", "h3", "output-0", "output-1"]);

    let second = merge_parallel_bridges(&first.creature).unwrap();
    assert_eq!(
        second.removed_neurons, 1,
        "the other group merges next call"
    );
    let uuids: Vec<&str> = second
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids, ["h1", "h3", "output-0", "output-1"]);

    assert_eq!(
        merge_parallel_bridges(&second.creature)
            .unwrap()
            .removed_neurons,
        0,
        "fixed point"
    );
}

#[test]
fn memetic_rows_for_the_removed_bridge_are_pruned_and_the_record_stays() {
    let mut creature = two_bridges("IDENTITY", 2.0, 3.0);
    let mut extra = serde_json::Map::new();
    extra.insert("generation".into(), serde_json::json!(7));
    creature.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h0".to_string(), 0.1), ("h1".to_string(), 0.2)]),
        weights: MemeticWeights::Rows(vec![MemeticWeightRowExport {
            from_uuid: Some("h0".to_string()),
            to_uuid: Some("output-0".to_string()),
            weight: Some(0.3),
        }]),
        extra,
    });
    let merged = merge_parallel_bridges(&creature).unwrap();
    assert_eq!(merged.removed_neurons, 1);
    let memetic = merged.creature.memetic.expect("the record stays");
    assert_eq!(
        memetic.biases.get("h0"),
        None,
        "removed bridge's bias row goes"
    );
    assert_eq!(memetic.biases.get("h1"), Some(&0.2));
    match memetic.weights {
        MemeticWeights::Rows(rows) => assert!(rows.is_empty(), "removed edge's row goes"),
        MemeticWeights::ById(_) => panic!("fixture is the row form"),
    }
    assert_eq!(memetic.extra.get("generation"), Some(&serde_json::json!(7)));
}

#[test]
fn the_input_creature_is_never_mutated() {
    for creature in [
        two_bridges("IDENTITY", 2.0, 3.0),
        two_bridges("MEAN", 2.0, 3.0),
        two_bridges("IDENTITY", 1e200, 1e200),
        two_groups(),
    ] {
        let snapshot = creature.clone();
        let _ = merge_parallel_bridges(&creature);
        assert_eq!(creature, snapshot);
    }
}
