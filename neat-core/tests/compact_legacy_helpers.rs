//! Frozen raw helper contracts, not native rewrite or activation parity (#776).
use serde_json::{Value, json};

#[test]
fn raw_helper_captures_preserve_legacy_contracts() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_helpers.json"
        ))
        .unwrap(),
    )
    .unwrap();
    for name in [
        "typed_zero",
        "duplicate_sum",
        "distinct_if_roles",
        "backward_repair",
        "forward_noop",
        "duplicate_tags",
        "memetic_count",
        "memetic_clean",
        "memetic_array_weights",
        "memetic_absent",
        "memetic_no_weights",
    ] {
        assert!(cases.iter().any(|c| c["name"] == name), "missing {name}");
    }
    assert_eq!(cases.len(), 11);
    for case in cases {
        let before = &case["before"];
        let after = &case["after"];
        let edges = after["synapses"].as_array().unwrap();
        match case["name"].as_str().unwrap() {
            "typed_zero" => {
                assert_eq!(case["preparation"], "none");
                assert_eq!(case["input"], *before);
                assert_eq!(before["synapses"].as_array().unwrap().len(), 7);
                assert_eq!(case["result"], json!({"removedSynapses": 1}));
                assert!(before.get("memetic").is_some());
                assert!(after.get("memetic").is_none());
                assert_edges(
                    edges,
                    &[
                        ("input-0", "-1", "", 0.3),
                        ("input-0", "-2", "", 0.0),
                        ("input-0", "if-0", "condition", 0.0),
                        ("input-0", "if-0", "positive", 0.2),
                        ("input-0", "if-0", "negative", -0.2),
                        ("input-0", "if-0", "", 0.0),
                    ],
                );
            }
            "duplicate_sum" => {
                assert_eq!(case["preparation"], "none");
                assert_eq!(case["input"], *before);
                assert_eq!(before["synapses"].as_array().unwrap().len(), 5);
                assert_eq!(case["result"], json!({"merged": 3}));
                assert!(before.get("memetic").is_some());
                assert!(after.get("memetic").is_none());
                // 0.5 - 0.2 = 0.3; 0.1 + 0.2 + 0.3 = 0.6.
                // Roles merge at the IDENTITY target; the first role survives.
                assert_edges(
                    edges,
                    &[
                        ("input-0", "output-0", "", 0.3),
                        ("input-0", "hidden-0", "condition", 0.6),
                    ],
                );
                assert_eq!(edges[0]["tags"], json!([{"name":"tag", "value":"b"}]));
                assert_eq!(
                    edges[1]["tags"],
                    json!([
                        {"name":"tag", "value":"a"}, {"name":"tag", "value":"b"}
                    ])
                );
            }
            "distinct_if_roles" => {
                assert_eq!(case["preparation"], "none");
                assert_eq!(case["input"], *before);
                assert_eq!(before["synapses"].as_array().unwrap().len(), 2);
                assert_eq!(after["neurons"][0]["squash"], "IF");
                assert_eq!(case["result"], json!({"merged": 0}));
                // Preserve the original incomplete IF input without load-time repair.
                assert_edges(
                    edges,
                    &[
                        ("input-0", "output-0", "condition", 0.3),
                        ("input-0", "output-0", "positive", 0.5),
                    ],
                );
            }
            "backward_repair" => {
                assert_eq!(case["preparation"], "normaliseCreatureExport");
                assert!(case["input"]["neurons"][0].get("id").is_none());
                assert!(before["neurons"][0].get("id").is_some());
                assert_eq!(before["synapses"].as_array().unwrap().len(), 4);
                assert_eq!(case["result"], json!({"removedSynapses": 1}));
                assert_eq!(before["neurons"], after["neurons"]);
                assert_edges(
                    edges,
                    &[
                        ("input-0", "hidden-0", "", 1.0),
                        ("hidden-0", "hidden-1", "", 0.5),
                        ("hidden-1", "output-0", "", 1.0),
                    ],
                );
            }
            "forward_noop" => {
                assert_eq!(case["preparation"], "normaliseCreatureExport");
                assert_eq!(case["result"], json!({"removedSynapses": 0}));
                assert_eq!(before, after);
                assert_edges(
                    edges,
                    &[
                        ("input-0", "hidden-0", "", 1.0),
                        ("hidden-0", "output-0", "", 1.0),
                    ],
                );
            }
            "duplicate_tags" => {
                assert_eq!(case["preparation"], "none");
                assert_eq!(case["input"], *before);
                assert_eq!(case["result"], json!({"merged": 1}));
                assert_eq!(before["synapses"].as_array().unwrap().len(), 2);
                assert_eq!(before["synapses"][0]["tags"], before["synapses"][1]["tags"]);
                // Two distinct tag objects have equal values; 0.25 + 0.75 = 1.
                assert_eq!(
                    after["synapses"],
                    json!([{
                        "fromUUID": "input-0", "toUUID": "output-0", "weight": 1,
                        "fromId": 0, "toId": -1,
                        "tags": [{"name": "meta", "value": "same"}]
                    }])
                );
                // The legacy merge helper itself adds the canonical output ID.
                assert_eq!(
                    after["neurons"],
                    json!([{
                        "type": "output", "uuid": "output-0", "squash": "IDENTITY",
                        "bias": 0, "id": -1
                    }])
                );
            }
            "memetic_count"
            | "memetic_clean"
            | "memetic_array_weights"
            | "memetic_absent"
            | "memetic_no_weights" => assert_memetic_capture(&case),
            other => panic!("unclassified helper capture: {other}"),
        }
    }
}

