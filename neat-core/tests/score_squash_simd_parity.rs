//! Parity guard for the SIMD squash wiring in the batched scoring path
//! (Issue #243). `CompiledNetwork::score_batch_into` now feeds its per-lane
//! sums through the vectorised `squash_x8` / `squash_x4` approximations for the
//! covered squash types (Tanh / Logistic / Gelu / Mish from #180 plus the
//! high-frequency production types added by #243), falling back to the scalar
//! inline squash for every other type.
//!
//! These are "what" tests: they build a real forward network, score a batch of
//! distinct records through the public `score_records_flat` entry point, and
//! assert the result matches the scalar `activate` reference within the
//! documented SIMD tolerance. Distinct per-record inputs mean a lane
//! transposition in the wiring would break the parity, so the tests double as a
//! regression guard for the batching itself.

use neat_core::squash::SquashType;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Build a small forward network: `num_inputs` inputs fully connected into two
/// hidden neurons and one output neuron, every non-input neuron using `squash`.
fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
    let mut synapses = Vec::new();
    let mut neurons = Vec::new();

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

    CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("fixture must satisfy the load-time index invariant")
}

/// Deterministic distinct input records that exercise the squash curves across
/// their negative/positive ranges (so a lane mix-up would surface).
fn build_records(num_records: usize, num_inputs: usize) -> Vec<Vec<f32>> {
    (0..num_records)
        .map(|r| {
            (0..num_inputs)
                .map(|i| -1.5 + 0.37 * (r as f32) - 0.21 * (i as f32))
                .collect()
        })
        .collect()
}

/// Scalar single-record reference: score each record via `activate` (the scalar
/// forward pass) and flatten to the `[record * num_outputs]` layout that
/// `score_records_flat` returns. Deliberately independent of the scoring path.
fn reference(net: &CompiledNetwork, records: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
    let mut scratch = net.clone();
    records
        .iter()
        .flat_map(|r| scratch.activate(r, num_outputs))
        .collect()
}

/// Per-element tolerance: each vectorised activation is within
/// `SQUASH_SIMD_MAX_ABS_ERR` (5e-6) of scalar, and the batched weighted sums
/// add ~1e-6 of reassociation noise; 1e-3 stays far below any scoring-decision
/// threshold while a real lane/order bug (an O(1) error) still trips it.
const TOL: f32 = 1e-3;

fn assert_parity(squash: SquashType, num_records: usize) {
    let num_inputs = 4;
    let num_outputs = 1;
    let net = build_network(num_inputs, squash);
    let records = build_records(num_records, num_inputs);

    // Flat input layout (Issue #386): record `i` occupies
    // `flat[i * num_inputs .. (i + 1) * num_inputs]`.
    let flat: Vec<f32> = records.iter().flat_map(|r| r.iter().copied()).collect();
    let expected = reference(&net, &records, num_outputs);
    let actual = net.score_records_flat(&flat, num_inputs, num_outputs);

    assert_eq!(
        actual.len(),
        expected.len(),
        "{squash:?} n={num_records}: length mismatch"
    );
    let mut worst = 0.0f32;
    let mut worst_at = 0usize;
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        let diff = (a - e).abs();
        if diff > worst {
            worst = diff;
            worst_at = i;
        }
    }
    assert!(
        worst <= TOL,
        "{squash:?} n={num_records}: max abs diff {worst} at {worst_at} exceeds {TOL} \
         (actual={}, expected={})",
        actual[worst_at],
        expected[worst_at]
    );
}

/// The full set of squash types the scoring path now vectorises. Record counts
/// straddle every batch boundary: 8-record block, 4-record block, and the
/// scalar tail (`% 4`).
const VECTORISED: [SquashType; 20] = [
    SquashType::Tanh,
    SquashType::Logistic,
    SquashType::Gelu,
    SquashType::Mish,
    SquashType::Absolute,
    SquashType::HardTanh,
    SquashType::Relu6,
    SquashType::LeakyRelu,
    SquashType::Bipolar,
    SquashType::Softsign,
    SquashType::BentIdentity,
    SquashType::Isru,
    SquashType::Gaussian,
    SquashType::Swish,
    SquashType::BipolarSigmoid,
    SquashType::Elu,
    SquashType::Selu,
    SquashType::Sine,
    SquashType::Cosine,
    SquashType::ArcTan,
];

#[test]
fn score_batch_matches_scalar_for_vectorised_squashes() {
    // 13 records = one 8-record block + one 4-record block + one tail record,
    // so every code path in `score_batch_into` is exercised per type.
    for squash in VECTORISED {
        assert_parity(squash, 13);
    }
}

#[test]
fn score_batch_matches_scalar_across_batch_boundaries() {
    // Counts that land exactly on / just past each block boundary.
    for squash in VECTORISED {
        for n in [1usize, 3, 4, 7, 8, 11, 16] {
            assert_parity(squash, n);
        }
    }
}

#[test]
fn score_batch_non_vectorised_squash_matches_scalar() {
    // Types that keep the scalar fallback must still match after the wiring.
    for squash in [
        SquashType::Identity,
        SquashType::Relu,
        SquashType::Softplus,
        SquashType::Square,
        SquashType::Cube,
    ] {
        for n in [5usize, 8, 13] {
            assert_parity(squash, n);
        }
    }
}
