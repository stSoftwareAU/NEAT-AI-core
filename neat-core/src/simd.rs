//! SIMD-optimised weighted sum functions for neural network activation.
//!
//! This module provides SIMD-accelerated implementations for computing weighted sums
//! of synapse activations. Issue #1178, #1197, #1202, #1209.
//!
//! ## Optimisation strategies
//!
//! - **Dual accumulator**: [`weighted_sum_simd`] uses two independent SIMD
//!   accumulators to hide FMA latency by allowing out-of-order execution of
//!   independent multiply-add chains. The other three single-record kernels run
//!   a single accumulator in chunks of four.
//! - **FMA (fused multiply-add)**: Uses `f32x4_relaxed_madd` from relaxed-simd to
//!   perform multiply and add in a single instruction with better precision.
//! - **Multi-record batching**: Processes the same neuron across 4 or 8 input records
//!   in parallel, amortising weight loads across records.
//! - **Native (`not(wasm32)`)**: `simd_native.rs` uses **AVX2** (8-wide) and **FMA+SSE** (4-wide)
//!   on `x86_64`, **NEON** on `aarch64`; otherwise scalar (same numerics as the old fallback).
//! - **SIMD aggregate helpers**: `weighted_sum_of_squares_simd` for Hypotenuse
//!   and `weighted_sum_for_mean_simd` for Mean activation functions.
//! - **ISA-neutral scalar layer**: the reference scalar kernels and the
//!   small-count guard live once in [`scalar`] (Issue #447) and are shared by
//!   the wasm and native paths.
//! - **Shared chunk-walk scaffold**: the wasm kernels gather, reduce and finish
//!   their remainder through one set of helpers (Issue #448) so an improvement
//!   to the walk cannot land on one kernel only.

// Issue #447 - the ISA-neutral scalar layer: reference kernels, the saturating
// count guard, and the seed-taking tail helpers, shared by the wasm kernels
// below and the native kernels in `simd_native.rs`.
pub mod scalar;

// Native single-record/multi-record kernels live in `simd_native.rs`; only the
// wasm32 implementations below reference `SynapseData` directly.
#[cfg(target_arch = "wasm32")]
use crate::network::SynapseData;

// Issue #1178 - WASM SIMD support
// SIMD intrinsics for vectorised synapse weight summation
// Issue #1197 - Added f32x4_relaxed_madd for FMA optimisation
#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    f32x4, f32x4_add, f32x4_extract_lane, f32x4_mul, f32x4_relaxed_madd, f32x4_splat, v128,
};

// ============================================================================
// Chunk-walk scaffold (Issue #448)
//
// One home for *how* a wasm kernel walks a synapse span: gather four synapses
// into `f32x4` lanes, reduce the lanes back to a scalar, then finish the 0..3
// remainder through the seed-taking tail helpers in [`scalar`]. Every
// single-record kernel below calls these, so an improvement to the walk cannot
// land on one copy only — which is exactly how the Issue #1197 dual-accumulator
// rework came to sit on `weighted_sum_simd` alone.
//
// The *fold* deliberately stays in each kernel: these are calls, not a
// parameterised super-helper. Unifying the four folds would need a mode flag,
// and a flag is what makes a shared helper drift back apart.
//
// Every helper here touches `v128`/`f32x4_*`, so each repeats the
// `#[target_feature]` attributes — without them the intrinsics do not compile
// and the vector arguments do not inline into the caller.
//
// Index precondition (Issue #207): the 0..3 remainder runs through the
// `scalar::tail_*` helpers, which index `activations` with `get_unchecked`, so
// every `from_index` in `start..end` must be a valid index into `activations`.
// `CompiledNetwork::new` enforces exactly that at load time
// (`NetworkError::InvalidSynapseIndex`), which is why the native kernels have
// always carried this contract — the wasm kernels now share it rather than
// re-inlining a checked copy of the same loop.
// ============================================================================

/// Gather the four synapses at `base..base + 4` into `(weights, activations)`
/// lanes.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
fn gather4(synapses: &[SynapseData], activations: &[f32], base: usize) -> (v128, v128) {
    let s0 = &synapses[base];
    let s1 = &synapses[base + 1];
    let s2 = &synapses[base + 2];
    let s3 = &synapses[base + 3];

    (
        f32x4(s0.weight, s1.weight, s2.weight, s3.weight),
        f32x4(
            activations[s0.from_index as usize],
            activations[s1.from_index as usize],
            activations[s2.from_index as usize],
            activations[s3.from_index as usize],
        ),
    )
}

/// Lane-wise `activation[from] * weight` for the four synapses at
/// `base..base + 4`.
///
/// Bit-identical to computing each product scalar-wise and packing the lanes:
/// `f32x4_mul` is a plain IEEE-754 multiply per lane, not a relaxed operation.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
fn gather4_products(synapses: &[SynapseData], activations: &[f32], base: usize) -> v128 {
    let (weights, acts) = gather4(synapses, activations, base);
    f32x4_mul(weights, acts)
}

