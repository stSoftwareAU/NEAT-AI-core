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
            "constant_fold_full"
            | "constant_fold_partial_if"
            | "constant_disconnected"
            | "constant_merge_forest"
            | "constant_merge_if_roles"
            | "constant_merge_surplus" => {
                assert_constant_contract(&case);
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
        if matches!(
            case.name.as_str(),
            "constant_transitive_fold"
                | "constant_with_varying_input"
                | "constant_fold_full"
                | "constant_fold_partial_if"
                | "if_positive"
                | "if_negative"
                | "if_zero"
        ) {
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
        "constant_fold_full",
        "constant_fold_partial_if",
        "constant_disconnected",
        "constant_merge_forest",
        "constant_merge_if_roles",
        "constant_merge_surplus",
    ] {
        assert!(
            captures.iter().any(|c| c.name == name),
            "missing legacy regression {name}"
        );
    }
}

fn assert_constant_contract(case: &Capture) {
    let after = case
        .after
        .as_ref()
        .expect("the original constant case compacts");
    assert!(after.forward_only);
    let constants: Vec<_> = after
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "constant")
        .collect();
    let neuron = |uuid: &str| after.neurons.iter().find(|n| n.uuid == uuid).unwrap();
    let weight = |from: &str, to: &str, role: Option<&str>| {
        let edges: Vec<_> = after
            .synapses
            .iter()
            .filter(|s| s.from_uuid == from && s.to_uuid == to && s.synapse_type.as_deref() == role)
            .collect();
        assert_eq!(edges.len(), 1, "{}: {from} -> {to}", case.name);
        edges[0].weight
    };
    // Original merge tests prohibit duplicate endpoint pairs, including when
    // distinct IF roles would otherwise collide on one canonical source.
    let pairs: std::collections::HashSet<_> = after
        .synapses
        .iter()
        .map(|s| (&s.from_uuid, &s.to_uuid))
        .collect();
    assert_eq!(pairs.len(), after.synapses.len());
    match case.name.as_str() {
        "constant_fold_full" => {
            assert!(constants.is_empty());
            let consumer = neuron("533d8616-037c-4278-b95c-3a2a1ce15ee6");
            assert_eq!(consumer.squash.as_deref(), Some("HARD_TANH"));
            assert!((consumer.bias - (0.2 + 0.5 * 0.5)).abs() <= 1e-9);
        }
        "constant_fold_partial_if" => {
            assert_eq!(constants.len(), 1);
            assert_eq!(constants[0].uuid, "constant-0");
            assert_eq!(constants[0].bias, 1.0);
            assert_eq!(neuron("if-consumer").squash.as_deref(), Some("IF"));
            assert!((neuron("nonif-consumer").bias - (0.1 + 0.4 * 0.5)).abs() <= 1e-9);
            assert!(
                (weight("constant-0", "if-consumer", Some("positive")) - 0.3 * 0.5).abs() <= 1e-9
            );
            assert!(
                !after
                    .synapses
                    .iter()
                    .any(|s| s.from_uuid == "constant-0" && s.to_uuid == "nonif-consumer")
            );
        }
        "constant_disconnected" => {
            assert!(constants.is_empty());
            assert_eq!(after.neurons.len(), 1);
            assert_eq!(neuron("output-0").bias, 0.0);
            assert_eq!(after.synapses.len(), 1);
            assert_eq!(weight("input-0", "output-0", None), 1.0);
        }
        "constant_merge_forest" => {
            assert_eq!(
                case.before
                    .neurons
                    .iter()
                    .filter(|n| n.neuron_type == "constant")
                    .count(),
                24
            );
            assert_eq!(constants.len(), 2);
            assert_eq!(after.neurons.len(), 2 + 12 + 1);
            assert_eq!(after.synapses.len(), 12 * 4 + 1);
            for c in constants {
                assert!(matches!(c.uuid.as_str(), "constant-0" | "constant-1"));
                assert_eq!(c.bias, 1.0);
            }
            for i in 0..12 {
                let target = format!("if-{i}");
                assert_eq!(neuron(&target).squash.as_deref(), Some("IF"));
                assert!(
                    (weight("constant-0", &target, Some("positive"))
                        - 0.5 * 0.1 * f64::from(i + 1))
                    .abs()
                        <= 1e-6
                );
                assert!(
                    (weight("constant-1", &target, Some("negative"))
                        - 0.7 * -0.2 * f64::from(i + 1))
                    .abs()
                        <= 1e-6
                );
            }
            assert!(
                case.after_score > case.before_score,
                "original forest score must improve"
            );
        }
        "constant_merge_if_roles" => {
            assert_eq!(constants.len(), 3);
            for (index, role, expected) in [
                (0, "condition", 0.4 * 0.5),
                (1, "positive", 0.6 * -0.25),
                (2, "negative", 0.3 * 2.0),
            ] {
                let source = format!("constant-{index}");
                assert_eq!(neuron(&source).bias, 1.0);
                assert!((weight(&source, "if-neuron", Some(role)) - expected).abs() <= 1e-6);
            }
            assert_eq!(neuron("if-neuron").squash.as_deref(), Some("IF"));
        }
        "constant_merge_surplus" => {
            let mut ids: Vec<_> = constants.iter().map(|n| n.uuid.as_str()).collect();
            ids.sort_unstable();
            assert_eq!(ids, ["const-d", "constant-0", "constant-1", "constant-2"]);
            assert_eq!(neuron("const-d").bias, 0.25);
            assert_eq!(weight("const-d", "max-neuron", None), 0.9);
            assert_eq!(neuron("max-neuron").squash.as_deref(), Some("MAXIMUM"));
        }
        other => panic!("unclassified constant capture: {other}"),
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
