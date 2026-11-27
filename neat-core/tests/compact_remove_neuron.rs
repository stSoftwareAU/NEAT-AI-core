//! Bias-fold removal of one hidden neuron, ported from `compactUnused`'s
//! `removeNeuron` (Issue #778).
//!
//! The four `removeNeuron` captures frozen by #776 in
//! `golden/compact_legacy_metadata.json` are the oracle: they were produced by
//! running the TypeScript at NEAT-AI 60d6b4e5, not by this crate. The other
//! tests derive their expected values by hand from the TypeScript rule.

use std::collections::BTreeMap;

use neat_core::{
    CreatureExport, MemeticExport, MemeticWeights, NeuronExport, RemoveNeuronError, SynapseExport,
    compile_creature, remove_neuron_by_bias_fold,
};
use serde_json::Value;

fn neuron(kind: &str, uuid: &str, squash: &str, bias: f64) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias,
        squash: Some(squash.to_string()),
    }
}

fn edge(from: &str, to: &str, weight: f64) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: None,
    }
}

fn creature(
    input: usize,
    neurons: Vec<NeuronExport>,
    synapses: Vec<SynapseExport>,
) -> CreatureExport {
    let output = neurons.iter().filter(|n| n.neuron_type == "output").count();
    CreatureExport {
        input,
        output,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

fn never_draw() -> f64 {
    panic!("no target needed a reconnect draw")
}

fn memetic() -> MemeticExport {
    MemeticExport {
        biases: BTreeMap::from([("output-0".to_string(), 0.01)]),
        weights: MemeticWeights::default(),
        extra: serde_json::Map::new(),
    }
}

fn number(value: &Value) -> f64 {
    match value {
        Value::String(text) => text.parse().unwrap(),
        other => other.as_f64().unwrap(),
    }
}

fn captures() -> Vec<Value> {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_metadata.json"
        ))
        .unwrap(),
    )
    .unwrap();
    cases
        .into_iter()
        .filter(|case| case["operation"] == "removeNeuron")
        .collect()
}

/// Rebuild the runtime `before` state as an export: input neurons are implicit,
/// the rest keep the capture's `input` UUIDs in runtime order. `before` carries
/// the original runtime injections, which `input` does not.
fn export_from_runtime(case: &Value) -> CreatureExport {
    let input = case["input"]["input"].as_u64().unwrap() as usize;
    let uuids: Vec<&str> = case["input"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    let uuid_of = |index: usize| {
        if index < input {
            format!("input-{index}")
        } else {
            uuids[index - input].to_string()
        }
    };
    let neurons = case["before"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["type"] != "input")
        .map(|n| {
            neuron(
                n["type"].as_str().unwrap(),
                &uuid_of(n["index"].as_u64().unwrap() as usize),
                n["squash"].as_str().unwrap(),
                number(&n["bias"]),
            )
        })
        .collect();
    let synapses = case["before"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            edge(
                &uuid_of(s["from"].as_u64().unwrap() as usize),
                &uuid_of(s["to"].as_u64().unwrap() as usize),
                number(&s["weight"]),
            )
        })
        .collect();
    creature(input, neurons, synapses)
}

/// The `after` state as (type, squash, bias bits) per non-input neuron and a
/// (from, to, weight bits) list per synapse in order, in runtime indices.
type Shape = (Vec<(String, String, u64)>, Vec<(usize, usize, u64)>);

fn runtime_shape(neurons: &Value, synapses: &Value) -> Shape {
    let kept: Vec<(String, String, u64)> = neurons
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["type"] != "input")
        .map(|n| {
            (
                n["type"].as_str().unwrap().to_string(),
                n["squash"].as_str().unwrap().to_string(),
                number(&n["bias"]).to_bits(),
            )
        })
        .collect();
    let edges: Vec<(usize, usize, u64)> = synapses
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["from"].as_u64().unwrap() as usize,
                s["to"].as_u64().unwrap() as usize,
                number(&s["weight"]).to_bits(),
            )
        })
        .collect();
    (kept, edges)
}

