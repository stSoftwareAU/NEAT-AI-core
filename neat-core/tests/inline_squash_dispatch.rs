//! Behavioural coverage for the hot-squash inline dispatch rule (Issue #443).
//!
//! One rule decides which squash types are hot enough to branch inline
//! (`Identity`, `Relu`, `Logistic`, `Tanh`) and what scalar formula each of
//! them uses; everything else defers to [`apply_squash`]. That rule is reached
//! from the single-record forward passes, the 4-way traced batch and the
//! `None`-fallback branch of every batched loss kernel, and all of them must
//! agree — the SIMD-batched and scalar-tail paths are only interchangeable
//! while they do.
//!
//! These are "what" tests: they activate real networks through the public
//! entry points and assert the observable activation equals the
//! [`apply_squash`] reference for **every** standard squash type, hot arms
//! included. A copy of the dispatch that drifted — a promoted fifth type, or a
//! reformulated logistic — fails here regardless of how the dispatch is
//! implemented.

use neat_core::loss::{
    cross_entropy_sum_batch_packed, hinge_sum_batch_packed, mae_sum_batch_packed,
    mape_sum_batch_packed, mse_sum_batch_packed, msle_sum_batch_packed,
};
use neat_core::range::apply_limit_range;
use neat_core::squash::{SquashType, apply_squash};
use neat_core::squash_simd::SQUASH_SIMD_MAX_ABS_ERR;
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Every standard (non-aggregate) squash type. Aggregates 32–37 are activated
/// by a different rule and are covered by `aggregate_squash_tail_parity.rs`.
const STANDARD_SQUASHES: [SquashType; 32] = [
    SquashType::Identity,
    SquashType::Relu,
    SquashType::Relu6,
    SquashType::LeakyRelu,
    SquashType::Selu,
    SquashType::Elu,
    SquashType::Logistic,
    SquashType::Tanh,
    SquashType::HardTanh,
    SquashType::Softsign,
    SquashType::Softplus,
    SquashType::Swish,
    SquashType::Mish,
    SquashType::Gelu,
    SquashType::Sine,
    SquashType::Cosine,
    SquashType::Tan,
    SquashType::ArcTan,
    SquashType::Gaussian,
    SquashType::BentIdentity,
    SquashType::BipolarSigmoid,
    SquashType::Bipolar,
    SquashType::Step,
    SquashType::Complement,
    SquashType::Absolute,
    SquashType::Square,
    SquashType::Cube,
    SquashType::Sqrt,
    SquashType::StdInverse,
    SquashType::Exponential,
    SquashType::LogSigmoid,
    SquashType::Isru,
];

/// Squash types the vectorised `squash_x4` / `squash_x8` approximations do not
/// handle, so the batched loss kernels take the scalar `None`-fallback branch —
/// the copies of the dispatch under test. `Identity` and `Relu` exercise the
/// hot inline arms, the rest the `apply_squash` fall-through.
const FALLBACK_SQUASHES: [SquashType; 6] = [
    SquashType::Identity,
    SquashType::Relu,
    SquashType::Softplus,
    SquashType::Step,
    SquashType::Sqrt,
    SquashType::LogSigmoid,
];

/// Sums that straddle every interesting part of the squash curves: negative,
/// zero (the `Sqrt` / `StdInverse` guard), and positive.
const PROBE_SUMS: [f32; 6] = [-3.25, -0.5, 0.0, 0.5, 1.75, 4.0];

/// Single input wired straight into one output neuron at unit weight and zero
/// bias, so the neuron's weighted sum is exactly the input value.
fn unit_network(squash: SquashType) -> CompiledNetwork {
    let synapses = vec![SynapseData {
        weight: 1.0,
        from_index: 0,
        synapse_type: 0,
    }];
    let neurons = vec![NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 1,
        squash_type: squash as u8,
        is_constant: false,
    }];

    CompiledNetwork {
        num_neurons: 2,
        num_inputs: 1,
        neurons,
        synapses,
        activations: vec![0.0; 2],
        hint_values_buffer: vec![0.0; 1],
        trace_data_buffer: Vec::new(),
        batch_activations: [vec![0.0; 2], vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]],
        batch_hints: [vec![0.0; 1], vec![0.0; 1], vec![0.0; 1], vec![0.0; 1]],
        batch_traces: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    }
}

