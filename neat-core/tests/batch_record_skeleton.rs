//! Behavioural coverage for the batched record-scan skeleton (Issue #445).
//!
//! One rule says how a packed record buffer is walked and reduced by the fused
//! loss kernels: records are grouped 8 → 4 → 1, each group's inputs are loaded
//! into per-lane activation buffers through the shared loader, and the
//! per-record errors accumulate into one `f64` sum. Every loss kind — MSE
//! included — obeys it.
//!
//! These are "what" tests: they drive the public `*_sum_batch_packed` entry
//! points with real networks and assert observable numbers against a
//! single-record reference built from `CompiledNetwork::activate`. A copy of the
//! skeleton that drifted — a loader that does not clamp the record to the
//! network's input count, one that leaves an uncovered input slot stale, or a
//! remainder split that drops or double-counts records — fails here no matter
//! how the scan is implemented.

use neat_core::loss::{
    cross_entropy_sum_batch_packed, hinge_sum_batch_packed, mae_sum_batch_packed,
    mape_sum_batch_packed, mse_sum_batch_packed, msle_sum_batch_packed,
};
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

const IDENTITY: u8 = 0;
const TANH: u8 = 7;
const MEAN: u8 = 37;

/// Every batched loss entry point shares the packed signature.
type PackedEntry = fn(&mut CompiledNetwork, &[f32], usize, usize, bool) -> f64;

/// A record's per-record reduction: `(targets, outputs) -> error`.
type Reduce = fn(&[f32], &[f32]) -> f64;

