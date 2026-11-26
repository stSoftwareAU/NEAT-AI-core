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
    ] {
        assert!(cases.iter().any(|c| c["name"] == name), "missing {name}");
    }
    assert_eq!(cases.len(), 5);
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
            other => panic!("unclassified helper capture: {other}"),
        }
    }
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
