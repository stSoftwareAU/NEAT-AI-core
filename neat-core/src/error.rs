//! Error calculation for backpropagation through activation functions.
//!
//! This module calculates the error in value-space given activation-space values.
//! Issue #1141 - WASM Migration Phase 9.
//! Issue #1213 - Added SIMD batch error computation for backpropagation.

use crate::derivative::apply_derivative;
use crate::squash::SquashType;
use crate::unsquash::apply_unsquash;

// Issue #1213 - WASM SIMD support for batch error computation
#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    f32x4, f32x4_abs, f32x4_extract_lane, f32x4_gt, f32x4_lt, f32x4_max, f32x4_min, f32x4_neg,
    f32x4_splat, f32x4_sub, v128_bitselect,
};

/// Error epsilon - smallest meaningful difference between target and actual activation
/// Used to short-circuit calculateError() for near-zero error cases.
pub const ERROR_EPSILON: f32 = 1e-6;

/// Maximum error magnitude for clamping
/// Prevents exploding gradients during backpropagation.
pub const MAX_ERROR_MAGNITUDE: f32 = 100.0;

/// Clamps error to a maximum absolute magnitude.
/// Avoids NaN propagation and prevents weight explosion.
#[inline(always)]
pub fn clamp_error(error: f32) -> f32 {
    if !error.is_finite() {
        return 0.0;
    }
    error.clamp(-MAX_ERROR_MAGNITUDE, MAX_ERROR_MAGNITUDE)
}

