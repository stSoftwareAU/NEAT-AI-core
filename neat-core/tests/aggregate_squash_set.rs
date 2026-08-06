//! Behavioural coverage for the aggregate-squash set (Issue #446).
//!
//! One rule decides *which squash types are aggregates* — `Minimum`,
//! `Maximum`, `If`, `Hypotenuse`, `HypotenuseV2` and `Mean`: the six that
//! cannot be lane-vectorised and must take the exact single-record kernel.
//! That one rule drives both dispatch (scalar kernel vs SIMD lane path) and
//! hint semantics, and every consumer must agree with it — a site that
//! disagreed would route an aggregate down the weighted-sum path and silently
//! produce numbers that differ from `activate()`.
//!
//! These are "what" tests: they call [`SquashType::is_aggregate`] and the
//! public activation, scoring and unsquash entry points and assert on the
//! observable results, so a drifted membership list fails here regardless of
//! how the set is spelled.

use neat_core::range::apply_limit_range;
use neat_core::squash::{SquashType, apply_squash};
use neat_core::unsquash::apply_unsquash;
use neat_core::{CompiledNetwork, NeuronData, SynapseData, hot_synapse_soa};

/// The six aggregate squashes.
const AGGREGATES: [SquashType; 6] = [
    SquashType::Minimum,
    SquashType::Maximum,
    SquashType::If,
    SquashType::Hypotenuse,
    SquashType::HypotenuseV2,
    SquashType::Mean,
];

/// Highest discriminant currently defined on `SquashType` (`Mean = 37`).
const MAX_SQUASH_ID: u8 = 37;

const INPUT_SIZE: usize = 6;

/// Enough records to cover the 8-record group, the 4-record group and the
/// scalar tail in one scoring call (8 + 4 + 1).
const RECORD_COUNT: usize = 13;

/// The batched kernels reach the same activation rule as the single-record
/// reference, so the only permitted gap is f32 summation order inside the SIMD
/// weighted sums.
const TOLERANCE: f32 = 1.0e-5;

/// Every `SquashType`, in discriminant order.
fn all_squash_types() -> Vec<SquashType> {
    (0..=MAX_SQUASH_ID).map(SquashType::from).collect()
}

/// Build a network: `INPUT_SIZE` inputs fully connected into two hidden
/// neurons and one output neuron, every non-input neuron using `squash`.
/// Mirrors the fixture in `aggregate_squash_tail_parity.rs`.
fn build_network(squash: SquashType) -> CompiledNetwork {
    let num_inputs = INPUT_SIZE;
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    for h in 0..2 {
        let start = synapses.len() as u32;
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                weight: 0.27 - 0.1 * (i as f32) + 0.06 * (h as f32),
                from_index: i as u16,
                // Alternate the synapse type so the `If` arm sees condition,
                // positive and negative edges rather than one branch only.
                synapse_type: (i % 3) as u8,
            });
        }
        neurons.push(NeuronData {
            bias: 0.05 * (h as f32) - 0.02,
            start_synapse: start,
            num_synapses: num_inputs as u16,
            squash_type: squash as u8,
            is_constant: false,
        });
    }

    let start = synapses.len() as u32;
    synapses.push(SynapseData {
        weight: 0.62,
        from_index: num_inputs as u16,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.44,
        from_index: num_inputs as u16 + 1,
        synapse_type: 1,
    });
    neurons.push(NeuronData {
        bias: 0.015,
        start_synapse: start,
        num_synapses: 2,
        squash_type: squash as u8,
        is_constant: false,
    });

    let num_non_inputs = neurons.len();
    let num_neurons = num_inputs + num_non_inputs;
    let (hot_weights, hot_from) = hot_synapse_soa(&synapses);
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        hot_weights,
        hot_from,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::new(),
        batch_activations: [
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
        ],
        batch_hints: [
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
        ],
        batch_traces: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        // NEAT-AI-scorer#531 — fused MSE interleaved scratch.
        mse_inter: vec![0.0; num_neurons * 8],
    }
}

/// Flat inputs, distinct per record so a mis-routed record cannot hide.
fn build_inputs(num_records: usize) -> Vec<f32> {
    let mut inputs = vec![0.0f32; num_records * INPUT_SIZE];
    for r in 0..num_records {
        for i in 0..INPUT_SIZE {
            inputs[r * INPUT_SIZE + i] = (((r * 7 + i * 3) as f32) * 0.013).sin() * 0.9;
        }
    }
    inputs
}

#[test]
fn is_aggregate_holds_for_exactly_the_six_aggregate_squashes() {
    for squash in all_squash_types() {
        let expected = AGGREGATES.contains(&squash);
        assert_eq!(
            squash.is_aggregate(),
            expected,
            "{squash:?}: is_aggregate() disagrees with the aggregate set"
        );
    }
}

#[test]
fn batched_scoring_matches_single_record_activation_for_every_squash_type() {
    let inputs = build_inputs(RECORD_COUNT);

    for squash in all_squash_types() {
        let batched = build_network(squash).score_records_flat(&inputs, INPUT_SIZE, 1);
        assert_eq!(batched.len(), RECORD_COUNT);

        let mut reference_net = build_network(squash);
        for r in 0..RECORD_COUNT {
            let record = &inputs[r * INPUT_SIZE..(r + 1) * INPUT_SIZE];
            let expected = reference_net.activate(record, 1)[0];
            let diff = (expected - batched[r]).abs();
            assert!(
                diff <= TOLERANCE || (expected.is_nan() && batched[r].is_nan()),
                "{squash:?} record {r}: batched {} diverged from single-record {expected} by {diff}",
                batched[r]
            );
        }
    }
}

#[test]
fn traced_hint_equals_activation_for_aggregate_squashes() {
    let inputs = build_inputs(1);

    for squash in AGGREGATES {
        let mut net = build_network(squash);
        let result = net.activate_and_trace(&inputs, 1);
        // Layout: [outputs, non-input activations, hint values, trace data].
        let activations = &result[1..4];
        let hints = &result[4..7];
        for (idx, (&activation, &hint)) in activations.iter().zip(hints).enumerate() {
            assert_eq!(
                hint, activation,
                "{squash:?} neuron {idx}: aggregate hint must be the activation"
            );
        }
    }
}

#[test]
fn traced_hint_is_the_pre_squash_value_for_standard_squashes() {
    let inputs = build_inputs(1);

    for squash in all_squash_types().into_iter().filter(|s| !s.is_aggregate()) {
        let mut net = build_network(squash);
        let result = net.activate_and_trace(&inputs, 1);
        let activations = &result[1..4];
        let hints = &result[4..7];
        for (idx, (&activation, &hint)) in activations.iter().zip(hints).enumerate() {
            let expected = apply_limit_range(squash, apply_squash(squash, hint));
            let diff = (expected - activation).abs();
            assert!(
                diff <= TOLERANCE || (expected.is_nan() && activation.is_nan()),
                "{squash:?} neuron {idx}: hint {hint} does not squash to activation {activation} (got {expected})"
            );
        }
    }
}

#[test]
fn unsquash_prefers_the_hint_for_every_aggregate_squash() {
    for squash in all_squash_types().into_iter().filter(|s| s.is_aggregate()) {
        // A finite hint is the recovered pre-squash value.
        assert_eq!(
            apply_unsquash(squash, 0.5, 1.25),
            1.25,
            "{squash:?}: aggregate unsquash must return the hint"
        );
        // Without a usable hint it falls back to the activation.
        assert_eq!(
            apply_unsquash(squash, 0.5, f32::NAN),
            0.5,
            "{squash:?}: aggregate unsquash must fall back to the activation"
        );
    }
}
