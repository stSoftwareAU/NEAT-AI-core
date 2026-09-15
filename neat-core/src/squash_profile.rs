//! One declarative row per [`SquashType`] for the per-variant *data* two
//! modules used to restate in parallel `match`es (Issue #673).
//!
//! `SquashType` is switched on in several modules, but most of those switches
//! are genuinely different *algorithms* — the forward value ([`crate::squash`]),
//! the derivative ([`crate::derivative`]), the inverse ([`crate::unsquash`]),
//! the error curve ([`crate::error`]) and the aggregate reductions
//! ([`crate::batch_scoring`]) share nothing but the tag, so a table is the wrong
//! tool for them and they stay as they are.
//!
//! Two of the switches were not algorithms at all: the **output range** in
//! [`crate::range`] and the **safe-zone rule** in [`crate::safe_zone`] are
//! per-variant *values*. They live here instead, one row per variant, in a
//! single exhaustive `match` — so adding an activation function is one row
//! rather than one arm in each of two files, and the compiler still refuses to
//! build a `SquashType` that has no row.
//!
//! This extends the pattern `aggregate_squash_patterns!()` established for the
//! aggregate membership list (Issue #446): the per-variant data has one home,
//! and every consumer reads it from there.

use crate::range::{F32_LARGE, GELU_MIN, MISH_MIN, SOFTPLUS_MAX, SOFTPLUS_MIN, SWISH_MIN};
use crate::squash::{SELU_ALPHA, SELU_LAMBDA, SOFTSIGN_LIMIT, SquashType};

/// A safe band with a symmetric linear fade either side of it.
///
/// The rule itself is `crate::safe_zone::bounded_safe_zone`; this is only the
/// per-variant data it takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SafeBand {
    /// Lower edge of the fully-safe band.
    pub min: f32,
    /// Upper edge of the fully-safe band.
    pub max: f32,
    /// Width over which the factor fades to zero past each edge.
    pub fade: f32,
}

/// A saturating sigmoid-shaped safe zone: a safe band, a wider fade window, and
/// a fixed recovery factor granted when the error already pulls the raw input
/// back toward the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SaturatingBand {
    /// `(low, high)` edges of the fully-safe band.
    pub safe: (f32, f32),
    /// `(low, high)` outer edges the fade reaches zero at.
    pub fade: (f32, f32),
    /// `(low, high)` thresholds at or beyond which a correcting error earns the
    /// recovery factor. Equal to `safe` unless the variant saturates hard
    /// outside its safe band (`HardTanh`).
    pub recovery: (f32, f32),
    /// The factor granted in the recovery direction.
    pub factor: f32,
}

/// A safe zone stated on `|raw_input|`: safe inside `safe`, a fixed recovery
/// factor when the error pulls back toward zero, then a linear fade out to
/// `fade_end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SymmetricBand {
    /// Magnitude up to which the gradient flows fully.
    pub safe: f32,
    /// Magnitude at which the fade reaches zero.
    pub fade_end: f32,
    /// The factor granted in the recovery direction.
    pub recovery: f32,
}

/// The safe-zone rule a squash type follows, as data.
///
/// The first four variants carry the whole rule; the rest name a genuinely
/// per-variant algorithm implemented in [`crate::safe_zone`]. Naming them as a
/// closed set is what keeps that dispatch exhaustively checked without a second
/// membership list: a new `SquashType` needs a row here, and a new bespoke
/// shape is a compile error in `safe_zone.rs` until it has an arm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SafeZoneRule {
    /// Safe band with a symmetric linear fade — the shared bounded rule.
    Band(SafeBand),
    /// Sigmoid-style saturation with a recovery factor outside the band.
    Saturating(SaturatingBand),
    /// `|raw_input|`-based safe zone with a recovery factor and a linear fade.
    Symmetric(SymmetricBand),
    /// A fixed factor, independent of input, error and weight.
    Constant(f32),
    /// Identity: safe unless an extreme raw input meets a vanishing weight.
    Identity,
    /// ReLU: the dead-ReLU rule.
    Relu,
    /// ReLU6: dead at both ends.
    Relu6,
    /// Sine: slope-based, flat where `cos(x)` vanishes.
    Sine,
    /// Cosine: slope-based, flat where `sin(x)` vanishes.
    Cosine,
    /// Tan: distance from the nearest asymptote.
    Tan,
    /// ArcTan: symmetric fade with a hard cut-off beyond the fade window.
    ArcTan,
    /// Step: threshold function, judged by which side of zero the error wants.
    Step,
    /// Absolute: safe unless an extreme raw input meets a tiny weight.
    Absolute,
    /// Sqrt: one-sided domain, safest in `[0.01, 10]`.
    Sqrt,
}

