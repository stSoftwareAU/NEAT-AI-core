//! Squash (activation) functions for neural network neurons.
//!
//! This module provides the squash function types and their implementations,
//! matching the TypeScript activation functions in the NEAT-AI project.

/// SELU scale parameter `alpha` (Klambauer et al., 2017).
pub const SELU_ALPHA: f32 = 1.673_263_2;
/// SELU scale parameter `lambda` (Klambauer et al., 2017).
pub const SELU_LAMBDA: f32 = 1.050_701;

/// GELU tanh-approximation cubic coefficient.
pub const GELU_COEFF: f32 = 0.044715;
/// GELU tanh-approximation scale factor `sqrt(2/pi)`.
pub const SQRT_2_OVER_PI: f32 = 0.797_884_6;

/// LeakyReLU negative-slope coefficient `alpha`.
pub const LEAKY_RELU_ALPHA: f32 = 0.01;

/// Practical upper bound for very large one-sided outputs (e.g. Exponential).
///
/// Matches the JS implementation, which uses `Number.MAX_SAFE_INTEGER`
/// (~9.007e15) as the upper bound for several unbounded activations.
pub const JS_MAX_SAFE_INTEGER: f32 = 9_007_199_254_740_992.0;

/// Softsign output limit — the function approaches but never reaches `±1`.
pub const SOFTSIGN_LIMIT: f32 = 0.99;

/// Output clamp for the unbounded Tan activation near its asymptotes (#2151).
pub const TAN_OUTPUT_CLAMP: f64 = 1000.0;
/// Output clamp for the unbounded Square activation (#2151).
pub const SQUARE_OUTPUT_CLAMP: f64 = 1e6;
/// Output clamp for the unbounded Cube activation (#2151).
pub const CUBE_OUTPUT_CLAMP: f64 = 1e6;

/// Squash function identifiers - must match TypeScript enum
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SquashType {
    /// Identity — passes the input through unchanged.
    Identity = 0,
    /// Rectified Linear Unit — `max(0, x)`.
    Relu = 1,
    /// ReLU clamped to a maximum of 6 — `min(max(0, x), 6)`.
    Relu6 = 2,
    /// Leaky ReLU — small negative slope for `x < 0`.
    LeakyRelu = 3,
    /// Scaled Exponential Linear Unit (self-normalising).
    Selu = 4,
    /// Exponential Linear Unit.
    Elu = 5,
    /// Logistic sigmoid — output in `(0, 1)`.
    Logistic = 6,
    /// Hyperbolic tangent — output in `(-1, 1)`.
    Tanh = 7,
    /// Hard tanh — tanh clamped linearly to `[-1, 1]`.
    HardTanh = 8,
    /// Softsign — `x / (1 + |x|)`.
    Softsign = 9,
    /// Softplus — smooth approximation of ReLU, `ln(1 + e^x)`.
    Softplus = 10,
    /// Swish — `x * sigmoid(x)`.
    Swish = 11,
    /// Mish — `x * tanh(softplus(x))`.
    Mish = 12,
    /// Gaussian Error Linear Unit (tanh approximation).
    Gelu = 13,
    /// Sine of the input.
    Sine = 14,
    /// Cosine of the input.
    Cosine = 15,
    /// Tangent of the input (clamped near its asymptotes).
    Tan = 16,
    /// Inverse tangent (arctangent) of the input.
    ArcTan = 17,
    /// Gaussian bell curve — `e^(-x^2)`.
    Gaussian = 18,
    /// Bent identity — `(sqrt(x^2 + 1) - 1) / 2 + x`.
    BentIdentity = 19,
    /// Bipolar sigmoid — logistic scaled to `(-1, 1)`.
    BipolarSigmoid = 20,
    /// Bipolar sign — `-1` for `x <= 0`, `+1` otherwise.
    Bipolar = 21,
    /// Step — `0` for `x < 0`, `1` otherwise.
    Step = 22,
    /// Complement — `1 - x`.
    Complement = 23,
    /// Absolute value — `|x|`.
    Absolute = 24,
    /// Square — `x^2` (clamped for stability).
    Square = 25,
    /// Cube — `x^3` (clamped for stability).
    Cube = 26,
    /// Square root of the input.
    Sqrt = 27,
    /// Standard inverse — `1 / x` guarded against division by zero.
    StdInverse = 28,
    /// Exponential — `e^x` (clamped for stability).
    Exponential = 29,
    /// Log-sigmoid — `ln(sigmoid(x))`.
    LogSigmoid = 30,
    /// Inverse Square Root Unit.
    Isru = 31,
    // Aggregate functions (Issue #1125)
    /// Aggregate minimum of the weighted inputs.
    Minimum = 32,
    /// Aggregate maximum of the weighted inputs.
    Maximum = 33,
    /// Conditional aggregate — gate-style selection over inputs.
    If = 34,
    // Deprecated aggregate functions (implemented for WASM parity, remove when possible)
    /// Deprecated: hypotenuse of the weighted inputs, then add bias.
    Hypotenuse = 35, // HYPOT: hypot(weighted_inputs) + bias
    /// Deprecated: hypotenuse of `bias + weighted_inputs`.
    HypotenuseV2 = 36, // HYPOTv2: hypot(bias + weighted_inputs)
    /// Deprecated: mean of the weighted inputs, then add bias.
    Mean = 37, // MEAN: (sum of weighted_inputs) / n + bias
}

