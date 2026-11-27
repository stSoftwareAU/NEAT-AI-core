//! Frozen TypeScript observations of bridge ordering and refusal contracts
//! (#776). Native replay of the same captures lives in
//! `neat-core/tests/compact_parallel_bridge.rs` (#777).
use serde_json::{Value, json};

#[test]
fn bridge_captures_preserve_ordering_and_refusal_contracts() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_bridges.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let names = [
        "late_source",
        "backward_refusal",
        "maximum_refusal",
        "if_refusal",
    ];
    assert_eq!(cases.len(), names.len());
    for name in names {
        let case = cases.iter().find(|c| c["name"] == name).expect(name);
        assert_eq!(case["revision"], "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab");
        assert_eq!(case["test"], "test/compact/ParallelBridgeMergeOrdering.ts");
        assert_eq!(case["source"], "src/compact/ParallelBridgeMerge.ts");
        let before = &case["before"];
        let after = &case["after"];
        assert_id_handles(after);
        if name == "late_source" {
            assert_eq!(case["result"], json!({"removedNeurons": 1}));
            let neurons = after["neurons"].as_array().unwrap();
            let order: Vec<_> = neurons
                .iter()
                .map(|n| n["uuid"].as_str().unwrap())
                .collect();
            assert_eq!(order, ["source-late", "bridge-late", "output-0"]);
            let mut expected = before.clone();
            expected["neurons"]
                .as_array_mut()
                .unwrap()
                .retain(|n| n["uuid"] != "bridge-early");
            // Both original outbound contributions become the kept bridge's bias.
            expected["neurons"][1]["bias"] = json!(1.5_f64 * 0.2 + 2.0 * 0.1);
            let edges = expected["synapses"].as_array_mut().unwrap();
            edges.retain(|s| s["fromUUID"] != "bridge-early");
            for edge in edges {
                match edge["toUUID"].as_str().unwrap() {
                    "bridge-early" => {
                        edge["toUUID"] = json!("bridge-late");
                        edge["weight"] = json!(1);
                    }
                    "bridge-late" => edge["weight"] = json!(1.5_f64 * 0.3),
                    "output-0" => edge["weight"] = json!(1),
                    _ => {}
                }
            }
            assert_eq!(without_ids(after), expected);
        } else {
            assert_eq!(case["result"], json!({"removedNeurons": 0}));
            // Raw callers gain IDs inside the helper, even on refusal. Compare
            // wire topology/metadata after stripping only those added handles.
            assert_eq!(without_ids(after), without_ids(before));
        }
        match name {
            "backward_refusal" | "maximum_refusal" => {
                assert_eq!(case["preparation"], "none");
                assert_eq!(case["input"], *before);
                assert!(case["reloaded"].is_null());
                assert!(case["probes"].is_null());
                assert!(before["neurons"][0].get("id").is_none());
                assert!(after["neurons"][0].get("id").is_some());
                if name == "backward_refusal" {
                    assert_eq!(after["synapses"][4]["fromUUID"], "bridge-b");
                    assert_eq!(after["synapses"][4]["toUUID"], "target");
                    assert_eq!(after["synapses"][4]["weight"], 1.5);
                } else {
                    assert_eq!(after["neurons"][6]["squash"], "MAXIMUM");
                    assert_eq!(after["neurons"].as_array().unwrap().len(), 6 + 2);
                }
            }
            "late_source" | "if_refusal" => {
                assert_eq!(
                    case["preparation"],
                    "Creature.fromJSON; exportJSONUnchecked"
                );
                let reloaded = &case["reloaded"];
                assert_eq!(without_ids(after), *reloaded);
                assert_eq!(
                    reloaded["synapses"].as_array().unwrap().len(),
                    after["synapses"].as_array().unwrap().len()
                );
                assert_forward(after);
                assert_forward(reloaded);
                let tolerance = if name == "late_source" { 1e-6 } else { 0.0 };
                assert_eq!(case["activationTolerance"], tolerance);
                let probes = case["probes"].as_array().unwrap();
                let outputs = case["beforeOutputs"].as_array().unwrap();
                let actual = case["afterOutputs"].as_array().unwrap();
                assert_eq!(probes.len(), 25);
                assert_eq!(outputs.len(), probes.len());
                assert_eq!(actual.len(), probes.len());
                for ((row, expected), result) in probes.iter().zip(outputs).zip(actual) {
                    assert_eq!(
                        row.as_array().unwrap().len(),
                        after["input"].as_u64().unwrap() as usize
                    );
                    assert_eq!(expected.as_array().unwrap().len(), 1);
                    assert_eq!(result.as_array().unwrap().len(), 1);
                    assert!(
                        (expected[0].as_f64().unwrap() - result[0].as_f64().unwrap()).abs()
                            <= tolerance
                    );
                }
                if name == "if_refusal" {
                    assert_eq!(after["neurons"][12]["squash"], "IF");
                    for role in ["condition", "positive", "negative"] {
                        assert_eq!(
                            after["synapses"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter(|s| s["type"] == role)
                                .count(),
                            4
                        );
                    }
                    assert_eq!(case["beforeOutputs"], case["afterOutputs"]);
                }
            }
            _ => unreachable!(),
        }
    }
}

fn without_ids(graph: &Value) -> Value {
    let mut graph = graph.clone();
    for n in graph["neurons"].as_array_mut().unwrap() {
        n.as_object_mut().unwrap().remove("id");
    }
    for s in graph["synapses"].as_array_mut().unwrap() {
        s.as_object_mut().unwrap().remove("fromId");
        s.as_object_mut().unwrap().remove("toId");
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

// UUID and integer handles must resolve to the same endpoints. This also
// catches stale redirected handles that a wire-only comparison cannot see.
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
