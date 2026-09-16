//! Safe zone adjustment for backpropagation through activation functions.
//!
//! This module determines how useful it is to backpropagate through a neuron
//! based on saturation levels. Issue #1140 - WASM Migration Phase 8.

use crate::squash::SquashType;
use crate::squash_profile::{
    SafeBand, SafeZoneRule, SaturatingBand, SymmetricBand, squash_profile,
};

/// Weight-guard band shared by every rule that defers to a recovering weight.
const MIN_WEIGHT: f32 = 1e-3;
/// Upper edge of that band.
const MAX_WEIGHT: f32 = 1e3;

/// True when the weight has left `[1e-3, 1e3]` **and** the error is already
/// pulling it back — the gradient should wait for the weight to recover first.
///
/// One home for a guard four rules used to restate inline (Issue #673).
#[inline(always)]
fn weight_is_recovering(weight: f32, error: f32) -> bool {
    let abs_weight = weight.abs();
    (abs_weight < MIN_WEIGHT && weight * error > 0.0)
        || (abs_weight > MAX_WEIGHT && weight * error < 0.0)
}

/// Gradient-flow factor for a squash whose safe band is `[band.min, band.max]`
/// and which fades linearly to zero over `band.fade` either side of it.
///
/// The rule: no gradient when the raw input is already outside the band and the
/// error pushes it further out; no gradient when the raw input is inside the
/// band but the weight sits outside `[1e-3, 1e3]` and the error is already
/// correcting it (let the weight recover first); full gradient inside the band;
/// otherwise a linear fade to zero across `band.fade` past each edge.
#[inline(always)]
fn bounded_safe_zone(raw_input: f32, error: f32, weight: f32, band: SafeBand) -> f32 {
    let SafeBand {
        min: safe_min,
        max: safe_max,
        fade,
    } = band;

    let in_safe_range = raw_input >= safe_min && raw_input <= safe_max;
    let raw_getting_worse =
        (raw_input < safe_min && error < 0.0) || (raw_input > safe_max && error > 0.0);

    if !in_safe_range && raw_getting_worse {
        return 0.0;
    }
    if in_safe_range && weight_is_recovering(weight, error) {
        return 0.0;
    }

    if in_safe_range {
        return 1.0;
    }
    if raw_input > safe_max && raw_input <= safe_max + fade {
        return 1.0 - (raw_input - safe_max) / fade;
    }
    if raw_input < safe_min && raw_input >= safe_min - fade {
        return 1.0 - (safe_min - raw_input) / fade;
    }

    0.0
}

/// Gradient-flow factor for a sigmoid-shaped squash: full inside the safe band,
/// a fixed recovery factor once the raw input has saturated past
/// `band.recovery` in the direction the error is already correcting, then a
/// linear fade out to `band.fade`.
#[inline(always)]
fn saturating_safe_zone(raw_input: f32, error: f32, band: SaturatingBand) -> f32 {
    let (safe_low, safe_high) = band.safe;
    let (fade_min, fade_max) = band.fade;
    let (recovery_low, recovery_high) = band.recovery;

    if raw_input >= safe_low && raw_input <= safe_high {
        return 1.0;
    }

    // Recovery: out of the band, but the error pulls back toward the centre.
    if raw_input <= recovery_low && error > 0.0 {
        return band.factor;
    }
    if raw_input >= recovery_high && error < 0.0 {
        return band.factor;
    }

    // Fade linearly from the band edge out to the saturation limit.
    if raw_input > safe_high && raw_input <= fade_max {
        return 1.0 - (raw_input - safe_high) / (fade_max - safe_high);
    }
    if raw_input < safe_low && raw_input >= fade_min {
        return (raw_input - fade_min) / (safe_low - fade_min);
    }

    0.0
}

/// Gradient-flow factor for a squash whose safe zone is stated on `|raw_input|`:
/// full inside `band.safe`, a fixed recovery factor when the error pulls back
/// toward zero, then a linear fade out to `band.fade_end`.
#[inline(always)]
fn symmetric_safe_zone(raw_input: f32, error: f32, band: SymmetricBand) -> f32 {
    let abs = raw_input.abs();

    if abs <= band.safe {
        return 1.0;
    }

    // Recovery: the error moves the raw input back toward the centre.
    if raw_input > band.safe && error < 0.0 {
        return band.recovery;
    }
    if raw_input < -band.safe && error > 0.0 {
        return band.recovery;
    }

    if abs <= band.fade_end {
        return 1.0 - (abs - band.safe) / (band.fade_end - band.safe);
    }

    0.0
}

/// IDENTITY: almost never saturates, but an extreme raw input paired with a
/// vanishing weight is better fixed by moving the weight.
#[inline(always)]
fn identity_safe_zone(raw_input: f32, weight: f32) -> f32 {
    let raw_is_extreme = raw_input.abs() > 1e6;
    let weight_too_small = weight.abs() < 1e-6;

    if raw_is_extreme && weight_too_small {
        return 0.0; // suggest adjusting the weight instead
    }

    1.0
}

