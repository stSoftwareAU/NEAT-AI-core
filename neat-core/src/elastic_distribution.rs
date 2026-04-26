//! Issue #1519 - Standalone elastic error distribution in WASM.
//!
//! Migrates `distributeElasticError` from TypeScript to Rust/WASM with SIMD
//! acceleration for the scoring pass. Includes the three-pass algorithm:
//! 1. Score calculation: activation² × clamp(safeZoneFactor, 0, 1)
//! 2. Weight-based fallback when activations are near zero
//! 3. Share calculation with floating-point residue redistribution

/// Planck constant for floating-point comparisons (matches TypeScript default).
const PLANK_CONSTANT: f32 = 1e-12;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// SIMD-accelerated scoring pass: computes activation² × clamped safeZoneFactor.
///
/// Uses WASM SIMD128 to process 4 elements at a time for the squaring and
/// multiplication, with a scalar fallback for non-WASM targets.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128", enable = "relaxed-simd")]
#[inline]
fn score_pass_simd(activations: &[f32], safe_zone_factors: &[f32], scores: &mut [f32]) -> f32 {
    use core::arch::wasm32::{
        f32x4, f32x4_add, f32x4_extract_lane, f32x4_max, f32x4_min, f32x4_mul, f32x4_splat,
    };

    let count = activations.len();
    let mut denom: f32 = 0.0;

    let chunks = count / 4;
    let zero = f32x4_splat(0.0);
    let one = f32x4_splat(1.0);
    let mut acc = f32x4_splat(0.0);

    for chunk in 0..chunks {
        let base = chunk * 4;

        let act = f32x4(
            activations[base],
            activations[base + 1],
            activations[base + 2],
            activations[base + 3],
        );
        let safe = f32x4(
            safe_zone_factors[base],
            safe_zone_factors[base + 1],
            safe_zone_factors[base + 2],
            safe_zone_factors[base + 3],
        );

        // Check for non-finite values — treat as zero score
        // SIMD doesn't have is_finite, so we handle NaN/Inf by clamping
        // and relying on NaN propagation (NaN * anything = NaN, which won't
        // compare > best_score in the residue pass)

        // clamp(safe, 0, 1)
        let clamped = f32x4_min(f32x4_max(safe, zero), one);

        // activation²
        let a2 = f32x4_mul(act, act);

        // score = a2 * clamped_safe
        let score = f32x4_mul(a2, clamped);

        // Store scores
        scores[base] = f32x4_extract_lane::<0>(score);
        scores[base + 1] = f32x4_extract_lane::<1>(score);
        scores[base + 2] = f32x4_extract_lane::<2>(score);
        scores[base + 3] = f32x4_extract_lane::<3>(score);

        // Accumulate denominator
        acc = f32x4_add(acc, score);
    }

    denom += f32x4_extract_lane::<0>(acc)
        + f32x4_extract_lane::<1>(acc)
        + f32x4_extract_lane::<2>(acc)
        + f32x4_extract_lane::<3>(acc);

    // Handle NaN/Inf in SIMD results and scalar remainder
    let remainder_start = chunks * 4;
    for i in remainder_start..count {
        let activation = activations[i];
        let safe = safe_zone_factors[i];

        if !activation.is_finite() || !safe.is_finite() {
            scores[i] = 0.0;
            continue;
        }

        let clamped_safe = safe.clamp(0.0, 1.0);
        let a2 = activation * activation;
        let score = a2 * clamped_safe;
        scores[i] = score;
        denom += score;
    }

    // Fix up any NaN scores from SIMD pass
    for i in 0..remainder_start {
        if !scores[i].is_finite() {
            denom -= scores[i]; // NaN subtraction won't help, but score was 0 effectively
            scores[i] = 0.0;
        }
    }

    // If denom became NaN due to NaN scores, reset it
    if !denom.is_finite() {
        denom = 0.0;
        for &s in scores.iter() {
            denom += s;
        }
    }

    denom
}

