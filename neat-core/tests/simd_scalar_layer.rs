//! Pins the ISA-neutral scalar layer shared by the wasm (`simd.rs`) and native
//! (`simd_native.rs`) weighted-sum kernels (Issue #447).
//!
//! The rule under test: `neat_core::simd::scalar` is the single home of the
//! reference scalar semantics of each weighted-sum kernel and of the
//! small-count guard in front of it. Every SIMD path must agree with it
//! bit-for-bit below the SIMD threshold, the guard must saturate rather than
//! underflow on a reversed range, and the seed-taking tail helpers must
//! continue the reference accumulation without reseeding.

use neat_core::network::SynapseData;
use neat_core::simd::scalar;
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd,
};

fn synapse(from_index: u16, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
    }
}

/// Nine synapses: enough for two full 4-wide SIMD chunks plus a scalar tail,
/// with values chosen so re-ordering the f32 additions is observable.
fn fixture() -> (Vec<SynapseData>, Vec<f32>) {
    let synapses = vec![
        synapse(0, 0.5),
        synapse(2, -1.5),
        synapse(1, 2.0),
        synapse(3, 0.25),
        synapse(4, -0.75),
        synapse(2, 1.25),
        synapse(0, 1e-7),
        synapse(4, 3.5),
        synapse(1, -0.125),
    ];
    let activations = vec![1.0_f32, 2.0, -3.0, 0.5, 4.0];
    (synapses, activations)
}

// ---- The small-count guard -------------------------------------------------

#[test]
fn synapse_count_measures_the_range() {
    assert_eq!(scalar::synapse_count(2, 7), 5);
    assert_eq!(scalar::synapse_count(0, 0), 0);
}

#[test]
fn synapse_count_saturates_when_end_precedes_start() {
    assert_eq!(
        scalar::synapse_count(9, 4),
        0,
        "a reversed range must saturate to zero, not underflow"
    );
    assert_eq!(scalar::synapse_count(1, 0), 0);
}

#[test]
fn reference_kernels_return_their_seed_for_a_reversed_range() {
    let (synapses, activations) = fixture();
    assert_eq!(
        scalar::weighted_sum(&synapses, &activations, 9, 4, 0.25),
        0.25
    );
    assert_eq!(
        scalar::weighted_sum_of_squares(&synapses, &activations, 9, 4),
        0.0
    );
    assert_eq!(
        scalar::weighted_sum_no_bias(&synapses, &activations, 9, 4),
        0.0
    );
    assert_eq!(
        scalar::weighted_sum_of_squares_v2(&synapses, &activations, 9, 4, 0.25),
        0.0
    );
}

#[test]
fn public_kernels_survive_a_reversed_range() {
    let (synapses, activations) = fixture();
    assert_eq!(weighted_sum_simd(&synapses, &activations, 9, 4, 0.25), 0.25);
    assert_eq!(
        weighted_sum_of_squares_simd(&synapses, &activations, 9, 4),
        0.0
    );
    assert_eq!(
        weighted_sum_no_bias_simd(&synapses, &activations, 9, 4),
        0.0
    );
    assert_eq!(
        weighted_sum_of_squares_v2_simd(&synapses, &activations, 9, 4, 0.25),
        0.0
    );
}

/// Below the SIMD threshold the public kernels *are* the reference scalar
/// kernels — bit-for-bit, not merely approximately.
#[test]
fn small_counts_are_bit_identical_to_the_reference_kernels() {
    let (synapses, activations) = fixture();
    for count in 0..scalar::SINGLE_RECORD_SIMD_MIN {
        let end = count;
        assert_eq!(
            weighted_sum_simd(&synapses, &activations, 0, end, 0.25).to_bits(),
            scalar::weighted_sum(&synapses, &activations, 0, end, 0.25).to_bits(),
            "weighted_sum_simd drifted from the reference at count {count}"
        );
        assert_eq!(
            weighted_sum_of_squares_simd(&synapses, &activations, 0, end).to_bits(),
            scalar::weighted_sum_of_squares(&synapses, &activations, 0, end).to_bits(),
            "weighted_sum_of_squares_simd drifted from the reference at count {count}"
        );
        assert_eq!(
            weighted_sum_no_bias_simd(&synapses, &activations, 0, end).to_bits(),
            scalar::weighted_sum_no_bias(&synapses, &activations, 0, end).to_bits(),
            "weighted_sum_no_bias_simd drifted from the reference at count {count}"
        );
        assert_eq!(
            weighted_sum_of_squares_v2_simd(&synapses, &activations, 0, end, -0.5).to_bits(),
            scalar::weighted_sum_of_squares_v2(&synapses, &activations, 0, end, -0.5).to_bits(),
            "weighted_sum_of_squares_v2_simd drifted from the reference at count {count}"
        );
    }
}

