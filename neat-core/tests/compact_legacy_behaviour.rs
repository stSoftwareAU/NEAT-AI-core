//! Legacy behaviour-guard captures; integrity checks, not native compactor parity.
use serde_json::{Value, json};

#[test]
fn original_behaviour_cases_retain_guard_boundaries_and_provenance() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_behaviour.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cases.len(), 3); // One record per original test.
    for name in ["logistic_chain", "identity_chain", "provenance"] {
        let matches: Vec<_> = cases.iter().filter(|c| c["name"] == name).collect();
        assert_eq!(matches.len(), 1, "missing or duplicate {name}");
        let case = matches[0];
        assert_eq!(case["revision"], "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab");
        assert_eq!(case["test"], "test/compact/CompactBehaviourGuard.ts");
        assert_eq!(case["feedbackLoop"], false);
        assert_eq!(case["input"]["input"], 1);
        assert_eq!(case["input"]["output"], 1);
        let mut expected_before = case["input"].clone();
        expected_before["semanticVersion"] = json!("4.0.0");
        if name == "provenance" {
            expected_before["tags"] = json!([
                {"name":"trainID","value":"T-3840"},
                {"name":"error","value":"0.6310"}
            ]);
        }
        assert_eq!(case["before"], expected_before);
        let candidate = &case["candidate"];
        if name == "logistic_chain" {
            // The conditional original does not establish that a candidate exists.
            assert!(candidate.is_null());
        } else {
            let mut expected = expected_before.clone();
            expected["neurons"]
                .as_array_mut()
                .unwrap()
                .retain(|n| n["uuid"] != "id-b");
            // One pass bypasses id-b: 2 * 0.5 = 1, retaining id-a and its 1.5 input.
            expected["synapses"] = json!([
                {"fromUUID":"input-0","toUUID":"id-a","weight":1.5},
                {"fromUUID":"id-a","toUUID":"output-0","weight":1}
            ]);
            let mut tags = if name == "provenance" {
                expected_before["tags"].as_array().unwrap().clone()
            } else {
                vec![]
            };
            tags.extend([
                json!({"name":"approach","value":"compact"}),
                json!({"name":"old-neurons","value":"2"}),
            ]);
            expected["tags"] = json!(tags);
            assert_eq!(candidate, &expected);
        }
        if name != "logistic_chain" {
            assert!(!candidate.is_null());
        }
        if name == "provenance" {
            for graph in [&case["before"], candidate] {
                let tags = graph["tags"].as_array().unwrap();
                for tag in [
                    json!({"name":"trainID","value":"T-3840"}),
                    json!({"name":"error","value":"0.6310"}),
                ] {
                    assert_eq!(tags.iter().filter(|t| **t == tag).count(), 1);
                }
            }
            assert!(
                candidate["tags"]
                    .as_array()
                    .unwrap()
                    .contains(&json!({"name":"approach","value":"compact"}))
            );
            assert!(case["guard"].is_null()); // The tag original does not call the guard.
        } else if !candidate.is_null() {
            let guard = &case["guard"];
            assert_eq!(guard["noiseFloor"], 1e-5);
            assert_eq!(guard["relativeTolerance"], 1e-4);
            let probes = guard["probes"].as_array().unwrap();
            assert_eq!(probes.len(), 24); // Original DEFAULT_PROBE_ROWS.
            let before = guard["beforeOutputs"].as_array().unwrap();
            let after = guard["afterOutputs"].as_array().unwrap();
            assert_eq!(before.len(), probes.len());
            assert_eq!(after.len(), probes.len());
            let mut state = 1_234_567_u64;
            let mut worst = 0_f64;
            let mut sum = 0_f64;
            let mut scale = 1e-3_f64;
            for (i, probe) in probes.iter().enumerate() {
                // Independent transcription of the frozen JS numeric generator:
                // f64 multiply/add, ToInt32 masking, then Float32Array rounding.
                state = ((state as f64 * 1_103_515_245.0 + 12_345.0) as u64) & 0x7fff_ffff;
                let expected_probe =
                    ((state as f64 / 2_147_483_647.0 * 2.0 - 1.0) * [1.0, 3.0, 0.25][i % 3]) as f32;
                assert_eq!(probe, &json!([f64::from(expected_probe)]));
                assert_eq!(before[i].as_array().unwrap().len(), 1);
                assert_eq!(after[i].as_array().unwrap().len(), 1);
                let a = before[i][0].as_f64().unwrap();
                let b = after[i][0].as_f64().unwrap();
                if name == "identity_chain" {
                    // The original zero-bias chain has weights 1.5, 2, 0.5.
                    let expected = (expected_probe * 1.5 * 2.0 * 0.5) + 0.25;
                    assert_eq!(a, f64::from(expected));
                    assert_eq!(b, f64::from(expected));
                }
                worst = worst.max((b - a).abs());
                sum += (b - a).abs();
                scale = scale.max(a.abs());
            }
            let deviation = &guard["deviation"];
            assert_eq!(deviation["rows"], 24);
            assert_eq!(deviation["comparisons"], 24);
            assert_eq!(deviation["worstDelta"].as_f64().unwrap(), worst);
            assert_eq!(deviation["meanDelta"].as_f64().unwrap(), sum / 24.0);
            assert_eq!(deviation["scale"].as_f64().unwrap(), scale);
            assert_eq!(deviation["nonFinite"], false);
            assert_eq!(guard["preserved"], true);
            assert!(worst <= 1e-5_f64.max(1e-4 * scale));
        } else {
            assert!(case["guard"].is_null());
        }
    }
}