/// The rule stated once: squash the sum, then clamp to the type's range.
fn expected(squash: SquashType, sum: f32) -> f32 {
    apply_limit_range(squash, apply_squash(squash, sum))
}

fn assert_exact(actual: f32, expected: f32, what: &str) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn activate_squashes_every_standard_type_like_apply_squash() {
    for squash in STANDARD_SQUASHES {
        let mut net = unit_network(squash);
        for sum in PROBE_SUMS {
            let got = net.activate(&[sum], 1)[0];
            assert_exact(
                got,
                expected(squash, sum),
                &format!("{squash:?} activate({sum})"),
            );
        }
    }
}

#[test]
fn activate_into_squashes_every_standard_type_like_apply_squash() {
    for squash in STANDARD_SQUASHES {
        let mut net = unit_network(squash);
        for sum in PROBE_SUMS {
            let mut out = [0.0f32; 1];
            net.activate_into(&[sum], &mut out);
            assert_exact(
                out[0],
                expected(squash, sum),
                &format!("{squash:?} activate_into({sum})"),
            );
        }
    }
}

#[test]
fn activate_and_trace_squashes_every_standard_type_like_apply_squash() {
    for squash in STANDARD_SQUASHES {
        let mut net = unit_network(squash);
        for sum in PROBE_SUMS {
            let got = net.activate_and_trace(&[sum], 1)[0];
            assert_exact(
                got,
                expected(squash, sum),
                &format!("{squash:?} activate_and_trace({sum})"),
            );
        }
    }
}

#[test]
fn traced_4way_batch_squashes_every_standard_type_like_apply_squash() {
    // The 4-way traced batch returns a 4-value length header followed by each
    // record's `[outputs..., activations..., hints..., trace...]` block; the
    // first value of a block is the record's single output activation.
    for squash in STANDARD_SQUASHES {
        let mut net = unit_network(squash);
        let sums = [PROBE_SUMS[0], PROBE_SUMS[1], PROBE_SUMS[3], PROBE_SUMS[5]];
        let result = net.activate_and_trace_batch_4way(&sums, 1, 1);

        let mut start = 4;
        for (lane, sum) in sums.iter().enumerate() {
            let want = expected(squash, *sum);
            let got = result[start];
            // Types `squash_x4` vectorises are allowed their documented
            // approximation error; the scalar fallback branch — the copy of the
            // dispatch under test — must land exactly on the reference.
            assert!(
                (got - want).abs() <= SQUASH_SIMD_MAX_ABS_ERR,
                "{squash:?} traced 4-way lane {lane} ({sum}): expected {want}, got {got}"
            );
            start += result[lane] as usize;
        }
    }
}

/// Packed records of `[input, target]` with distinct values per record, so a
/// lane mix-up in the batched kernels would break the parity too. The input
/// sign alternates so **every** group — the 8-record group, the 4-record
/// remainder and the scalar tail — straddles zero and therefore exercises the
/// sign-sensitive arms (`Relu`, `Step`, `Sqrt`) rather than one side only.
fn packed_records(num_records: usize) -> Vec<f32> {
    let mut records = vec![0.0f32; num_records * 2];
    for r in 0..num_records {
        let magnitude = 0.6 + 0.17 * (r as f32);
        records[r * 2] = if r % 2 == 0 { -magnitude } else { magnitude };
        records[r * 2 + 1] = 0.25 + 0.03 * (r as f32);
    }
    records
}

type PackedLoss = fn(&mut CompiledNetwork, &[f32], usize, usize, bool) -> f64;