fn export_shape(creature: &CreatureExport) -> Shape {
    let index_of = |uuid: &str| {
        uuid.strip_prefix("input-")
            .and_then(|i| i.parse::<usize>().ok())
            .unwrap_or_else(|| {
                creature.input
                    + creature
                        .neurons
                        .iter()
                        .position(|n| n.uuid == uuid)
                        .unwrap()
            })
    };
    let kept = creature
        .neurons
        .iter()
        .map(|n| {
            (
                n.neuron_type.clone(),
                n.squash.clone().unwrap(),
                n.bias.to_bits(),
            )
        })
        .collect();
    let edges: Vec<(usize, usize, u64)> = creature
        .synapses
        .iter()
        .map(|s| {
            (
                index_of(&s.from_uuid),
                index_of(&s.to_uuid),
                s.weight.to_bits(),
            )
        })
        .collect();
    (kept, edges)
}

#[test]
fn frozen_remove_neuron_captures_replay_exactly() {
    let cases = captures();
    let names: Vec<&str> = cases.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "finite_normal",
            "finite_overflow",
            "finite_nan",
            "finite_atomic"
        ]
    );
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let before = export_from_runtime(case);
        let hidden = before.neurons[0].uuid.clone();
        assert_eq!(before.neurons[0].neuron_type, "hidden", "{name}");
        let activation = number(&case["activation"]);
        let expected = runtime_shape(&case["after"]["neurons"], &case["after"]["synapses"]);

        // finite_normal's seeded draw reconnected output-0 from input 1 of 2,
        // so any u in [0.5, 1) reproduces it.
        let mut draws = vec![0.75];
        let result =
            remove_neuron_by_bias_fold(&before, &hidden, activation, || draws.pop().unwrap())
                .unwrap();
        match case["result"].as_bool().unwrap() {
            true => {
                let removed = result.unwrap_or_else(|| panic!("{name}: expected a removal"));
                assert_eq!(export_shape(&removed), expected, "{name}");
                assert!(draws.is_empty(), "{name}: the reconnect draw was not taken");
            }
            false => {
                assert!(
                    result.is_none(),
                    "{name}: expected the finite guard to refuse"
                );
                assert_eq!(
                    export_shape(&before),
                    expected,
                    "{name}: refusal changed nothing"
                );
                assert_eq!(draws, vec![0.75], "{name}: a refusal takes no draw");
            }
        }
    }
}

#[test]
fn the_reconnect_input_is_the_floor_of_the_draw_times_the_input_count() {
    let base = creature(
        3,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-1", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    // floor(u * 3): 0.0 -> 0, 0.34 -> 1, 0.999 -> 2.
    for (u, expected) in [(0.0, "input-0"), (0.34, "input-1"), (0.999, "input-2")] {
        let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, || u)
            .unwrap()
            .unwrap();
        assert_eq!(
            removed.synapses,
            vec![edge(expected, "output-0", 0.0)],
            "u = {u}"
        );
    }
}

#[test]
fn a_target_that_keeps_two_inbound_takes_no_draw_and_no_new_edge() {
    let base = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 1.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 3.0),
        ],
    );
    let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 0.25, never_draw)
        .unwrap()
        .unwrap();
    // 1.0 + 2.0 * 0.25 — the inbound count is taken before removal, so the
    // edge from hidden-0 still counts and output-0 is not reconnected.
    assert_eq!(
        removed.neurons,
        vec![neuron("output", "output-0", "IDENTITY", 1.5)]
    );
    assert_eq!(removed.synapses, vec![edge("input-1", "output-0", 3.0)]);
}

#[test]
fn a_feeder_left_without_outward_synapses_is_removed_in_cascade() {
    let base = creature(
        2,
        vec![
            neuron("hidden", "feeder-only", "TANH", 0.0),
            neuron("hidden", "feeder-shared", "TANH", 0.0),
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "feeder-only", 1.0),
            edge("input-0", "feeder-shared", 1.0),
            edge("feeder-only", "hidden-0", 1.0),
            edge("feeder-shared", "hidden-0", 1.0),
            edge("feeder-shared", "output-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, never_draw)
        .unwrap()
        .unwrap();
    let uuids: Vec<&str> = removed.neurons.iter().map(|n| n.uuid.as_str()).collect();
    assert_eq!(uuids, ["feeder-shared", "output-0"]);
    assert_eq!(
        removed.synapses,
        vec![
            edge("input-0", "feeder-shared", 1.0),
            edge("feeder-shared", "output-0", 1.0),
        ]
    );
}

#[test]
fn removal_clears_the_memetic_record_and_a_refusal_keeps_it() {
    let mut base = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
        ],
    );
    base.memetic = Some(memetic());
    let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 1.0, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(removed.memetic, None);

    // f64::MAX * 2.0 overflows the folded bias, so nothing is applied.
    let refused = remove_neuron_by_bias_fold(&base, "hidden-0", f64::MAX, never_draw);
    assert_eq!(refused, Ok(None));
    let refused_nan = remove_neuron_by_bias_fold(&base, "hidden-0", f64::NAN, never_draw);
    assert_eq!(refused_nan, Ok(None));
    assert_eq!(base.memetic, Some(memetic()), "the input is never modified");
}

