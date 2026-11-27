//! Frozen TypeScript parallel-bridge case observations (#776). Native replay
//! of the `mergeParallelBridges` cases lives in
//! `neat-core/tests/compact_parallel_bridge.rs` (#777).
use serde_json::{Value, json};

const REVISION: &str = "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab";

#[test]
fn bridge_case_captures_preserve_original_contracts() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_bridge_cases.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let names = [
        "complement_pair",
        "complement_weights",
        "mixed_squash",
        "logistic_refusal",
        "tanh_refusal",
        "relu_refusal",
        "absolute_refusal",
        "maximum_neuron_refusal",
        "complement_three",
        "identity_still_merges",
        "uuid_only",
        "typed_condition",
        "typed_positive",
        "kept_source",
        "duplicate_sources",
        "post_prune_shape",
        "complement_typed",
        "identity_integrity",
        "complement_integrity",
        "if_target_declined",
        "maximum_target_declined",
        "backward_source_left",
        "keep_last_bridge",
        "identity_compact",
        "complement_compact",
    ];
    assert_eq!(cases.len(), names.len());
    for (case, name) in cases.iter().zip(names) {
        assert_eq!(case["name"], name);
        assert_eq!(case["revision"], REVISION);
        assert!(case["test"].as_str().unwrap().starts_with("test/compact/"));
        endpoints_resolve(&case["after"]);
    }

    for name in [
        "mixed_squash",
        "logistic_refusal",
        "tanh_refusal",
        "relu_refusal",
        "absolute_refusal",
        "maximum_neuron_refusal",
        "typed_condition",
        "typed_positive",
        "duplicate_sources",
        "complement_typed",
    ] {
        let case = find(&cases, name);
        assert_eq!(case["result"], json!({"removedNeurons": 0}));
        assert_eq!(case["preparation"], "none");
        assert_eq!(case["after"], case["before"]);
        assert_eq!(
            case["after"]["neurons"].as_array().unwrap().len(),
            case["before"]["neurons"].as_array().unwrap().len()
        );
    }

    let if_target = find(&cases, "if_target_declined");
    let maximum_target = find(&cases, "maximum_target_declined");
    for case in [if_target, maximum_target] {
        assert_eq!(case["result"], json!({"removedNeurons": 0}));
        assert!(case["before"]["neurons"][0].get("id").is_none());
        assert!(case["after"]["neurons"][0].get("id").is_some());
        assert_eq!(without_ids(&case["after"]), without_ids(&case["before"]));
    }
    assert_eq!(if_target["after"]["neurons"].as_array().unwrap().len(), 3);
    assert_eq!(if_target["after"]["neurons"][2]["squash"], "IF");
    assert_eq!(role_count(if_target, "condition"), 2);
    assert_eq!(role_count(if_target, "positive"), 1);
    assert_eq!(role_count(if_target, "negative"), 1);
    assert_eq!(maximum_target["after"]["neurons"][2]["squash"], "MAXIMUM");

    let pair = find(&cases, "complement_pair");
    assert_eq!(pair["result"], json!({"removedNeurons": 1}));
    assert_eq!(hidden(pair).len(), 1);
    assert_eq!(hidden(pair)[0]["squash"], "IDENTITY");
    near(&hidden(pair)[0]["bias"], 3.5);

    let weights = find(&cases, "complement_weights");
    assert_eq!(weights["result"], json!({"removedNeurons": 1}));
    let kept = &hidden(weights)[0];
    assert_eq!(kept["squash"], "IDENTITY");
    near(&kept["bias"], 3.0);
    near(&weight_from(weights, 0), -1.0);
    near(&weight_from(weights, 1), -0.45);
    near(&outbound_weight(weights, kept["id"].as_i64().unwrap()), 1.0);

    let three = find(&cases, "complement_three");
    assert_eq!(three["result"], json!({"removedNeurons": 2}));
    assert_eq!(hidden(three).len(), 1);
    assert_eq!(hidden(three)[0]["squash"], "IDENTITY");
    near(&hidden(three)[0]["bias"], 2.13);
    near(&weight_from(three, 0), -0.5);
    near(&weight_from(three, 1), -1.6);
    near(&weight_from(three, 2), 1.2);

    for name in [
        "identity_still_merges",
        "identity_integrity",
        "post_prune_shape",
    ] {
        let case = find(&cases, name);
        assert_eq!(case["result"]["removedNeurons"], 1);
        assert_eq!(hidden(case).len(), 1);
        assert_eq!(hidden(case)[0]["squash"], "IDENTITY");
    }
    near(
        &hidden(find(&cases, "identity_still_merges"))[0]["bias"],
        0.5,
    );
    near(&hidden(find(&cases, "identity_integrity"))[0]["bias"], 0.5);
    near(&hidden(find(&cases, "post_prune_shape"))[0]["bias"], 0.0);

    let uuid_only = find(&cases, "uuid_only");
    assert_eq!(uuid_only["result"], json!({"removedNeurons": 1}));
    assert_eq!(hidden(uuid_only).len(), 1);
    near(&hidden(uuid_only)[0]["bias"], 0.5);
    near(&weight_uuid(uuid_only, "input-0", "h1"), 1.0);
    near(&weight_uuid(uuid_only, "input-1", "h1"), 0.45);
    assert_id_handles(&uuid_only["after"]);

    let kept_source = find(&cases, "kept_source");
    assert_eq!(kept_source["result"]["removedNeurons"], 1);
    assert!(
        kept_source["after"]["neurons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == 5002)
    );
    near(&weight_between(kept_source, 5002, 5001), 1.0);
    near(&weight_between(kept_source, 5002, -1), 0.1);

    let backward = find(&cases, "backward_source_left");
    assert_eq!(backward["result"], json!({"removedNeurons": 1}));
    let order: Vec<_> = backward["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["back-fed", "later", "bridge-c", "output-0"]);
    near(&weight_uuid(backward, "later", "back-fed"), 0.4);
    near(&hidden_uuid(backward, "bridge-c")["bias"], -0.05);
    near(&weight_uuid(backward, "input-1", "bridge-c"), 1.0);
    near(&weight_uuid(backward, "input-2", "bridge-c"), 0.45);

    let keep_last = find(&cases, "keep_last_bridge");
    assert_eq!(
        keep_last["preparation"],
        "Creature.fromJSON; exportJSONUnchecked"
    );
    assert_eq!(keep_last["result"], json!({"removedNeurons": 1}));
    let survivors: Vec<_> = hidden(keep_last)
        .into_iter()
        .filter(|n| n["squash"] == "IDENTITY")
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    assert_eq!(survivors, ["bridge-late"]);
    assert_eq!(synapse_uuid(keep_last, "input-0")["toUUID"], "bridge-late");
    assert_forward(&keep_last["after"]);
    assert_forward(&keep_last["reloaded"]);
    assert_eq!(
        without_ids(&keep_last["after"]),
        without_ids(&keep_last["reloaded"])
    );
    near(&hidden_uuid(keep_last, "bridge-late")["bias"], -0.25);
    near(&weight_uuid(keep_last, "input-0", "bridge-late"), 1.0);
    near(&weight_uuid(keep_last, "late-source", "bridge-late"), 0.45);
    near(&weight_uuid(keep_last, "bridge-late", "output-0"), 1.0);
    assert_eq!(
        keep_last["after"]["synapses"].as_array().unwrap().len(),
        keep_last["before"]["synapses"].as_array().unwrap().len() - 1
    );

    let identity = find(&cases, "identity_compact");
    assert_eq!(identity["operation"], "compactCreature");
    assert_eq!(identity["activationTolerance"], 1e-6);
    assert_eq!(identity["result"], json!({"candidate": true}));
    assert_eq!(hidden(identity).len(), 1);
    near(&hidden(identity)[0]["bias"], 0.5);
    near(&weight_uuid(identity, "input-0", "h1"), 1.0);
    near(&weight_uuid(identity, "input-1", "h1"), 0.45);
    let before = identity["beforeOutputs"].as_array().unwrap();
    let after = identity["afterOutputs"].as_array().unwrap();
    assert_eq!(before.len(), 3);
    for (expected, actual) in before.iter().zip(after) {
        let delta = (expected[0].as_f64().unwrap() - actual[0].as_f64().unwrap()).abs();
        assert!(delta <= 1e-6, "activation moved by {delta}");
    }

    // Whole-creature compaction folds one COMPLEMENT bridge into the output
    // and leaves the other. The original test only requires fewer than two
    // hidden neurons; this is the graph that pipeline actually emitted.
    let complement = find(&cases, "complement_compact");
    assert_eq!(complement["result"], json!({"candidate": true}));
    assert_eq!(hidden(complement).len(), 1);
    assert_eq!(hidden(complement)[0]["uuid"], "h1");
    assert_eq!(hidden(complement)[0]["squash"], "COMPLEMENT");
    near(&hidden(complement)[0]["bias"], 0.2);
    near(&neuron_uuid(complement, "output-0")["bias"], 0.9);
    near(&weight_uuid(complement, "input-0", "output-0"), -0.4);
    near(&weight_uuid(complement, "input-1", "h1"), 0.6);
}

fn find<'a>(cases: &'a [Value], name: &str) -> &'a Value {
    cases.iter().find(|c| c["name"] == name).expect(name)
}

