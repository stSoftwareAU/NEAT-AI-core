//! Original conversion identity regressions; capture integrity, not native parity.
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn semantic_graph(graph: &Value) -> Value {
    let mut graph = graph.clone();
    for neuron in graph["neurons"].as_array_mut().unwrap() {
        neuron.as_object_mut().unwrap().remove("id");
    }
    for synapse in graph["synapses"].as_array_mut().unwrap() {
        synapse.as_object_mut().unwrap().remove("fromId");
        synapse.as_object_mut().unwrap().remove("toId");
    }
    graph["neurons"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|n| n["uuid"].to_string());
    graph["synapses"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|s| (s["fromUUID"].to_string(), s["toUUID"].to_string()));
    graph
}

#[test]
fn converted_constants_keep_distinct_source_uuids_and_connections() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_metadata.json"
        ))
        .unwrap(),
    )
    .unwrap();
    for name in ["converted_uuid_roundtrip", "converted_uuid_raw"] {
        let found: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(found.len(), 1, "missing or duplicate {name}");
        let case = found[0];
        assert_eq!(case["revision"], "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab");
        assert_eq!(
            case["test"],
            "test/compact/ConvertedConstantKeepsUuid_test.ts"
        );
        assert_eq!(case["seed"], 4071);
        assert_eq!(case["result"], json!({"removed":0,"converted":1}));
        assert_eq!(
            semantic_graph(&case["before"]),
            semantic_graph(&case["input"])
        );
        let runtime = name == "converted_uuid_roundtrip";
        assert_eq!(
            case["operation"],
            if runtime {
                "cleanupOrphanedNeuronsInCreature"
            } else {
                "cleanupOrphanedNeurons"
            }
        );
        let uuid = if runtime { "U" } else { "hidden-no-inward" };
        let mut expected = case["input"].clone();
        let neuron = expected["neurons"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["uuid"] == uuid)
            .unwrap();
        assert_eq!(neuron["type"], "hidden");
        neuron["type"] = json!("constant");
        neuron.as_object_mut().unwrap().remove("squash");
        // IDENTITY(0.3) remains 0.3; the raw LOGISTIC(0.5) conversion is f64.
        if !runtime {
            neuron["bias"] = json!(1.0 / (1.0 + (-0.5_f64).exp()));
        }
        assert_eq!(semantic_graph(&case["after"]), semantic_graph(&expected));
        if runtime {
            assert_eq!(case["preparation"], "Creature.fromJSON(input, false)");
            assert_eq!(case["validation"], json!({"valid":true}));
            assert_eq!(semantic_graph(&case["reloaded"]), semantic_graph(&expected));
            assert_eq!(
                case["constants"],
                json!([
                    {"uuid":"U","outward":1},
                    {"uuid":"legacy-neuron-1777777","outward":1}
                ])
            );
            let ids: Vec<_> = case["runtimeIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["id"].as_i64().unwrap())
                .collect();
            assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
            let labels: BTreeSet<_> = case["runtimeIds"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|n| n["uuid"].as_str())
                .collect();
            assert_eq!(
                labels,
                BTreeSet::from(["U", "legacy-neuron-1777777", "H2", "output-0"])
            );
            // Wire export omits numeric IDs. Reload derives U's ID from its UUID;
            // the original contract retains UUIDs, not the input's numeric ID.
            assert_eq!(
                case["runtimeIds"].as_array().unwrap().len(),
                2 + labels.len()
            );
        } else {
            assert_eq!(case["preparation"], "raw export; no loader");
            assert!(case["validation"].is_null());
            assert!(case["reloaded"].is_null());
            let after = &case["after"];
            let neuron_ids: BTreeSet<_> = after["neurons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n["id"].as_i64().unwrap())
                .collect();
            assert_eq!(neuron_ids.len(), after["neurons"].as_array().unwrap().len());
            for edge in after["synapses"].as_array().unwrap() {
                for (label, handle) in [("fromUUID", "fromId"), ("toUUID", "toId")] {
                    let expected_id = if edge[label] == "input-0" {
                        0
                    } else {
                        after["neurons"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|n| n["uuid"] == edge[label])
                            .unwrap()["id"]
                            .as_i64()
                            .unwrap()
                    };
                    assert_eq!(edge[handle], expected_id);
                }
            }
        }
    }
}
