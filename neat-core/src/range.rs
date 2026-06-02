//! Range validation for activation function outputs.
//!
//! This module provides functions to get, validate, and limit activation ranges.
//! Issue #1142 - WASM Migration Phase 10.

use crate::squash::{SELU_ALPHA, SELU_LAMBDA, SOFTSIGN_LIMIT, SquashType};

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
#[inline(always)]
pub fn apply_get_range(squash_type: SquashType) -> (f32, f32) {
    match squash_type {
        // Unbounded functions: use large f32 values
        SquashType::Identity => (-F32_LARGE, F32_LARGE),
        SquashType::LeakyRelu => (-F32_LARGE, F32_LARGE),
        SquashType::Tan => (-F32_LARGE, F32_LARGE),
        SquashType::BentIdentity => (-F32_LARGE, F32_LARGE),
        SquashType::Complement => (-F32_LARGE, F32_LARGE),
        SquashType::Cube => (-F32_LARGE, F32_LARGE),
        SquashType::StdInverse => (-F32_LARGE, F32_LARGE),

        // One-sided unbounded [0, inf)
        SquashType::Relu => (0.0, F32_LARGE),
        SquashType::Absolute => (0.0, F32_LARGE),
        SquashType::Square => (0.0, F32_LARGE),
        SquashType::Sqrt => (0.0, F32_LARGE),
        SquashType::Exponential => (0.0, F32_LARGE),

        // Bounded [0, 1]
        SquashType::Logistic => (0.0, 1.0),
        SquashType::Gaussian => (0.0, 1.0),
        SquashType::Step => (0.0, 1.0),

        // Bounded [-1, 1]
        SquashType::Tanh => (-1.0, 1.0),
        SquashType::HardTanh => (-1.0, 1.0),
        SquashType::Sine => (-1.0, 1.0),
        SquashType::Cosine => (-1.0, 1.0),
        SquashType::BipolarSigmoid => (-1.0, 1.0),
        SquashType::Bipolar => (-1.0, 1.0),
        SquashType::Isru => (-1.0, 1.0),

        // Specific bounded ranges
        SquashType::Relu6 => (0.0, 6.0),
        SquashType::Softsign => (-SOFTSIGN_LIMIT, SOFTSIGN_LIMIT),
        SquashType::Softplus => (SOFTPLUS_MIN, SOFTPLUS_MAX),
        SquashType::ArcTan => (-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),

        // Special bounded with negative lower
        SquashType::Elu => (-1.0, F32_LARGE), // ELU with alpha=1 has min of -1
        SquashType::Selu => (-SELU_ALPHA * SELU_LAMBDA, F32_LARGE), // SELU minimum
        SquashType::LogSigmoid => (-F32_LARGE, 0.0), // Output is always <= 0

        // Functions with empirically determined minimums
        SquashType::Swish => (SWISH_MIN, F32_LARGE),
        SquashType::Mish => (MISH_MIN, F32_LARGE),
        SquashType::Gelu => (GELU_MIN, F32_LARGE),

        // Aggregate functions - unbounded
        SquashType::Minimum => (-F32_LARGE, F32_LARGE),
        SquashType::Maximum => (-F32_LARGE, F32_LARGE),
        SquashType::If => (-F32_LARGE, F32_LARGE),
        SquashType::Hypotenuse => (-F32_LARGE, F32_LARGE),
        SquashType::HypotenuseV2 => (0.0, F32_LARGE), // HYPOTv2 output >= 0
        SquashType::Mean => (-F32_LARGE, F32_LARGE),
    }
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

/// Clamp a value to the valid range for an activation function
/// Issue #1142 - WASM Migration Phase 10
///
/// Returns the value clamped to the valid range.
/// Infinity values are clamped to the bounds.
/// NaN returns 0.0 as a safe default.
#[inline(always)]
pub fn apply_limit_range(squash_type: SquashType, value: f32) -> f32 {
    // Handle NaN - return 0 as a safe default
    if value.is_nan() {
        return 0.0;
    }

    let (low, high) = apply_get_range(squash_type);

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

/// Clamp an `f64` value to the valid output range of `squash_type`.
///
/// `NaN` maps to `0.0`. Infinities are clamped to the activation's finite
/// bounds, themselves capped at `±F32_LARGE` so the result never overflows
/// back to infinity. The `f32` counterpart is [`apply_limit_range`].
#[allow(dead_code)]
#[inline(always)]
pub fn apply_limit_range_f64(squash_type: SquashType, value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }

    let (low_f32, high_f32) = apply_get_range(squash_type);
    let low = low_f32 as f64;
    let high = high_f32 as f64;

    if value == f64::INFINITY {
        return high.min(F32_LARGE as f64);
    }
    if value == f64::NEG_INFINITY {
        return low.max(-(F32_LARGE as f64));
    }

    value.max(low).min(high)
}
