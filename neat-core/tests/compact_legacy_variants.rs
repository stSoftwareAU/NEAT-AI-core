//! Frozen variant/selection contracts, not a native selection implementation (#776).
use serde_json::{Value, json};

#[test]
fn variant_captures_preserve_original_selection_contracts() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_variants.json"
        ))
        .unwrap(),
    )
    .unwrap();
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
    assert_eq!(cases.len(), expected.len());
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