/// The per-variant data for one squash type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SquashProfile {
    /// `(low, high)` bounds of the activation's output range.
    pub range: (f32, f32),
    /// How [`crate::safe_zone::apply_safe_zone_adjustment`] treats the type.
    pub safe_zone: SafeZoneRule,
}

/// Shorthand for a row whose safe zone is the shared bounded rule.
const fn band(range: (f32, f32), min: f32, max: f32, fade: f32) -> SquashProfile {
    SquashProfile {
        range,
        safe_zone: SafeZoneRule::Band(SafeBand { min, max, fade }),
    }
}

/// Shorthand for a row whose safe-zone factor is a constant.
const fn constant(range: (f32, f32), factor: f32) -> SquashProfile {
    SquashProfile {
        range,
        safe_zone: SafeZoneRule::Constant(factor),
    }
}

/// Shorthand for a row whose safe zone is a named per-variant algorithm.
const fn shaped(range: (f32, f32), safe_zone: SafeZoneRule) -> SquashProfile {
    SquashProfile { range, safe_zone }
}

/// Unbounded both ways, as far as an `f32` activation goes.
const UNBOUNDED: (f32, f32) = (-F32_LARGE, F32_LARGE);
/// Non-negative and otherwise unbounded.
const NON_NEGATIVE: (f32, f32) = (0.0, F32_LARGE);
/// The unit interval.
const UNIT: (f32, f32) = (0.0, 1.0);
/// The bipolar unit interval.
const BIPOLAR_UNIT: (f32, f32) = (-1.0, 1.0);

