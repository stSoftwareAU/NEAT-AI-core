//! Parity guard for routing the fused MSE batch loss lane through the #287
//! record-interleaved gather (Issue #384).
//!
//! `mse_sum_batch_packed` — the `#[wasm_bindgen]` entry production scoring
//! calls — now dispatches standard-squash networks through the interleaved
//! gather and keeps aggregate-squash networks on the exact per-lane path. These
//! are "what" tests: they build real forward-only networks, score packed
//! batches through the public `mse_sum_batch_packed` entry point, and assert the
//! forward-only batched result matches the scalar single-record reference
//! (`forward_only = false`) within the documented SIMD tolerance. Distinct
//! per-record inputs mean a lane transposition in the interleaved transpose or
//! the MSE reduction would break the parity.
//!
//! Record counts cover the acceptance set (0, 1, 7, 8, 9, 12, 4096): the scalar
//! path (< 4), the 4-way path (4–7), the 8-way group, the group-plus-tail and
//! group-plus-4-way-remainder splits, and the production steady state.

use neat_core::loss::mse_sum_batch_packed;
use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Build a forward-only network: `num_inputs` inputs fully connected into two
/// hidden neurons and one output neuron, every non-input neuron using `squash`.
fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    for h in 0..2 {
        let start = synapses.len() as u32;
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                weight: 0.27 - 0.1 * (i as f32) + 0.06 * (h as f32),
                from_index: i as u16,
                synapse_type: 0,
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
    let hidden0 = num_inputs as u16;
    let hidden1 = num_inputs as u16 + 1;
    synapses.push(SynapseData {
        weight: 0.62,
        from_index: hidden0,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.44,
        from_index: hidden1,
        synapse_type: 0,
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
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
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
    }
}

/// Packed `[inputs..., target]` records, distinct per record so a lane mix-up
/// cannot hide. Inputs are bounded (sinusoidal) so the per-record MSE stays
/// O(1) regardless of `num_records` — the summed SIMD-squash approximation gap
/// then grows only linearly, keeping the parity tolerance meaningful at the
/// production 4096-record count.
fn build_records(num_records: usize, input_size: usize) -> Vec<f32> {
    let values_per_record = input_size + 1;
    let mut records = vec![0.0f32; num_records * values_per_record];
    for r in 0..num_records {
        let base = r * values_per_record;
        for i in 0..input_size {
            records[base + i] = (((r * 7 + i * 3) as f32) * 0.013).sin() * 0.9;
        }
        records[base + input_size] = (((r * 5 + 1) as f32) * 0.011).cos() * 0.5;
    }
    records
}

const INPUT_SIZE: usize = 6;
/// Record counts from the acceptance criteria: scalar (0, 1), 4-way (7), 8-way
/// group (8), group + scalar tail (9), group + 4-way remainder (12), and the
/// production steady state (4096).
const COUNTS: [usize; 7] = [0, 1, 7, 8, 9, 12, 4096];

/// Summed-MSE tolerance. Each activation of a vectorised squash is within
/// `SQUASH_SIMD_MAX_ABS_ERR` of scalar, so the per-record squared-error gap is
/// bounded and the summed gap grows linearly in the record count; a generous
/// per-record allowance stays far below any gross dispatch/lane bug (which
/// would shift the fused sum by orders of magnitude on these bounded inputs).
/// The strict bit-identity of the interleaved reroute is proven separately by
/// the `interleaved_mse_parity` unit tests.
fn tolerance(num_records: usize) -> f64 {
    (num_records as f64) * 3.0e-3 + 1.0e-6
}

fn assert_parity(squash: SquashType, num_records: usize) {
    let mut batched_net = build_network(INPUT_SIZE, squash);
    let mut reference_net = build_network(INPUT_SIZE, squash);
    let records = build_records(num_records, INPUT_SIZE);

    // forward_only = true drives the SIMD batch paths under test; false forces
    // the scalar single-record reference on identical records.
    let batched = mse_sum_batch_packed(&mut batched_net, &records, INPUT_SIZE, 1, true);
    let reference = mse_sum_batch_packed(&mut reference_net, &records, INPUT_SIZE, 1, false);

    let diff = (reference - batched).abs();
    assert!(
        diff <= tolerance(num_records),
        "{squash:?} n={num_records}: batched MSE {batched} diverged from scalar {reference} by {diff} (tol {})",
        tolerance(num_records)
    );
}

/// Standard-squash networks route through the interleaved gather. Covers the
/// vectorised transcendental types and the scalar-fallback types.
#[test]
fn interleaved_path_matches_scalar_reference() {
    for squash in [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
        SquashType::Relu,
        SquashType::Identity,
        SquashType::Sine,
    ] {
        for &n in &COUNTS {
            assert_parity(squash, n);
        }
    }
}

// Aggregate-squash networks (Minimum/Maximum/If/Hypotenuse/HypotenuseV2/Mean)
// keep the exact per-lane scattered path — the dispatch routes them away from
// the interleaved gather, leaving their result bit-identical to the pre-#384
// code. That bit-identity is proven directly against the scattered kernel in
// `loss::interleaved_mse_parity::aggregate_dispatch_stays_on_scattered_path`
// (a unit test, since it compares the two private kernels), which is a tighter
// guard than an approximate scalar-reference comparison here.