/// ReLU: the dead-ReLU problem — only safe when positive, or when the error is
/// pushing the raw input back into the positive zone.
#[inline(always)]
fn relu_safe_zone(raw_input: f32, error: f32) -> f32 {
    if raw_input > 0.0 {
        return 1.0; // Fully active
    }

    // Recovery: try to push back into positive zone
    if raw_input <= 0.0 && error > 0.0 {
        return 1.0;
    }

    // Dead and shouldn't wake up
    0.0
}

/// ReLU6: both ends saturate, and either can be reactivated by a correcting
/// error.
#[inline(always)]
fn relu6_safe_zone(raw_input: f32, error: f32) -> f32 {
    if raw_input > 0.0 && raw_input < 6.0 {
        return 1.0;
    }

    if raw_input <= 0.0 && error > 0.0 {
        return 1.0; // Try to reactivate
    }

    if raw_input >= 6.0 && error < 0.0 {
        return 1.0; // Try to lower from saturated high
    }

    0.0
}

/// SINE: periodic, so safety is the local slope `cos(x)` rather than a band.
#[inline(always)]
fn sine_safe_zone(raw_input: f32, error: f32, weight: f32) -> f32 {
    let slope = raw_input.cos(); // derivative of sin(x)
    let in_flat_zone = slope.abs() < 0.1;
    let raw_getting_worse = slope * error < 0.0;

    if in_flat_zone && raw_getting_worse {
        return 0.0;
    }
    if !in_flat_zone && weight_is_recovering(weight, error) {
        return 0.0;
    }

    if !in_flat_zone {
        return 1.0;
    }

    // Soft fade for near-flat slope areas
    let fade = slope.abs() / 0.1;
    fade.clamp(0.0, 1.0)
}

/// COSINE: periodic, judged by `|sin(x)|` — the magnitude of its slope.
#[inline(always)]
fn cosine_safe_zone(raw_input: f32, error: f32, weight: f32) -> f32 {
    let slope = raw_input.sin().abs();

    // When slope is strong
    if slope > 0.1 {
        if weight_is_recovering(weight, error) {
            return 0.0; // allow weight to correct first
        }
        return 1.0;
    }

    // Fade zone
    if slope > 0.05 {
        return (slope - 0.05) / 0.05;
    }

    // Flat zone -- poor for learning
    0.0
}

/// TAN: avoid the asymptotes at `±pi/2` (mod `pi`).
#[inline(always)]
fn tan_safe_zone(raw_input: f32, error: f32, weight: f32) -> f32 {
    let pi = std::f32::consts::PI;
    let modulo = raw_input % pi;
    let dist_from_asymptote = (modulo.abs() - pi / 2.0).abs();

    let near_asymptote = dist_from_asymptote < 0.2;
    let raw_getting_worse =
        (modulo > pi / 2.0 && error > 0.0) || (modulo < -pi / 2.0 && error < 0.0);

    if near_asymptote && raw_getting_worse {
        return 0.0;
    }
    if !near_asymptote && weight_is_recovering(weight, error) {
        return 0.0;
    }

    // Soft fade if near pi/2 mod
    if dist_from_asymptote < 0.5 {
        return 1.0 - (0.5 - dist_from_asymptote) * 2.0;
    }

    1.0
}

/// ArcTan: a symmetric fade, with a hard cut-off beyond the fade window that
/// overrides the recovery factor.
#[inline(always)]
fn arctan_safe_zone(raw_input: f32, error: f32) -> f32 {
    let abs = raw_input.abs();

    // Ideal gradient zone: roughly x in [-2, 2]
    if abs <= 2.0 {
        return 1.0;
    }

    // Out of bounds: too flat for meaningful updates.
    if abs > 4.0 {
        return 0.0;
    }

    // Recovery zone: allow updates that move toward centre
    if raw_input > 2.0 && error < 0.0 {
        return 0.3;
    }
    if raw_input < -2.0 && error > 0.0 {
        return 0.3;
    }

    // Fade zone: x in [2, 4]
    1.0 - (abs - 2.0) / 2.0
}

/// STEP: a threshold at zero — safe only while the error still wants to cross
/// it.
#[inline(always)]
fn step_safe_zone(raw_input: f32, error: f32) -> f32 {
    let is_above = raw_input > 0.0;
    let expected_above = error > 0.0;

    // If we're on the wrong side and the error pushes us toward the correct side
    if is_above != expected_above {
        return 1.0;
    }

    // If we're on the correct side, but error is still non-zero, reduce confidence
    0.2
}

/// ABSOLUTE: loses sign information; an extreme raw input with a tiny weight is
/// better fixed by moving the weight.
#[inline(always)]
fn absolute_safe_zone(raw_input: f32, weight: f32) -> f32 {
    let very_large_input = raw_input.abs() > 1000.0;
    let tiny_weight = weight.abs() < 1e-3;

    if very_large_input && tiny_weight {
        return 0.0;
    } // raw input extreme, but weight could move

    1.0
}

