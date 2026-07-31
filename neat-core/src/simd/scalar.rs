//! ISA-neutral scalar layer of the weighted-sum kernels (Issue #447).
//!
//! This module is the single home of one rule: **the reference scalar semantics
//! of each weighted-sum kernel, and the small-count guard in front of it** — the
//! thing every SIMD path must agree with bit-for-bit. It carries no intrinsics
//! and no `cfg`, so the `wasm32` kernels in [`crate::simd`] and the x86/aarch64
//! kernels in `simd_native.rs` share one copy instead of drifting apart.
//!
//! Two layers, deliberately distinct:
//!
//! - **Reference kernels** ([`weighted_sum`], [`weighted_sum_of_squares`],
//!   [`weighted_sum_no_bias`], [`weighted_sum_of_squares_v2`]) seed their own
//!   accumulator and index the activation buffer safely. They are what a
//!   below-threshold count falls back to on every target.
//! - **Seed-taking tail helpers** ([`tail_sum`], [`tail_sum_of_squares`],
//!   [`tail_sum_of_squares_v2`]) take the caller's *running* accumulator, so a
//!   SIMD kernel's 0..3 remainder continues the same f32 rounding order rather
//!   than starting a second sum. They index with `get_unchecked` under the
//!   load-time index-validation invariant documented in `AGENTS.md`
//!   ("Unsafe & SIMD invariants"), which is why they are `unsafe fn`.
//!
//! The two layers are pinned to each other by
//! `neat-core/tests/simd_scalar_layer.rs`: splitting a range at any point and
//! continuing through a tail helper reproduces the reference result exactly.

use crate::network::SynapseData;

/// SIMD setup (lane gather, horizontal reduce) only pays off once there is at
/// least one full 4-wide chunk; below that the scalar reference kernels win.
pub const SINGLE_RECORD_SIMD_MIN: usize = 4;

/// Number of synapses in `start..end`.
///
/// Saturating: a reversed range (`end < start`) counts as zero rather than
/// underflowing, so a caller that hands over a degenerate range gets the
/// empty-range answer instead of a panic in debug or a colossal count in
/// release.
#[inline]
pub fn synapse_count(start: usize, end: usize) -> usize {
    end.saturating_sub(start)
}

/// Reference kernel: `bias + sum(activation[from] * weight)`.
#[inline]
pub fn weighted_sum(
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

/// Reference kernel: `sum((activation[from] * weight)^2)` (Hypotenuse).
#[inline]
pub fn weighted_sum_of_squares(
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

/// Reference kernel: `sum(activation[from] * weight)`, no bias (Mean).
#[inline]
pub fn weighted_sum_no_bias(
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

/// Reference kernel: `sum((bias + activation[from] * weight)^2)` (HypotenuseV2).
#[inline]
pub fn weighted_sum_of_squares_v2(
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

/// Continue a running plain weighted sum over the `i..end` remainder.
///
/// Takes `acc` — the caller's running accumulator — so a SIMD kernel's tail
/// keeps the reference f32 rounding order instead of starting a second sum and
/// merging it. Seeding it with `bias` and `i == start` reproduces
/// [`weighted_sum`] exactly.
///
/// # Safety
/// `end` must be `<= synapses.len()`, and every `synapse.from_index` in
/// `i..end` must be a valid index into `activations`. `CompiledNetwork::new`
/// enforces the index range at load time (`NetworkError::InvalidSynapseIndex`),
/// so callers holding a loaded network already satisfy this — see the
/// "Unsafe & SIMD invariants" section of `AGENTS.md`.
#[inline]
pub unsafe fn tail_sum(
    synapses: &[SynapseData],
    activations: &[f32],
    i: usize,
    end: usize,
    acc: f32,
) -> f32 {
    let mut sum = acc;
    let mut idx = i;
    while idx < end {
        // SAFETY: `idx < end <= synapses.len()`, and the caller guarantees
        // every `from_index` in `i..end` indexes `activations` (load-time
        // validation in `CompiledNetwork::new`).
        let s = unsafe { synapses.get_unchecked(idx) };
        sum += unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
        idx += 1;
    }
    sum
}

/// Continue a running sum of squared weighted activations over `i..end`.
///
/// Seeding it with `0.0` and `i == start` reproduces
/// [`weighted_sum_of_squares`] exactly.
///
/// # Safety
/// Same contract as [`tail_sum`].
#[inline]
pub unsafe fn tail_sum_of_squares(
    synapses: &[SynapseData],
    activations: &[f32],
    i: usize,
    end: usize,
    acc: f32,
) -> f32 {
    let mut sum = acc;
    let mut idx = i;
    while idx < end {
        // SAFETY: same as `tail_sum` — load-time index validation.
        let s = unsafe { synapses.get_unchecked(idx) };
        let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
        sum += val * val;
        idx += 1;
    }
    sum
}

/// Continue a running sum of squared `(bias + weighted activation)` over
/// `i..end`.
///
/// Seeding it with `0.0` and `i == start` reproduces
/// [`weighted_sum_of_squares_v2`] exactly.
///
/// # Safety
/// Same contract as [`tail_sum`].
#[inline]
pub unsafe fn tail_sum_of_squares_v2(
    synapses: &[SynapseData],
    activations: &[f32],
    i: usize,
    end: usize,
    acc: f32,
    bias: f32,
) -> f32 {
    let mut sum = acc;
    let mut idx = i;
    while idx < end {
        // SAFETY: same as `tail_sum` — load-time index validation.
        let s = unsafe { synapses.get_unchecked(idx) };
        let val = bias + unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
        sum += val * val;
        idx += 1;
    }
    sum
}
