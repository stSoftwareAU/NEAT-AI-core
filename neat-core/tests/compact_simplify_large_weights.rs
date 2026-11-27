//! Large-weight simplification (Issue #778), graded against the frozen
//! TypeScript captures, the shared magnitude-penalty corpus and an independent
//! forward-pass oracle (`compile_creature` + `activate`).

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeights};
use neat_core::{
    CreatureExport, MagnitudePenaltyError, McmcAcceptance, NeuronExport, SynapseExport,
    compile_creature, magnitude_penalty, mean_magnitude_penalty, simplify_large_weights,
    weight_bias_penalty,
};

fn records() -> Vec<serde_json::Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy_simplify.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn record(name: &str) -> serde_json::Value {
    records()
        .into_iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("capture {name}"))
}

/// A JavaScript `String(number)`.
fn js_number(value: &serde_json::Value) -> f64 {
    match value.as_str().expect("numbers are captured as strings") {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        text => text.parse::<f64>().unwrap(),
    }
}

fn same_bits(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

fn memetic() -> MemeticExport {
    MemeticExport {
        biases: BTreeMap::from([("hidden-0".to_string(), 0.5)]),
        weights: MemeticWeights::default(),
        extra: serde_json::Map::new(),
    }
}

fn creature_from(value: &serde_json::Value, with_memetic: bool) -> CreatureExport {
    CreatureExport {
        input: value["input"].as_u64().unwrap() as usize,
        output: value["output"].as_u64().unwrap() as usize,
        neurons: value["neurons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| NeuronExport {
                id: None,
                neuron_type: n["type"].as_str().unwrap().to_string(),
                uuid: n["uuid"].as_str().unwrap().to_string(),
                bias: js_number(&n["bias"]),
                squash: n["squash"].as_str().map(str::to_string),
            })
            .collect(),
        synapses: value["synapses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| SynapseExport {
                from_uuid: s["fromUUID"].as_str().unwrap().to_string(),
                to_uuid: s["toUUID"].as_str().unwrap().to_string(),
                weight: js_number(&s["weight"]),
                synapse_type: s["type"].as_str().map(str::to_string),
            })
            .collect(),
        semantic_version: None,
        forward_only: true,
        memetic: with_memetic.then(memetic),
    }
}

#[test]
fn frozen_captures_replay_bit_for_bit() {
    let mut replayed = 0;
    for r in records() {
        if r["operation"] != "simplifyLargeWeights" {
            continue;
        }
        let name = r["name"].as_str().unwrap().to_string();
        let input = creature_from(&r["input"], true);
        let draw = r["draw"].as_f64();
        let mut calls = 0u64;
        let mut uniform = || {
            calls += 1;
            draw.unwrap_or(0.5)
        };
        let mcmc = r["temperature"].as_f64().map(|temperature| McmcAcceptance {
            temperature,
            uniform: &mut uniform,
        });

        let result = simplify_large_weights(&input, mcmc);

        assert_eq!(result.changed(), r["changed"].as_bool().unwrap(), "{name}");
        assert_eq!(calls, r["draws"].as_u64().unwrap(), "{name}: draws");
        for (got, want) in result
            .creature
            .neurons
            .iter()
            .zip(r["neurons"].as_array().unwrap())
        {
            assert!(
                same_bits(got.bias, js_number(&want["bias"])),
                "{name}: bias"
            );
        }
        assert_eq!(
            result.creature.neurons.len(),
            r["neurons"].as_array().unwrap().len()
        );
        for (got, want) in result
            .creature
            .synapses
            .iter()
            .zip(r["synapses"].as_array().unwrap())
        {
            assert!(
                same_bits(got.weight, js_number(&want["weight"])),
                "{name}: weight"
            );
        }
        assert_eq!(
            result.creature.synapses.len(),
            r["synapses"].as_array().unwrap().len()
        );
        if r["memeticKept"].as_bool().unwrap() {
            assert_eq!(result.creature.memetic, input.memetic, "{name}");
        } else {
            assert!(result.creature.memetic.is_none(), "{name}");
        }
        assert!(
            (weight_bias_penalty(&input) - r["before"].as_f64().unwrap()).abs() < 1e-12,
            "{name}"
        );
        assert!(
            (weight_bias_penalty(&result.creature) - r["after"].as_f64().unwrap()).abs() < 1e-12,
            "{name}"
        );
        replayed += 1;
    }
    // 15 hand-made cases plus the 8 per-squash fixtures of the original
    // `CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts`.
    assert_eq!(replayed, 15 + 8);
}

#[test]
fn an_empty_creature_has_no_penalty() {
    let r = record("penalty_empty");
    let empty = CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![],
        synapses: vec![],
        semantic_version: None,
        forward_only: true,
        memetic: None,
    };
    assert_eq!(r["penalty"].as_f64().unwrap(), 0.0);
    assert_eq!(weight_bias_penalty(&empty), 0.0);
}

