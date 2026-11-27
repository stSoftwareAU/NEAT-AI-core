//! Creature metadata/repair captures; no native repair parity is claimed (#776).
use serde_json::{Value, json};

fn captures() -> Vec<Value> {
    serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_metadata.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn creature_metadata_captures_preserve_original_validation_contracts() {
    let cases = captures();
    assert_eq!(cases.len(), 2 + 4); // Two metadata and four finite-guard inputs.
    for name in ["creature_memetic_prune", "creature_memetic_fix"] {
        let found: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(found.len(), 1, "missing or duplicate {name}");
        let case = found[0];
        assert_eq!(
            case["preparation"],
            "Creature.fromJSON(input, true), then attach memetic"
        );
        let before = &case["before"];
        let after = &case["after"];
        assert_eq!(before["memetic"]["biases"]["1998066541"], 0.5);
        assert!(after["memetic"]["biases"].get("1998066541").is_none());
        assert_eq!(case["afterValidation"]["valid"], true);
        assert_eq!(before["neurons"], after["neurons"]);
        assert_eq!(before["synapses"], after["synapses"]);
        let mut expected_memetic = before["memetic"].clone();
        expected_memetic["biases"]
            .as_object_mut()
            .unwrap()
            .remove("1998066541");
        assert_eq!(after["memetic"], expected_memetic);
        for index in 0..2 {
            assert_eq!(before["neurons"][index]["type"], "input");
            assert_eq!(before["neurons"][index]["bias"], "Infinity");
        }
        let mut expected_export = case["input"].clone();
        expected_export["semanticVersion"] = json!("4.0.0");
        expected_export["memetic"] = json!({
            "generation": 1, "score": 0, "biases": {}, "weights": []
        });
        if name == "creature_memetic_prune" {
            assert_eq!(case["beforeValidation"]["valid"], false);
            assert_eq!(case["beforeValidation"]["name"], "ValidationError");
            assert!(
                case["beforeValidation"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("1998066541")
            );
            assert_eq!(case["result"], 1);
            let hidden_id = case["hiddenId"].as_i64().unwrap().to_string();
            assert_eq!(before["memetic"]["biases"][&hidden_id], 0.1);
            assert_eq!(after["memetic"]["biases"][&hidden_id], 0.1);
            assert_eq!(after["memetic"]["biases"].as_object().unwrap().len(), 1);
            assert_eq!(case["afterExport"]["memetic"]["biases"]["hidden-a"], 0.1);
            expected_export["memetic"]["biases"]["hidden-a"] = json!(0.1);
        } else {
            // The original fix test validates only after repair.
            assert!(case["beforeValidation"].is_null());
            assert!(case["result"].is_null());
            assert_eq!(case["options"]["forwardOnly"], true);
            assert_eq!(after["forwardOnly"], true);
            assert_eq!(case["afterExport"]["forwardOnly"], true);
            expected_export["forwardOnly"] = json!(true);
        }
        assert_eq!(case["afterExport"], expected_export);
    }
}

#[test]
fn finite_guard_captures_preserve_bias_adjustment_and_atomic_refusal() {
    let cases = captures();
    for name in [
        "finite_normal",
        "finite_overflow",
        "finite_nan",
        "finite_atomic",
    ] {
        let found: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(found.len(), 1, "missing or duplicate {name}");
        let case = found[0];
        assert_eq!(case["operation"], "removeNeuron");
        let loaded = &case["loaded"];
        let before = &case["before"];
        let after = &case["after"];
        let hidden_id = case["hiddenId"].as_i64().unwrap();
        let neurons = before["neurons"].as_array().unwrap();
        assert!(
            neurons
                .iter()
                .any(|n| n["id"] == hidden_id && n["type"] == "hidden")
        );
        let output_bias = |state: &Value, id: i64| {
            state["neurons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id)
                .unwrap()["bias"]
                .as_f64()
                .unwrap()
        };
        if name == "finite_normal" {
            assert_eq!(case["activation"], 0.5);
            assert_eq!(case["result"], true);
            assert_eq!(loaded, before);
            assert!((output_bias(after, -1) - (0.5 + 0.3 * 0.5)).abs() <= 1e-12);
            assert!(
                !after["neurons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|n| n["id"] == hidden_id)
            );
            let edges = after["synapses"].as_array().unwrap();
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0]["weight"], 0);
            assert_eq!(edges[0]["to"], 2); // Two inputs precede the remaining output.
            assert!(matches!(edges[0]["from"].as_u64(), Some(0 | 1)));
            assert_eq!(case["captureSeed"], 1910);
        } else {
            assert_eq!(case["result"], false);
            // Equality covers this runtime projection, not cached UUID/state.
            assert_eq!(before, after, "{name}: no partial topology/bias edits");
            assert!(case["captureSeed"].is_null());
            match name {
                "finite_overflow" => {
                    assert_eq!(case["activation"], 1);
                    assert_eq!(output_bias(loaded, -1), 0.0);
                    assert_eq!(output_bias(before, -1), f64::MAX / 2.0);
                    assert_eq!(loaded["synapses"][1]["weight"], 1);
                    assert_eq!(before["synapses"][1]["weight"].as_f64(), Some(f64::MAX));
                }
                "finite_nan" => {
                    assert_eq!(case["activation"], "NaN");
                    assert_eq!(loaded, before);
                    assert_eq!(before["synapses"][1]["weight"], 0.5);
                }
                "finite_atomic" => {
                    assert_eq!(case["activation"].as_f64(), Some(f64::MAX));
                    assert_eq!(loaded["synapses"][2]["weight"], 1);
                    assert_eq!(before["synapses"][1]["weight"], 0.5);
                    assert_eq!(before["synapses"][2]["weight"].as_f64(), Some(f64::MAX));
                    assert_eq!(output_bias(after, -1), 0.0);
                    assert_eq!(output_bias(after, -2), 0.0);
                }
                _ => unreachable!(),
            }
        }
    }
}