/// The aggregate squash variants as an or-pattern — the single home of the
/// membership list behind [`SquashType::is_aggregate`] (Issue #446).
///
/// Prefer `is_aggregate()`; reach for this macro only where an exhaustive
/// `match` over `SquashType` needs the set as a *pattern* (a guard arm would
/// forfeit the compiler's exhaustiveness check).
macro_rules! aggregate_squash_patterns {
    () => {
        $crate::squash::SquashType::Minimum
            | $crate::squash::SquashType::Maximum
            | $crate::squash::SquashType::If
            | $crate::squash::SquashType::Hypotenuse
            | $crate::squash::SquashType::HypotenuseV2
            | $crate::squash::SquashType::Mean
    };
}
pub(crate) use aggregate_squash_patterns;

impl SquashType {
    /// True for the six **aggregate** squashes — `Minimum`, `Maximum`, `If`,
    /// `Hypotenuse`, `HypotenuseV2` and `Mean` (Issue #446).
    ///
    /// These reduce a neuron's whole synapse range at once rather than
    /// squashing a weighted sum, so they cannot be lane-vectorised: every
    /// batched kernel routes them to the exact single-record kernel, and
    /// tracing reports the activation itself as the hint. This predicate is
    /// the single home of that membership rule — a site that restated the list
    /// and missed a type would silently route it down the weighted-sum path.
    ///
    /// # Examples
    ///
    /// ```
    /// use neat_core::squash::SquashType;
    /// assert!(SquashType::Mean.is_aggregate());
    /// assert!(!SquashType::Relu.is_aggregate());
    /// ```
    #[must_use]
    pub const fn is_aggregate(self) -> bool {
        matches!(self, aggregate_squash_patterns!())
    }
}

impl From<u8> for SquashType {
    fn from(v: u8) -> Self {
        match v {
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
            // Aggregate functions (Issue #1125)
            32 => SquashType::Minimum,
            33 => SquashType::Maximum,
            34 => SquashType::If,
            35 => SquashType::Hypotenuse,
            36 => SquashType::HypotenuseV2,
            37 => SquashType::Mean,
            _ => SquashType::Identity,
        }
    }
}

