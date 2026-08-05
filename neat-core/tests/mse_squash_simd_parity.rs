//! Parity guard for the SIMD squash wiring in the MSE batched loss paths
//! (Issue #246). The batched MSE kernels feed their
//! per-lane sums through the vectorised `squash_x8` / `squash_x4`
//! approximations (shipped by #180) for the hot transcendental squashes,
//! falling back to the scalar inline squash for every other type.
//!
//! These are "what" tests: they build a real forward-only network, score a
//! packed batch through the public `mse_sum_batch_packed` entry point, and
//! assert the batched result matches the scalar single-record reference within
//! the documented SIMD tolerance. Distinct per-record inputs mean a lane
//! transposition in the wiring would break the parity, so the tests double as a
//! regression guard for the batching itself.

use neat_core::loss::mse_sum_batch_packed;
use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Build a tiny forward-only network: `num_inputs` inputs fully connected into
/// two hidden neurons and one output neuron, every non-input neuron using
/// `squash`.
fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    // Two hidden neurons, each drawing from all inputs.
    for h in 0..2 {
        let start = synapses.len() as u32;
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                weight: 0.3 - 0.11 * (i as f32) + 0.07 * (h as f32),
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

    // Output neuron draws from the two hidden neurons.
    let start = synapses.len() as u32;
    let hidden0 = num_inputs as u16;
    let hidden1 = num_inputs as u16 + 1;
    synapses.push(SynapseData {
        weight: 0.6,
        from_index: hidden0,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.4,
        from_index: hidden1,
        synapse_type: 0,
    });
    neurons.push(NeuronData {
        bias: 0.01,
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
        // NEAT-AI-scorer#531 — fused MSE interleaved scratch.
        mse_inter: vec![0.0; num_neurons * 8],
    }
}

/// Deterministic packed records: each record is `[inputs..., target]`, with
/// distinct values per record so a lane mix-up would surface.
fn build_records(num_records: usize, input_size: usize) -> Vec<f32> {
    let values_per_record = input_size + 1;
    let mut records = vec![0.0f32; num_records * values_per_record];
    for r in 0..num_records {
        let base = r * values_per_record;
        for i in 0..input_size {
            // Spread inputs across a range that exercises the squash curves.
            records[base + i] = -1.5 + 0.37 * (r as f32) - 0.21 * (i as f32);
        }
        records[base + input_size] = 0.25 + 0.03 * (r as f32);
    }
    records
}

/// Scalar single-record reference: `forward_only = false` forces the
/// non-batched path (scalar `activate_into` + scalar squash) so we compare the
/// SIMD batched result against the exact scalar numerics on identical records.
fn reference_mse(squash: SquashType, num_records: usize) -> f64 {
    let input_size = 4;
    let mut net = build_network(input_size, squash);
    let records = build_records(num_records, input_size);
    mse_sum_batch_packed(&mut net, &records, input_size, 1, false)
}

/// Batched result via the forward-only SIMD paths under test.
fn batched_mse(squash: SquashType, num_records: usize) -> f64 {
    let input_size = 4;
    let mut net = build_network(input_size, squash);
    let records = build_records(num_records, input_size);
    mse_sum_batch_packed(&mut net, &records, input_size, 1, true)
}

/// Summed-MSE tolerance: each activation is within `SQUASH_SIMD_MAX_ABS_ERR`
/// of scalar, so the per-record squared-error delta is tiny; a generous
/// per-record allowance still stays far below any scoring-decision threshold.
fn tolerance(num_records: usize) -> f64 {
    (num_records as f64) * 1.0e-4
}

fn assert_parity(squash: SquashType, num_records: usize) {
    let reference = reference_mse(squash, num_records);
    let batched = batched_mse(squash, num_records);
    let diff = (reference - batched).abs();
    assert!(
        diff <= tolerance(num_records),
        "{squash:?} n={num_records}: batched MSE {batched} diverged from scalar {reference} by {diff} (tol {})",
        tolerance(num_records)
    );
}

/// 8-record batch drives the 8-way SIMD squash block for each vectorised type.
#[test]
fn mse_8way_matches_scalar_for_vectorised_squashes() {
    for squash in [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
    ] {
        assert_parity(squash, 8);
    }
}

/// 12 records exercise the 8-way block plus the 4-record remainder block inside
/// `mse_sum_batch_8way`.
#[test]
fn mse_8way_with_remainder_matches_scalar() {
    for squash in [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
    ] {
        assert_parity(squash, 12);
    }
}

/// 5–7 records fall to the shared skeleton's 4-record group, driving the 4-way
/// SIMD block.
#[test]
fn mse_4way_matches_scalar_for_vectorised_squashes() {
    for squash in [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
    ] {
        for n in 5..=7 {
            assert_parity(squash, n);
        }
    }
}

/// Non-vectorised types (e.g. Sine) hit the `None` branch and keep the scalar
/// fallback, so the SIMD wiring leaves their numerics as before: batched and
/// scalar paths still agree within tolerance across the 4-way and 8-way paths.
#[test]
fn mse_non_vectorised_squash_matches_scalar() {
    for squash in [SquashType::Sine, SquashType::Relu, SquashType::Identity] {
        for n in [5, 8, 12] {
            assert_parity(squash, n);
        }
    }
}
