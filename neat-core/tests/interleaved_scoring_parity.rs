//! Parity guard for the record-interleaved scoring fast path (Issue #287).
//!
//! `CompiledNetwork::score_batch_into` dispatches between a record-interleaved
//! layout (fast path, taken when the network has no aggregate-squash neurons)
//! and the original per-lane layout (fallback, taken when it does). These are
//! "what" tests: they build real forward networks, score batches through the
//! public `score_records_flat` entry point, and assert the flat output matches
//! the scalar single-record `activate` reference — so a lane transposition, a
//! dropped tail record, or a wrong dispatch would surface as a parity break.
//!
//! Record counts straddle the 8-record group boundary (1, 7, 8, 9, 16, 17) so
//! both the vectorised group path and the exact single-record tail are covered.

use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Build a two-hidden-plus-one-output feedforward network where every non-input
/// neuron uses `squash`. `num_inputs` inputs fully connect into each hidden
/// neuron; both hiddens feed the output.
fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    for h in 0..2 {
        let start = synapses.len() as u32;
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                weight: 0.25 - 0.09 * (i as f32) + 0.05 * (h as f32),
                from_index: i as u16,
                synapse_type: 0,
            });
        }
        neurons.push(NeuronData {
            bias: 0.03 * (h as f32) - 0.01,
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
        weight: 0.7,
        from_index: hidden0,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.5,
        from_index: hidden1,
        synapse_type: 0,
    });
    neurons.push(NeuronData {
        bias: 0.02,
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

/// Deterministic distinct input records so a lane mix-up cannot hide.
fn records(num_inputs: usize, count: usize) -> Vec<Vec<f32>> {
    (0..count)
        .map(|r| {
            (0..num_inputs)
                .map(|i| ((r * 7 + i * 3) as f32 * 0.017).sin() * 0.9)
                .collect()
        })
        .collect()
}

/// Pack per-record vectors into the flat `record * stride` layout the scoring
/// entry point takes (Issue #386).
fn flatten(recs: &[Vec<f32>]) -> Vec<f32> {
    recs.iter().flat_map(|r| r.iter().copied()).collect()
}

/// Reference outputs from the scalar single-record `activate` path.
fn reference(net: &CompiledNetwork, recs: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
    let mut scratch = net.clone();
    let mut out = Vec::with_capacity(recs.len() * num_outputs);
    for rec in recs {
        out.extend_from_slice(&scratch.activate(rec, num_outputs));
    }
    out
}

const COUNTS: [usize; 6] = [1, 7, 8, 9, 16, 17];
/// SIMD tolerance the batched vectorised squash/weighted-sum already carries
/// (Issue #230 / #243); the exact single-record tail is well within it.
const TOL: f32 = 2e-3;

#[test]
fn interleaved_fast_path_matches_reference_across_boundaries() {
    // All-Tanh network → no aggregate neurons → interleaved fast path.
    let num_inputs = 12;
    let net = build_network(num_inputs, SquashType::Tanh);
    for &count in &COUNTS {
        let recs = records(num_inputs, count);
        let got = net.score_records_flat(&flatten(&recs), num_inputs, 1);
        let want = reference(&net, &recs, 1);
        assert_eq!(got.len(), want.len(), "count {count}: length mismatch");
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() <= TOL,
                "count {count} record {i}: interleaved {g} vs reference {w}"
            );
        }
    }
}

#[test]
fn interleaved_single_record_is_bit_identical_to_activate() {
    // A single record runs the exact scalar tail, so it must match bit-for-bit.
    let num_inputs = 12;
    let net = build_network(num_inputs, SquashType::Tanh);
    let recs = records(num_inputs, 1);
    assert_eq!(
        net.score_records_flat(&flatten(&recs), num_inputs, 1),
        reference(&net, &recs, 1)
    );
}

#[test]
fn aggregate_fallback_path_matches_reference_across_boundaries() {
    // A Maximum network has aggregate neurons → forces the per-lane fallback,
    // whose exact single-record kernels are bit-identical to the reference.
    let num_inputs = 12;
    let net = build_network(num_inputs, SquashType::Maximum);
    for &count in &COUNTS {
        let recs = records(num_inputs, count);
        let got = net.score_records_flat(&flatten(&recs), num_inputs, 1);
        let want = reference(&net, &recs, 1);
        assert_eq!(got, want, "count {count}: aggregate fallback must be exact");
    }
}