/// The single declarative list: one row per [`SquashType`].
///
/// Exhaustive by construction — a new variant fails to compile here until it is
/// given a range and a safe-zone rule, which is the property the two former
/// per-module `match`es each provided separately.
#[inline(always)]
pub(crate) const fn squash_profile(squash_type: SquashType) -> SquashProfile {
    use SafeZoneRule as Z;

    match squash_type {
        SquashType::Identity => shaped(UNBOUNDED, Z::Identity),
        SquashType::Relu => shaped(NON_NEGATIVE, Z::Relu),
        SquashType::Relu6 => shaped((0.0, 6.0), Z::Relu6),
        SquashType::LeakyRelu => band(UNBOUNDED, -50.0, 50.0, 20.0),
        // SELU's minimum is `-alpha * lambda`.
        SquashType::Selu => band((-SELU_ALPHA * SELU_LAMBDA, F32_LARGE), -10.0, 10.0, 10.0),
        // ELU with alpha = 1 bottoms out at -1.
        SquashType::Elu => band((-1.0, F32_LARGE), -10.0, 10.0, 10.0),
        SquashType::Logistic => shaped(
            UNIT,
            Z::Saturating(SaturatingBand {
                safe: (-6.0, 6.0),
                fade: (-10.0, 10.0),
                recovery: (-6.0, 6.0),
                factor: 0.2,
            }),
        ),
        SquashType::Tanh => shaped(
            BIPOLAR_UNIT,
            Z::Saturating(SaturatingBand {
                safe: (-2.0, 2.0),
                fade: (-6.0, 6.0),
                recovery: (-2.0, 2.0),
                factor: 0.2,
            }),
        ),
        // HardTanh saturates hard at ±1, outside its ±0.9 safe band, so its
        // recovery thresholds are the clamp edges rather than the band edges.
        SquashType::HardTanh => shaped(
            BIPOLAR_UNIT,
            Z::Saturating(SaturatingBand {
                safe: (-0.9, 0.9),
                fade: (-1.2, 1.2),
                recovery: (-1.0, 1.0),
                factor: 0.2,
            }),
        ),
        SquashType::Softsign => band((-SOFTSIGN_LIMIT, SOFTSIGN_LIMIT), -10.0, 10.0, 10.0),
        SquashType::Softplus => band((SOFTPLUS_MIN, SOFTPLUS_MAX), -10.0, 20.0, 10.0),
        SquashType::Swish => band((SWISH_MIN, F32_LARGE), -10.0, 10.0, 10.0),
        SquashType::Mish => band((MISH_MIN, F32_LARGE), -10.0, 10.0, 10.0),
        SquashType::Gelu => band((GELU_MIN, F32_LARGE), -6.0, 6.0, 10.0),
        SquashType::Sine => shaped(BIPOLAR_UNIT, Z::Sine),
        SquashType::Cosine => shaped(BIPOLAR_UNIT, Z::Cosine),
        SquashType::Tan => shaped(UNBOUNDED, Z::Tan),
        SquashType::ArcTan => shaped(
            (-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
            Z::ArcTan,
        ),
        SquashType::Gaussian => band(UNIT, -3.0, 3.0, 3.0),
        SquashType::BentIdentity => shaped(
            UNBOUNDED,
            Z::Symmetric(SymmetricBand {
                safe: 10.0,
                fade_end: 20.0,
                recovery: 0.3,
            }),
        ),
        SquashType::BipolarSigmoid => band(BIPOLAR_UNIT, -4.0, 4.0, 4.0),
        // Discontinuous — no gradient ever flows.
        SquashType::Bipolar => constant(BIPOLAR_UNIT, 0.0),
        SquashType::Step => shaped(UNIT, Z::Step),
        // Linear with slope -1, so it never saturates.
        SquashType::Complement => constant(UNBOUNDED, 1.0),
        SquashType::Absolute => shaped(NON_NEGATIVE, Z::Absolute),
        SquashType::Square => shaped(
            NON_NEGATIVE,
            Z::Symmetric(SymmetricBand {
                safe: 5.0,
                fade_end: 10.0,
                recovery: 0.2,
            }),
        ),
        SquashType::Cube => shaped(
            UNBOUNDED,
            Z::Symmetric(SymmetricBand {
                safe: 5.0,
                fade_end: 10.0,
                recovery: 0.2,
            }),
        ),
        SquashType::Sqrt => shaped(NON_NEGATIVE, Z::Sqrt),
        SquashType::StdInverse => band(UNBOUNDED, -10.0, 10.0, 10.0),
        SquashType::Exponential => band(NON_NEGATIVE, -10.0, 30.0, 10.0),
        // Log-sigmoid output is always <= 0.
        SquashType::LogSigmoid => band((-F32_LARGE, 0.0), -20.0, 20.0, 10.0),
        SquashType::Isru => band(BIPOLAR_UNIT, -10.0, 10.0, 10.0),
        // Aggregate functions (Issue #1125) — unbounded, and not differentiable,
        // so no gradient flows through them.
        SquashType::Minimum => constant(UNBOUNDED, 0.0),
        SquashType::Maximum => constant(UNBOUNDED, 0.0),
        SquashType::If => constant(UNBOUNDED, 0.0),
        SquashType::Hypotenuse => constant(UNBOUNDED, 0.0),
        // HYPOTv2 takes a square root last, so its output is non-negative.
        SquashType::HypotenuseV2 => constant(NON_NEGATIVE, 0.0),
        SquashType::Mean => constant(UNBOUNDED, 0.0),
    }
}