/// Horizontal sum of the four lanes, in lane order.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
fn reduce4(acc: v128) -> f32 {
    f32x4_extract_lane::<0>(acc)
        + f32x4_extract_lane::<1>(acc)
        + f32x4_extract_lane::<2>(acc)
        + f32x4_extract_lane::<3>(acc)
}

/// Issue #1178 - SIMD-optimised weighted sum for standard activations
/// Issue #1197 - Uses FMA (fused multiply-add) via relaxed-simd for better performance
///
/// Uses a dual-accumulator approach: processes 8 synapses per iteration with two
/// independent f32x4 accumulators to hide FMA latency via instruction-level
/// parallelism. Falls back to 4-wide for counts 4..7 and scalar for < 4.
///
/// Walks the span through the shared chunk-walk scaffold ([`gather4`],
/// [`reduce4`], the seed-taking tail in [`scalar`]) — Issue #448.
///
/// Issue #207 - the 0..3 tail delegates to a `get_unchecked` helper, so every
/// `synapse.from_index` in `start..end` must be a valid index into
/// `activations`. `CompiledNetwork::new` enforces this at load time
/// (`NetworkError::InvalidSynapseIndex`), so callers holding a loaded network
/// already satisfy the precondition.
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
    // For very small counts, scalar is faster due to SIMD setup overhead.
    let count = scalar::synapse_count(start, end);
    if count < scalar::SINGLE_RECORD_SIMD_MIN {
        return scalar::weighted_sum(synapses, activations, start, end, bias);
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
        let (weights0, acts0) = gather4(synapses, activations, i);
        acc0 = f32x4_relaxed_madd(weights0, acts0, acc0);

        // Second group of 4 -> acc1 (independent chain)
        let (weights1, acts1) = gather4(synapses, activations, i + 4);
        acc1 = f32x4_relaxed_madd(weights1, acts1, acc1);

        i += 8;
    }

    // Handle remaining chunk of 4 if present
    if scalar::synapse_count(i, end) >= 4 {
        let (weights, acts) = gather4(synapses, activations, i);
        acc0 = f32x4_relaxed_madd(weights, acts, acc0);
        i += 4;
    }

    // Merge accumulators, then reduce the lanes
    scalar_sum += reduce4(f32x4_add(acc0, acc1));

    // SAFETY: `end <= synapses.len()` (the caller's range indexes the chunk
    // loop above), and every `from_index` in `i..end` indexes `activations` —
    // load-time validation in `CompiledNetwork::new` (`AGENTS.md`,
    // "Unsafe & SIMD invariants") is `tail_sum`'s stated precondition.
    unsafe { scalar::tail_sum(synapses, activations, i, end, scalar_sum) }
}

/// Issue #1178 - SIMD-optimised sum of squared weighted activations for Hypotenuse.
///
/// Computes sum((activation[from] * weight)^2) using SIMD.
/// Used by the Hypotenuse squash function: sqrt(sum_sq) + bias.
///
/// Shares the chunk-walk scaffold ([`gather4_products`], [`reduce4`], the
/// seed-taking tail) with the other single-record kernels, and runs a single
/// accumulator over chunks of four. Same index precondition as
/// [`weighted_sum_simd`].
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
    let count = scalar::synapse_count(start, end);
    if count < scalar::SINGLE_RECORD_SIMD_MIN {
        return scalar::weighted_sum_of_squares(synapses, activations, start, end);
    }

    let mut acc = f32x4_splat(0.0);
    let chunks = count / 4;

    for chunk in 0..chunks {
        // Square and accumulate: acc += products * products
        let products = gather4_products(synapses, activations, start + chunk * 4);
        acc = f32x4_relaxed_madd(products, products, acc);
    }

    let scalar_sum = reduce4(acc);
    let remainder_start = start + chunks * 4;

    // SAFETY: same contract as `weighted_sum_simd`'s tail — load-time index
    // validation in `CompiledNetwork::new`.
    unsafe { scalar::tail_sum_of_squares(synapses, activations, remainder_start, end, scalar_sum) }
}

/// Issue #1178 - SIMD-optimised weighted sum for Mean activation.
///
/// Computes the plain weighted sum (without bias) using SIMD, intended for
/// the Mean squash: sum / n + bias. Omits the bias to keep the division clean.
///
/// Shares the chunk-walk scaffold ([`gather4`], [`reduce4`], the seed-taking
/// tail) with the other single-record kernels, but runs a **single**
/// accumulator over chunks of four — the Issue #1197 dual-accumulator form is
/// on [`weighted_sum_simd`] only. Same index precondition as
/// [`weighted_sum_simd`].
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
    let count = scalar::synapse_count(start, end);
    if count < scalar::SINGLE_RECORD_SIMD_MIN {
        return scalar::weighted_sum_no_bias(synapses, activations, start, end);
    }

    let mut acc = f32x4_splat(0.0);
    let chunks = count / 4;

    for chunk in 0..chunks {
        let (weights, acts) = gather4(synapses, activations, start + chunk * 4);
        acc = f32x4_relaxed_madd(weights, acts, acc);
    }

    let scalar_sum = reduce4(acc);
    let remainder_start = start + chunks * 4;

    // SAFETY: same contract as `weighted_sum_simd`'s tail — load-time index
    // validation in `CompiledNetwork::new`.
    unsafe { scalar::tail_sum(synapses, activations, remainder_start, end, scalar_sum) }
}

