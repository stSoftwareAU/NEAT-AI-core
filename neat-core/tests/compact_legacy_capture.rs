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
            "if_positive" | "if_negative" | "if_zero" | "if_varying" => {
                assert_if_contract(&case);
            }
            "zero_weight_orphan" => {
                let after = case
                    .after
                    .as_ref()
                    .expect("zero edge strands a hidden neuron");
                assert_eq!(after.neurons.len(), 1);
                assert_eq!(after.neurons[0].uuid, "output-0");
                assert_eq!(after.neurons[0].bias, 0.1);
                assert_eq!(after.synapses.len(), 1);
                assert_eq!(after.synapses[0].from_uuid, "input-0");
                assert_eq!(after.synapses[0].to_uuid, "output-0");
                assert_eq!(after.synapses[0].weight, 1.25);
            }
            name if name.starts_with("parallel_") => assert_parallel_contract(&case),
            other => panic!("unclassified capture: {other}"),
        }
        let tolerance = if case.name == "zero_weight_orphan" {
            1e-7
        } else if case.name.starts_with("constant_")
            || case.name.starts_with("if_")
            || case.name.starts_with("parallel_")
        {
            1e-6
        } else {
            1e-5
        };
        assert_eq!(
            case.activation_tolerance, tolerance,
            "original tolerance must not widen"
        );
        if case.name.starts_with("constant_")
            || matches!(
                case.name.as_str(),
                "if_positive" | "if_negative" | "if_zero"
            )
        {
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
                assert_eq!(probe.len(), creature.input, "{}: probe width", case.name);
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

fn assert_parallel_contract(case: &Capture) {
    if matches!(
        case.name.as_str(),
        "parallel_duplicate_sources" | "parallel_nonlinear" | "parallel_fanin"
    ) {
        assert!(case.after.is_none(), "{} must not compact", case.name);
        return;
    }
    let after = case.after.as_ref().expect("parallel bridges must merge");
    // These are one-pass captures: the merged bridge remains, rather than
    // flattening the whole linear network into the output in a second pass.
    assert_eq!(after.neurons.len(), 2);
    let (kept_uuid, bias, weights, output_bias): (&str, f64, &[f64], f64) = match case.name.as_str()
    {
        "parallel_two" | "parallel_forward_only" => ("h1", 0.0, &[1.0, 0.45], 0.0),
        // 2 * 0.1 + 1.5 * 0.2 = 0.5; inbound weights are products.
        "parallel_weights" => ("h1", 0.5, &[1.0, 0.45], 0.0),
        // 2 * 0.5 + 3 * -0.3 + 0.5 * 0.1 = 0.15.
        "parallel_bias" => ("h2", 0.15, &[2.0, 3.0, 0.5], 0.2),
        // 0.5 * 0.1 + 0.8 * -0.2 + 1.2 * 0.3 = 0.25.
        "parallel_three" => ("h2", 0.25, &[0.5, 1.6, -1.2], 0.0),
        other => panic!("unclassified parallel capture: {other}"),
    };
    let kept = &after.neurons[0];
    assert_eq!(kept.uuid, kept_uuid);
    assert_eq!(kept.neuron_type, "hidden");
    assert_eq!(kept.squash.as_deref(), Some("IDENTITY"));
    assert!(
        (kept.bias - bias).abs() <= 1e-12,
        "{}: merged bias",
        case.name
    );
    assert_eq!(after.neurons[1].uuid, "output-0");
    assert_eq!(after.neurons[1].bias, output_bias);
    assert_eq!(after.synapses.len(), weights.len() + 1);
    for (index, expected) in weights.iter().enumerate() {
        let edge = after
            .synapses
            .iter()
            .find(|s| s.from_uuid == format!("input-{index}") && s.to_uuid == kept_uuid)
            .expect("each original input must reach the retained bridge");
        assert!(
            (edge.weight - expected).abs() <= 1e-12,
            "{}: input {index}",
            case.name
        );
        assert!(edge.synapse_type.is_none());
    }
    let outbound = after
        .synapses
        .iter()
        .find(|s| s.from_uuid == kept_uuid && s.to_uuid == "output-0")
        .expect("retained bridge must reach the output");
    assert_eq!(outbound.weight, 1.0);
    assert!(outbound.synapse_type.is_none());
    if case.name == "parallel_forward_only" {
        assert!(after.forward_only);
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
        "if_positive",
        "if_negative",
        "if_zero",
        "if_varying",
        "zero_weight_orphan",
        "parallel_two",
        "parallel_weights",
        "parallel_bias",
        "parallel_duplicate_sources",
        "parallel_nonlinear",
        "parallel_fanin",
        "parallel_forward_only",
        "parallel_three",
    ] {
        assert!(
            captures.iter().any(|c| c.name == name),
            "missing legacy regression {name}"
        );
    }
}

fn assert_if_contract(case: &Capture) {
    let after = case.after.as_ref().unwrap_or(&case.before);
    let neuron = after
        .neurons
        .iter()
        .find(|n| n.uuid == "if-neuron")
        .unwrap();
    let inbound: Vec<_> = after
        .synapses
        .iter()
        .filter(|s| s.to_uuid == "if-neuron")
        .collect();
    if case.name == "if_varying" {
        assert_eq!(neuron.squash.as_deref(), Some("IF"));
        let condition = inbound
            .iter()
            .find(|s| s.synapse_type.as_deref() == Some("condition"))
            .unwrap();
        assert_eq!(condition.from_uuid, "input-0");
        assert_eq!(condition.weight, 0.5);
    } else {
        assert!(case.after.is_some(), "constant IF must produce a candidate");
        assert_eq!(neuron.squash.as_deref(), Some("IDENTITY"));
        // Exactly the selected arm survives, with its original weight and no role.
        let (source, weight) = if case.name == "if_positive" {
            ("input-0", 0.6)
        } else {
            ("input-1", 0.7)
        };
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].from_uuid, source);
        assert_eq!(inbound[0].weight, weight);
        assert!(inbound[0].synapse_type.is_none());
        assert!(after.neurons.iter().all(|n| n.neuron_type != "constant"));
    }
}