/// Apply a squash (activation) function to a value.
///
/// # Examples
///
/// ```
/// use neat_core::squash::{apply_squash, SquashType};
///
/// // ReLU maps negatives to zero and passes positives through.
/// assert_eq!(apply_squash(SquashType::Relu, -2.0), 0.0);
/// assert_eq!(apply_squash(SquashType::Relu, 3.0), 3.0);
/// // Identity returns the input unchanged.
/// assert_eq!(apply_squash(SquashType::Identity, 1.5), 1.5);
/// ```
#[inline(always)]
pub fn apply_squash(squash_type: SquashType, x: f32) -> f32 {
    match squash_type {
        SquashType::Identity => x,
        SquashType::Relu => x.max(0.0),
        SquashType::Relu6 => x.clamp(0.0, 6.0),
        SquashType::LeakyRelu => {
            if x >= 0.0 {
                x
            } else {
                LEAKY_RELU_ALPHA * x
            }
        }
        // Match TypeScript behaviour (SELU clamps upper bound to avoid exp overflow).
        SquashType::Selu => {
            if !x.is_finite() {
                return -JS_MAX_SAFE_INTEGER;
            }
            let safe_x = (x as f64).min(709.0);
            let fx = if safe_x > 0.0 {
                safe_x
            } else {
                (SELU_ALPHA as f64) * safe_x.exp() - (SELU_ALPHA as f64)
            };
            ((SELU_LAMBDA as f64) * fx) as f32
        }
        SquashType::Elu => {
            if x > 0.0 {
                x
            } else {
                x.exp() - 1.0
            }
        }
        SquashType::Logistic => 1.0 / (1.0 + (-x).exp()),
        SquashType::Tanh => x.tanh(),
        SquashType::HardTanh => x.clamp(-1.0, 1.0),
        SquashType::Softsign => {
            // Match JS ActivationRange bounds (+/-0.99) and avoid tiny f32 overshoots
            // that can fail validation (see test/propagate/ToValue.ts).
            let y = x / (1.0 + x.abs());
            // IMPORTANT: `0.99` in f32 is slightly *greater* than `0.99` in JS (f64),
            // so clamping to `SOFTSIGN_LIMIT` can still yield values > 0.99 in JS.
            // Use the next smaller f32 value to stay within the JS bounds.
            let limit = f32::from_bits(SOFTSIGN_LIMIT.to_bits() - 1);
            y.max(-limit).min(limit)
        }
        // Match TypeScript behaviour (Softplus clamps at x>=709 to 100, non-finite -> 1e-15).
        SquashType::Softplus => {
            if !x.is_finite() {
                return 1e-15;
            }
            if x >= 709.0 {
                return 100.0;
            }
            ((1.0f64 + (x as f64).exp()).ln()) as f32
        }
        SquashType::Swish => x / (1.0 + (-x).exp()),
        SquashType::Mish => x * (1.0 + x.exp()).ln().tanh(),
        SquashType::Gelu => {
            0.5 * x * (1.0 + (SQRT_2_OVER_PI * (x + GELU_COEFF * x * x * x)).tanh())
        }
        SquashType::Sine => x.sin(),
        SquashType::Cosine => ((x as f64).cos()) as f32,
        SquashType::Tan => {
            let result = (x as f64).tan();
            if !result.is_finite() {
                0.0
            } else {
                result.clamp(-TAN_OUTPUT_CLAMP, TAN_OUTPUT_CLAMP) as f32
            }
        }
        SquashType::ArcTan => x.atan(),
        SquashType::Gaussian => (-x * x).exp(),
        SquashType::BentIdentity => ((x * x + 1.0).sqrt() - 1.0) / 2.0 + x,
        SquashType::BipolarSigmoid => 2.0 / (1.0 + (-x).exp()) - 1.0,
        SquashType::Bipolar => {
            if x > 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        SquashType::Step => {
            if x > 0.0 {
                1.0
            } else {
                0.0
            }
        }
        SquashType::Complement => 1.0 - x,
        SquashType::Absolute => x.abs(),
        SquashType::Square => {
            let xf = x as f64;
            (xf * xf).min(SQUARE_OUTPUT_CLAMP) as f32
        }
        SquashType::Cube => {
            let xf = x as f64;
            (xf * xf * xf).clamp(-CUBE_OUTPUT_CLAMP, CUBE_OUTPUT_CLAMP) as f32
        }
        SquashType::Sqrt => {
            if x >= 0.0 {
                x.sqrt()
            } else {
                0.0
            }
        }
        SquashType::StdInverse => {
            if x.abs() < 1e-10 {
                if x >= 0.0 { 1e10 } else { -1e10 }
            } else {
                1.0 / x
            }
        }
        SquashType::Exponential => {
            // Match TypeScript behaviour:
            // - For non-finite x, return a safe capped value.
            // - For x >= 36, clamp to MAX_SAFE_INTEGER to prevent runaway growth.
            //   (JS uses this to avoid overflow and destabilising downstream sums.)
            if !x.is_finite() || x >= 36.0 {
                JS_MAX_SAFE_INTEGER
            } else {
                ((x as f64).exp()) as f32
            }
        }
        SquashType::LogSigmoid => {
            // Match TypeScript behaviour:
            // - Non-finite -> MIN_SAFE_INTEGER
            // - For x <= -709, exp(-x) overflows in JS; TS clamps to low bound.
            if !x.is_finite() || x <= -709.0 {
                return -JS_MAX_SAFE_INTEGER;
            }
            let xf = x as f64;
            let exp_neg_x = (-xf).exp();
            (-(1.0f64 + exp_neg_x).ln()) as f32
        }
        SquashType::Isru => x / (1.0 + x * x).sqrt(),
        // Aggregate functions (Issue #1125) - these are handled specially in the
        // neuron activation loop and don't use the standard sum-then-squash pattern.
        // Return identity as a fallback if they're ever called directly.
        SquashType::Minimum | SquashType::Maximum | SquashType::If => x,
        // Deprecated aggregates: single-value fallback (hypot(x)=|x|, mean(x)=x)
        SquashType::Hypotenuse | SquashType::HypotenuseV2 => x.abs(),
        SquashType::Mean => x,
    }
}
