//! Frozen constant-merge helper observations, not native rewrite parity (#776).
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn constant_helper_captures_preserve_roles_slots_frozen_state_and_fixpoint() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_constants.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let names = [
        "if_roles",
        "fixpoint",
        "same_role",
        "maximum_slots",
        "frozen",
        "reuse_slot",
    ];
    assert_eq!(cases.len(), names.len());
    for name in names {
        let c = cases.iter().find(|c| c["name"] == name).expect(name);
        assert_eq!(c["revision"], "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab");
        assert_eq!(c["test"], "test/compact/ConstantMerge.ts");
        assert_eq!(c["operation"], "mergeRedundantConstants");
        assert_eq!(c["weightTolerance"], 1e-12);
        let input = &c["input"];
        let before = &c["before"];
        let after = &c["after"];
        assert_eq!(after["semanticVersion"], input["semanticVersion"]);
        assert_eq!(after["forwardOnly"], input["forwardOnly"]);
        assert_eq!(after["input"], input["input"]);
        assert_eq!(after["output"], input["output"]);
        if name == "reuse_slot" {
            assert_eq!(c["preparation"], "normaliseCreatureExport");
            assert!(input["neurons"][0].get("id").is_none());
            assert!(before["neurons"][0]["id"].is_number());
            assert_eq!(strip_ids(before), *input);
        } else {
            assert_eq!(c["preparation"], "none");
            assert_eq!(*before, *input);
        }
        let (removed, removed_edges, slots, retained): (u64, u64, usize, &[&str]) = match name {
            "if_roles" | "fixpoint" => (3, 0, 3, &[]),
            "same_role" => (2, 1, 1, &[]),
            "maximum_slots" => (3, 0, 3, &["const-d"]),
            "frozen" => (0, 0, 0, &["const-frozen", "const-locked-edge"]),
            "reuse_slot" => (1, 0, 2, &[]),
            _ => unreachable!(),
        };
        assert_eq!(
            c["result"],
            json!({"changed":name!="frozen","removedNeurons":removed,"removedSynapses":removed_edges})
        );
        let mut expected_neurons: Vec<Value> = input["neurons"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] != "constant" || retained.contains(&n["uuid"].as_str().unwrap()))
            .cloned()
            .collect();
        for slot in 0..slots {
            expected_neurons
                .push(json!({"type":"constant","uuid":format!("constant-{slot}"),"bias":1}));
        }
        let mut actual_neurons = strip_ids(&after["neurons"]).as_array().unwrap().clone();
        expected_neurons.sort_by_key(|n| n["uuid"].as_str().unwrap().to_owned());
        actual_neurons.sort_by_key(|n| n["uuid"].as_str().unwrap().to_owned());
        assert_eq!(actual_neurons, expected_neurons, "{name} neurons");
        let expected_edges: Vec<(&str, &str, &str, f64)> = match name {
            "if_roles" | "fixpoint" => vec![
                ("input-0", "if-neuron", "condition", 1.0),
                ("input-1", "if-neuron", "positive", 0.7),
                ("if-neuron", "output-0", "", 1.0),
                ("constant-0", "if-neuron", "condition", 0.4 * 0.5),
                ("constant-1", "if-neuron", "positive", 0.6 * -0.25),
                ("constant-2", "if-neuron", "negative", 0.3 * 2.0),
            ],
            "same_role" => vec![
                ("input-0", "if-neuron", "condition", 1.0),
                ("input-1", "if-neuron", "negative", 1.0),
                ("if-neuron", "output-0", "", 1.0),
                (
                    "constant-0",
                    "if-neuron",
                    "positive",
                    0.6 * 0.5 + 0.25 * 4.0,
                ),
            ],
            "maximum_slots" => vec![
                ("input-0", "max-neuron", "", 1.0),
                ("max-neuron", "output-0", "", 1.0),
                ("constant-0", "max-neuron", "", 0.6 * 0.5),
                ("constant-1", "max-neuron", "", 0.7 * -2.0),
                ("constant-2", "max-neuron", "", 0.8 * 3.0),
                ("const-d", "max-neuron", "", 0.9),
            ],
            "frozen" => vec![
                ("input-0", "if-neuron", "condition", 1.0),
                ("if-neuron", "output-0", "", 1.0),
                ("const-frozen", "if-neuron", "positive", 0.6),
                ("const-locked-edge", "if-neuron", "negative", 0.3),
            ],
            "reuse_slot" => vec![
                ("input-0", "if-neuron", "condition", 1.0),
                ("if-neuron", "output-0", "", 1.0),
                ("constant-0", "if-neuron", "positive", 0.6),
                ("constant-1", "if-neuron", "negative", 0.3 * 0.5),
            ],
            _ => unreachable!(),
        };
        assert_edges(after, &expected_edges);
        if name == "frozen" {
            assert_eq!(strip_ids(after), *input);
        }
        if name == "reuse_slot" {
            let existing = |g: &Value| {
                g["neurons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|n| n["uuid"] == "constant-0")
                    .unwrap()["id"]
                    .as_i64()
                    .unwrap()
            };
            assert_eq!(existing(before), existing(after));
        }
        if name == "fixpoint" {
            let second = &c["second"];
            assert_eq!(
                second["result"],
                json!({"changed":false,"removedNeurons":0,"removedSynapses":0})
            );
            assert_eq!(second["before"], *after);
            assert_eq!(second["after"], *after);
            // The original checks JSON.stringify equality, including member order.
            assert_eq!(second["beforeJson"], second["afterJson"]);
            for field in ["beforeJson", "afterJson"] {
                assert_eq!(
                    serde_json::from_str::<Value>(second[field].as_str().unwrap()).unwrap(),
                    *after
                );
            }
        } else {
            assert!(c["second"].is_null());
        }
    }
}

fn assert_edges(graph: &Value, expected: &[(&str, &str, &str, f64)]) {
    let mut ids = BTreeMap::new();
    for i in 0..graph["input"].as_i64().unwrap() {
        ids.insert(format!("input-{i}"), i);
    }
    for n in graph["neurons"].as_array().unwrap() {
        ids.insert(
            n["uuid"].as_str().unwrap().to_owned(),
            n["id"].as_i64().unwrap(),
        );
    }
    assert_eq!(
        ids.values().copied().collect::<BTreeSet<_>>().len(),
        ids.len()
    );
    let edges = graph["synapses"].as_array().unwrap();
    assert_eq!(edges.len(), expected.len());
    let mut pairs = BTreeSet::new();
    for s in edges {
        let from = s["fromUUID"].as_str().unwrap();
        let to = s["toUUID"].as_str().unwrap();
        assert_eq!(s["fromId"].as_i64().unwrap(), ids[from]);
        assert_eq!(s["toId"].as_i64().unwrap(), ids[to]);
        assert!(pairs.insert((from, to)), "duplicate endpoint pair");
        let role = s["type"].as_str().unwrap_or("");
        let e = expected
            .iter()
            .find(|e| (e.0, e.1, e.2) == (from, to, role))
            .expect("unexpected edge or role");
        assert!(
            (s["weight"].as_f64().unwrap() - e.3).abs() <= 1e-12,
            "{from} -> {to}"
        );
    }
}

fn strip_ids(value: &Value) -> Value {
    match value {
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "id" | "fromId" | "toId"))
                .map(|(k, v)| (k.clone(), strip_ids(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(strip_ids).collect()),
        _ => value.clone(),
    }
}
