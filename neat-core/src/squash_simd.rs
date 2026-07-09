//! Lane-parallel (SIMD-friendly) approximations of the transcendental-heavy
//! squash functions used by the batched activation paths (Issue #180).
//!
//! ## Why
//!
//! The batched activation paths (`activate_and_trace_batch_4way` in
//! [`crate::network`] and the 8-record loss path in [`crate::loss`]) compute one
//! pre-activation **per record** with the SIMD weighted-sum kernels, but then
//! apply the squash **per record with the scalar `apply_squash`**. For the hot
//! transcendental squashes (`Tanh`, `Logistic`, `Gelu`, `Mish`) that scalar
//! `libm` call dominates the per-neuron cost on the wide/shallow production
//! creature (~1673 non-input neurons, ~13 average fan-in).
//!
//! Because a batch already holds 4 (or 8) records' pre-activations contiguously,
//! the squash is a natural fit for a lane-parallel evaluation. The functions here
//! are written **branchlessly over fixed-size arrays** so LLVM auto-vectorises
//! the per-lane loop (SSE/AVX/FMA on `x86_64`, NEON on `aarch64`, `simd128` on
//! `wasm32`) without hand-written platform intrinsics.
//!
//! ## Accuracy
//!
//! Every vectorised type is bounded within [`SQUASH_SIMD_MAX_ABS_ERR`] of the
//! scalar [`apply_squash`](crate::squash::apply_squash) across the finite input range (asserted by the range
//! tests below). Squash types **without** a vectorised implementation return
//! `None` from [`squash_x4`] / [`squash_x8`] so the caller falls back to the
//! scalar path, leaving their numerics unchanged. The scalar `apply_squash`
//! remains the single source of truth for correctness.

use crate::squash::{
    GELU_COEFF, LEAKY_RELU_ALPHA, SELU_ALPHA, SELU_LAMBDA, SOFTSIGN_LIMIT, SQRT_2_OVER_PI,
    SquashType,
};

/// Documented maximum absolute error of the vectorised squashes versus the
/// scalar [`apply_squash`](crate::squash::apply_squash) over the finite input range. Chosen tighter than the
/// `1e-5` tolerance the batch parity tests already assert, so the vectorised
/// path is a safe drop-in for the hot transcendental squashes.
pub const SQUASH_SIMD_MAX_ABS_ERR: f32 = 5.0e-6;

/// Branchless single-lane `tanh` approximation (rational minimax, valid after
/// clamping to `±C`). Accurate to a few `1e-7` over the whole finite range; the
/// odd symmetry and saturation to `±1` are preserved exactly. Kept `#[inline]`
/// and branchless so the array wrappers auto-vectorise.
#[inline(always)]
fn tanh_approx(x: f32) -> f32 {
    // Beyond ±C, tanh is ±1 to well within f32 precision; clamping keeps the
    // rational polynomial in its valid range.
    const C: f32 = 7.999_882;
    let x = x.clamp(-C, C);
    let x2 = x * x;

    // Numerator: odd polynomial p(x) = x * (a1 + x2*(a3 + ... + x2*a13)).
    let mut p = -2.760_768_5e-16;
    p = p * x2 + 2.000_188e-13;
    p = p * x2 + -8.604_672e-11;
    p = p * x2 + 5.122_297e-8;
    p = p * x2 + 1.485_722_4e-5;
    p = p * x2 + 6.372_619_4e-4;
    p = p * x2 + 4.893_524_6e-3;
    p *= x;

    // Denominator: even polynomial q(x) = b0 + x2*(b2 + x2*(b4 + x2*b6)).
    let mut q = 1.198_258_4e-6;
    q = q * x2 + 1.185_347e-4;
    q = q * x2 + 2.268_434_6e-3;
    q = q * x2 + 4.893_525e-3;

    p / q
}

