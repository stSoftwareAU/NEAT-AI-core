//! SIMD-optimised weighted sum functions for neural network activation.
//!
//! This module provides SIMD-accelerated implementations for computing weighted sums
//! of synapse activations. Issue #1178, #1197, #1202, #1209.
//!
//! ## Optimisation strategies
//!
//! - **Dual accumulator**: Uses two independent SIMD accumulators to hide FMA latency
//!   by allowing out-of-order execution of independent multiply-add chains.
//! - **FMA (fused multiply-add)**: Uses `f32x4_relaxed_madd` from relaxed-simd to
//!   perform multiply and add in a single instruction with better precision.
//! - **Multi-record batching**: Processes the same neuron across 4 or 8 input records
//!   in parallel, amortising weight loads across records.
//! - **Native (`not(wasm32)`)**: `simd_native.rs` uses **AVX2** (8-wide) and **FMA+SSE** (4-wide)
//!   on `x86_64`, **NEON** on `aarch64`; otherwise scalar (same numerics as the old fallback).
//! - **SIMD aggregate helpers**: `weighted_sum_of_squares_simd` for Hypotenuse
//!   and `weighted_sum_for_mean_simd` for Mean activation functions.

use crate::network::SynapseData;

// Issue #1178 - WASM SIMD support
// SIMD intrinsics for vectorised synapse weight summation
// Issue #1197 - Added f32x4_relaxed_madd for FMA optimisation
#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{f32x4, f32x4_add, f32x4_extract_lane, f32x4_relaxed_madd, f32x4_splat};

/// Issue #1178 - SIMD-optimised weighted sum for standard activations
/// Issue #1197 - Uses FMA (fused multiply-add) via relaxed-simd for better performance
///
/// Uses a dual-accumulator approach: processes 8 synapses per iteration with two
/// independent f32x4 accumulators to hide FMA latency via instruction-level
/// parallelism. Falls back to 4-wide for counts 4..7 and scalar for < 4.
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let count = end - start;
    if count == 0 {
        return bias;
    }

    // For very small counts, scalar is faster due to SIMD setup overhead
    if count < 4 {
        let mut sum = bias;
        for i in start..end {
            let synapse = &synapses[i];
            sum += activations[synapse.from_index as usize] * synapse.weight;
        }
        return sum;
    }

    // Dual-accumulator approach: two independent accumulators hide FMA latency
    // by allowing out-of-order execution of independent dependency chains.
    let mut acc0 = f32x4_splat(0.0);
    let mut acc1 = f32x4_splat(0.0);
    let mut scalar_sum = bias;

    // Process in chunks of 8 (two groups of 4) for dual accumulation
    let chunks_of_8 = count / 8;
    let mut i = start;

    for _ in 0..chunks_of_8 {
        // First group of 4 -> acc0
        let s0 = &synapses[i];
        let s1 = &synapses[i + 1];
        let s2 = &synapses[i + 2];
        let s3 = &synapses[i + 3];
        let weights0 = f32x4(s0.weight, s1.weight, s2.weight, s3.weight);
        let acts0 = f32x4(
            activations[s0.from_index as usize],
            activations[s1.from_index as usize],
            activations[s2.from_index as usize],
            activations[s3.from_index as usize],
        );
        acc0 = f32x4_relaxed_madd(weights0, acts0, acc0);

        // Second group of 4 -> acc1 (independent chain)
        let s4 = &synapses[i + 4];
        let s5 = &synapses[i + 5];
        let s6 = &synapses[i + 6];
        let s7 = &synapses[i + 7];
        let weights1 = f32x4(s4.weight, s5.weight, s6.weight, s7.weight);
        let acts1 = f32x4(
            activations[s4.from_index as usize],
            activations[s5.from_index as usize],
            activations[s6.from_index as usize],
            activations[s7.from_index as usize],
        );
        acc1 = f32x4_relaxed_madd(weights1, acts1, acc1);

        i += 8;
    }

    // Handle remaining chunk of 4 if present
    let remaining = end - i;
    if remaining >= 4 {
        let s0 = &synapses[i];
        let s1 = &synapses[i + 1];
        let s2 = &synapses[i + 2];
        let s3 = &synapses[i + 3];
        let weights = f32x4(s0.weight, s1.weight, s2.weight, s3.weight);
        let acts = f32x4(
            activations[s0.from_index as usize],
            activations[s1.from_index as usize],
            activations[s2.from_index as usize],
            activations[s3.from_index as usize],
        );
        acc0 = f32x4_relaxed_madd(weights, acts, acc0);
        i += 4;
    }

    // Merge accumulators
    let merged = f32x4_add(acc0, acc1);

    // Horizontal sum of merged SIMD accumulator
    scalar_sum += f32x4_extract_lane::<0>(merged)
        + f32x4_extract_lane::<1>(merged)
        + f32x4_extract_lane::<2>(merged)
        + f32x4_extract_lane::<3>(merged);

    // Handle scalar remainder (0-3 synapses)
    for idx in i..end {
        let synapse = &synapses[idx];
        scalar_sum += activations[synapse.from_index as usize] * synapse.weight;
    }

    scalar_sum
}

