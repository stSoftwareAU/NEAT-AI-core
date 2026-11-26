//! Frozen variant/selection contracts, not a native selection implementation (#776).
use serde_json::{Value, json};

fn captures() -> Vec<Value> {
    serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_variants.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn variant_captures_preserve_original_selection_contracts() {
    let cases = captures();
    let expected = [
        ("fold_variants", "safe"),
        ("noop_variants", "none"),
        ("select_safe_only", "safe"),
        ("select_aggressive_only", "aggressive"),
        ("select_empty", "none"),
        ("select_duplicate", "safe"),
        ("select_higher", "aggressive"),
        ("select_lower", "safe"),
        ("select_tie", "safe"),
    ];
    // Five aggressive records below cover seven additional original tests.
    assert_eq!(cases.len(), expected.len() + 5);
    for (name, selected) in expected {
        let found: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(found.len(), 1, "missing or duplicate {name}");
        let case = found[0];
        assert_eq!(case["selected"], selected, "{name}");
        let safe = &case["safe"];
        let aggressive = &case["aggressive"];
        match name {
            "fold_variants" => {
                assert_eq!(case["operation"], "compactCreatureVariants");
                assert_eq!(case["feedbackLoop"], false);
                assert!(!safe.is_null() && !aggressive.is_null());
                assert_eq!(safe["uuid"], aggressive["uuid"]);
                assert_eq!(safe["graph"], aggressive["graph"]);
                assert_eq!(case["wrapperSafe"]["uuid"], safe["uuid"]);
                assert_eq!(case["wrapperSafe"]["graph"], safe["graph"]);
                assert_eq!(case["distinctCandidates"], 1);
                let before = case["before"]["neurons"].as_array().unwrap();
                let after = safe["graph"]["neurons"].as_array().unwrap();
                assert!(before.iter().any(|n| n["type"] == "constant"));
                assert!(!after.iter().any(|n| n["type"] == "constant"));
                let consumer = after
                    .iter()
                    .find(|n| n["uuid"] == "533d8616-037c-4278-b95c-3a2a1ce15ee6")
                    .unwrap();
                // Fixture's original 0.2 bias plus constant 0.5 * weight 0.5.
                assert!((consumer["bias"].as_f64().unwrap() - 0.45).abs() <= 1e-9);
            }
            "noop_variants" => {
                assert_eq!(case["operation"], "compactCreatureVariants");
                assert_eq!(case["feedbackLoop"], false);
                assert!(safe.is_null() && aggressive.is_null());
                assert_eq!(case["distinctCandidates"], 0);
                assert_tiny_input(&case["input"], 0.1);
            }
            "select_safe_only" => {
                assert_tiny_candidate(safe, 0.1, Value::Null);
                assert!(aggressive.is_null());
            }
            "select_aggressive_only" => {
                assert!(safe.is_null());
                assert_tiny_candidate(aggressive, 0.2, Value::Null);
            }
            "select_empty" => assert!(safe.is_null() && aggressive.is_null()),
            "select_duplicate" => {
                assert_tiny_candidate(safe, 0.1, json!(5));
                assert_tiny_candidate(aggressive, 0.1, json!(999));
                assert_eq!(case["sameInstance"], false);
                assert_eq!(safe["uuid"], aggressive["uuid"]);
                assert_eq!(safe["graph"], aggressive["graph"]);
            }
            "select_higher" | "select_lower" | "select_tie" => {
                let (safe_score, aggressive_score) = match name {
                    "select_higher" => (5, 6),
                    "select_lower" => (6, 5),
                    _ => (5, 5),
                };
                assert_tiny_candidate(safe, 0.1, json!(safe_score));
                assert_tiny_candidate(aggressive, 0.2, json!(aggressive_score));
                assert_ne!(safe["uuid"], aggressive["uuid"]);
            }
            _ => unreachable!(),
        }
        for candidate in [safe, aggressive] {
            if !candidate.is_null() {
                assert!(!candidate["uuid"].as_str().unwrap().is_empty());
            }
        }
    }
}

#[test]
fn aggressive_captures_preserve_speculation_and_structural_guards() {
    let cases = captures();
    for name in [
        "aggressive_maximum",
        "aggressive_frozen",
        "aggressive_last_input",
        "aggressive_variants",
        "aggressive_equal",
    ] {
        let found: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(found.len(), 1, "missing or duplicate {name}");
        let case = found[0];
        // Freeze the original input scale, not a new native threshold policy.
        assert_eq!(case["threshold"], 1e-3);
        assert_eq!(case["tiny"], 1e-6);
        if matches!(name, "aggressive_variants" | "aggressive_equal") {
            assert_eq!(case["feedbackLoop"], false);
            let safe = &case["safe"];
            let aggressive = &case["aggressive"];
            assert!(!safe.is_null() && !aggressive.is_null());
            assert_eq!(safe["graph"]["neurons"][0]["squash"], "MAXIMUM");
            assert_eq!(aggressive["graph"]["neurons"][0]["squash"], "MAXIMUM");
            let safe_edges = safe["graph"]["synapses"].as_array().unwrap();
            let aggressive_edges = aggressive["graph"]["synapses"].as_array().unwrap();
            assert_eq!(aggressive_edges.len(), 1);
            assert_eq!(aggressive_edges[0]["fromUUID"], "input-0");
            assert_eq!(aggressive_edges[0]["weight"], 2);
            if name == "aggressive_variants" {
                assert_eq!(safe_edges.len(), 2);
                assert_eq!(safe_edges[1]["fromUUID"], "input-2");
                assert_eq!(safe_edges[1]["weight"], 1e-6);
                assert_ne!(safe["uuid"], aggressive["uuid"]);
                assert_eq!(case["wrapperSafe"]["uuid"], safe["uuid"]);
                assert_eq!(case["wrapperSafe"]["graph"], safe["graph"]);
                assert_eq!(safe["score"], 10);
                assert_eq!(aggressive["score"], 5);
                assert_eq!(case["selected"], "safe");
            } else {
                assert_eq!(safe_edges.len(), 1);
                assert_eq!(safe["uuid"], aggressive["uuid"]);
                assert_eq!(safe["graph"], aggressive["graph"]);
            }
        } else {
            assert_eq!(case["operation"], "aggressivePrune");
            assert_eq!(case["preparation"], "none");
            assert_eq!(case["before"], case["input"]);
            assert!(case["input"]["neurons"][0].get("id").is_none());
            assert_eq!(case["after"]["neurons"][0]["id"], -1);
            let mut expected_neurons = case["input"]["neurons"].clone();
            expected_neurons[0]["id"] = json!(-1);
            assert_eq!(case["after"]["neurons"], expected_neurons);
            let edges = case["after"]["synapses"].as_array().unwrap();
            if name == "aggressive_frozen" {
                assert_eq!(case["result"], json!({"changed":false,"removedSynapses":0}));
                assert_eq!(edges.len(), 2);
                assert_eq!(edges[0]["weight"], 1.5);
                assert_eq!(edges[1]["frozen"], true);
                assert_eq!(edges[1]["weight"], 1e-6);
            } else {
                assert_eq!(case["result"], json!({"changed":true,"removedSynapses":1}));
                assert_eq!(edges.len(), 1);
                let weight = if name == "aggressive_maximum" {
                    1.5
                } else {
                    1e-6
                };
                assert_eq!(edges[0]["weight"], weight);
                assert_eq!(edges[0]["fromId"], 0);
                assert_eq!(edges[0]["toId"], -1);
            }
        }
    }
}

fn assert_tiny_input(input: &Value, bias: f64) {
    assert_eq!(
        *input,
        json!({
            "input": 1, "output": 1,
            "neurons": [{"type": "output", "uuid": "output-0", "bias": bias, "squash": "IDENTITY"}],
            "synapses": [{"fromUUID": "input-0", "toUUID": "output-0", "weight": 1}]
        })
    );
}

fn assert_tiny_candidate(candidate: &Value, bias: f64, score: Value) {
    assert_tiny_input(&candidate["input"], bias);
    assert_eq!(candidate["score"], score);
    let graph = &candidate["graph"];
    assert_eq!(graph["input"], 1);
    assert_eq!(graph["output"], 1);
    assert_eq!(graph["neurons"].as_array().unwrap().len(), 1);
    assert_eq!(graph["neurons"][0]["type"], "output");
    assert_eq!(graph["neurons"][0]["uuid"], "output-0");
    assert_eq!(graph["neurons"][0]["bias"], bias);
    assert_eq!(graph["neurons"][0]["squash"], "IDENTITY");
    assert_eq!(graph["synapses"].as_array().unwrap().len(), 1);
    assert_eq!(graph["synapses"][0]["fromUUID"], "input-0");
    assert_eq!(graph["synapses"][0]["toUUID"], "output-0");
    assert_eq!(graph["synapses"][0]["weight"], 1);
}
