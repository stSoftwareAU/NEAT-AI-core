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
            "complement_bypass" => {
                let after = case.after.as_ref().expect("a linear bridge must compact");
                assert!(after.neurons.iter().all(|n| n.neuron_type == "output"));
            }
            "identity_fanin_noop" | "aggregate_guard" | "nonlinear_noop" => {
                assert!(case.after.is_none(), "{} must not compact", case.name);
            }
            other => panic!("unclassified capture: {other}"),
        }
        assert!(!case.probes.is_empty(), "{}: missing probes", case.name);
        assert_eq!(case.probes.len(), case.outputs.len(), "{}", case.name);
        for creature in [&case.before, case.after.as_ref().unwrap_or(&case.before)] {
            let mut net = compile_creature(creature).unwrap();
            for (probe, expected) in case.probes.iter().zip(&case.outputs) {
                let actual = net.activate(probe, creature.output);
                assert_eq!(actual.len(), expected.len(), "{}", case.name);
                for (actual, expected) in actual.iter().zip(expected) {
                    // The original COMPLEMENT regression uses 1e-5 for f32 activation.
                    assert!(
                        (actual - expected).abs() <= 1e-5,
                        "{}: {probe:?}: {actual} != {expected}",
                        case.name
                    );
                }
            }
        }
    }
}