/// Issue #1178 - SIMD-optimised sum of squared weighted activations for Hypotenuse.
///
/// Computes sum((activation[from] * weight)^2) using SIMD with dual accumulators.
/// Used by the Hypotenuse squash function: sqrt(sum_sq) + bias.
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_of_squares_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let count = end - start;
    if count == 0 {
        return 0.0;
    }

    if count < 4 {
        let mut sum_sq = 0.0f32;
        for i in start..end {
            let synapse = &synapses[i];
            let val = activations[synapse.from_index as usize] * synapse.weight;
            sum_sq += val * val;
        }
        return sum_sq;
    }

    let mut acc = f32x4_splat(0.0);
    let mut scalar_sum = 0.0f32;
    let chunks = count / 4;

    for chunk in 0..chunks {
        let base = start + chunk * 4;
        let s0 = &synapses[base];
        let s1 = &synapses[base + 1];
        let s2 = &synapses[base + 2];
        let s3 = &synapses[base + 3];

        // Compute weighted activations
        let products = f32x4(
            activations[s0.from_index as usize] * s0.weight,
            activations[s1.from_index as usize] * s1.weight,
            activations[s2.from_index as usize] * s2.weight,
            activations[s3.from_index as usize] * s3.weight,
        );

        // Square and accumulate: acc += products * products
        acc = f32x4_relaxed_madd(products, products, acc);
    }

    scalar_sum += f32x4_extract_lane::<0>(acc)
        + f32x4_extract_lane::<1>(acc)
        + f32x4_extract_lane::<2>(acc)
        + f32x4_extract_lane::<3>(acc);

    let remainder_start = start + chunks * 4;
    for i in remainder_start..end {
        let synapse = &synapses[i];
        let val = activations[synapse.from_index as usize] * synapse.weight;
        scalar_sum += val * val;
    }

    scalar_sum
}