fn assert_memetic_capture(case: &Value) {
    assert_eq!(case["helper"], "pruneOrphanMemeticReferences");
    assert_eq!(case["preparation"], "none");
    assert_eq!(case["input"], case["before"]);
    let mut expected = case["before"].clone();
    let count = match case["name"].as_str().unwrap() {
        "memetic_count" => {
            // One stale bias, one missing-edge row, one absent-source entry
            // (despite its two rows), and one stale ancestry row: 1+1+1+1.
            expected["memetic"]["biases"]
                .as_object_mut()
                .unwrap()
                .remove("99");
            expected["memetic"]["weights"]["0"]
                .as_array_mut()
                .unwrap()
                .remove(1);
            expected["memetic"]["weights"]
                .as_object_mut()
                .unwrap()
                .remove("42");
            expected["memetic"]["ancestry"][0]["weights"]["0"]
                .as_array_mut()
                .unwrap()
                .remove(1);
            4
        }
        "memetic_array_weights" => {
            // Wire array rows are not integer-keyed runtime weights. Only the
            // stale bias is removed; all three array slots survive unchanged.
            assert!(expected["memetic"]["weights"].is_array());
            expected["memetic"]["biases"]
                .as_object_mut()
                .unwrap()
                .remove("99");
            1
        }
        "memetic_no_weights" => {
            // Root and ancestry each lose one stale bias; absent weights stay
            // absent, and both valid bias values survive.
            assert!(expected["memetic"].get("weights").is_none());
            assert!(expected["memetic"]["ancestry"][0].get("weights").is_none());
            expected["memetic"]["biases"]
                .as_object_mut()
                .unwrap()
                .remove("99");
            expected["memetic"]["ancestry"][0]["biases"]
                .as_object_mut()
                .unwrap()
                .remove("98");
            2
        }
        "memetic_absent" => {
            assert!(expected.get("memetic").is_none());
            0
        }
        "memetic_clean" => 0,
        other => panic!("unclassified memetic capture: {other}"),
    };
    assert_eq!(case["result"], count);
    assert_eq!(case["after"], expected);
}

fn assert_edges(actual: &[Value], expected: &[(&str, &str, &str, f64)]) {
    assert_eq!(actual.len(), expected.len());
    for (edge, (from, to, role, weight)) in actual.iter().zip(expected) {
        assert_eq!(edge["fromUUID"], *from);
        let target = edge
            .get("toUUID")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| edge["toId"].to_string());
        assert_eq!(target, *to);
        assert_eq!(
            edge.get("type").and_then(Value::as_str).unwrap_or(""),
            *role
        );
        assert!((edge["weight"].as_f64().unwrap() - weight).abs() <= 1e-12);
    }
}