// ---- The seed-taking tail helpers ------------------------------------------
//
// A tail helper must continue the caller's running accumulator, never reseed
// it: splitting a range anywhere has to reproduce the reference result exactly.

#[test]
fn tail_sum_continues_the_reference_accumulation_bit_for_bit() {
    let (synapses, activations) = fixture();
    let reference = scalar::weighted_sum(&synapses, &activations, 0, synapses.len(), 0.25);
    for split in 0..=synapses.len() {
        let head = scalar::weighted_sum(&synapses, &activations, 0, split, 0.25);
        // SAFETY: every `from_index` in the fixture is < activations.len() and
        // `end == synapses.len()`, satisfying the helper's index contract.
        let full =
            unsafe { scalar::tail_sum(&synapses, &activations, split, synapses.len(), head) };
        assert_eq!(
            full.to_bits(),
            reference.to_bits(),
            "tail_sum reseeded or reordered at split {split}"
        );
    }
}

#[test]
fn tail_sum_of_squares_continues_the_reference_accumulation_bit_for_bit() {
    let (synapses, activations) = fixture();
    let reference = scalar::weighted_sum_of_squares(&synapses, &activations, 0, synapses.len());
    for split in 0..=synapses.len() {
        let head = scalar::weighted_sum_of_squares(&synapses, &activations, 0, split);
        // SAFETY: as above — the fixture satisfies the index contract.
        let full = unsafe {
            scalar::tail_sum_of_squares(&synapses, &activations, split, synapses.len(), head)
        };
        assert_eq!(
            full.to_bits(),
            reference.to_bits(),
            "tail_sum_of_squares reseeded or reordered at split {split}"
        );
    }
}

#[test]
fn tail_sum_of_squares_v2_continues_the_reference_accumulation_bit_for_bit() {
    let (synapses, activations) = fixture();
    let bias = -0.5_f32;
    let reference =
        scalar::weighted_sum_of_squares_v2(&synapses, &activations, 0, synapses.len(), bias);
    for split in 0..=synapses.len() {
        let head = scalar::weighted_sum_of_squares_v2(&synapses, &activations, 0, split, bias);
        // SAFETY: as above — the fixture satisfies the index contract.
        let full = unsafe {
            scalar::tail_sum_of_squares_v2(
                &synapses,
                &activations,
                split,
                synapses.len(),
                head,
                bias,
            )
        };
        assert_eq!(
            full.to_bits(),
            reference.to_bits(),
            "tail_sum_of_squares_v2 reseeded or reordered at split {split}"
        );
    }
}

#[test]
fn tail_helpers_return_the_seed_for_an_empty_tail() {
    let (synapses, activations) = fixture();
    // SAFETY: an empty tail reads nothing.
    unsafe {
        assert_eq!(scalar::tail_sum(&synapses, &activations, 4, 4, 1.5), 1.5);
        assert_eq!(
            scalar::tail_sum_of_squares(&synapses, &activations, 4, 4, 1.5),
            1.5
        );
        assert_eq!(
            scalar::tail_sum_of_squares_v2(&synapses, &activations, 4, 4, 1.5, -0.5),
            1.5
        );
    }
}

// ---- The reference kernels themselves --------------------------------------

#[test]
fn reference_kernels_compute_the_documented_formulas() {
    let synapses = vec![synapse(1, 0.5), synapse(0, -2.0)];
    let activations = vec![3.0_f32, 4.0];
    // a*w = 4.0*0.5 = 2.0 and 3.0*-2.0 = -6.0
    assert_eq!(
        scalar::weighted_sum(&synapses, &activations, 0, 2, 1.0),
        1.0 + 2.0 - 6.0
    );
    assert_eq!(
        scalar::weighted_sum_no_bias(&synapses, &activations, 0, 2),
        2.0 - 6.0
    );
    assert_eq!(
        scalar::weighted_sum_of_squares(&synapses, &activations, 0, 2),
        4.0 + 36.0
    );
    // (bias + a*w)^2 with bias = 1.0 → 3.0^2 + (-5.0)^2
    assert_eq!(
        scalar::weighted_sum_of_squares_v2(&synapses, &activations, 0, 2, 1.0),
        9.0 + 25.0
    );
}
