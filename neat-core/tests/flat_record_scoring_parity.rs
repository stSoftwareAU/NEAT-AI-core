//! Parity guard for the flat-slice record-**input** scoring path (Issue #386).
//!
//! `score_records` takes one owned `Vec<f32>` per record; `score_records_flat`
//! takes a single contiguous buffer where record `i` occupies
//! `inputs[i * stride .. i * stride + stride]` — mirroring the flat *output*
//! contract Issue #229 established. Both must drive the identical batched
//! kernel, so their results are **bit-identical**, not merely close.
//!
//! These are "what" tests: they score real networks through both public entry
//! points and compare the returned buffers. Record counts straddle the
//! 8-record SIMD group boundary (0, 1, 7, 8, 9, 12) plus a production-width
//! shard, and both dispatch arms are covered — the record-interleaved fast path
//! (all-standard squash) and the aggregate-squash per-lane fallback. A stride
//! or offset slip shows up as wrong lane values at the 7/9/12 remainder counts.

use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{NETWORKS, NetSpec, build_network, build_records};

/// Record counts either side of the 8-record SIMD group boundary, including the
/// empty batch and the 4-record-group + scalar-tail split (12).
const COUNTS: [usize; 6] = [0, 1, 7, 8, 9, 12];

/// Records at production input width for the shard-scale case. Large enough to
/// span many full 8-record groups plus a non-empty scalar tail, small enough to
/// stay well inside the 120 s unit-test budget in a debug build.
const SHARD_RECORDS: usize = 259;

/// Build a two-hidden-plus-one-output feedforward network where every non-input
/// neuron uses `squash` (mirrors `interleaved_scoring_parity.rs`).
fn build_local_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
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
    synapses.push(SynapseData {
        weight: 0.7,
        from_index: num_inputs as u16,
        synapse_type: 0,
    });
    synapses.push(SynapseData {
        weight: -0.5,
        from_index: num_inputs as u16 + 1,
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
    }
}

/// Deterministic distinct records of `width` values each, so a lane or stride
/// mix-up cannot hide behind repeated rows.
fn records(width: usize, count: usize) -> Vec<Vec<f32>> {
    (0..count)
        .map(|r| {
            (0..width)
                .map(|i| ((r * 7 + i * 3) as f32 * 0.017).sin() * 0.9)
                .collect()
        })
        .collect()
}

/// Pack per-record vectors into the flat `record * stride` layout.
fn flatten(recs: &[Vec<f32>]) -> Vec<f32> {
    recs.iter().flat_map(|r| r.iter().copied()).collect()
}

fn spec(label: &str) -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
}

/// Assert the two entry points agree bit-for-bit across every boundary count.
fn assert_flat_matches_per_record(net: &CompiledNetwork, width: usize, num_outputs: usize) {
    for &count in &COUNTS {
        let recs = records(width, count);
        let flat = flatten(&recs);
        let from_flat = net.score_records_flat(&flat, width, num_outputs);
        let from_vecs = net.score_records(&recs, num_outputs);
        assert_eq!(
            from_flat.len(),
            count * num_outputs,
            "count {count}: flat output length"
        );
        assert_eq!(
            from_flat, from_vecs,
            "count {count}: flat-slice input must be bit-identical to per-record input"
        );
    }
}

#[test]
fn flat_input_matches_per_record_on_the_interleaved_arm() {
    // All-Tanh network → no aggregate neurons → record-interleaved fast path.
    let net = build_local_network(12, SquashType::Tanh);
    assert_flat_matches_per_record(&net, 12, 1);
}

#[test]
fn flat_input_matches_per_record_on_the_aggregate_squash_arm() {
    // A Maximum network has aggregate neurons → per-lane fallback dispatch.
    let net = build_local_network(12, SquashType::Maximum);
    assert_flat_matches_per_record(&net, 12, 1);
}

#[test]
fn flat_input_matches_per_record_at_production_shard_width() {
    let s = spec("production");
    let net = build_network(s, 0x5EED);
    let recs = build_records(net.num_inputs(), SHARD_RECORDS);
    let flat = flatten(&recs);
    assert_eq!(
        net.score_records_flat(&flat, net.num_inputs(), s.num_outputs),
        net.score_records(&recs, s.num_outputs),
        "production-width shard must be bit-identical across input layouts"
    );
}

#[test]
fn flat_input_zero_fills_a_stride_narrower_than_the_network() {
    // A record shorter than the network's input arity is zero-filled, exactly as
    // the per-record path does for a short `Vec`.
    let net = build_local_network(12, SquashType::Tanh);
    let short = records(5, 9);
    let flat = flatten(&short);
    assert_eq!(
        net.score_records_flat(&flat, 5, 1),
        net.score_records(&short, 1),
        "a narrow stride must zero-fill the uncovered inputs"
    );
}

#[test]
fn flat_input_into_writes_the_same_buffer_as_the_allocating_entry_point() {
    let net = build_local_network(12, SquashType::Tanh);
    let recs = records(12, 9);
    let flat = flatten(&recs);
    let mut out = vec![f32::NAN; 9];
    net.score_records_flat_into(&flat, 12, 1, &mut out);
    assert_eq!(out, net.score_records_flat(&flat, 12, 1));
}

#[test]
fn parallel_flat_input_matches_the_sequential_flat_path() {
    let s = spec("production");
    let net = build_network(s, 0x5EED);
    let recs = build_records(net.num_inputs(), SHARD_RECORDS);
    let flat = flatten(&recs);
    assert_eq!(
        net.score_records_parallel_flat(&flat, net.num_inputs(), s.num_outputs),
        net.score_records_flat(&flat, net.num_inputs(), s.num_outputs),
        "the parallel flat path must be deterministic and match the sequential one"
    );
}

#[test]
#[should_panic(expected = "stride must be greater than zero")]
fn flat_input_rejects_a_zero_stride() {
    // Fail loud (Issue #3234): a zero stride cannot describe records at all.
    let net = build_local_network(12, SquashType::Tanh);
    net.score_records_flat(&[0.0, 1.0], 0, 1);
}

#[test]
#[should_panic(expected = "not a whole number of records")]
fn flat_input_rejects_a_ragged_buffer() {
    // Fail loud: a buffer that is not a multiple of the stride is malformed.
    let net = build_local_network(12, SquashType::Tanh);
    net.score_records_flat(&[0.0; 7], 3, 1);
}