fn hidden(case: &Value) -> Vec<&Value> {
    case["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["type"] == "hidden")
        .collect()
}

fn near(value: &Value, expected: f64) {
    let actual = value.as_f64().unwrap();
    assert!(
        (actual - expected).abs() <= 1e-12,
        "{actual} differs from {expected}"
    );
}

fn weight_from(case: &Value, from_id: i64) -> Value {
    let kept = hidden(case)[0]["id"].as_i64().unwrap();
    weight_between(case, from_id, kept)
}

fn weight_between(case: &Value, from_id: i64, to_id: i64) -> Value {
    case["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["fromId"] == from_id && s["toId"] == to_id)
        .expect("synapse")["weight"]
        .clone()
}

fn outbound_weight(case: &Value, from_id: i64) -> Value {
    case["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["fromId"] == from_id)
        .expect("outbound")["weight"]
        .clone()
}

fn weight_uuid(case: &Value, from: &str, to: &str) -> Value {
    case["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["fromUUID"] == from && s["toUUID"] == to)
        .expect("uuid synapse")["weight"]
        .clone()
}

fn synapse_uuid<'a>(case: &'a Value, from: &str) -> &'a Value {
    case["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["fromUUID"] == from)
        .expect("synapse")
}

fn hidden_uuid<'a>(case: &'a Value, uuid: &str) -> &'a Value {
    hidden(case)
        .into_iter()
        .find(|n| n["uuid"] == uuid)
        .expect(uuid)
}

fn neuron_uuid<'a>(case: &'a Value, uuid: &str) -> &'a Value {
    case["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["uuid"] == uuid)
        .expect(uuid)
}

