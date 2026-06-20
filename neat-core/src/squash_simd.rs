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

use crate::squash::{GELU_COEFF, SQRT_2_OVER_PI, SquashType};

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

/// Vectorised squash dispatch over `N` lanes.
///
/// Returns `Some([..])` for the hot transcendental squashes that have a
/// lane-parallel approximation within [`SQUASH_SIMD_MAX_ABS_ERR`] of scalar
/// [`apply_squash`](crate::squash::apply_squash); returns `None` for every other type so the caller keeps the
/// existing scalar path (unchanged numerics).
#[inline]
fn squash_lanes<const N: usize>(squash: SquashType, x: [f32; N]) -> Option<[f32; N]> {
    match squash {
        SquashType::Tanh => Some(tanh_lanes(x)),
        SquashType::Logistic => Some(logistic_lanes(x)),
        SquashType::Gelu => Some(gelu_lanes(x)),
        SquashType::Mish => Some(mish_lanes(x)),
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
    const VECTORISED: [SquashType; 4] = [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
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
            SquashType::Sine,
            SquashType::Gaussian,
            SquashType::Softplus,
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