/// Branchless single-lane `exp` approximation (Cephes `expf` reduction). Used to
/// build `Logistic` and `Mish`. Accurate to ~1 ULP; the argument is clamped to a
/// finite window so `2^n` reconstruction never overflows to a NaN.
#[inline(always)]
fn exp_approx(x: f32) -> f32 {
    // Clamp to the representable exp window (exp(88.7) ≈ f32::MAX).
    const HI: f32 = 88.722_84;
    const LO: f32 = -87.336_55;
    let x = x.clamp(LO, HI);

    const C1: f32 = 0.693_359_4; // ln2 high
    const C2: f32 = -2.121_944_4e-4; // ln2 low

    // n = round(x / ln2); reduce x into [-ln2/2, ln2/2].
    let fx = (x * core::f32::consts::LOG2_E + 0.5).floor();
    let xr = x - fx * C1 - fx * C2;
    let z = xr * xr;

    // Degree-5 minimax polynomial for exp on the reduced range.
    let mut y = 1.987_569_1e-4;
    y = y * xr + 1.398_199_9e-3;
    y = y * xr + 8.333_452e-3;
    y = y * xr + 4.166_579_6e-2;
    y = y * xr + 1.666_666_5e-1;
    y = y * xr + 5e-1;
    y = y * z + xr + 1.0;

    // Reconstruct 2^n by direct exponent-bits assembly (branchless ldexp).
    let n = fx as i32;
    let pow2n = f32::from_bits(((n + 127) as u32) << 23);
    y * pow2n
}

/// `tanh` over a lane array.
#[inline]
fn tanh_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = tanh_approx(x[i]);
    }
    out
}

/// `Logistic` (sigmoid) over a lane array: `1 / (1 + exp(-x))`.
#[inline]
fn logistic_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = 1.0 / (1.0 + exp_approx(-x[i]));
    }
    out
}

/// `Gelu` (tanh approximation) over a lane array, matching the scalar formula
/// `0.5 * x * (1 + tanh(sqrt(2/pi) * (x + 0.044715 x^3)))`.
#[inline]
fn gelu_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        let inner = SQRT_2_OVER_PI * (v + GELU_COEFF * v * v * v);
        out[i] = 0.5 * v * (1.0 + tanh_approx(inner));
    }
    out
}

/// `Mish` over a lane array.
///
/// Uses the closed-form identity `mish(x) = x * tanh(softplus(x))` rewritten
/// purely in terms of `w = exp(x)`:
///
/// ```text
/// tanh(softplus(x)) = w(w + 2) / (w(w + 2) + 2)
/// ```
///
/// so only a single `exp` per lane is needed (no separate `ln`/`tanh`). The exp
/// argument is clamped at `20` for the ratio only — beyond that the ratio is
/// `1.0` to f32 precision and `mish(x) → x` — which keeps `w^2` finite and avoids
/// an `inf/inf` NaN while leaving the outer `x` factor exact.
#[inline]
fn mish_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        let w = exp_approx(v.min(20.0));
        let t = w * (w + 2.0);
        out[i] = v * (t / (t + 2.0));
    }
    out
}

/// Branchless single-lane `sin` approximation (Cephes `sinf` octant reduction).
/// Accurate to ~1e-7 over the finite range where the argument reduction holds
/// (`|x|` up to a few thousand); the value stays finite for any input. Shared by
/// [`sine_lanes`] and [`cosine_lanes`].
#[inline(always)]
fn sin_approx(x: f32) -> f32 {
    // 4/pi and the three-part high-precision pi/4 split (Cody–Waite).
    const FOPI: f32 = 1.273_239_5;
    const DP1: f32 = 0.785_156_25;
    const DP2: f32 = 2.418_756_5e-4;
    const DP3: f32 = 3.774_895e-8;

    let sign_in = if x < 0.0 { -1.0_f32 } else { 1.0_f32 };
    let mut xa = x.abs();
    // Guard the integer reduction against overflow for extreme inputs; beyond
    // this the periodic value is meaningless anyway, and callers only require a
    // finite result there.
    xa = xa.min(1.0e9);

    let mut j = (FOPI * xa) as i32;
    let mut y = j as f32;
    if j & 1 != 0 {
        j += 1;
        y += 1.0;
    }
    j &= 7;
    let mut sign = sign_in;
    if j > 3 {
        sign = -sign;
        j -= 4;
    }

    let z = ((xa - y * DP1) - y * DP2) - y * DP3;
    let zz = z * z;

    let result = if j == 1 || j == 2 {
        // cos polynomial on the reduced range.
        let mut p = 2.443_315_7e-5;
        p = p * zz - 1.388_731_6e-3;
        p = p * zz + 4.166_664_6e-2;
        1.0 - 0.5 * zz + p * zz * zz
    } else {
        // sin polynomial on the reduced range.
        let mut p = -1.951_529_6e-4;
        p = p * zz + 8.332_161e-3;
        p = p * zz - 1.666_665_5e-1;
        z + p * z * zz
    };

    sign * result
}