fn role_count(case: &Value, role: &str) -> usize {
    case["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["type"] == role)
        .count()
}

fn without_ids(graph: &Value) -> Value {
    let mut graph = graph.clone();
    for n in graph["neurons"].as_array_mut().unwrap() {
        n.as_object_mut().unwrap().remove("id");
    }
    for s in graph["synapses"].as_array_mut().unwrap() {
        let synapse = s.as_object_mut().unwrap();
        synapse.remove("fromId");
        synapse.remove("toId");
    }
    graph
}

fn assert_forward(graph: &Value) {
    let inputs = graph["input"].as_u64().unwrap() as usize;
    let neurons = graph["neurons"].as_array().unwrap();
    let position = |uuid: &str| -> usize {
        if let Some(index) = uuid.strip_prefix("input-") {
            return index.parse().unwrap();
        }
        inputs + neurons.iter().position(|n| n["uuid"] == uuid).expect(uuid)
    };
    for edge in graph["synapses"].as_array().unwrap() {
        assert!(
            position(edge["fromUUID"].as_str().unwrap())
                < position(edge["toUUID"].as_str().unwrap())
        );
    }
}

fn assert_id_handles(graph: &Value) {
    let neurons = graph["neurons"].as_array().unwrap();
    let id = |uuid: &str| -> i64 {
        if let Some(index) = uuid.strip_prefix("input-") {
            return index.parse().unwrap();
        }
        neurons.iter().find(|n| n["uuid"] == uuid).expect(uuid)["id"]
            .as_i64()
            .unwrap()
    };
    for edge in graph["synapses"].as_array().unwrap() {
        assert_eq!(
            edge["fromId"].as_i64().unwrap(),
            id(edge["fromUUID"].as_str().unwrap())
        );
        assert_eq!(
            edge["toId"].as_i64().unwrap(),
            id(edge["toUUID"].as_str().unwrap())
        );
    }
}

fn endpoints_resolve(graph: &Value) {
    let mut ids = std::collections::HashSet::new();
    for index in 0..graph["input"].as_u64().unwrap() {
        ids.insert(index as i64);
    }
    let mut uuids = std::collections::HashSet::new();
    for index in 0..graph["input"].as_u64().unwrap() {
        uuids.insert(format!("input-{index}"));
    }
    for neuron in graph["neurons"].as_array().unwrap() {
        if let Some(id) = neuron.get("id").and_then(Value::as_i64) {
            ids.insert(id);
        }
        if let Some(uuid) = neuron.get("uuid").and_then(Value::as_str) {
            uuids.insert(uuid.to_string());
        }
    }
    for edge in graph["synapses"].as_array().unwrap() {
        if let Some(id) = edge.get("fromId").and_then(Value::as_i64) {
            assert!(ids.contains(&id), "dangling fromId {id}");
        }
        if let Some(id) = edge.get("toId").and_then(Value::as_i64) {
            assert!(ids.contains(&id), "dangling toId {id}");
        }
        if let Some(uuid) = edge.get("fromUUID").and_then(Value::as_str) {
            assert!(uuids.contains(uuid), "dangling fromUUID {uuid}");
        }
        if let Some(uuid) = edge.get("toUUID").and_then(Value::as_str) {
            assert!(uuids.contains(uuid), "dangling toUUID {uuid}");
        }
    }
}
