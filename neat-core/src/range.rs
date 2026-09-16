//! Range validation for activation function outputs.
//!
//! This module provides functions to get, validate, and limit activation ranges.
//! Issue #1142 - WASM Migration Phase 10.

use crate::squash::SquashType;
use crate::squash_profile::squash_profile;

// Special range constants based on TypeScript implementations.

/// GELU minimum output (around `x ≈ -0.509`, value `≈ -0.17`).
pub const GELU_MIN: f32 = -0.17;

/// Swish minimum output (around `x ≈ -1.278`, value `≈ -0.278`).
pub const SWISH_MIN: f32 = -0.278;

/// Mish minimum output (around `x ≈ -1.19`, value `≈ -0.309`).
pub const MISH_MIN: f32 = -0.309;

/// Softplus practical lower bound (small positive).
pub const SOFTPLUS_MIN: f32 = 1e-15;

/// Softplus practical upper bound (prevents overflow).
pub const SOFTPLUS_MAX: f32 = 100.0;

/// Practical "unbounded" magnitude (`f32::MAX`) for WASM/f32 ranges.
pub const F32_LARGE: f32 = 3.4028235e38;

/// Get the range (low, high) for an activation function
/// Issue #1142 - WASM Migration Phase 10
///
/// Returns a tuple (low, high) representing the valid output range.
///
/// The bounds themselves are per-variant *data*, not an algorithm, so they live
/// in the one declarative `squash_profile` list beside the safe-zone rule
/// rather than in a `match` of their own (Issue #673).
#[inline(always)]
pub fn apply_get_range(squash_type: SquashType) -> (f32, f32) {
    squash_profile(squash_type).range
}

/// Validate that an activation value is within the valid range
/// Issue #1142 - WASM Migration Phase 10
///
/// Returns true if the activation is within the valid range, false otherwise.
/// Also returns false for NaN and Infinity values.
#[inline(always)]
pub fn apply_validate_range(squash_type: SquashType, activation: f32) -> bool {
    // NaN and Infinity are never valid
    if !activation.is_finite() {
        return false;
    }

    let (low, high) = apply_get_range(squash_type);
    activation >= low && activation <= high
}

/// Clamp a value to an already-resolved `(low, high)` activation range.
///
/// This is the range-independent core of [`apply_limit_range`]: it takes the
/// bounds directly instead of re-deriving them from a [`SquashType`] via
/// [`apply_get_range`]. The batched scoring paths resolve the range **once per
/// neuron** and then clamp every lane through this helper (Issue #245), so the
/// per-record inner loop no longer re-runs the per-variant range lookup for each
/// of the 8 (then 4) records. `NaN` maps to `0.0` and infinities clamp to the
/// finite bounds, byte-for-byte identical to [`apply_limit_range`].
#[inline(always)]
pub fn apply_limit_range_bounds(low: f32, high: f32, value: f32) -> f32 {
    // Handle NaN - return 0 as a safe default
    if value.is_nan() {
        return 0.0;
    }

    // Handle infinities by clamping to bounds
    if value == f32::INFINITY {
        return high.min(F32_LARGE);
    }
    if value == f32::NEG_INFINITY {
        return low.max(-F32_LARGE);
    }

    // Clamp to range
    value.max(low).min(high)
}

/// Clamp a value to the valid range for an activation function
/// Issue #1142 - WASM Migration Phase 10
///
/// Returns the value clamped to the valid range.
/// Infinity values are clamped to the bounds.
/// NaN returns 0.0 as a safe default.
#[inline(always)]
pub fn apply_limit_range(squash_type: SquashType, value: f32) -> f32 {
    let (low, high) = apply_get_range(squash_type);
    apply_limit_range_bounds(low, high, value)
}