/// Branchless single-lane `atan` approximation (Cephes `atanf` range folding).
/// Accurate to a few `1e-7` over the whole finite range; odd-symmetric and
/// saturating to `±pi/2`.
#[inline(always)]
fn atan_approx(x: f32) -> f32 {
    const TAN_3PI_8: f32 = 2.414_213_6; // tan(3*pi/8)
    const TAN_PI_8: f32 = 0.414_213_57; // tan(pi/8)
    const FRAC_PI_2: f32 = core::f32::consts::FRAC_PI_2;
    const FRAC_PI_4: f32 = core::f32::consts::FRAC_PI_4;

    let sign = if x < 0.0 { -1.0_f32 } else { 1.0_f32 };
    let xa = x.abs();

    // Fold |x| into [0, tan(pi/8)] and remember the added angle.
    let (xr, y) = if xa > TAN_3PI_8 {
        (-1.0 / xa, FRAC_PI_2)
    } else if xa > TAN_PI_8 {
        ((xa - 1.0) / (xa + 1.0), FRAC_PI_4)
    } else {
        (xa, 0.0)
    };

    let z = xr * xr;
    let mut p = 8.053_744_5e-2;
    p = p * z - 1.387_768_6e-1;
    p = p * z + 1.997_771_1e-1;
    p = p * z - 3.333_295e-1;
    let poly = p * z * xr + xr;

    sign * (y + poly)
}

/// `Absolute` (`|x|`) over a lane array — exact, branchless.
#[inline]
fn absolute_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = x[i].abs();
    }
    out
}

/// `HardTanh` (`clamp(x, -1, 1)`) over a lane array — exact, branchless.
#[inline]
fn hard_tanh_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = x[i].clamp(-1.0, 1.0);
    }
    out
}

/// `Relu6` (`clamp(x, 0, 6)`) over a lane array — exact, branchless.
#[inline]
fn relu6_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = x[i].clamp(0.0, 6.0);
    }
    out
}

/// `LeakyRelu` over a lane array — exact, branchless select.
#[inline]
fn leaky_relu_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = if v >= 0.0 { v } else { LEAKY_RELU_ALPHA * v };
    }
    out
}

/// `Bipolar` (`x > 0 → 1, else -1`) over a lane array — exact, branchless.
#[inline]
fn bipolar_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = if x[i] > 0.0 { 1.0 } else { -1.0 };
    }
    out
}

/// `Softsign` (`x / (1 + |x|)`, clamped to the JS `±0.99` limit) over a lane
/// array. Reproduces the scalar clamp bit-for-bit so numerics are unchanged.
#[inline]
fn softsign_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    // Same limit as scalar `apply_squash`: the next f32 below 0.99 so clamped
    // values stay within the JS (f64) bounds.
    let limit = f32::from_bits(SOFTSIGN_LIMIT.to_bits() - 1);
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        let y = v / (1.0 + v.abs());
        out[i] = y.max(-limit).min(limit);
    }
    out
}

/// `BentIdentity` (`(sqrt(x^2 + 1) - 1) / 2 + x`) over a lane array — exact
/// f32, branchless.
#[inline]
fn bent_identity_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = ((v * v + 1.0).sqrt() - 1.0) / 2.0 + v;
    }
    out
}

/// `Isru` (`x / sqrt(1 + x^2)`) over a lane array — exact f32, branchless.
#[inline]
fn isru_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = v / (1.0 + v * v).sqrt();
    }
    out
}

/// `Gaussian` (`exp(-x^2)`) over a lane array via [`exp_approx`].
#[inline]
fn gaussian_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = exp_approx(-v * v);
    }
    out
}