/// SQRT: only defined for `x >= 0`, and safest in `[0.01, 10]`.
#[inline(always)]
fn sqrt_safe_zone(raw_input: f32, error: f32) -> f32 {
    // SQRT is undefined for x < 0; never propagate toward negatives
    if raw_input < 0.0 && error < 0.0 {
        return 0.0;
    }

    // Strong incentive to stay in a stable gradient zone: x in [0.01, 10]
    if (0.01..=10.0).contains(&raw_input) {
        return 1.0;
    }

    // If we're below safe zone and trying to go up (into domain), allow it
    if raw_input < 0.01 && error > 0.0 {
        return 0.3;
    }

    // Fade zone: x in [10, 20] -- flatter gradients, lower gain
    if raw_input > 10.0 && raw_input <= 20.0 {
        return 1.0 - (raw_input - 10.0) / 10.0;
    }

    // Above 20, gradients are too flat; prefer weight/bias adjustment
    0.0
}

/// Apply safe zone adjustment for a given activation function
/// Issue #1140 - WASM Migration Phase 8: Implement safeZoneAdjustment() in Rust/WASM
///
/// Returns a float from 0 (not safe) to 1 (fully safe) indicating how useful it is
/// to backpropagate through a neuron based on saturation levels.
///
/// - 1.0: Fully in safe zone, gradient flows freely
/// - 0.0: Completely saturated, no gradient should flow
/// - 0.0-1.0: Partial safety, used for gradual fade-out
///
/// The per-variant *data* — which band, which fade, which recovery factor — is
/// not restated here: it comes from the one declarative `squash_profile` list
/// (Issue #673), and this function only dispatches on the rule that list names.
/// Fourteen types share the bounded band, three the saturating one, three the
/// symmetric one, and eight are a flat constant; the remaining ten are genuinely
/// different algorithms and keep a named function each.
///
/// # Arguments
/// * `squash_type` - The type of activation function
/// * `raw_input` - The raw input value before squashing
/// * `error` - The error value from backpropagation
/// * `weight` - The synapse weight (used by some activation functions)
#[inline(always)]
pub fn apply_safe_zone_adjustment(
    squash_type: SquashType,
    raw_input: f32,
    error: f32,
    weight: f32,
) -> f32 {
    // Non-finite inputs are never safe
    if !raw_input.is_finite() {
        return 0.0;
    }

    match squash_profile(squash_type).safe_zone {
        SafeZoneRule::Band(band) => bounded_safe_zone(raw_input, error, weight, band),
        SafeZoneRule::Saturating(band) => saturating_safe_zone(raw_input, error, band),
        SafeZoneRule::Symmetric(band) => symmetric_safe_zone(raw_input, error, band),
        SafeZoneRule::Constant(factor) => factor,
        SafeZoneRule::Identity => identity_safe_zone(raw_input, weight),
        SafeZoneRule::Relu => relu_safe_zone(raw_input, error),
        SafeZoneRule::Relu6 => relu6_safe_zone(raw_input, error),
        SafeZoneRule::Sine => sine_safe_zone(raw_input, error, weight),
        SafeZoneRule::Cosine => cosine_safe_zone(raw_input, error, weight),
        SafeZoneRule::Tan => tan_safe_zone(raw_input, error, weight),
        SafeZoneRule::ArcTan => arctan_safe_zone(raw_input, error),
        SafeZoneRule::Step => step_safe_zone(raw_input, error),
        SafeZoneRule::Absolute => absolute_safe_zone(raw_input, weight),
        SafeZoneRule::Sqrt => sqrt_safe_zone(raw_input, error),
    }
}

/// Issue #1376 - Batch safe zone adjustment to eliminate WASM boundary crossings.
///
/// Processes multiple safe zone adjustments in a single WASM call, avoiding the
/// overhead of individual boundary crossings (~8.7ns each). For a neuron with S
/// inbound synapses, this replaces S WASM calls with 1.
///
/// # Arguments
/// * `squash_types` - Array of squash type enum values (u8 per synapse)
/// * `raw_inputs` - Array of pre-squash values for upstream neurons
/// * `error` - The provisional error per link (same for all synapses)
/// * `weights` - Array of synapse weights
///
/// # Returns
/// `Vec<f32>` of safe zone factors (0.0 to 1.0), one per synapse
///
/// # Malformed input (Issue #658)
/// `squash_types` sets the count and the other two slices are walked with the
/// same index, so a shorter `raw_inputs` or `weights` would index out of range —
/// and a panic on wasm aborts the whole module instance. Such a call returns an
/// empty `Vec` instead, the sentinel a successful call over a non-empty
/// `squash_types` can never produce.
pub fn apply_safe_zone_adjustment_batch(
    squash_types: &[u8],
    raw_inputs: &[f32],
    error: f32,
    weights: &[f32],
) -> Vec<f32> {
    let count = squash_types.len();

    if raw_inputs.len() < count || weights.len() < count {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(count);

    for i in 0..count {
        let squash = SquashType::from(squash_types[i]);
        let raw_input = raw_inputs[i];
        let weight = if weights[i].is_finite() {
            weights[i]
        } else {
            1.0
        };
        results.push(apply_safe_zone_adjustment(squash, raw_input, error, weight));
    }

    results
}
