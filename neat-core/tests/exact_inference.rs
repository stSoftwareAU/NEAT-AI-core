//! Behavioural coverage for the validated exact-size inference prototype
//! (Issue #511).
//!
//! The prototype is an A/B experiment behind the non-default
//! `experimental-exact-inference` feature; these are its parity and
//! error-behaviour tests. They are "what" tests: real networks are activated
//! through the public entry points and the observable outputs, errors and
//! post-call state are asserted.
//!
//! Two contracts are pinned:
//!
//! - **Parity** — for a correctly-shaped call, `activate_into_exact` produces
//!   bit-identical outputs to the control `activate_into`, across every squash
//!   family (standard, aggregate, constant) and across repeated calls.
//! - **Explicit failure** — a wrong input or output length is a typed error,
//!   never a silent truncation, and the network is not mutated.

#![cfg(feature = "experimental-exact-inference")]

use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NetworkError, NeuronData, SynapseData};

/// Build a deterministic network: `num_inputs` inputs feeding a layer of
/// `squashes.len()` neurons, each fully connected to every input.
///
/// Fan-in is the full input width, so the standard neurons run the SIMD
/// weighted-sum kernel and the aggregates run their scalar loops — the same
/// mix a real creature presents.
fn layered_network(num_inputs: usize, squashes: &[SquashType]) -> CompiledNetwork {
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    for (n, squash) in squashes.iter().enumerate() {
        let start = synapses.len();
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                // Deterministic, sign-varying weights.
                weight: 0.25 + (i as f32) * 0.1 - (n as f32) * 0.15,
                from_index: i as u16,
                synapse_type: (i % 3) as u8,
            });
        }
        neurons.push(NeuronData {
            bias: 0.05 * (n as f32) - 0.1,
            start_synapse: start as u32,
            num_synapses: num_inputs as u16,
            squash_type: *squash as u8,
            is_constant: false,
        });
    }

    let num_neurons = num_inputs + neurons.len();
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_neurons - num_inputs],
        trace_data_buffer: Vec::new(),
        batch_activations: std::array::from_fn(|_| vec![0.0; num_neurons]),
        batch_hints: std::array::from_fn(|_| vec![0.0; num_neurons - num_inputs]),
        batch_traces: std::array::from_fn(|_| Vec::new()),
    }
}

/// A representative mix: hot inline squashes, a `libm` squash, all three
/// aggregates and the three vectorised aggregates.
const MIXED_SQUASHES: [SquashType; 10] = [
    SquashType::Tanh,
    SquashType::Relu,
    SquashType::Logistic,
    SquashType::Gelu,
    SquashType::Minimum,
    SquashType::Maximum,
    SquashType::If,
    SquashType::Hypotenuse,
    SquashType::Mean,
    SquashType::Identity,
];

fn deterministic_input(len: usize, seed: f32) -> Vec<f32> {
    (0..len)
        .map(|i| ((i as f32) * 0.37 + seed).sin() * 1.7)
        .collect()
}

fn assert_bit_identical(actual: &[f32], expected: &[f32], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            e.to_bits(),
            "{what}: output[{i}] expected {e}, got {a}"
        );
    }
}

#[test]
fn exact_entry_point_matches_control_bit_for_bit() {
    let num_inputs = 37;
    let mut control = layered_network(num_inputs, &MIXED_SQUASHES);
    let mut prototype = layered_network(num_inputs, &MIXED_SQUASHES);

    for num_outputs in [1usize, 4, MIXED_SQUASHES.len()] {
        for seed in [0.0f32, 1.3, -2.7] {
            let input = deterministic_input(num_inputs, seed);
            let mut control_out = vec![0.0f32; num_outputs];
            let mut prototype_out = vec![0.0f32; num_outputs];

            control.activate_into(&input, &mut control_out);
            prototype
                .activate_into_exact(&input, &mut prototype_out)
                .expect("correctly shaped call");

            assert_bit_identical(
                &prototype_out,
                &control_out,
                &format!("num_outputs={num_outputs} seed={seed}"),
            );
        }
    }
}

#[test]
fn exact_entry_point_matches_control_across_repeated_calls() {
    // Buffer reuse means call N sees call N-1's activations; the two paths must
    // stay in step over a sequence, not just on a single call.
    let num_inputs = 16;
    let mut control = layered_network(num_inputs, &MIXED_SQUASHES);
    let mut prototype = layered_network(num_inputs, &MIXED_SQUASHES);
    let mut control_out = vec![0.0f32; 3];
    let mut prototype_out = vec![0.0f32; 3];

    for call in 0..8 {
        let input = deterministic_input(num_inputs, call as f32 * 0.9);
        control.activate_into(&input, &mut control_out);
        prototype
            .activate_into_exact(&input, &mut prototype_out)
            .expect("correctly shaped call");
        assert_bit_identical(&prototype_out, &control_out, &format!("call {call}"));
    }
}