/// `Swish` (`x / (1 + exp(-x))`) over a lane array via [`exp_approx`].
#[inline]
fn swish_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = v / (1.0 + exp_approx(-v));
    }
    out
}

/// `BipolarSigmoid` (`2 / (1 + exp(-x)) - 1`) over a lane array.
#[inline]
fn bipolar_sigmoid_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = 2.0 / (1.0 + exp_approx(-x[i])) - 1.0;
    }
    out
}

/// `Elu` (`x > 0 → x, else exp(x) - 1`) over a lane array — branchless select.
#[inline]
fn elu_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let v = x[i];
        out[i] = if v > 0.0 { v } else { exp_approx(v) - 1.0 };
    }
    out
}

/// `Selu` over a lane array — branchless select matching the scalar structure
/// (`lambda * (x > 0 ? x : alpha*exp(x) - alpha)`), with the same `x <= 709`
/// safety clamp the scalar path applies before the exponential.
#[inline]
fn selu_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        let sx = x[i].min(709.0);
        let fx = if sx > 0.0 {
            sx
        } else {
            SELU_ALPHA * exp_approx(sx) - SELU_ALPHA
        };
        out[i] = SELU_LAMBDA * fx;
    }
    out
}

/// `Sine` (`sin(x)`) over a lane array via [`sin_approx`].
#[inline]
fn sine_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = sin_approx(x[i]);
    }
    out
}

/// `Cosine` (`cos(x) = sin(x + pi/2)`) over a lane array via [`sin_approx`].
#[inline]
fn cosine_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    const FRAC_PI_2: f32 = core::f32::consts::FRAC_PI_2;
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = sin_approx(x[i] + FRAC_PI_2);
    }
    out
}

/// `ArcTan` (`atan(x)`) over a lane array via [`atan_approx`].
#[inline]
fn arctan_lanes<const N: usize>(x: [f32; N]) -> [f32; N] {
    let mut out = [0.0_f32; N];
    for i in 0..N {
        out[i] = atan_approx(x[i]);
    }
    out
}

/// Vectorised squash dispatch over `N` lanes.
///
/// Returns `Some([..])` for the squashes that have a lane-parallel
/// approximation within [`SQUASH_SIMD_MAX_ABS_ERR`] of scalar
/// [`apply_squash`](crate::squash::apply_squash); returns `None` for every other type so the caller keeps the
/// existing scalar path (unchanged numerics).
#[inline]
fn squash_lanes<const N: usize>(squash: SquashType, x: [f32; N]) -> Option<[f32; N]> {
    match squash {
        // Hot transcendental squashes (Issue #180).
        SquashType::Tanh => Some(tanh_lanes(x)),
        SquashType::Logistic => Some(logistic_lanes(x)),
        SquashType::Gelu => Some(gelu_lanes(x)),
        SquashType::Mish => Some(mish_lanes(x)),
        // High-frequency production squashes (Issue #243). Cheap / algebraic:
        SquashType::Absolute => Some(absolute_lanes(x)),
        SquashType::HardTanh => Some(hard_tanh_lanes(x)),
        SquashType::Relu6 => Some(relu6_lanes(x)),
        SquashType::LeakyRelu => Some(leaky_relu_lanes(x)),
        SquashType::Bipolar => Some(bipolar_lanes(x)),
        SquashType::Softsign => Some(softsign_lanes(x)),
        SquashType::BentIdentity => Some(bent_identity_lanes(x)),
        SquashType::Isru => Some(isru_lanes(x)),
        // High-frequency production squashes (Issue #243). Transcendental:
        SquashType::Gaussian => Some(gaussian_lanes(x)),
        SquashType::Swish => Some(swish_lanes(x)),
        SquashType::BipolarSigmoid => Some(bipolar_sigmoid_lanes(x)),
        SquashType::Elu => Some(elu_lanes(x)),
        SquashType::Selu => Some(selu_lanes(x)),
        SquashType::Sine => Some(sine_lanes(x)),
        SquashType::Cosine => Some(cosine_lanes(x)),
        SquashType::ArcTan => Some(arctan_lanes(x)),
        _ => None,
    }
}

/// Vectorised squash for a 4-record batch lane. See `squash_lanes`.
#[inline]
pub fn squash_x4(squash: SquashType, x: [f32; 4]) -> Option<[f32; 4]> {
    squash_lanes(squash, x)
}