/// Issue #1178 - SIMD-optimised sum of squared (bias + weighted activation) for HypotenuseV2.
///
/// Computes sum((bias + activation[from] * weight)^2) using SIMD.
/// Used by the HypotenuseV2 squash function: sqrt(sum_sq).
///
/// Shares the chunk-walk scaffold ([`gather4_products`], [`reduce4`], the
/// seed-taking tail) with the other single-record kernels, and runs a single
/// accumulator over chunks of four. Same index precondition as
/// [`weighted_sum_simd`].
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
    let count = scalar::synapse_count(start, end);
    if count < scalar::SINGLE_RECORD_SIMD_MIN {
        return scalar::weighted_sum_of_squares_v2(synapses, activations, start, end, bias);
    }

    let bias_vec = f32x4_splat(bias);
    let mut acc = f32x4_splat(0.0);
    let chunks = count / 4;

    for chunk in 0..chunks {
        // Compute bias + activation * weight, then square and accumulate
        let weighted = gather4_products(synapses, activations, start + chunk * 4);
        let vals = f32x4_add(bias_vec, weighted);
        acc = f32x4_relaxed_madd(vals, vals, acc);
    }

    let scalar_sum = reduce4(acc);
    let remainder_start = start + chunks * 4;

    // SAFETY: same contract as `weighted_sum_simd`'s tail — load-time index
    // validation in `CompiledNetwork::new`.
    unsafe {
        scalar::tail_sum_of_squares_v2(
            synapses,
            activations,
            remainder_start,
            end,
            scalar_sum,
            bias,
        )
    }
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
    if scalar::synapse_count(start, end) == 0 {
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
    if scalar::synapse_count(start, end) == 0 {
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

/// Issue #287 - record-interleaved 8-lane weighted sum for the batched scoring
/// hot path.
///
/// `inter` is the transposed batch activation buffer: lane `l` of source neuron
/// `n` lives at `inter[n * 8 + l]`, so all eight records for a synapse's source
/// are contiguous. Each gather is then two adjacent 4-wide reads from one cache
/// line instead of eight scattered per-lane loads, cutting gather traffic on the
/// gather-bound production topology. Numerically identical to
/// [`weighted_sum_simd_8records`]: same per-synapse FMA order, bias seeded into
/// every lane.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
pub fn weighted_sum_interleaved_8(
    synapses: &[SynapseData],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; 8] {
    if scalar::synapse_count(start, end) == 0 {
        return [bias; 8];
    }

    let mut acc03 = f32x4_splat(bias);
    let mut acc47 = f32x4_splat(bias);

    for i in start..end {
        let synapse = &synapses[i];
        let base = synapse.from_index as usize * 8;
        let weights = f32x4_splat(synapse.weight);

        // The eight lanes are contiguous, so these read one cache line.
        let acts03 = f32x4(
            inter[base],
            inter[base + 1],
            inter[base + 2],
            inter[base + 3],
        );
        let acts47 = f32x4(
            inter[base + 4],
            inter[base + 5],
            inter[base + 6],
            inter[base + 7],
        );

        acc03 = f32x4_relaxed_madd(weights, acts03, acc03);
        acc47 = f32x4_relaxed_madd(weights, acts47, acc47);
    }

    [
        f32x4_extract_lane::<0>(acc03),
        f32x4_extract_lane::<1>(acc03),
        f32x4_extract_lane::<2>(acc03),
        f32x4_extract_lane::<3>(acc03),
        f32x4_extract_lane::<0>(acc47),
        f32x4_extract_lane::<1>(acc47),
        f32x4_extract_lane::<2>(acc47),
        f32x4_extract_lane::<3>(acc47),
    ]
}

// Native (non-wasm32) multi-record helpers now live in `simd_native.rs` and use
// AVX2/FMA on x86_64, NEON on aarch64, falling back to scalar elsewhere.
#[cfg(not(target_arch = "wasm32"))]
#[path = "simd_native.rs"]
mod simd_native;

// Issue #153 - The single-record primitives are also native-SIMD accelerated
// (AVX2/FMA on x86_64, NEON on aarch64, scalar fallback elsewhere and for the
// 0..3 synapse tail). They live in `simd_native.rs` alongside the multi-record
// kernels and run on the primary `activate()` forward-pass hot path.
#[cfg(not(target_arch = "wasm32"))]
pub use simd_native::{
    weighted_sum_interleaved_8, weighted_sum_no_bias_simd, weighted_sum_of_squares_simd,
    weighted_sum_of_squares_v2_simd, weighted_sum_simd, weighted_sum_simd_4records,
    weighted_sum_simd_8records,
};