/// Every squash TypeScript sends down the constant branch is refused, taken
/// from the frozen captures rather than listed here: each capture records
/// `definesPropagate`, a successful removal and a unity constant neuron wired in
/// with the target's bias untouched.
#[test]
fn every_propagate_squash_target_is_refused_as_the_unported_constant_branch() {
    let all: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_metadata.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let cases: Vec<&Value> = all
        .iter()
        .filter(|c| c["operation"] == "removeNeuronConstantBranch")
        .collect();
    let squashes: Vec<&str> = cases
        .iter()
        .map(|c| c["squash"].as_str().unwrap())
        .collect();
    assert_eq!(
        squashes,
        ["IF", "MINIMUM", "MAXIMUM", "MEAN", "HYPOT", "HYPOTv2"]
    );
    for case in cases {
        let squash = case["squash"].as_str().unwrap();
        assert_eq!(case["definesPropagate"], true, "{squash}");
        assert_eq!(case["result"], true, "{squash}: TypeScript removed it");
        assert_eq!(
            case["constantNeuronAdded"], true,
            "{squash}: TypeScript wired in a constant neuron"
        );
        assert_eq!(
            number(&case["targetBias"]),
            0.25,
            "{squash}: the bias was not folded"
        );

        // The capture's own creature, replayed through the port.
        let base = creature(
            2,
            vec![
                neuron("hidden", "hidden-0", "IDENTITY", 0.0),
                neuron("output", "output-0", squash, 0.25),
            ],
            vec![
                edge("input-0", "hidden-0", 1.0),
                edge("hidden-0", "output-0", 2.0),
                edge("input-1", "output-0", 1.0),
            ],
        );
        assert_eq!(
            remove_neuron_by_bias_fold(&base, "hidden-0", number(&case["activation"]), never_draw),
            Err(RemoveNeuronError::ConstantBranchNotPorted {
                target: "output-0".to_string()
            }),
            "{squash}"
        );
    }
}

/// The positive control: a target without `propagate` folds into its bias.
#[test]
fn a_target_without_propagate_takes_the_bias_fold() {
    for squash in ["IDENTITY", "TANH", "ReLU"] {
        let base = creature(
            2,
            vec![
                neuron("hidden", "hidden-0", "IDENTITY", 0.0),
                neuron("output", "output-0", squash, 0.25),
            ],
            vec![
                edge("input-0", "hidden-0", 1.0),
                edge("hidden-0", "output-0", 2.0),
                edge("input-1", "output-0", 1.0),
            ],
        );
        let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 1.5, never_draw)
            .unwrap()
            .unwrap();
        let target = removed
            .neurons
            .iter()
            .find(|n| n.uuid == "output-0")
            .unwrap();
        assert_eq!(target.bias, 3.25, "{squash}: 0.25 + 2.0 * 1.5");
    }
}

#[test]
fn unknown_and_non_hidden_neurons_and_bad_draws_are_refused() {
    let base = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    assert_eq!(
        remove_neuron_by_bias_fold(&base, "missing", 0.0, never_draw),
        Err(RemoveNeuronError::UnknownNeuron {
            uuid: "missing".to_string()
        })
    );
    assert_eq!(
        remove_neuron_by_bias_fold(&base, "output-0", 0.0, never_draw),
        Err(RemoveNeuronError::NotRemovable {
            uuid: "output-0".to_string(),
            neuron_type: "output".to_string()
        })
    );
    for draw in [1.0, -0.1] {
        assert_eq!(
            remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, || draw),
            Err(RemoveNeuronError::DrawOutOfRange { draw })
        );
    }
    let nan = remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, || f64::NAN);
    assert!(matches!(nan, Err(RemoveNeuronError::DrawOutOfRange { draw }) if draw.is_nan()));
}