/// Issue #1178 - SIMD-optimised weighted sum for Mean activation.
///
/// Computes the plain weighted sum (without bias) using SIMD, intended for
/// the Mean squash: sum / n + bias. Shares the dual-accumulator approach
/// with `weighted_sum_simd` but omits the bias to keep the division clean.
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_no_bias_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let count = end - start;
    if count == 0 {
        return 0.0;
    }

    if count < 4 {
        let mut sum = 0.0f32;
        for i in start..end {
            let synapse = &synapses[i];
            sum += activations[synapse.from_index as usize] * synapse.weight;
        }
        return sum;
    }

    let mut acc = f32x4_splat(0.0);
    let mut scalar_sum = 0.0f32;
    let chunks = count / 4;

    for chunk in 0..chunks {
        let base = start + chunk * 4;
        let s0 = &synapses[base];
        let s1 = &synapses[base + 1];
        let s2 = &synapses[base + 2];
        let s3 = &synapses[base + 3];

        let weights = f32x4(s0.weight, s1.weight, s2.weight, s3.weight);
        let acts = f32x4(
            activations[s0.from_index as usize],
            activations[s1.from_index as usize],
            activations[s2.from_index as usize],
            activations[s3.from_index as usize],
        );
        acc = f32x4_relaxed_madd(weights, acts, acc);
    }

    scalar_sum += f32x4_extract_lane::<0>(acc)
        + f32x4_extract_lane::<1>(acc)
        + f32x4_extract_lane::<2>(acc)
        + f32x4_extract_lane::<3>(acc);

    let remainder_start = start + chunks * 4;
    for i in remainder_start..end {
        let synapse = &synapses[i];
        scalar_sum += activations[synapse.from_index as usize] * synapse.weight;
    }

    scalar_sum
}

/// Issue #1178 - SIMD-optimised sum of squared (bias + weighted activation) for HypotenuseV2.
///
/// Computes sum((bias + activation[from] * weight)^2) using SIMD.
/// Used by the HypotenuseV2 squash function: sqrt(sum_sq).
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_of_squares_v2_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let count = end - start;
    if count == 0 {
        return 0.0;
    }

    if count < 4 {
        let mut sum_sq = 0.0f32;
        for i in start..end {
            let synapse = &synapses[i];
            let val = bias + activations[synapse.from_index as usize] * synapse.weight;
            sum_sq += val * val;
        }
        return sum_sq;
    }

    let bias_vec = f32x4_splat(bias);
    let mut acc = f32x4_splat(0.0);
    let mut scalar_sum = 0.0f32;
    let chunks = count / 4;

    for chunk in 0..chunks {
        let base = start + chunk * 4;
        let s0 = &synapses[base];
        let s1 = &synapses[base + 1];
        let s2 = &synapses[base + 2];
        let s3 = &synapses[base + 3];

        // Compute bias + activation * weight
        let weighted = f32x4(
            activations[s0.from_index as usize] * s0.weight,
            activations[s1.from_index as usize] * s1.weight,
            activations[s2.from_index as usize] * s2.weight,
            activations[s3.from_index as usize] * s3.weight,
        );
        let vals = f32x4_add(bias_vec, weighted);

        // Square and accumulate: acc += vals * vals
        acc = f32x4_relaxed_madd(vals, vals, acc);
    }

    scalar_sum += f32x4_extract_lane::<0>(acc)
        + f32x4_extract_lane::<1>(acc)
        + f32x4_extract_lane::<2>(acc)
        + f32x4_extract_lane::<3>(acc);

    let remainder_start = start + chunks * 4;
    for i in remainder_start..end {
        let synapse = &synapses[i];
        let val = bias + activations[synapse.from_index as usize] * synapse.weight;
        scalar_sum += val * val;
    }

    scalar_sum
}

/// Issue #1202 - SIMD-optimised weighted sum for 4 records simultaneously.
///
/// Processes the same neuron for 4 different records in parallel using SIMD.
/// Each record has its own activation buffer, but weights are shared.
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_simd_4records(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32) {
    let count = end - start;
    if count == 0 {
        return (bias, bias, bias, bias);
    }

    // Initialise accumulators with bias for all 4 records
    let mut acc = f32x4_splat(bias);

    // Process each synapse, gathering activations from all 4 records
    for i in start..end {
        let synapse = &synapses[i];
        let from = synapse.from_index as usize;
        let weight = synapse.weight;

        // Gather activations from 4 different records at the same position
        let acts = f32x4(act0[from], act1[from], act2[from], act3[from]);

        // Broadcast weight to all 4 lanes
        let weights = f32x4_splat(weight);

        // FMA: acc = weights * acts + acc
        acc = f32x4_relaxed_madd(weights, acts, acc);
    }

    // Extract results for all 4 records
    (
        f32x4_extract_lane::<0>(acc),
        f32x4_extract_lane::<1>(acc),
        f32x4_extract_lane::<2>(acc),
        f32x4_extract_lane::<3>(acc),
    )
}