/// Every batched loss kernel carrying a copy of the dispatch in its scalar
/// `None`-fallback branch.
const PACKED_LOSSES: [(&str, PackedLoss); 6] = [
    ("mse", mse_sum_batch_packed),
    ("mae", mae_sum_batch_packed),
    ("cross_entropy", cross_entropy_sum_batch_packed),
    ("mape", mape_sum_batch_packed),
    ("msle", msle_sum_batch_packed),
    ("hinge", hinge_sum_batch_packed),
];

/// Two inputs feeding a `Minimum` aggregate hidden neuron and a `squash`
/// output neuron. The aggregate routes MSE onto the *scattered* 8-way kernel,
/// which carries its own copy of the dispatch for the standard-squash neurons
/// travelling with it.
fn mixed_aggregate_network(squash: SquashType) -> CompiledNetwork {
    let synapses = vec![
        SynapseData {
            weight: 0.8,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: -0.6,
            from_index: 1,
            synapse_type: 0,
        },
        SynapseData {
            weight: 1.0,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: 0.5,
            from_index: 2,
            synapse_type: 0,
        },
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.0,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: SquashType::Minimum as u8,
            is_constant: false,
        },
        NeuronData {
            bias: 0.0,
            start_synapse: 2,
            num_synapses: 2,
            squash_type: squash as u8,
            is_constant: false,
        },
    ];

    CompiledNetwork {
        num_neurons: 4,
        num_inputs: 2,
        neurons,
        synapses,
        activations: vec![0.0; 4],
        hint_values_buffer: vec![0.0; 2],
        trace_data_buffer: Vec::new(),
        batch_activations: [vec![0.0; 4], vec![0.0; 4], vec![0.0; 4], vec![0.0; 4]],
        batch_hints: [vec![0.0; 2], vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]],
        batch_traces: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    }
}

/// Packed `[in0, in1, target]` records with alternating input signs.
fn mixed_records(num_records: usize) -> Vec<f32> {
    let mut records = vec![0.0f32; num_records * 3];
    for r in 0..num_records {
        let magnitude = 0.6 + 0.17 * (r as f32);
        records[r * 3] = if r % 2 == 0 { -magnitude } else { magnitude };
        records[r * 3 + 1] = 0.4 - 0.09 * (r as f32);
        records[r * 3 + 2] = 0.25 + 0.03 * (r as f32);
    }
    records
}

#[test]
fn scattered_mse_fallback_squash_matches_the_scalar_reference() {
    for num_records in [8usize, 13] {
        let records = mixed_records(num_records);
        for squash in FALLBACK_SQUASHES {
            let mut batched_net = mixed_aggregate_network(squash);
            let mut scalar_net = mixed_aggregate_network(squash);
            let batched = mse_sum_batch_packed(&mut batched_net, &records, 2, 1, true);
            let scalar = mse_sum_batch_packed(&mut scalar_net, &records, 2, 1, false);
            assert!(
                (batched - scalar).abs() <= 1.0e-6 * (num_records as f64),
                "scattered mse {squash:?} n={num_records}: batched {batched} vs scalar {scalar}"
            );
        }
    }
}

#[test]
fn batched_loss_fallback_squash_matches_the_scalar_reference() {
    // 4 → 4-way group, 8 → 8-way group, 13 → 8-way plus 4-way plus tail.
    for num_records in [4usize, 8, 13] {
        let records = packed_records(num_records);
        for squash in FALLBACK_SQUASHES {
            for (name, loss_fn) in PACKED_LOSSES {
                let mut batched_net = unit_network(squash);
                let mut scalar_net = unit_network(squash);
                // `forward_only = true` takes the SIMD-batched path with the
                // inline squash fallback; `false` the single-record reference.
                let batched = loss_fn(&mut batched_net, &records, 1, 1, true);
                let scalar = loss_fn(&mut scalar_net, &records, 1, 1, false);
                assert!(
                    (batched - scalar).abs() <= 1.0e-6 * (num_records as f64),
                    "{name} {squash:?} n={num_records}: batched {batched} vs scalar {scalar}"
                );
            }
        }
    }
}
