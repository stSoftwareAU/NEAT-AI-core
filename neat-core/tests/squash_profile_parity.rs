//! Differential parity for the Issue #673 per-variant data table.
//!
//! `apply_get_range` and `apply_safe_zone_adjustment` moved their per-variant
//! data out of two parallel `match`es and into the single `squash_profile`
//! list. The refactor adds no behaviour, so the oracle here is the **verbatim
//! pre-change implementation** of both functions (AGENTS.md "Oracles and
//! mutation evidence", rule 1): an independent route to the expected value that
//! does not share a line of code with the path under test.
//!
//! Every assertion is bit-exact — a data-only move must not shift a single
//! float.

#![allow(clippy::all)]

use neat_core::range::{F32_LARGE, GELU_MIN, MISH_MIN, SOFTPLUS_MAX, SOFTPLUS_MIN, SWISH_MIN};
use neat_core::squash::{SELU_ALPHA, SELU_LAMBDA, SOFTSIGN_LIMIT};
use neat_core::{
    SquashType, apply_get_range, apply_limit_range, apply_safe_zone_adjustment,
    apply_validate_range,
};

/// Every `SquashType`, by discriminant, so no variant escapes the sweep.
const ALL_SQUASH: [SquashType; 38] = {
    let mut all = [SquashType::Identity; 38];
    let mut i = 0;
    while i < 38 {
        all[i] = squash_from_index(i);
        i += 1;
    }
    all
};

const fn squash_from_index(i: usize) -> SquashType {
    match i {
        0 => SquashType::Identity,
        1 => SquashType::Relu,
        2 => SquashType::Relu6,
        3 => SquashType::LeakyRelu,
        4 => SquashType::Selu,
        5 => SquashType::Elu,
        6 => SquashType::Logistic,
        7 => SquashType::Tanh,
        8 => SquashType::HardTanh,
        9 => SquashType::Softsign,
        10 => SquashType::Softplus,
        11 => SquashType::Swish,
        12 => SquashType::Mish,
        13 => SquashType::Gelu,
        14 => SquashType::Sine,
        15 => SquashType::Cosine,
        16 => SquashType::Tan,
        17 => SquashType::ArcTan,
        18 => SquashType::Gaussian,
        19 => SquashType::BentIdentity,
        20 => SquashType::BipolarSigmoid,
        21 => SquashType::Bipolar,
        22 => SquashType::Step,
        23 => SquashType::Complement,
        24 => SquashType::Absolute,
        25 => SquashType::Square,
        26 => SquashType::Cube,
        27 => SquashType::Sqrt,
        28 => SquashType::StdInverse,
        29 => SquashType::Exponential,
        30 => SquashType::LogSigmoid,
        31 => SquashType::Isru,
        32 => SquashType::Minimum,
        33 => SquashType::Maximum,
        34 => SquashType::If,
        35 => SquashType::Hypotenuse,
        36 => SquashType::HypotenuseV2,
        37 => SquashType::Mean,
        _ => SquashType::Identity,
    }
}

// ---------------------------------------------------------------------------
// Verbatim pre-change implementations — the differential oracle.
// Copied from `neat-core/src/range.rs` and `neat-core/src/safe_zone.rs` at the
// commit before Issue #673. Do not "tidy" these: their value is that they are
// an untouched second route to the same numbers.
// ---------------------------------------------------------------------------