/// Apply calculateError function for a given activation function
/// Issue #1141 - WASM Migration Phase 9: Implement calculateError() in Rust/WASM
///
/// Calculates the error in value-space given:
/// - `current_activation`: The neuron's current output (after squash)
/// - `target_activation`: The desired output
/// - `current_value`: The pre-squash value (hint for unSquash)
///
/// The basic algorithm:
/// 1. Compute raw error: rawError = targetActivation - currentActivation
/// 2. If raw error is tiny (< ERROR_EPSILON), return 0
/// 3. If derivative (slope) is strong: error = rawError / slope
/// 4. Otherwise fall back to: error = unSquash(targetActivation) - currentValue
/// 5. Clamp error to prevent weight explosion
///
/// # Arguments
/// * `squash_type` - The type of activation function
/// * `current_activation` - The squashed activation value
/// * `target_activation` - The desired activation value
/// * `current_value` - The pre-squash value (used as hint for unSquash)
#[inline(always)]
pub fn apply_calculate_error(
    squash_type: SquashType,
    current_activation: f32,
    target_activation: f32,
    current_value: f32,
) -> f32 {
    let raw_error = target_activation - current_activation;

    // Short-circuit for tiny errors
    if raw_error.abs() < ERROR_EPSILON {
        return 0.0;
    }

    match squash_type {
        // IDENTITY: Always use raw error directly (slope = 1)
        SquashType::Identity => clamp_error(raw_error),

        // COMPLEMENT: Always use derivative (slope = -1)
        SquashType::Complement => clamp_error(raw_error / -1.0),

        // ReLU: Use raw error when active, unSquash fallback otherwise
        SquashType::Relu => {
            let error = if current_value > 0.0 {
                raw_error
            } else {
                apply_unsquash(SquashType::Relu, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // ReLU6: Use raw error when active, unSquash fallback otherwise
        SquashType::Relu6 => {
            let error = if current_value > 0.0 && current_value < 6.0 {
                raw_error
            } else {
                apply_unsquash(SquashType::Relu6, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // LeakyReLU: Always use unSquash (per JS implementation)
        SquashType::LeakyRelu => {
            let target_value =
                apply_unsquash(SquashType::LeakyRelu, target_activation, current_value);
            let error = target_value - current_value;
            clamp_error(error)
        }

        // SELU: Use derivative with fallback
        SquashType::Selu => {
            let slope = apply_derivative(SquashType::Selu, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Selu, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // ELU: Use derivative with fallback
        SquashType::Elu => {
            let slope = apply_derivative(SquashType::Elu, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Elu, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // LOGISTIC: Use activation-based slope (f(x)*(1-f(x))) with fallback
        SquashType::Logistic => {
            let slope = current_activation * (1.0 - current_activation);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Logistic, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // TANH: Use derivative with fallback
        SquashType::Tanh => {
            let slope = apply_derivative(SquashType::Tanh, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Tanh, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // HardTanh: Use derivative with fallback
        SquashType::HardTanh => {
            let slope = apply_derivative(SquashType::HardTanh, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::HardTanh, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // Softsign: Use derivative with fallback
        SquashType::Softsign => {
            let slope = apply_derivative(SquashType::Softsign, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Softsign, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // Softplus: Use derivative with fallback
        SquashType::Softplus => {
            let slope = apply_derivative(SquashType::Softplus, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Softplus, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // Swish: Use derivative with fallback
        SquashType::Swish => {
            let slope = apply_derivative(SquashType::Swish, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Swish, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Mish: Always use derivative (no fallback per JS implementation)
        SquashType::Mish => {
            let slope = apply_derivative(SquashType::Mish, current_value);
            // JS code uses rawError / slope directly without abs() check
            let error = raw_error / slope;
            clamp_error(error)
        }

        // GELU: Use derivative with fallback
        SquashType::Gelu => {
            let slope = apply_derivative(SquashType::Gelu, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Gelu, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // SINE: Use derivative with fallback
        SquashType::Sine => {
            let slope = apply_derivative(SquashType::Sine, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Sine, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Cosine: Use derivative with fallback
        SquashType::Cosine => {
            let slope = apply_derivative(SquashType::Cosine, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Cosine, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // TAN: Use derivative with fallback, cap slope to prevent explosion
        SquashType::Tan => {
            let slope = apply_derivative(SquashType::Tan, current_value);
            let error = if slope.abs() > 1e-8 && slope < 1000.0 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Tan, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // ArcTan: Use derivative with fallback
        SquashType::ArcTan => {
            let slope = apply_derivative(SquashType::ArcTan, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::ArcTan, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Gaussian: Use derivative with fallback
        SquashType::Gaussian => {
            let slope = apply_derivative(SquashType::Gaussian, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Gaussian, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // BentIdentity: Use derivative with fallback
        SquashType::BentIdentity => {
            let slope = apply_derivative(SquashType::BentIdentity, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::BentIdentity, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // BipolarSigmoid: Use derivative with fallback
        SquashType::BipolarSigmoid => {
            let slope = apply_derivative(SquashType::BipolarSigmoid, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::BipolarSigmoid, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // Bipolar: Non-differentiable, use unSquash fallback
        SquashType::Bipolar => {
            let target_value =
                apply_unsquash(SquashType::Bipolar, target_activation, current_value);
            let error = target_value - current_value;
            clamp_error(error)
        }

        // Step: Non-differentiable, use unSquash fallback
        SquashType::Step => {
            let target_value = apply_unsquash(SquashType::Step, target_activation, current_value);
            let error = target_value - current_value;
            clamp_error(error)
        }

        // ABSOLUTE: Use closest target approach (both -target and +target are valid)
        SquashType::Absolute => {
            let neg_target = -target_activation;
            let pos_target = target_activation;

            // Choose the target value closest to currentValue
            let closest_target =
                if (current_value - neg_target).abs() < (current_value - pos_target).abs() {
                    neg_target
                } else {
                    pos_target
                };

            clamp_error(closest_target - current_value)
        }

        // SQUARE: Use derivative with safe slope clamping
        SquashType::Square => {
            let slope = apply_derivative(SquashType::Square, current_value);
            let error = if slope.abs() > 1e-8 {
                let safe_slope = slope.clamp(-50.0, 50.0);
                raw_error / safe_slope
            } else {
                apply_unsquash(SquashType::Square, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Cube: Use derivative with fallback
        SquashType::Cube => {
            let slope = apply_derivative(SquashType::Cube, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Cube, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Sqrt: Use derivative with safe slope clamping
        SquashType::Sqrt => {
            let slope = apply_derivative(SquashType::Sqrt, current_value);
            let error = if slope.abs() > 1e-8 {
                let safe_slope = slope.clamp(-50.0, 50.0);
                raw_error / safe_slope
            } else {
                apply_unsquash(SquashType::Sqrt, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // StdInverse: Use derivative with fallback
        SquashType::StdInverse => {
            let slope = apply_derivative(SquashType::StdInverse, current_value);
            let error = if slope.abs() > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::StdInverse, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // Exponential: Use derivative with range checks
        SquashType::Exponential => {
            let slope = apply_derivative(SquashType::Exponential, current_value);
            const MIN_SLOPE: f32 = 1e-8;
            const MAX_SLOPE: f32 = 1e8;

            let error = if slope > MIN_SLOPE && slope < MAX_SLOPE {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Exponential, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // LogSigmoid: Use derivative with fallback
        SquashType::LogSigmoid => {
            let slope = apply_derivative(SquashType::LogSigmoid, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::LogSigmoid, target_activation, current_value)
                    - current_value
            };
            clamp_error(error)
        }

        // ISRU: Use derivative with fallback
        SquashType::Isru => {
            let slope = apply_derivative(SquashType::Isru, current_value);
            let error = if slope > 1e-8 {
                raw_error / slope
            } else {
                apply_unsquash(SquashType::Isru, target_activation, current_value) - current_value
            };
            clamp_error(error)
        }

        // Aggregate functions - not differentiable, return 0
        SquashType::Minimum | SquashType::Maximum | SquashType::If => 0.0,
        SquashType::Hypotenuse | SquashType::HypotenuseV2 | SquashType::Mean => 0.0,
    }
}

/// Issue #1213 - SIMD-optimised error computation for 4 records simultaneously.
///
/// Computes backpropagation errors for 4 activation values in parallel using SIMD operations.
/// This is particularly useful during backpropagation where the same activation function
/// error needs to be computed for multiple neurons/records.
///
/// # Arguments
/// * `squash_type` - The type of activation function
/// * `current_activations` - Array of 4 squashed activation values
/// * `target_activations` - Array of 4 desired activation values
/// * `current_values` - Array of 4 pre-squash values (used as hints for unSquash)
///
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
#[inline]
pub fn apply_calculate_error_batch_4way(
    squash_type: SquashType,
    current_activations: &[f32; 4],
    target_activations: &[f32; 4],
    current_values: &[f32; 4],
) -> (f32, f32, f32, f32) {
    let curr_acts = f32x4(
        current_activations[0],
        current_activations[1],
        current_activations[2],
        current_activations[3],
    );
    let tgt_acts = f32x4(
        target_activations[0],
        target_activations[1],
        target_activations[2],
        target_activations[3],
    );
    let curr_vals = f32x4(
        current_values[0],
        current_values[1],
        current_values[2],
        current_values[3],
    );

    let zeros = f32x4_splat(0.0);
    let epsilon = f32x4_splat(ERROR_EPSILON);
    let max_err = f32x4_splat(MAX_ERROR_MAGNITUDE);
    let neg_max_err = f32x4_neg(max_err);

    // Compute raw error: rawError = targetActivation - currentActivation
    let raw_error = f32x4_sub(tgt_acts, curr_acts);

    // Short-circuit mask: |rawError| < epsilon → result is 0
    let abs_raw_error = f32x4_abs(raw_error);
    let tiny_error_mask = f32x4_lt(abs_raw_error, epsilon);

    let result = match squash_type {
        // IDENTITY: Always use raw error directly (slope = 1)
        SquashType::Identity => {
            // error = rawError, clamped
            let clamped = f32x4_min(f32x4_max(raw_error, neg_max_err), max_err);
            v128_bitselect(zeros, clamped, tiny_error_mask)
        }

        // COMPLEMENT: Always use derivative (slope = -1)
        SquashType::Complement => {
            // error = rawError / -1 = -rawError
            let error = f32x4_neg(raw_error);
            let clamped = f32x4_min(f32x4_max(error, neg_max_err), max_err);
            v128_bitselect(zeros, clamped, tiny_error_mask)
        }

        // ReLU: Use raw error when active, fallback otherwise
        SquashType::Relu => {
            // For active neurons (current_value > 0): use raw_error
            // For inactive neurons: compute scalar fallback
            let active_mask = f32x4_gt(curr_vals, zeros);

            let (re0, re1, re2, re3) = (
                f32x4_extract_lane::<0>(raw_error),
                f32x4_extract_lane::<1>(raw_error),
                f32x4_extract_lane::<2>(raw_error),
                f32x4_extract_lane::<3>(raw_error),
            );

            // Compute scalar fallbacks for inactive neurons
            let e0 = if current_values[0] > 0.0 {
                re0
            } else {
                apply_calculate_error(
                    SquashType::Relu,
                    current_activations[0],
                    target_activations[0],
                    current_values[0],
                )
            };
            let e1 = if current_values[1] > 0.0 {
                re1
            } else {
                apply_calculate_error(
                    SquashType::Relu,
                    current_activations[1],
                    target_activations[1],
                    current_values[1],
                )
            };
            let e2 = if current_values[2] > 0.0 {
                re2
            } else {
                apply_calculate_error(
                    SquashType::Relu,
                    current_activations[2],
                    target_activations[2],
                    current_values[2],
                )
            };
            let e3 = if current_values[3] > 0.0 {
                re3
            } else {
                apply_calculate_error(
                    SquashType::Relu,
                    current_activations[3],
                    target_activations[3],
                    current_values[3],
                )
            };

            // Use SIMD for active neurons, scalar results for inactive
            let scalar_result = f32x4(e0, e1, e2, e3);
            let simd_result = f32x4_min(f32x4_max(raw_error, neg_max_err), max_err);
            let mixed = v128_bitselect(simd_result, scalar_result, active_mask);
            v128_bitselect(zeros, mixed, tiny_error_mask)
        }

        // For other squash types, use scalar computation for each lane
        // This still provides memory access benefits from batching
        _ => {
            let e0 = apply_calculate_error(
                squash_type,
                current_activations[0],
                target_activations[0],
                current_values[0],
            );
            let e1 = apply_calculate_error(
                squash_type,
                current_activations[1],
                target_activations[1],
                current_values[1],
            );
            let e2 = apply_calculate_error(
                squash_type,
                current_activations[2],
                target_activations[2],
                current_values[2],
            );
            let e3 = apply_calculate_error(
                squash_type,
                current_activations[3],
                target_activations[3],
                current_values[3],
            );
            f32x4(e0, e1, e2, e3)
        }
    };

    (
        f32x4_extract_lane::<0>(result),
        f32x4_extract_lane::<1>(result),
        f32x4_extract_lane::<2>(result),
        f32x4_extract_lane::<3>(result),
    )
}

/// Scalar fallback for non-WASM targets (for testing).
/// Issue #1213 - SIMD batch error computation.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn apply_calculate_error_batch_4way(
    squash_type: SquashType,
    current_activations: &[f32; 4],
    target_activations: &[f32; 4],
    current_values: &[f32; 4],
) -> (f32, f32, f32, f32) {
    (
        apply_calculate_error(
            squash_type,
            current_activations[0],
            target_activations[0],
            current_values[0],
        ),
        apply_calculate_error(
            squash_type,
            current_activations[1],
            target_activations[1],
            current_values[1],
        ),
        apply_calculate_error(
            squash_type,
            current_activations[2],
            target_activations[2],
            current_values[2],
        ),
        apply_calculate_error(
            squash_type,
            current_activations[3],
            target_activations[3],
            current_values[3],
        ),
    )
}