#[test]
fn magnitude_penalty_matches_the_shared_corpus() {
    let r = record("magnitude_penalty_corpus");
    let tolerance = r["corpus"]["tolerance"].as_f64().unwrap();
    let cases = r["corpus"]["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let magnitude = case["magnitude"].as_f64().unwrap();
        let want = case["penalty"].as_f64().unwrap();
        let got = magnitude_penalty(magnitude).unwrap();
        assert!(
            (got - want).abs() <= tolerance,
            "{magnitude}: {got} vs {want}"
        );
        assert_eq!(
            magnitude_penalty(-magnitude).unwrap(),
            got,
            "sign of {magnitude}"
        );
    }
    assert_eq!(
        magnitude_penalty(f64::NAN),
        Err(MagnitudePenaltyError::NotANumber)
    );
}

#[test]
fn the_mean_skips_non_finite_values_and_is_zero_when_empty() {
    // log10(10) = 1 -> 0.999 / 12; 0.5 costs nothing; NaN and infinity are
    // skipped, so the mean is over the two finite values.
    let expected = 0.999 / 12.0 / 2.0;
    let got = mean_magnitude_penalty([f64::NAN, f64::INFINITY, 10.0, 0.5]);
    assert!((got - expected).abs() < 1e-15, "{got} vs {expected}");
    assert_eq!(mean_magnitude_penalty(Vec::new()), 0.0);
}

/// The compiled network evaluates in `f32` (about 6e-8 relative per rounding),
/// so 1e-9 is unreachable; 1e-6 is far above rounding and far below any real slip.
const F32_TOLERANCE: f64 = 1e-6;

fn outputs(creature: &CreatureExport, inputs: &[f32]) -> Vec<f32> {
    let mut network = compile_creature(creature).expect("fixture compiles");
    network.activate(inputs, creature.output)
}

#[test]
fn rescaling_preserves_behaviour() {
    let mut checked = 0;
    // Every squash `CANDIDATE_SQUASHES` lists is rescaled by at least one case.
    for name in [
        "absolute",
        "relu_negative",
        "tiny_inbound",
        "typed_if",
        "supported_ReLU",
        "supported_LeakyReLU",
        "supported_MAXIMUM",
        "supported_MINIMUM",
        "supported_IF",
        "supported_HYPOT",
        "supported_HYPOTv2",
        "supported_MEAN",
    ] {
        let r = record(name);
        let input = creature_from(&r["input"], false);
        let result = simplify_large_weights(&input, None);
        assert!(result.changed(), "{name}");
        // The sample points of the original tests, by input width.
        let samples: Vec<Vec<f32>> = match input.input {
            1 => [-3.0, -1.0, -0.25, 0.0, 0.25, 1.0, 3.0, 0.5, -2.0]
                .iter()
                .map(|&x| vec![x])
                .collect(),
            2 => vec![
                vec![-2.0, -2.0],
                vec![-2.0, 2.0],
                vec![2.0, -2.0],
                vec![2.0, 2.0],
            ],
            _ => vec![
                vec![-2.0, -3.0, -4.0],
                vec![-2.0, 3.0, 4.0],
                vec![2.0, -3.0, -4.0],
                vec![2.0, 3.0, 4.0],
                vec![1.0, 2.0, 3.0],
                vec![-1.0, 2.0, 3.0],
                vec![0.5, -2.0, 4.0],
            ],
        };
        for sample in samples {
            let before = outputs(&input, &sample);
            let after = outputs(&result.creature, &sample);
            assert_eq!(before.len(), after.len(), "{name}");
            for (a, b) in before.iter().zip(&after) {
                let (a, b) = (f64::from(*a), f64::from(*b));
                assert!(
                    (a - b).abs() <= F32_TOLERANCE * a.abs().max(1.0),
                    "{name} {sample:?}: {a} vs {b}"
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 12);
}

#[test]
fn a_non_positive_temperature_is_greedy() {
    let r = record("worse_greedy");
    let input = creature_from(&r["input"], true);
    for temperature in [0.0, -1.0] {
        let mut calls = 0;
        let mut uniform = || {
            calls += 1;
            0.0
        };
        let result = simplify_large_weights(
            &input,
            Some(McmcAcceptance {
                temperature,
                uniform: &mut uniform,
            }),
        );
        assert_eq!(calls, 0, "temperature {temperature}");
        assert!(!result.changed());
        assert_eq!(result.creature, input);
    }
}

#[test]
fn the_rescaled_copy_differs_from_an_input_that_is_left_alone() {
    let input = creature_from(&record("absolute")["input"], true);
    let snapshot = input.clone();
    let result = simplify_large_weights(&input, None);
    assert!(result.changed());
    // Derivation: the frozen capture rescales ABSOLUTE's 1e6 bias, so the
    // returned copy must differ from the input, which stays as it was.
    assert_ne!(result.creature, input);
    assert_eq!(input, snapshot, "the input must not be modified");
}
