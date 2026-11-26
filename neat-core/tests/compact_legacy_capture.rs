//! Independent TypeScript output captures for the compaction migration (#776).
//! This grades activation/wire compatibility, not a Rust compaction API.
use neat_core::{CreatureExport, compile_creature};
use serde::Deserialize;

#[derive(Deserialize)]
struct Capture {
    name: String,
    before: CreatureExport,
    after: Option<CreatureExport>,
    probes: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
    #[serde(rename = "activationTolerance")]
    activation_tolerance: f32,
    #[serde(rename = "beforeScore")]
    before_score: f64,
    #[serde(rename = "afterScore")]
    after_score: f64,
}

#[test]
fn native_activation_reproduces_legacy_compaction_outputs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy.json"
    );
    let captures: Vec<Capture> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(!captures.is_empty(), "the legacy oracle must contain cases");
    for case in captures {
        match case.name.as_str() {
            "complement_bypass" | "constant_transitive_fold" => {
                let after = case
                    .after
                    .as_ref()
                    .expect("this fixture must produce a candidate");
                assert!(after.neurons.iter().all(|n| n.neuron_type == "output"));
            }
            "constant_with_varying_input" => {
                let after = case.after.as_ref().expect("the constant input folds");
                assert!(after.neurons.iter().any(|n| n.uuid == "hidden-h"));
                assert!(!after.neurons.iter().any(|n| n.uuid == "const-c"));
            }
            "identity_fanin_noop" | "aggregate_guard" | "nonlinear_noop" => {
                assert!(case.after.is_none(), "{} must not compact", case.name);
            }
            other => panic!("unclassified capture: {other}"),
        }
        let tolerance = if case.name.starts_with("constant_") {
            1e-6
        } else {
            1e-5
        };
        assert_eq!(
            case.activation_tolerance, tolerance,
            "original tolerance must not widen"
        );
        if case.name.starts_with("constant_") {
            assert!(
                case.after_score >= case.before_score - 1e-9,
                "{}: legacy score regressed",
                case.name
            );
        }
        assert!(!case.probes.is_empty(), "{}: missing probes", case.name);
        assert_eq!(case.probes.len(), case.outputs.len(), "{}", case.name);
        for creature in [&case.before, case.after.as_ref().unwrap_or(&case.before)] {
            let mut net = compile_creature(creature).unwrap();
            for (probe, expected) in case.probes.iter().zip(&case.outputs) {
                let actual = net.activate(probe, creature.output);
                assert_eq!(actual.len(), expected.len(), "{}", case.name);
                for (actual, expected) in actual.iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() <= tolerance,
                        "{}: {probe:?}: {actual} != {expected}",
                        case.name
                    );
                }
            }
        }
    }
}

#[test]
fn every_recorded_legacy_contract_is_present() {
    let captures: Vec<Capture> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy.json"
        ))
        .unwrap(),
    )
    .unwrap();
    for name in [
        "complement_bypass",
        "identity_fanin_noop",
        "aggregate_guard",
        "nonlinear_noop",
        "constant_transitive_fold",
        "constant_with_varying_input",
    ] {
        assert!(
            captures.iter().any(|c| c.name == name),
            "missing legacy regression {name}"
        );
    }
}