/// Vectorised squash for an 8-record batch lane. See `squash_lanes`.
#[inline]
pub fn squash_x8(squash: SquashType, x: [f32; 8]) -> Option<[f32; 8]> {
    squash_lanes(squash, x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squash::apply_squash;

    /// Squash types with a vectorised implementation, and the others which must
    /// opt out (so the caller falls back to scalar with unchanged numerics).
    const VECTORISED: [SquashType; 20] = [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
        // Issue #243 additions.
        SquashType::Absolute,
        SquashType::HardTanh,
        SquashType::Relu6,
        SquashType::LeakyRelu,
        SquashType::Bipolar,
        SquashType::Softsign,
        SquashType::BentIdentity,
        SquashType::Isru,
        SquashType::Gaussian,
        SquashType::Swish,
        SquashType::BipolarSigmoid,
        SquashType::Elu,
        SquashType::Selu,
        SquashType::Sine,
        SquashType::Cosine,
        SquashType::ArcTan,
    ];

    /// Per-type finite sweep windows for the Issue #243 additions. Each window
    /// covers the range the activation is realistically used over; the assert is
    /// the same [`SQUASH_SIMD_MAX_ABS_ERR`] bound as the hot transcendentals.
    /// Bipolar is a step function (exact away from the `x = 0` discontinuity) so
    /// it is checked separately rather than swept across the jump.
    const RANGE_CASES: [(SquashType, f32, f32); 18] = [
        (SquashType::Absolute, -1.0e6, 1.0e6),
        (SquashType::HardTanh, -10.0, 10.0),
        (SquashType::Relu6, -10.0, 10.0),
        (SquashType::LeakyRelu, -100.0, 100.0),
        (SquashType::Softsign, -1.0e4, 1.0e4),
        (SquashType::BentIdentity, -1.0e3, 1.0e3),
        (SquashType::Isru, -1.0e3, 1.0e3),
        (SquashType::Gaussian, -20.0, 20.0),
        (SquashType::Swish, -30.0, 30.0),
        (SquashType::BipolarSigmoid, -40.0, 40.0),
        (SquashType::Elu, -40.0, 40.0),
        (SquashType::Selu, -30.0, 30.0),
        (SquashType::Sine, -50.0, 50.0),
        (SquashType::Cosine, -50.0, 50.0),
        (SquashType::ArcTan, -1.0e3, 1.0e3),
        // Extra transcendental windows around the origin, where the curvature is
        // highest and the approximation is most stressed.
        (SquashType::Swish, -6.0, 6.0),
        (SquashType::Sine, -6.3, 6.3),
        (SquashType::Cosine, -6.3, 6.3),
    ];

    /// Max absolute error of a vectorised squash versus scalar `apply_squash`
    /// across a fine sweep of the finite input range.
    fn max_abs_err(squash: SquashType, lo: f32, hi: f32, steps: usize) -> f32 {
        let mut worst = 0.0_f32;
        for k in 0..=steps {
            let x = lo + (hi - lo) * (k as f32) / (steps as f32);
            let want = apply_squash(squash, x);
            let got = squash_x4(squash, [x, x, x, x]).unwrap()[0];
            worst = worst.max((want - got).abs());
        }
        worst
    }

    #[test]
    fn tanh_within_tolerance_over_range() {
        let err = max_abs_err(SquashType::Tanh, -20.0, 20.0, 40_000);
        assert!(
            err <= SQUASH_SIMD_MAX_ABS_ERR,
            "tanh max abs err {err} exceeds {SQUASH_SIMD_MAX_ABS_ERR}"
        );
    }

    #[test]
    fn logistic_within_tolerance_over_range() {
        let err = max_abs_err(SquashType::Logistic, -40.0, 40.0, 40_000);
        assert!(
            err <= SQUASH_SIMD_MAX_ABS_ERR,
            "logistic max abs err {err} exceeds {SQUASH_SIMD_MAX_ABS_ERR}"
        );
    }

    #[test]
    fn gelu_within_tolerance_over_range() {
        // Gelu grows like x, so compare on a bounded window where the activation
        // is used in practice; the relative shape is captured by abs error here.
        let err = max_abs_err(SquashType::Gelu, -10.0, 10.0, 40_000);
        assert!(
            err <= SQUASH_SIMD_MAX_ABS_ERR,
            "gelu max abs err {err} exceeds {SQUASH_SIMD_MAX_ABS_ERR}"
        );
    }

    #[test]
    fn mish_within_tolerance_over_range() {
        let err = max_abs_err(SquashType::Mish, -20.0, 20.0, 40_000);
        assert!(
            err <= SQUASH_SIMD_MAX_ABS_ERR,
            "mish max abs err {err} exceeds {SQUASH_SIMD_MAX_ABS_ERR}"
        );
    }

    #[test]
    fn issue_243_additions_within_tolerance_over_range() {
        for (squash, lo, hi) in RANGE_CASES {
            let err = max_abs_err(squash, lo, hi, 40_000);
            assert!(
                err <= SQUASH_SIMD_MAX_ABS_ERR,
                "{squash:?} max abs err {err} over [{lo}, {hi}] exceeds {SQUASH_SIMD_MAX_ABS_ERR}"
            );
        }
    }

    #[test]
    fn bipolar_matches_scalar_sign() {
        // Bipolar is a step at x = 0; away from the jump the vectorised path must
        // reproduce the scalar sign exactly.
        for &x in &[-100.0_f32, -1.0, -1e-3, 1e-3, 1.0, 100.0] {
            let want = apply_squash(SquashType::Bipolar, x);
            let got = squash_x4(SquashType::Bipolar, [x, x, x, x]).unwrap()[0];
            assert_eq!(want, got, "Bipolar({x}): want {want}, got {got}");
        }
    }

    #[test]
    fn all_four_lanes_match_scalar() {
        // The same value in every lane must reproduce the scalar result; distinct
        // values per lane must each match their own scalar squash.
        let xs = [-1.3_f32, 0.0, 0.42, 2.7];
        for squash in VECTORISED {
            let got = squash_x4(squash, xs).unwrap();
            for (lane, &x) in xs.iter().enumerate() {
                let want = apply_squash(squash, x);
                assert!(
                    (want - got[lane]).abs() <= SQUASH_SIMD_MAX_ABS_ERR,
                    "{squash:?} lane {lane}: want {want}, got {}",
                    got[lane]
                );
            }
        }
    }

    #[test]
    fn x8_matches_x4() {
        let xs = [-3.0_f32, -0.5, 0.0, 0.25, 0.9, 1.5, 4.0, -2.2];
        for squash in VECTORISED {
            let got = squash_x8(squash, xs).unwrap();
            for (lane, &x) in xs.iter().enumerate() {
                let want = apply_squash(squash, x);
                assert!(
                    (want - got[lane]).abs() <= SQUASH_SIMD_MAX_ABS_ERR,
                    "{squash:?} lane {lane}: want {want}, got {}",
                    got[lane]
                );
            }
        }
    }

    #[test]
    fn non_vectorised_types_opt_out() {
        // A representative spread of non-transcendental / specially-handled types
        // must return None so the caller keeps the scalar numerics.
        for squash in [
            SquashType::Identity,
            SquashType::Relu,
            SquashType::Softplus,
            SquashType::Tan,
            SquashType::Square,
            SquashType::Cube,
            SquashType::Sqrt,
            SquashType::Exponential,
            SquashType::LogSigmoid,
            SquashType::StdInverse,
            SquashType::Complement,
            SquashType::Step,
            SquashType::Minimum,
        ] {
            assert!(
                squash_x4(squash, [0.1, 0.2, 0.3, 0.4]).is_none(),
                "{squash:?} must fall back to scalar"
            );
        }
    }

    #[test]
    fn extreme_inputs_are_finite() {
        // Saturating / overflow-prone inputs must not produce NaN/Inf.
        for squash in VECTORISED {
            for &x in &[-1e6_f32, -100.0, 100.0, 1e6, 0.0] {
                let got = squash_x4(squash, [x, x, x, x]).unwrap()[0];
                assert!(got.is_finite(), "{squash:?}({x}) = {got} not finite");
            }
        }
    }
}
