//! The per-value weight and bias magnitude penalty (Issue #778).
//!
//! This is the penalty NEAT-AI `Score.ts` charges for one weight or bias
//! (`magnitudePenalty`) and for a stream of them (`meanMagnitudePenalty`). A
//! magnitude at or below 1 costs nothing; above that the cost grows with the
//! number of decades, `0.999 * d / 12` up to twelve decades and an asymptotic
//! tail towards 1 beyond, so every decade costs the same and the curve never
//! reaches 1. Magnitudes past [`MAX_SAFE_MAGNITUDE`] are clamped to it first.
//!
//! NEAT-AI-scorer `rust_scorer/src/scoring.rs` holds the existing Rust copy of
//! this rule. The two must stay identical until the scorer reads this one; this
//! change does not switch it. Both are pinned to the shared magnitude-penalty
//! corpus.

/// Number of decades after which the penalty switches to its asymptotic tail.
pub const MAGNITUDE_DECADE_CAP: f64 = 12.0;

/// Largest magnitude charged; anything bigger is clamped to it (`2^53 - 1`).
pub const MAX_SAFE_MAGNITUDE: f64 = 9_007_199_254_740_991.0;

/// Why [`magnitude_penalty`] refused a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MagnitudePenaltyError {
    /// The value was `NaN`, which has no magnitude to charge.
    NotANumber,
}

impl std::fmt::Display for MagnitudePenaltyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MagnitudePenaltyError::NotANumber => {
                write!(f, "Magnitude penalty is undefined for NaN")
            }
        }
    }
}

impl std::error::Error for MagnitudePenaltyError {}

/// Penalty charged for a single weight or bias, in `[0, 1)`.
///
/// The sign is ignored, infinities are clamped to [`MAX_SAFE_MAGNITUDE`], and
/// a magnitude of 1 or less costs `0.0`.
///
/// # Errors
///
/// [`MagnitudePenaltyError::NotANumber`] for `NaN`.
///
/// # Examples
///
/// ```
/// use neat_core::magnitude_penalty;
///
/// assert_eq!(magnitude_penalty(0.5).unwrap(), 0.0);
/// assert_eq!(magnitude_penalty(-100.0).unwrap(), magnitude_penalty(100.0).unwrap());
/// assert!(magnitude_penalty(f64::NAN).is_err());
/// ```
pub fn magnitude_penalty(value: f64) -> Result<f64, MagnitudePenaltyError> {
    if value.is_nan() {
        return Err(MagnitudePenaltyError::NotANumber);
    }
    let magnitude = value.abs().min(MAX_SAFE_MAGNITUDE);
    if magnitude <= 1.0 {
        return Ok(0.0);
    }
    let decades = magnitude.log10();
    Ok(if decades < MAGNITUDE_DECADE_CAP {
        0.999 * decades / MAGNITUDE_DECADE_CAP
    } else {
        0.999 + 0.001 * (1.0 - MAGNITUDE_DECADE_CAP / decades)
    })
}

/// Mean [`magnitude_penalty`] over the finite values, in order.
///
/// Non-finite values (`NaN` and the infinities) are skipped and do not count
/// towards the mean. With no finite value the result is `0.0`.
///
/// # Examples
///
/// ```
/// use neat_core::mean_magnitude_penalty;
///
/// assert_eq!(mean_magnitude_penalty([]), 0.0);
/// assert_eq!(mean_magnitude_penalty([0.5, f64::NAN]), 0.0);
/// ```
pub fn mean_magnitude_penalty<I: IntoIterator<Item = f64>>(values: I) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for value in values {
        if !value.is_finite() {
            continue;
        }
        // A finite value is never NaN, so the penalty is always defined.
        if let Ok(penalty) = magnitude_penalty(value) {
            total += penalty;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}