#[test]
fn a_dangling_outward_target_is_refused_before_any_draw() {
    let base = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
            edge("hidden-0", "ghost", 1.0),
        ],
    );
    assert_eq!(
        remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, never_draw),
        Err(RemoveNeuronError::DanglingTarget {
            target: "ghost".to_string()
        })
    );
}

#[test]
fn the_reconnect_edge_lands_in_from_to_index_order() {
    // Runtime indices: input-0 = 0, input-1 = 1, hidden-0 = 2, output-0 = 3,
    // output-1 = 4. The list is in (from, to) order, as TypeScript keeps it.
    let base = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", "IDENTITY", 0.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
            neuron("output", "output-1", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-1", "output-1", 1.0),
            edge("hidden-0", "output-0", 1.0),
            edge("hidden-0", "output-1", 1.0),
        ],
    );
    // u = 0 reconnects output-0 from input-0: (0, 3) sorts before (1, 4).
    let removed = remove_neuron_by_bias_fold(&base, "hidden-0", 0.0, || 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(
        removed.synapses,
        vec![
            edge("input-0", "output-0", 0.0),
            edge("input-1", "output-1", 1.0),
        ]
    );
}

/// Native counterpart of NEAT-AI `test/compact/UnusedClipped.ts`.
///
/// `hidden-3` is `CLIPPED` (hard tanh, clamp to `[-1, 1]`) with bias 2 and
/// inbound weights -0.3 / +0.3 from inputs in `[0, 1)`, so its pre-activation
/// stays in `(1.7, 2.3)` and it always outputs exactly 1. TypeScript's trace
/// therefore reports a mean activation of 1, and folding 1 into the targets
/// must leave both outputs unchanged on every input. Choosing the neuron and
/// collecting the trace are caller policy; the fold is what is under test.
#[test]
fn removing_a_saturated_clipped_neuron_preserves_behaviour() {
    let base = creature(
        3,
        vec![
            neuron("hidden", "hidden-3", "CLIPPED", 2.0),
            neuron("output", "output-0", "IDENTITY", 1.0),
            neuron("output", "output-1", "IDENTITY", 0.0),
        ],
        vec![
            edge("input-0", "hidden-3", -0.3),
            edge("input-1", "hidden-3", 0.3),
            edge("hidden-3", "output-0", 0.6),
            edge("hidden-3", "output-1", 0.7),
            edge("input-2", "output-1", 0.8),
        ],
    );
    // The clamp of any pre-activation in (1.7, 2.3) to [-1, 1].
    let traced_mean = 1.0;
    let removed = remove_neuron_by_bias_fold(&base, "hidden-3", traced_mean, || 0.5)
        .unwrap()
        .unwrap();
    assert!(removed.neurons.iter().all(|n| n.uuid != "hidden-3"));
    // 1.0 + 0.6 * 1 and 0.0 + 0.7 * 1.
    assert_eq!(removed.neurons[0].bias, 1.6);
    assert_eq!(removed.neurons[1].bias, 0.7);

    let mut before = compile_creature(&base).expect("fixture compiles");
    let mut after = compile_creature(&removed).expect("compacted creature compiles");
    let steps = [0.0f32, 0.13, 0.5, 0.77, 0.999];
    let mut checked = 0;
    for &a in &steps {
        for &b in &steps {
            for &c in &steps {
                let sample = [a, b, c];
                let expected = before.activate(&sample, 2);
                let actual = after.activate(&sample, 2);
                for (e, x) in expected.iter().zip(&actual) {
                    // f32 forward pass: about 6e-8 relative per rounding.
                    assert!(
                        (e - x).abs() <= 1e-6 * e.abs().max(1.0),
                        "{sample:?}: {e} vs {x}"
                    );
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, steps.len().pow(3));
}

#[test]
fn a_creature_with_no_inputs_is_refused_by_the_width_rule() {
    let base = creature(
        0,
        vec![
            neuron("constant", "constant-0", "IDENTITY", 1.0),
            neuron("output", "output-0", "IDENTITY", 0.0),
        ],
        vec![edge("constant-0", "output-0", 1.0)],
    );
    let refused = remove_neuron_by_bias_fold(&base, "constant-0", 1.0, never_draw);
    assert!(
        matches!(refused, Err(RemoveNeuronError::InvalidWidth { .. })),
        "{refused:?}"
    );
}