#[test]
fn constant_neurons_activate_identically_through_both_paths() {
    let num_inputs = 8;
    let mut control = layered_network(num_inputs, &MIXED_SQUASHES);
    control.neurons[2].is_constant = true;
    control.neurons[5].is_constant = true;
    let mut prototype = control.clone();

    let input = deterministic_input(num_inputs, 0.4);
    let mut control_out = vec![0.0f32; 6];
    let mut prototype_out = vec![0.0f32; 6];

    control.activate_into(&input, &mut control_out);
    prototype
        .activate_into_exact(&input, &mut prototype_out)
        .expect("correctly shaped call");

    assert_bit_identical(&prototype_out, &control_out, "constant neurons");
}

#[test]
fn short_input_is_rejected_instead_of_truncated() {
    let mut net = layered_network(12, &MIXED_SQUASHES);
    let mut out = vec![0.0f32; 2];

    let err = net
        .activate_into_exact(&deterministic_input(11, 0.0), &mut out)
        .expect_err("short input must be rejected");

    assert_eq!(
        err,
        NetworkError::InputLengthMismatch {
            expected: 12,
            actual: 11,
        }
    );
}

#[test]
fn long_input_is_rejected_instead_of_truncated() {
    let mut net = layered_network(12, &MIXED_SQUASHES);
    let mut out = vec![0.0f32; 2];

    let err = net
        .activate_into_exact(&deterministic_input(13, 0.0), &mut out)
        .expect_err("long input must be rejected");

    assert_eq!(
        err,
        NetworkError::InputLengthMismatch {
            expected: 12,
            actual: 13,
        }
    );
}

#[test]
fn empty_and_oversized_output_buffers_are_rejected() {
    let mut net = layered_network(12, &MIXED_SQUASHES);
    let input = deterministic_input(12, 0.2);
    let max = MIXED_SQUASHES.len();

    let mut empty: Vec<f32> = Vec::new();
    assert_eq!(
        net.activate_into_exact(&input, &mut empty)
            .expect_err("empty output must be rejected"),
        NetworkError::OutputLengthMismatch { max, actual: 0 }
    );

    let mut oversized = vec![0.0f32; max + 1];
    assert_eq!(
        net.activate_into_exact(&input, &mut oversized)
            .expect_err("oversized output must be rejected"),
        NetworkError::OutputLengthMismatch {
            max,
            actual: max + 1,
        }
    );
}

#[test]
fn a_rejected_call_leaves_network_and_buffers_untouched() {
    let num_inputs = 12;
    let mut net = layered_network(num_inputs, &MIXED_SQUASHES);
    let good = deterministic_input(num_inputs, 1.1);
    let mut out = vec![0.0f32; 3];

    // Establish state with a valid call, then record the reference outputs.
    net.activate_into_exact(&good, &mut out)
        .expect("valid call");
    let after_valid = net.activations.clone();

    let mut scratch = vec![7.5f32; 3];
    let _ = net.activate_into_exact(&deterministic_input(3, 0.0), &mut scratch);
    let mut empty: Vec<f32> = Vec::new();
    let _ = net.activate_into_exact(&good, &mut empty);

    assert_eq!(
        net.activations, after_valid,
        "a rejected call must not write activations"
    );
    assert_eq!(
        scratch,
        vec![7.5f32; 3],
        "a rejected call must not write the caller's output buffer"
    );
}

/// The stale-input asymmetry the issue flags: after a full-width call, the
/// control's shorter follow-up call keeps the previous call's values in the
/// input slots it does not cover. This documents the *contrast* — it is a
/// property of the control, not a performance claim, and the exact entry point
/// rejects the call that would produce it.
#[test]
fn control_keeps_stale_inputs_where_exact_entry_point_rejects_the_call() {
    let num_inputs = 12;
    let mut net = layered_network(num_inputs, &[SquashType::Identity]);
    let mut out = vec![0.0f32; 1];

    let full = deterministic_input(num_inputs, 0.6);
    net.activate_into(&full, &mut out);

    // Control: a 4-wide input leaves slots 4..12 holding the previous call.
    let short = deterministic_input(4, 0.6);
    net.activate_into(&short, &mut out);
    assert_eq!(
        net.activations[num_inputs - 1].to_bits(),
        full[num_inputs - 1].to_bits(),
        "control retains the earlier call's value in an uncovered input slot"
    );

    // Prototype: the same call is refused outright.
    assert_eq!(
        net.activate_into_exact(&short, &mut out)
            .expect_err("short input must be rejected"),
        NetworkError::InputLengthMismatch {
            expected: num_inputs,
            actual: 4,
        }
    );
}

#[test]
fn dimension_errors_describe_the_mismatch() {
    let input_err = NetworkError::InputLengthMismatch {
        expected: 2461,
        actual: 2460,
    };
    assert_eq!(
        input_err.to_string(),
        "Input length 2460 does not match the network's 2461 inputs"
    );

    let output_err = NetworkError::OutputLengthMismatch {
        max: 1666,
        actual: 0,
    };
    assert_eq!(
        output_err.to_string(),
        "Output length 0 is not fillable by this network (expected 1..=1666)"
    );
}
