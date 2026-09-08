//! Issue #613 — the safe public SIMD surface must not read out of bounds.
//!
//! Every kernel re-exported from `neat_core::simd` is a **safe** `pub fn`, so
//! by Rust's soundness convention no combination of safe arguments may trigger
//! undefined behaviour. Before this fix their bodies reached
//! `activations.get_unchecked(from_index)` on the strength of a precondition
//! only `CompiledNetwork::new` upheld, which left any caller that does not hold
//! a loaded network able to drive an out-of-bounds read from entirely safe
//! code.
//!
//! Each test below hands a kernel the issue's reproducer — a `from_index` far
//! past the end of the activation buffer — and asserts the call fails loud
//! (panics on the checked fallback) rather than reading out of bounds. Against
//! the unfixed code the same calls returned a value read from beyond the
//! buffer, so every `expect_out_of_bounds_panic` here fails without the fix.

use std::panic::{AssertUnwindSafe, catch_unwind};

use neat_core::network::{SynapseData, hot_synapse_soa};
use neat_core::simd::{
    weighted_sum_interleaved_8, weighted_sum_no_bias_simd, weighted_sum_of_squares_simd,
    weighted_sum_of_squares_v2_simd, weighted_sum_simd, weighted_sum_simd_4records,
    weighted_sum_simd_8records,
};

fn synapse(from_index: u16, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
    }
}

/// The issue's reproducer: eight synapses all sourcing neuron 9,999 — enough to
/// drive the 8-wide chunk, the 4-wide chunk and the scalar tail of every
/// single-record kernel.
fn out_of_range_span() -> (Vec<SynapseData>, Vec<f32>) {
    (vec![synapse(9_999, 1.0); 8], vec![0.25_f32; 1])
}

/// Run `f` and assert it failed loud instead of returning a value read from
/// outside the activation buffer.
fn expect_out_of_bounds_panic<T>(what: &str, f: impl FnOnce() -> T) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    assert!(
        outcome.is_err(),
        "{what} accepted an out-of-range from_index instead of failing loud"
    );
}

#[test]
fn weighted_sum_simd_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_simd", || {
        weighted_sum_simd(&synapses, &activations, 0, synapses.len(), 0.0)
    });
}

#[test]
fn weighted_sum_no_bias_simd_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_no_bias_simd", || {
        weighted_sum_no_bias_simd(&synapses, &activations, 0, synapses.len())
    });
}

#[test]
fn weighted_sum_of_squares_simd_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_of_squares_simd", || {
        weighted_sum_of_squares_simd(&synapses, &activations, 0, synapses.len())
    });
}

#[test]
fn weighted_sum_of_squares_v2_simd_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_of_squares_v2_simd", || {
        weighted_sum_of_squares_v2_simd(&synapses, &activations, 0, synapses.len(), 0.5)
    });
}

#[test]
fn weighted_sum_simd_4records_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_simd_4records", || {
        weighted_sum_simd_4records(
            &synapses,
            &activations,
            &activations,
            &activations,
            &activations,
            0,
            synapses.len(),
            0.0,
        )
    });
}

#[test]
fn weighted_sum_simd_8records_rejects_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    expect_out_of_bounds_panic("weighted_sum_simd_8records", || {
        weighted_sum_simd_8records(
            &synapses,
            &activations,
            &activations,
            &activations,
            &activations,
            &activations,
            &activations,
            &activations,
            &activations,
            0,
            synapses.len(),
            0.0,
        )
    });
}

#[test]
fn weighted_sum_interleaved_8_rejects_out_of_range_from_index() {
    let (synapses, _) = out_of_range_span();
    let (hot_weights, hot_from) = hot_synapse_soa(&synapses);
    // One neuron's worth of 8-lane tile; every `from_index` is 9,999.
    let inter = vec![0.25_f32; 8];
    expect_out_of_bounds_panic("weighted_sum_interleaved_8", || {
        weighted_sum_interleaved_8(&hot_weights, &hot_from, &inter, 0, hot_from.len(), 0.0)
    });
}
