//! Parity guard for the **scalar tails** of the fused batch-loss kernels on
//! aggregate-squash networks (Issue #441).
//!
//! The batched loss kernels group records into 8s, then a 4-record group, then
//! a single-record scalar tail. Every group size must compute a neuron's
//! activation by the same rule as the single-record reference
//! ([`CompiledNetwork::activate_into`], reached with `forward_only = false`),
//! including the aggregate squashes — `Minimum`, `Maximum`, `If`,
//! `Hypotenuse`, `HypotenuseV2` and `Mean` — which are not a weighted sum.
//!
//! These are "what" tests: they score a real aggregate-squash network through
//! the public packed entry points at record counts that leave a non-empty tail
//! (5/6/7 → 4-way + tail, 9/13 → 8-way group + tail) and assert the batched
//! error matches the per-record reference. A tail that falls through to a plain
//! weighted sum shifts the tail record's output by orders of magnitude, so the
//! tight tolerance below fails loudly.

use neat_core::loss::{
    cross_entropy_sum_batch_packed, hinge_sum_batch_packed, mae_sum_batch_packed,
    mape_sum_batch_packed, mse_sum_batch_packed, msle_sum_batch_packed,
};
use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Every aggregate squash the activation rule dispatches on.
const AGGREGATES: [SquashType; 6] = [
    SquashType::Minimum,
    SquashType::Maximum,
    SquashType::If,
    SquashType::Hypotenuse,
    SquashType::HypotenuseV2,
    SquashType::Mean,
];

/// Record counts with a non-empty scalar tail: 5/6/7 exercise the 4-way group
/// plus tail, 9 and 13 the 8-way group plus (4-way plus) tail.
const TAIL_COUNTS: [usize; 5] = [5, 6, 7, 9, 13];

const INPUT_SIZE: usize = 6;

/// The batched kernels reach the same activation rule as the reference, so the
/// only permitted gap is f32 summation order inside the SIMD weighted sums.
const TOLERANCE: f64 = 1.0e-5;

/// Build a forward-only network: `num_inputs` inputs fully connected into two
/// hidden neurons and one output neuron, every non-input neuron using `squash`.
/// Mirrors the fixture in `mse_batch_interleaved_parity.rs`.
fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
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

/// Packed `[inputs..., target]` records, distinct per record so a tail-record
/// mix-up cannot hide. Targets stay in `(0, 1)` so cross-entropy, MAPE and MSLE
/// are all well defined on the same fixture.
fn build_records(num_records: usize, input_size: usize) -> Vec<f32> {
    let values_per_record = input_size + 1;
    let mut records = vec![0.0f32; num_records * values_per_record];
    for r in 0..num_records {
        let base = r * values_per_record;
        for i in 0..input_size {
            records[base + i] = (((r * 7 + i * 3) as f32) * 0.013).sin() * 0.9;
        }
        records[base + input_size] = 0.5 + (((r * 5 + 1) as f32) * 0.011).cos() * 0.25;
    }
    records
}

type PackedLoss = fn(&mut CompiledNetwork, &[f32], usize, usize, bool) -> f64;

/// Score `num_records` through `loss` on the batched (`forward_only = true`)
/// and the per-record reference (`forward_only = false`) paths and assert they
/// agree. The reference activates each record on its own through
/// `CompiledNetwork::activate_into`.
fn assert_tail_matches_reference(
    name: &str,
    loss: PackedLoss,
    squash: SquashType,
    num_records: usize,
) {
    let mut batched_net = build_network(INPUT_SIZE, squash);
    let mut reference_net = build_network(INPUT_SIZE, squash);
    let records = build_records(num_records, INPUT_SIZE);

    let batched = loss(&mut batched_net, &records, INPUT_SIZE, 1, true);
    let reference = loss(&mut reference_net, &records, INPUT_SIZE, 1, false);

    let diff = (reference - batched).abs();
    assert!(
        diff <= TOLERANCE,
        "{name} {squash:?} n={num_records}: batched {batched} diverged from per-record reference {reference} by {diff} (tol {TOLERANCE})"
    );
}

#[test]
fn mse_scalar_tail_matches_reference_for_every_aggregate_squash() {
    for squash in AGGREGATES {
        for &n in &TAIL_COUNTS {
            assert_tail_matches_reference("mse", mse_sum_batch_packed, squash, n);
        }
    }
}

#[test]
fn shared_8way_scalar_tail_matches_reference_for_every_aggregate_squash() {
    // MAE, cross-entropy, MAPE, MSLE and hinge all share the 8-way activation
    // macro, so they share one scalar tail. Counts 9 and 13 leave that tail
    // non-empty; the 8-way path needs at least 8 records.
    let losses: [(&str, PackedLoss); 5] = [
        ("mae", mae_sum_batch_packed),
        ("cross_entropy", cross_entropy_sum_batch_packed),
        ("mape", mape_sum_batch_packed),
        ("msle", msle_sum_batch_packed),
        ("hinge", hinge_sum_batch_packed),
    ];
    for (name, loss) in losses {
        for squash in AGGREGATES {
            for &n in &[9usize, 13] {
                assert_tail_matches_reference(name, loss, squash, n);
            }
        }
    }
}
