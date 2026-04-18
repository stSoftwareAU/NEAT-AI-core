//! Batch score computation functions for large networks.
//!
//! Issue #1521 - Migrates full-scan score computation paths from TypeScript to
//! Rust. These functions are called when the score cache is cold or when
//! the tracked second-maximum becomes stale after structural mutations.
//!
//! Three exported functions:
//!
//! - `compute_score_components` - single-pass abs-sum, max, and second-max
//!   over flat weight and bias arrays.
//! - `scan_max_weight` - find max and second-max after a weight change,
//!   excluding one index.
//! - `scan_max_bias` - find max and second-max after a bias change,
//!   excluding one index.
//!
//! All functions use f64 precision to match JavaScript `number` semantics,
//! ensuring parity between Rust and TypeScript code paths.

// ---------------------------------------------------------------------------
// compute_score_components
// ---------------------------------------------------------------------------

/// Batch-compute abs-sum, max, and second-max over weight and bias arrays.
///
/// Returns a tuple with 4 elements:
///   (total_abs, count, max_abs, second_max_abs)
///
/// The caller provides flat arrays of synapse weights and non-input neuron
/// biases. This replaces the inner loops of `computeAndCacheScoreComponents`
/// in `Score.ts`.
///
/// # Arguments
/// * `weights` - flat f64 array of synapse weights
/// * `biases` - flat f64 array of non-input neuron biases
pub fn compute_score_components(weights: &[f64], biases: &[f64]) -> (f64, usize, f64, f64) {
    let mut total: f64 = 0.0;
    let mut max: f64 = 0.0;
    let mut second_max: f64 = 0.0;
    let count = weights.len() + biases.len();

    // Process weights
    accumulate_abs_stats(weights, &mut total, &mut max, &mut second_max);

    // Process biases
    accumulate_abs_stats(biases, &mut total, &mut max, &mut second_max);

    (total, count, max, second_max)
}

/// Accumulate absolute-value sum, max, and second-max from a slice.
pub fn accumulate_abs_stats(values: &[f64], total: &mut f64, max: &mut f64, second_max: &mut f64) {
    for &v in values {
        let a = v.abs();
        *total += a;
        if a > *max {
            *second_max = *max;
            *max = a;
        } else if a > *second_max {
            *second_max = a;
        }
    }
}

// ---------------------------------------------------------------------------
// scan_max_weight
// ---------------------------------------------------------------------------

/// Scan all weights and biases to find the new max and second-max after a
/// weight change. The weight at `exclude_idx` is excluded (it is being
/// replaced); `new_weight` is included instead.
///
/// Returns a tuple with 2 elements: (max, second_max).
///
/// # Arguments
/// * `weights` - flat f64 array of all synapse weights
/// * `biases` - flat f64 array of all non-input neuron biases
/// * `exclude_idx` - index in `weights` to skip (the old weight)
/// * `new_weight` - the replacement weight value
pub fn scan_max_weight(
    weights: &[f64],
    biases: &[f64],
    exclude_idx: usize,
    new_weight: f64,
) -> (f64, f64) {
    let mut max: f64 = new_weight.abs();
    let mut second_max: f64 = 0.0;

    // Scan weights, skipping exclude_idx
    for (i, &w) in weights.iter().enumerate() {
        if i == exclude_idx {
            continue;
        }
        let a = w.abs();
        if a > max {
            second_max = max;
            max = a;
        } else if a > second_max {
            second_max = a;
        }
    }

    // Scan biases (no exclusion)
    for &b in biases {
        let a = b.abs();
        if a > max {
            second_max = max;
            max = a;
        } else if a > second_max {
            second_max = a;
        }
    }

    (max, second_max)
}

// ---------------------------------------------------------------------------
// scan_max_bias
// ---------------------------------------------------------------------------

/// Scan all weights and biases to find the new max and second-max after a
/// bias change. The bias at `exclude_idx` is excluded (it is being
/// replaced); `new_bias` is included instead.
///
/// Returns a tuple with 2 elements: (max, second_max).
///
/// # Arguments
/// * `weights` - flat f64 array of all synapse weights
/// * `biases` - flat f64 array of all non-input neuron biases
/// * `exclude_idx` - index in `biases` to skip (the old bias)
/// * `new_bias` - the replacement bias value
pub fn scan_max_bias(
    weights: &[f64],
    biases: &[f64],
    exclude_idx: usize,
    new_bias: f64,
) -> (f64, f64) {
    let mut max: f64 = new_bias.abs();
    let mut second_max: f64 = 0.0;

    // Scan all weights (no exclusion)
    for &w in weights {
        let a = w.abs();
        if a > max {
            second_max = max;
            max = a;
        } else if a > second_max {
            second_max = a;
        }
    }

    // Scan biases, skipping exclude_idx
    for (i, &b) in biases.iter().enumerate() {
        if i == exclude_idx {
            continue;
        }
        let a = b.abs();
        if a > max {
            second_max = max;
            max = a;
        } else if a > second_max {
            second_max = a;
        }
    }

    (max, second_max)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