/// One batched loss entry point paired with its single-record reduction.
type LossCase = (&'static str, PackedEntry, Reduce);

/// Absolute tolerance for a batched sum against the single-record reference.
/// The across-records SIMD kernels re-associate the weighted sums in `f32`, so
/// parity is "within a small tolerance", not bit-for-bit.
const TOL: f64 = 1e-5;

fn network(
    num_inputs: usize,
    neurons: Vec<NeuronData>,
    synapses: Vec<SynapseData>,
) -> CompiledNetwork {
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

/// Three inputs → two hidden neurons → one output, all standard squashes, so
/// the network routes through the interleaved/standard batch kernels.
/// `num_neurons` is 6, deliberately smaller than the widest `input_size` a test
/// passes, so an unclamped loader writes past the activation buffer.
fn standard_network(hidden_squash: u8) -> CompiledNetwork {
    let num_inputs = 3;
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

    for h in 0..2 {
        let start = synapses.len() as u32;
        for i in 0..num_inputs {
            synapses.push(SynapseData {
                weight: 0.37 - 0.11 * (i as f32) + 0.13 * (h as f32),
                from_index: i as u16,
                synapse_type: 0,
            });
        }
        neurons.push(NeuronData {
            bias: 0.05 * (h as f32) - 0.02,
            start_synapse: start,
            num_synapses: num_inputs as u16,
            squash_type: hidden_squash,
            is_constant: false,
        });
    }

    let start = synapses.len() as u32;
    synapses.push(SynapseData {
        weight: 0.61,
        from_index: num_inputs as u16,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.44,
        from_index: num_inputs as u16 + 1,
        synapse_type: 0,
    });
    neurons.push(NeuronData {
        bias: 0.03,
        start_synapse: start,
        num_synapses: 2,
        squash_type: IDENTITY,
        is_constant: false,
    });

    network(num_inputs, neurons, synapses)
}

/// Same topology with an aggregate (`MEAN`) hidden neuron, which routes the
/// batch through the scattered per-lane kernels instead.
fn aggregate_network() -> CompiledNetwork {
    standard_network(MEAN)
}

/// Packed `[inputs…, targets…]` records with distinct per-record values, so a
/// lane mix-up or a dropped record cannot hide. Input columns beyond the
/// network's `num_inputs` are padding the network must ignore.
fn build_records(num_records: usize, input_size: usize, num_outputs: usize) -> Vec<f32> {
    let values_per_record = input_size + num_outputs;
    let mut records = vec![0.0f32; num_records * values_per_record];
    for r in 0..num_records {
        let base = r * values_per_record;
        for i in 0..input_size {
            records[base + i] = -0.9 + 0.23 * (r as f32) - 0.17 * (i as f32);
        }
        for j in 0..num_outputs {
            records[base + input_size + j] = 0.2 + 0.031 * (r as f32) + 0.05 * (j as f32);
        }
    }
    records
}

/// Single-record reference: activate each record on its own (which clamps the
/// record to the network's input count) and reduce with `reduce`.
fn reference_sum(
    net: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    reduce: impl Fn(&[f32], &[f32]) -> f64,
) -> f64 {
    let values_per_record = input_size + num_outputs;
    let num_records = records.len() / values_per_record;
    let mut sum = 0.0;
    for r in 0..num_records {
        let base = r * values_per_record;
        let outputs = net.activate(&records[base..base + input_size], num_outputs);
        sum += reduce(
            &records[base + input_size..base + values_per_record],
            &outputs,
        );
    }
    sum
}

fn mse_reduce(targets: &[f32], outputs: &[f32]) -> f64 {
    let mut sq = 0.0;
    for (t, o) in targets.iter().zip(outputs.iter()) {
        let d = (*t - *o) as f64;
        sq += d * d;
    }
    sq / targets.len() as f64
}

fn mae_reduce(targets: &[f32], outputs: &[f32]) -> f64 {
    let mut abs = 0.0;
    for (t, o) in targets.iter().zip(outputs.iter()) {
        abs += ((*t - *o) as f64).abs();
    }
    abs / targets.len() as f64
}

/// Every batched loss entry point paired with its per-record reduction.
fn loss_entries() -> Vec<LossCase> {
    vec![
        (
            "mse",
            mse_sum_batch_packed as PackedEntry,
            mse_reduce as Reduce,
        ),
        ("mae", mae_sum_batch_packed as PackedEntry, mae_reduce),
        (
            "cross_entropy",
            cross_entropy_sum_batch_packed as PackedEntry,
            |targets: &[f32], outputs: &[f32]| {
                const EPSILON: f64 = 1e-15;
                let mut ce = 0.0;
                for (t, o) in targets.iter().zip(outputs.iter()) {
                    let t = *t as f64;
                    let o = (*o as f64).clamp(EPSILON, 1.0 - EPSILON);
                    ce -= t * o.ln() + (1.0 - t) * (1.0 - o).ln();
                }
                ce / targets.len() as f64
            },
        ),
        (
            "mape",
            mape_sum_batch_packed as PackedEntry,
            |targets: &[f32], outputs: &[f32]| {
                const EPSILON: f64 = 1e-15;
                let mut mape = 0.0;
                for (t, o) in targets.iter().zip(outputs.iter()) {
                    let t = (*t as f64).max(EPSILON);
                    mape += ((*o as f64 - t) / t).abs();
                }
                mape / targets.len() as f64
            },
        ),
        (
            "msle",
            msle_sum_batch_packed as PackedEntry,
            |targets: &[f32], outputs: &[f32]| {
                const EPSILON: f64 = 1e-15;
                let mut msle = 0.0;
                for (t, o) in targets.iter().zip(outputs.iter()) {
                    msle += (*t as f64).max(EPSILON).ln() - (*o as f64).max(EPSILON).ln();
                }
                msle
            },
        ),
        (
            "hinge",
            hinge_sum_batch_packed as PackedEntry,
            |targets: &[f32], outputs: &[f32]| {
                let mut hinge = 0.0;
                for (t, o) in targets.iter().zip(outputs.iter()) {
                    hinge += (1.0 - (*t as f64) * (*o as f64)).max(0.0);
                }
                hinge
            },
        ),
    ]
}

/// The 8 → 4 → 1 grouping must not change the answer: for every record count
/// that straddles a group boundary the batched sum equals the sum of the
/// records scored one at a time.
#[test]
fn grouping_does_not_change_the_batched_sum() {
    for (name, entry, reduce) in loss_entries() {
        for &num_records in &[1usize, 3, 4, 5, 7, 8, 9, 11, 12, 13, 16, 17, 24, 33] {
            let records = build_records(num_records, 3, 1);
            let mut batched = standard_network(TANH);
            let mut reference = standard_network(TANH);

            let actual = entry(&mut batched, &records, 3, 1, true);
            let expected = reference_sum(&mut reference, &records, 3, 1, reduce);

            assert!(
                (actual - expected).abs() < TOL,
                "{name} n={num_records}: batched {actual} != single-record {expected}"
            );
        }
    }
}

/// A network whose aggregate squash keeps it on the per-lane kernels obeys the
/// same grouping rule.
#[test]
fn aggregate_network_grouping_does_not_change_the_batched_sum() {
    for &num_records in &[4usize, 5, 8, 9, 12, 13, 17] {
        let records = build_records(num_records, 3, 1);
        let mut batched = aggregate_network();
        let mut reference = aggregate_network();

        let actual = mse_sum_batch_packed(&mut batched, &records, 3, 1, true);
        let expected = reference_sum(&mut reference, &records, 3, 1, mse_reduce);

        assert!(
            (actual - expected).abs() < TOL,
            "aggregate n={num_records}: batched {actual} != single-record {expected}"
        );
    }
}

/// Input columns beyond the network's `num_inputs` are padding: the batched
/// loader clamps the copy exactly as `CompiledNetwork::activate` does, so the
/// extras change nothing (and never write past the activation buffer).
#[test]
fn input_columns_beyond_the_network_inputs_are_ignored() {
    // `input_size` (8) exceeds both `num_inputs` (3) and `num_neurons` (6).
    let input_size = 8;
    for (name, entry, reduce) in loss_entries() {
        for &num_records in &[4usize, 5, 8, 9, 13, 17] {
            let records = build_records(num_records, input_size, 1);
            let mut batched = standard_network(TANH);
            let mut reference = standard_network(TANH);

            let actual = entry(&mut batched, &records, input_size, 1, true);
            let expected = reference_sum(&mut reference, &records, input_size, 1, reduce);

            assert!(
                (actual - expected).abs() < TOL,
                "{name} n={num_records}: padded-record batched {actual} != single-record {expected}"
            );
        }
    }
}

/// The aggregate (scattered) route clamps the same way.
#[test]
fn aggregate_network_ignores_input_columns_beyond_the_network_inputs() {
    let input_size = 8;
    for &num_records in &[4usize, 8, 13] {
        let records = build_records(num_records, input_size, 1);
        let mut batched = aggregate_network();
        let mut reference = aggregate_network();

        let actual = mse_sum_batch_packed(&mut batched, &records, input_size, 1, true);
        let expected = reference_sum(&mut reference, &records, input_size, 1, mse_reduce);

        assert!(
            (actual - expected).abs() < TOL,
            "aggregate n={num_records}: padded-record batched {actual} != single-record {expected}"
        );
    }
}

/// A record that covers fewer columns than the network has inputs leaves the
/// uncovered input slots reading zero for every record — no value from an
/// earlier group leaks in.
#[test]
fn uncovered_input_slots_score_as_zero() {
    // The network has 3 inputs; each record supplies only 1.
    let input_size = 1;
    for &num_records in &[4usize, 8, 9, 13, 17] {
        let records = build_records(num_records, input_size, 1);
        let mut batched = standard_network(TANH);
        let mut reference = standard_network(TANH);

        let actual = mse_sum_batch_packed(&mut batched, &records, input_size, 1, true);
        let expected = reference_sum(&mut reference, &records, input_size, 1, mse_reduce);

        assert!(
            (actual - expected).abs() < TOL,
            "n={num_records}: short-record batched {actual} != single-record {expected}"
        );
    }
}
