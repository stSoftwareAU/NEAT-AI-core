//! Chunk-walk scaffold parity (Issue #448).
//!
//! The four single-record weighted-sum kernels share one walk over a synapse
//! span: gather four synapses into lanes, fold, reduce the lanes, then finish
//! the 0..3 remainder from the running accumulator. This file pins the
//! observable consequence of that walk — **every** kernel reproduces its
//! reference scalar result at **every** offset and count, not just at
//! `start == 0`.
//!
//! `simd_weighted_sums.rs` sweeps counts from `start == 0`; the offset sweep
//! here is what catches a scaffold that computes its chunk base from the chunk
//! index alone, because then the first gather silently reads from the wrong
//! synapse whenever `start > 0`.
//!
//! References come from `neat_core::simd::scalar` (Issue #447), the single home
//! of what each kernel means, so this test cannot drift from the semantics it
//! is checking.

use neat_core::network::SynapseData;
use neat_core::simd::scalar;
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd,
};

/// Widest span the sweep builds: two dual-accumulator chunks of eight, a
/// trailing chunk of four, and a 1..3 scalar remainder, from any offset.
const MAX_SPAN: usize = 24;

/// Fan-in wide enough to exercise repeated `from_index` gathers without the
/// activation buffer dominating the arithmetic.
const NUM_INPUTS: usize = 11;

fn fixture() -> (Vec<SynapseData>, Vec<f32>) {
    let synapses = (0..MAX_SPAN + 10)
        .map(|i| SynapseData {
            weight: ((i as f32) * 0.37).cos() * 1.5,
            from_index: (i % NUM_INPUTS) as u16,
            synapse_type: 0,
        })
        .collect();
    let activations = (0..NUM_INPUTS)
        .map(|i| ((i as f32) * 0.61).sin() - 0.25)
        .collect();
    (synapses, activations)
}

/// f32 accumulation over a span drifts from the reference by a few ULP once the
/// SIMD path fuses multiply-adds, so compare relative to the magnitude.
fn assert_close(got: f32, want: f32, what: &str) {
    let tol = 1e-4 * (1.0 + want.abs());
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got}, want {want} (tol {tol})"
    );
}

#[test]
fn every_kernel_matches_its_reference_from_any_offset() {
    let (synapses, activations) = fixture();
    let bias = -0.45f32;

    for start in 0..10usize {
        for count in 0..=MAX_SPAN {
            let end = start + count;
            let at = format!("start={start} count={count}");

            assert_close(
                weighted_sum_simd(&synapses, &activations, start, end, bias),
                scalar::weighted_sum(&synapses, &activations, start, end, bias),
                &format!("weighted_sum_simd {at}"),
            );
            assert_close(
                weighted_sum_no_bias_simd(&synapses, &activations, start, end),
                scalar::weighted_sum_no_bias(&synapses, &activations, start, end),
                &format!("weighted_sum_no_bias_simd {at}"),
            );
            assert_close(
                weighted_sum_of_squares_simd(&synapses, &activations, start, end),
                scalar::weighted_sum_of_squares(&synapses, &activations, start, end),
                &format!("weighted_sum_of_squares_simd {at}"),
            );
            assert_close(
                weighted_sum_of_squares_v2_simd(&synapses, &activations, start, end, bias),
                scalar::weighted_sum_of_squares_v2(&synapses, &activations, start, end, bias),
                &format!("weighted_sum_of_squares_v2_simd {at}"),
            );
        }
    }
}

#[test]
fn the_remainder_continues_the_span_rather_than_restarting_it() {
    // A span of 8 + 3 and a span of 8 + 0 must differ by exactly the three
    // trailing synapses' contribution: the tail continues the running
    // accumulator, it does not start a second sum that is merged in.
    let (synapses, activations) = fixture();
    let bias = 0.8f32;
    let start = 3usize;

    let body_only = weighted_sum_no_bias_simd(&synapses, &activations, start, start + 8);
    let with_tail = weighted_sum_no_bias_simd(&synapses, &activations, start, start + 11);
    let tail_contribution =
        scalar::weighted_sum_no_bias(&synapses, &activations, start + 8, start + 11);
    assert_close(
        with_tail,
        body_only + tail_contribution,
        "no_bias tail continues the span",
    );

    let body_only = weighted_sum_simd(&synapses, &activations, start, start + 8, bias);
    let with_tail = weighted_sum_simd(&synapses, &activations, start, start + 11, bias);
    let tail_contribution =
        scalar::weighted_sum_no_bias(&synapses, &activations, start + 8, start + 11);
    assert_close(
        with_tail,
        body_only + tail_contribution,
        "weighted_sum tail continues the span",
    );
}

#[test]
fn an_empty_or_reversed_span_is_the_kernel_seed() {
    let (synapses, activations) = fixture();
    let bias = 1.25f32;

    // Reversed ranges saturate to a zero count (Issue #447), so each kernel
    // returns its seed rather than walking a colossal span.
    for (start, end) in [(5usize, 5usize), (9, 2)] {
        assert_eq!(
            weighted_sum_simd(&synapses, &activations, start, end, bias),
            bias
        );
        assert_eq!(
            weighted_sum_no_bias_simd(&synapses, &activations, start, end),
            0.0
        );
        assert_eq!(
            weighted_sum_of_squares_simd(&synapses, &activations, start, end),
            0.0
        );
        assert_eq!(
            weighted_sum_of_squares_v2_simd(&synapses, &activations, start, end, bias),
            0.0
        );
    }
}