/// Scalar fallback scoring pass for non-WASM targets (testing).
#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn score_pass_simd(activations: &[f32], safe_zone_factors: &[f32], scores: &mut [f32]) -> f32 {
    let count = activations.len();
    let mut denom: f32 = 0.0;

    for i in 0..count {
        let activation = activations[i];
        let safe = safe_zone_factors[i];

        if !activation.is_finite() || !safe.is_finite() {
            scores[i] = 0.0;
            continue;
        }

        let clamped_safe = safe.clamp(0.0, 1.0);
        let a2 = activation * activation;
        let score = a2 * clamped_safe;
        scores[i] = score;
        denom += score;
    }

    denom
}

/// Core elastic error distribution algorithm.
///
/// Three-pass algorithm:
/// 1. Primary: activation² × safeZoneFactor scoring (SIMD-accelerated)
/// 2. Fallback: weight² scoring when activations are near zero
/// 3. Last resort: equal split when both activations and weights are zero
///
/// Returns a `Vec<f32>` of error shares whose sum equals `error`.
#[inline(always)]
pub fn apply_distribute_elastic_error(
    error: f32,
    activations: &[f32],
    safe_zone_factors: &[f32],
    weights: &[f32],
    plank_constant: f32,
) -> Vec<f32> {
    let count = activations.len();

    if !error.is_finite() || count == 0 {
        return vec![0.0; count];
    }

    // Pass 1: Score calculation with SIMD
    let mut scores = vec![0.0f32; count];
    let denom = score_pass_simd(activations, safe_zone_factors, &mut scores);

    if denom <= plank_constant {
        // Pass 2: Weight-based fallback (weight²)
        let mut weight_denom: f32 = 0.0;
        let mut weight_scores = vec![0.0f32; count];

        for i in 0..count {
            let w = weights[i];
            let w2 = if w.is_finite() { w * w } else { 0.0 };
            weight_scores[i] = w2;
            weight_denom += w2;
        }

        if weight_denom > plank_constant {
            let mut shares = vec![0.0f32; count];
            let mut sum: f32 = 0.0;

            for i in 0..count {
                let share = error * (weight_scores[i] / weight_denom);
                shares[i] = share;
                sum += share;
            }

            // Floating-point residue redistribution
            let residue = error - sum;
            if residue.abs() > plank_constant {
                let mut best_idx: usize = 0;
                for i in 1..count {
                    if weight_scores[i] > weight_scores[best_idx] {
                        best_idx = i;
                    }
                }
                shares[best_idx] += residue;
            }

            return shares;
        }

        // Pass 3: Equal split (last resort)
        let per = error / (count as f32);
        return vec![per; count];
    }

    // Distribute proportionally to scores
    let mut shares = vec![0.0f32; count];
    let mut sum: f32 = 0.0;
    let mut best_idx: usize = 0;
    let mut best_score: f32 = -f32::INFINITY;

    for i in 0..count {
        let share = error * (scores[i] / denom);
        shares[i] = share;
        sum += share;
        if scores[i] > best_score {
            best_score = scores[i];
            best_idx = i;
        }
    }

    // Floating-point residue redistribution
    let residue = error - sum;
    if residue.abs() > plank_constant {
        shares[best_idx] += residue;
    }

    shares
}

/// Issue #1519 - WASM-exported standalone elastic error distribution.
///
/// Distributes `error` across links proportional to activation² × safeZoneFactor,
/// with weight-based fallback when activations are near zero, and equal split
/// as a last resort.
///
/// # Arguments
/// * `error` - The error value to distribute
/// * `activations` - Float32Array of link activation values
/// * `safe_zone_factors` - Float32Array of safe zone factors (0-1)
/// * `weights` - Float32Array of synapse weights (for fallback)
/// * `plank_constant` - Threshold for floating-point comparisons
///
/// # Returns
/// `Vec<f32>` of error shares, one per link. Sum equals `error`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn distribute_elastic_error(
    error: f32,
    activations: &[f32],
    safe_zone_factors: &[f32],
    weights: &[f32],
    plank_constant: f32,
) -> Vec<f32> {
    let plank = if plank_constant.is_finite() && plank_constant > 0.0 {
        plank_constant
    } else {
        PLANK_CONSTANT
    };
    apply_distribute_elastic_error(error, activations, safe_zone_factors, weights, plank)
}