/// Issue #1209 - SIMD-optimised weighted sum for 8 records simultaneously.
///
/// Processes the same neuron for 8 different records in parallel using two SIMD accumulators.
/// Each record has its own activation buffer, but weights are shared.
/// This extends the 4-record approach (Issue #1202) by stacking two v128 operations
/// for better cache utilisation and amortised overhead.
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn weighted_sum_simd_8records(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    act4: &[f32],
    act5: &[f32],
    act6: &[f32],
    act7: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
    let count = end - start;
    if count == 0 {
        return (bias, bias, bias, bias, bias, bias, bias, bias);
    }

    // Initialise accumulators with bias for all 8 records (two SIMD vectors)
    let mut acc_0_3 = f32x4_splat(bias);
    let mut acc_4_7 = f32x4_splat(bias);

    // Process each synapse, gathering activations from all 8 records
    for i in start..end {
        let synapse = &synapses[i];
        let from = synapse.from_index as usize;
        let weight = synapse.weight;

        // Gather activations from 8 different records at the same position
        let acts_0_3 = f32x4(act0[from], act1[from], act2[from], act3[from]);
        let acts_4_7 = f32x4(act4[from], act5[from], act6[from], act7[from]);

        // Broadcast weight to all lanes
        let weights = f32x4_splat(weight);

        // FMA: acc = weights * acts + acc
        acc_0_3 = f32x4_relaxed_madd(weights, acts_0_3, acc_0_3);
        acc_4_7 = f32x4_relaxed_madd(weights, acts_4_7, acc_4_7);
    }

    // Extract results for all 8 records
    (
        f32x4_extract_lane::<0>(acc_0_3),
        f32x4_extract_lane::<1>(acc_0_3),
        f32x4_extract_lane::<2>(acc_0_3),
        f32x4_extract_lane::<3>(acc_0_3),
        f32x4_extract_lane::<0>(acc_4_7),
        f32x4_extract_lane::<1>(acc_4_7),
        f32x4_extract_lane::<2>(acc_4_7),
        f32x4_extract_lane::<3>(acc_4_7),
    )
}

// Native (non-wasm32) multi-record helpers now live in `simd_native.rs` and use
// AVX2/FMA on x86_64, NEON on aarch64, falling back to scalar elsewhere.
#[cfg(not(target_arch = "wasm32"))]
#[path = "simd_native.rs"]
mod simd_native;

#[cfg(not(target_arch = "wasm32"))]
pub use simd_native::{weighted_sum_simd_4records, weighted_sum_simd_8records};

/// Scalar fallback for non-WASM targets (for testing)
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn weighted_sum_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let mut sum = bias;
    for synapse in synapses.iter().take(end).skip(start) {
        sum += activations[synapse.from_index as usize] * synapse.weight;
    }
    sum
}

/// Scalar fallback for non-WASM targets (for testing)
/// Issue #1178 - Sum of squares for Hypotenuse
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn weighted_sum_of_squares_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let mut sum_sq = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        let val = activations[synapse.from_index as usize] * synapse.weight;
        sum_sq += val * val;
    }
    sum_sq
}

/// Scalar fallback for non-WASM targets (for testing)
/// Issue #1178 - Weighted sum without bias for Mean
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn weighted_sum_no_bias_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let mut sum = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        sum += activations[synapse.from_index as usize] * synapse.weight;
    }
    sum
}

/// Scalar fallback for non-WASM targets (for testing)
/// Issue #1178 - Sum of squares V2 for HypotenuseV2
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn weighted_sum_of_squares_v2_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let mut sum_sq = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        let val = bias + activations[synapse.from_index as usize] * synapse.weight;
        sum_sq += val * val;
    }
    sum_sq
}