fn reference_get_range(squash_type: SquashType) -> (f32, f32) {
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

fn reference_bounded_safe_zone(
    raw_input: f32,
    error: f32,
    weight: f32,
    safe_min: f32,
    safe_max: f32,
    fade: f32,
) -> f32 {
    const MIN_WEIGHT: f32 = 1e-3;
    const MAX_WEIGHT: f32 = 1e3;

    let in_safe_range = raw_input >= safe_min && raw_input <= safe_max;
    let raw_getting_worse =
        (raw_input < safe_min && error < 0.0) || (raw_input > safe_max && error > 0.0);

    let abs_weight = weight.abs();
    let weight_too_small = abs_weight < MIN_WEIGHT;
    let weight_too_large = abs_weight > MAX_WEIGHT;
    let weight_improving =
        (weight_too_small && weight * error > 0.0) || (weight_too_large && weight * error < 0.0);

    if !in_safe_range && raw_getting_worse {
        return 0.0;
    }
    if in_safe_range && (weight_too_small || weight_too_large) && weight_improving {
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
/// # Arguments
/// * `squash_type` - The type of activation function
/// * `raw_input` - The raw input value before squashing
/// * `error` - The error value from backpropagation
/// * `weight` - The synapse weight (used by some activation functions)
fn reference_safe_zone_adjustment(
    squash_type: SquashType,
    raw_input: f32,
    error: f32,
    weight: f32,
) -> f32 {
    // Non-finite inputs are never safe
    if !raw_input.is_finite() {
        return 0.0;
    }

    match squash_type {
        // IDENTITY: Almost never saturates, but checks for extreme raw inputs with tiny weights
        SquashType::Identity => {
            let abs_raw = raw_input.abs();
            let abs_weight = weight.abs();

            let raw_is_extreme = abs_raw > 1e6;
            let weight_too_small = abs_weight < 1e-6;

            if raw_is_extreme && weight_too_small {
                return 0.0; // suggest adjusting the weight instead
            }

            1.0
        }

        // ReLU: Dead ReLU problem - only safe when positive or recovering
        SquashType::Relu => {
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

        // ReLU6: Both ends saturate
        SquashType::Relu6 => {
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

        // LeakyReLU: Never fully saturates, but has weight-based logic
        SquashType::LeakyRelu => {
            reference_bounded_safe_zone(raw_input, error, weight, -50.0, 50.0, 20.0)
        }

        // SELU: Similar to ELU but with specific safe zones
        SquashType::Selu => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // ELU: Similar pattern to SELU
        SquashType::Elu => reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0),

        // LOGISTIC (Sigmoid): Classic sigmoid saturation
        SquashType::Logistic => {
            let safe_low = -6.0;
            let safe_high = 6.0;
            let min = -10.0;
            let max = 10.0;

            // Fully safe zone
            if raw_input >= safe_low && raw_input <= safe_high {
                return 1.0;
            }

            // Recovery logic: if we're out of zone, but error would push us back in
            if raw_input < safe_low && error > 0.0 {
                return 0.2; // Pushes rawInput toward centre
            }
            if raw_input > safe_high && error < 0.0 {
                return 0.2;
            }

            // Fading out logic: scale linearly from edge of safe zone to extreme
            if raw_input > safe_high && raw_input <= max {
                return 1.0 - (raw_input - safe_high) / (max - safe_high); // fade from 1 to 0
            }
            if raw_input < safe_low && raw_input >= min {
                return (raw_input - min) / (safe_low - min); // fade from 0 to 1
            }

            // Beyond hard saturation
            0.0
        }

        // TANH: Similar to logistic
        SquashType::Tanh => {
            let safe_low = -2.0;
            let safe_high = 2.0;
            let min = -6.0;
            let max = 6.0;

            // Fully in safe zone
            if raw_input >= safe_low && raw_input <= safe_high {
                return 1.0;
            }

            // Recovery direction logic
            if raw_input < safe_low && error > 0.0 {
                return 0.2;
            }
            if raw_input > safe_high && error < 0.0 {
                return 0.2;
            }

            // Gradual fade to saturation
            if raw_input > safe_high && raw_input <= max {
                return 1.0 - (raw_input - safe_high) / (max - safe_high);
            }
            if raw_input < safe_low && raw_input >= min {
                return (raw_input - min) / (safe_low - min);
            }

            0.0
        }

        // HardTanh: Hard boundaries at -1 and 1
        SquashType::HardTanh => {
            let safe_low = -0.9;
            let safe_high = 0.9;
            let min = -1.2;
            let max = 1.2;

            // Fully safe region
            if raw_input >= safe_low && raw_input <= safe_high {
                return 1.0;
            }

            // Recovery: out of bounds but error would bring it back
            if raw_input <= -1.0 && error > 0.0 {
                return 0.2;
            }
            if raw_input >= 1.0 && error < 0.0 {
                return 0.2;
            }

            // Fade into the dead zone
            if raw_input > safe_high && raw_input <= max {
                return 1.0 - (raw_input - safe_high) / (max - safe_high);
            }
            if raw_input < safe_low && raw_input >= min {
                return (raw_input - min) / (safe_low - min);
            }

            0.0
        }

        // Softsign: Slow saturation
        SquashType::Softsign => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // Softplus: One-sided saturation
        SquashType::Softplus => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 20.0, 10.0)
        }

        // Swish: Similar to tanh in behaviour
        SquashType::Swish => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // Mish: Similar to Swish
        SquashType::Mish => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // GELU: Similar to ReLU but smoother
        SquashType::Gelu => reference_bounded_safe_zone(raw_input, error, weight, -6.0, 6.0, 10.0),

        // SINE: Periodic, always varying
        SquashType::Sine => {
            let abs_weight = weight.abs();
            let min_weight = 1e-3;
            let max_weight = 1e3;

            let slope = raw_input.cos(); // derivative of sin(x)
            let in_flat_zone = slope.abs() < 0.1;
            let raw_getting_worse = slope * error < 0.0;

            let weight_too_small = abs_weight < min_weight;
            let weight_too_large = abs_weight > max_weight;
            let weight_improving = (weight_too_small && weight * error > 0.0)
                || (weight_too_large && weight * error < 0.0);

            if in_flat_zone && raw_getting_worse {
                return 0.0;
            }
            if !in_flat_zone && (weight_too_small || weight_too_large) && weight_improving {
                return 0.0;
            }

            if !in_flat_zone {
                return 1.0;
            }

            // Soft fade for near-flat slope areas
            let fade = slope.abs() / 0.1;
            fade.clamp(0.0, 1.0)
        }

        // Cosine: Periodic
        SquashType::Cosine => {
            let slope = raw_input.sin().abs();
            let abs_weight = weight.abs();
            let min_weight = 1e-3;
            let max_weight = 1e3;

            // When slope is strong
            if slope > 0.1 {
                if (abs_weight < min_weight && weight * error > 0.0)
                    || (abs_weight > max_weight && weight * error < 0.0)
                {
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

        // TAN: Avoid asymptotes at +/-pi/2
        SquashType::Tan => {
            let abs_weight = weight.abs();
            let min_weight = 1e-3;
            let max_weight = 1e3;

            let pi = std::f32::consts::PI;
            let modulo = raw_input % pi;
            let dist_from_asymptote = (modulo.abs() - pi / 2.0).abs();

            let near_asymptote = dist_from_asymptote < 0.2;
            let raw_getting_worse =
                (modulo > pi / 2.0 && error > 0.0) || (modulo < -pi / 2.0 && error < 0.0);

            let weight_too_small = abs_weight < min_weight;
            let weight_too_large = abs_weight > max_weight;
            let weight_improving = (weight_too_small && weight * error > 0.0)
                || (weight_too_large && weight * error < 0.0);

            if near_asymptote && raw_getting_worse {
                return 0.0;
            }
            if !near_asymptote && (weight_too_small || weight_too_large) && weight_improving {
                return 0.0;
            }

            // Soft fade if near pi/2 mod
            if dist_from_asymptote < 0.5 {
                return 1.0 - (0.5 - dist_from_asymptote) * 2.0;
            }

            1.0
        }

        // ArcTan: Fade at extremes
        SquashType::ArcTan => {
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
            if abs <= 4.0 {
                return 1.0 - (abs - 2.0) / 2.0;
            }
            0.0
        }

        // GAUSSIAN: Bell curve
        SquashType::Gaussian => {
            let abs_raw = raw_input.abs();
            let abs_weight = weight.abs();
            let min_weight = 1e-3;
            let max_weight = 1e3;

            let in_safe_zone = abs_raw <= 3.0;

            let raw_getting_worse =
                (raw_input < -3.0 && error < 0.0) || (raw_input > 3.0 && error > 0.0);

            let weight_too_small = abs_weight < min_weight;
            let weight_too_large = abs_weight > max_weight;
            let weight_improving = (weight_too_small && weight * error > 0.0)
                || (weight_too_large && weight * error < 0.0);

            if !in_safe_zone && raw_getting_worse {
                return 0.0;
            }
            if in_safe_zone && (weight_too_small || weight_too_large) && weight_improving {
                return 0.0;
            }

            if in_safe_zone {
                return 1.0;
            }
            if abs_raw <= 6.0 {
                return 1.0 - (abs_raw - 3.0) / 3.0;
            }

            0.0
        }

        // BentIdentity: Never saturates
        SquashType::BentIdentity => {
            let abs = raw_input.abs();

            // Safe/strong zone: x in [-10, 10] is nearly linear
            if abs <= 10.0 {
                return 1.0;
            }

            // Allow recovery if error is pulling us back toward centre
            if raw_input > 10.0 && error < 0.0 {
                return 0.3;
            }
            if raw_input < -10.0 && error > 0.0 {
                return 0.3;
            }

            // Fade between 10 and 20
            if abs <= 20.0 {
                return 1.0 - (abs - 10.0) / 10.0;
            }

            0.0
        }

        // BipolarSigmoid: Similar to logistic
        SquashType::BipolarSigmoid => {
            let abs_raw = raw_input.abs();
            let abs_weight = weight.abs();
            let min_weight = 1e-3;
            let max_weight = 1e3;

            let raw_getting_worse =
                (raw_input < -4.0 && error < 0.0) || (raw_input > 4.0 && error > 0.0);

            let weight_too_small = abs_weight < min_weight;
            let weight_too_large = abs_weight > max_weight;
            let weight_improving = (weight_too_small && weight * error > 0.0)
                || (weight_too_large && weight * error < 0.0);

            if !(-4.0..=4.0).contains(&raw_input) && raw_getting_worse {
                return 0.0;
            }

            if (-4.0..=4.0).contains(&raw_input) {
                if (weight_too_small || weight_too_large) && weight_improving {
                    return 0.0;
                }
                return 1.0;
            }

            // Gradual fade out for raw inputs in [4, 8] or [-8, -4]
            if abs_raw <= 8.0 {
                return 1.0 - (abs_raw - 4.0) / 4.0;
            }

            0.0
        }

        // BIPOLAR: Discontinuous - always return 0
        SquashType::Bipolar => 0.0,

        // STEP: Special handling for threshold function
        SquashType::Step => {
            // STEP function: threshold at x = 0
            let is_above = raw_input > 0.0;
            let expected_above = error > 0.0;

            // If we're on the wrong side and the error pushes us toward the correct side
            if is_above != expected_above {
                return 1.0;
            }

            // If we're on the correct side, but error is still non-zero, reduce confidence
            0.2
        }

        // COMPLEMENT: Never saturates (linear function)
        SquashType::Complement => 1.0,

        // ABSOLUTE: Loses sign information
        SquashType::Absolute => {
            let abs_input = raw_input.abs();
            let abs_weight = weight.abs();

            let very_large_input = abs_input > 1000.0;
            let tiny_weight = abs_weight < 1e-3;

            if very_large_input && tiny_weight {
                return 0.0;
            } // raw input extreme, but weight could move

            1.0
        }

        // SQUARE: x^2 grows fast
        SquashType::Square => {
            let abs = raw_input.abs();

            // Safe zone: input in [-5, 5]
            if abs <= 5.0 {
                return 1.0;
            }

            // If error direction pushes input toward centre, allow it (recovery zone)
            if raw_input > 5.0 && error < 0.0 {
                return 0.2;
            }
            if raw_input < -5.0 && error > 0.0 {
                return 0.2;
            }

            // Fade between 5 and 10
            if abs <= 10.0 {
                return 1.0 - (abs - 5.0) / 5.0;
            }

            // Beyond 10, input dominates and gradients explode
            0.0
        }

        // Cube: x^3 grows extremely fast
        SquashType::Cube => {
            let abs = raw_input.abs();

            // Safe zone: x in [-5, 5]
            if abs <= 5.0 {
                return 1.0;
            }

            // Recovery: error moves us back in
            if raw_input < -5.0 && error > 0.0 {
                return 0.2;
            }
            if raw_input > 5.0 && error < 0.0 {
                return 0.2;
            }

            // Fade: x in [5, 10]
            if abs <= 10.0 {
                return 1.0 - (abs - 5.0) / 5.0;
            }

            0.0
        }

        // SQRT: Only defined for x >= 0
        SquashType::Sqrt => {
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

        // StdInverse: Sensitive around zero
        SquashType::StdInverse => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // Exponential: Grows rapidly
        SquashType::Exponential => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 30.0, 10.0)
        }

        // LogSigmoid: Flattens sharply for large negative inputs
        SquashType::LogSigmoid => {
            reference_bounded_safe_zone(raw_input, error, weight, -20.0, 20.0, 10.0)
        }

        // ISRU: Saturates at large |x|
        SquashType::Isru => {
            reference_bounded_safe_zone(raw_input, error, weight, -10.0, 10.0, 10.0)
        }

        // Aggregate functions - not differentiable, always return 0
        SquashType::Minimum | SquashType::Maximum | SquashType::If => 0.0,
        SquashType::Hypotenuse | SquashType::HypotenuseV2 | SquashType::Mean => 0.0,
    }
}

// ---------------------------------------------------------------------------
// The sweeps.
// ---------------------------------------------------------------------------

/// Raw inputs chosen to straddle every documented edge in the safe-zone rules:
/// the band edges (0.9, 1, 2, 3, 4, 5, 6, 10, 20, 30, 50), the fade ends, the
/// asymptote spacing for `Tan`, and both signs of each.
fn raw_input_grid() -> Vec<f32> {
    let mut grid = Vec::new();
    for magnitude in [
        0.0f32, 0.005, 0.01, 0.05, 0.1, 0.5, 0.85, 0.9, 0.95, 1.0, 1.1, 1.2, 1.3, 1.5, 1.9, 2.0,
        2.1, 2.5, 2.9, 3.0, 3.1, 3.9, 4.0, 4.1, 4.9, 5.0, 5.1, 5.7, 6.0, 6.1, 7.0, 7.9, 8.0, 8.1,
        9.9, 10.0, 10.1, 12.0, 15.0, 19.9, 20.0, 20.1, 25.0, 29.9, 30.0, 30.1, 45.0, 49.9, 50.0,
        50.1, 61.0, 70.0, 1e3, 1e6, 1e7,
    ] {
        grid.push(magnitude);
        if magnitude != 0.0 {
            grid.push(-magnitude);
        }
    }
    grid
}

#[test]
fn get_range_matches_the_pre_change_implementation_for_every_squash_type() {
    for squash in ALL_SQUASH {
        let expected = reference_get_range(squash);
        let actual = apply_get_range(squash);
        assert_eq!(
            actual.0.to_bits(),
            expected.0.to_bits(),
            "{squash:?}: low bound drifted ({} vs {})",
            actual.0,
            expected.0
        );
        assert_eq!(
            actual.1.to_bits(),
            expected.1.to_bits(),
            "{squash:?}: high bound drifted ({} vs {})",
            actual.1,
            expected.1
        );
    }
}

#[test]
fn limit_and_validate_range_still_ride_on_the_table_bounds() {
    // `apply_limit_range` / `apply_validate_range` derive from the range, so the
    // table move must leave their observable results untouched too.
    for squash in ALL_SQUASH {
        let (low, high) = reference_get_range(squash);
        for value in [
            f32::NEG_INFINITY,
            -1e9,
            low,
            (low + high) / 2.0,
            0.0,
            0.5,
            high,
            1e9,
            f32::INFINITY,
            f32::NAN,
        ] {
            let limited = apply_limit_range(squash, value);
            assert!(
                limited.is_finite(),
                "{squash:?}: clamped {value} to non-finite {limited}"
            );
            if value.is_nan() {
                // Documented behaviour: NaN is the one input that ignores the
                // bounds and returns the 0.0 safe default.
                assert_eq!(
                    limited, 0.0,
                    "{squash:?}: NaN must clamp to the 0.0 default"
                );
            } else {
                assert!(
                    limited >= low.max(-F32_LARGE) && limited <= high.min(F32_LARGE),
                    "{squash:?}: clamped {value} to {limited}, outside [{low}, {high}]"
                );
            }
            let valid = apply_validate_range(squash, value);
            let expected_valid = value.is_finite() && value >= low && value <= high;
            assert_eq!(
                valid, expected_valid,
                "{squash:?}: validate({value}) disagreed with the table bounds"
            );
        }
    }
}

#[test]
fn safe_zone_matches_the_pre_change_implementation_for_every_squash_type() {
    let weights = [
        0.0f32,
        1e-6,
        5e-4,
        1e-3,
        2e-3,
        0.5,
        1.0,
        900.0,
        1e3,
        2e3,
        1e6,
        -1e-4,
        -1.0,
        -2e3,
        f32::NAN,
    ];
    let errors = [-5.0f32, -1.0, -1e-9, 0.0, 1e-9, 1.0, 5.0];

    for squash in ALL_SQUASH {
        for raw in raw_input_grid() {
            for &error in &errors {
                for &weight in &weights {
                    let expected = reference_safe_zone_adjustment(squash, raw, error, weight);
                    let actual = apply_safe_zone_adjustment(squash, raw, error, weight);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{squash:?}: raw={raw} error={error} weight={weight} \
                         gave {actual}, pre-change gave {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn safe_zone_rejects_non_finite_raw_inputs_for_every_squash_type() {
    for squash in ALL_SQUASH {
        for raw in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let expected = reference_safe_zone_adjustment(squash, raw, 1.0, 1.0);
            let actual = apply_safe_zone_adjustment(squash, raw, 1.0, 1.0);
            assert_eq!(
                actual, 0.0,
                "{squash:?}: non-finite raw input must be unsafe"
            );
            assert_eq!(actual.to_bits(), expected.to_bits(), "{squash:?}");
        }
    }
}
