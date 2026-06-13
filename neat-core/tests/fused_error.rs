//! Fused error distribution tests (moved from `src/fused_error.rs`).

use neat_core::{
    SquashType, apply_calculate_error, apply_fused_error_distribution, apply_safe_zone_adjustment,
};

#[test]
fn test_fused_basic_identity() {
    // Identity neuron: error = target - current = 0.3
    let result = apply_fused_error_distribution(
        SquashType::Identity,
        0.5,
        0.8,
        0.5,
        &[SquashType::Identity as u8, SquashType::Identity as u8],
        &[1.0, 2.0],
        &[1.0, 2.0],
        &[1.0, 1.0],
    );

    // Result layout: [error, safe0, safe1, share0, share1]
    assert_eq!(result.len(), 5);
    let error = result[0];
    assert!(
        (error - 0.3).abs() < 1e-5,
        "error should be ~0.3, got {}",
        error
    );

    // Identity safe zones at moderate inputs should be 1.0
    let safe0 = result[1];
    let safe1 = result[2];
    assert!(safe0 > 0.0);
    assert!(safe1 > 0.0);

    // shares should sum to error
    let share0 = result[3];
    let share1 = result[4];
    assert!(
        (share0 + share1 - error).abs() < 1e-5,
        "shares should sum to error: {} + {} = {} vs {}",
        share0,
        share1,
        share0 + share1,
        error,
    );

    // activation² weighting: 1²=1, 2²=4, so share1 should be ~4× share0
    let ratio = share1 / share0;
    assert!(
        (ratio - 4.0).abs() < 0.5,
        "ratio should be ~4, got {}",
        ratio,
    );
}

#[test]
fn test_fused_zero_error() {
    let result = apply_fused_error_distribution(
        SquashType::Identity,
        1.0,
        1.0,
        1.0,
        &[SquashType::Relu as u8],
        &[1.0],
        &[1.0],
        &[1.0],
    );

    assert_eq!(result.len(), 3);
    assert_eq!(result[0], 0.0); // error
    assert_eq!(result[2], 0.0); // share
}

#[test]
fn test_fused_empty_synapses() {
    let result =
        apply_fused_error_distribution(SquashType::Identity, 0.5, 0.8, 0.5, &[], &[], &[], &[]);

    assert_eq!(result.len(), 1);
    assert!((result[0] - 0.3).abs() < 1e-5);
}

#[test]
fn test_fused_single_synapse() {
    let result = apply_fused_error_distribution(
        SquashType::Tanh,
        0.0,
        0.5,
        0.0,
        &[SquashType::Identity as u8],
        &[1.0],
        &[1.0],
        &[1.0],
    );

    // Layout: [error, safe0, share0]
    assert_eq!(result.len(), 3);
    let error = result[0];
    let share = result[2];
    // Single synapse gets all the error
    assert!(
        (share - error).abs() < 1e-5,
        "single synapse should get all error: share={}, error={}",
        share,
        error,
    );
}

/// Issue #156 — wide-neuron regression guard.
///
/// The Step-2 safe-zone factors are written directly into `result[1 .. 1 + count]`
/// (no separate `safe_zone_factors` Vec/copy loop) and read back in the Step-3
/// scores loop. This test independently recomputes the entire expected output
/// using the public `apply_calculate_error` / `apply_safe_zone_adjustment`
/// primitives and asserts a **bit-for-bit** match, so any change to the stored
/// or read-back values (not just an arithmetic regression) is caught.
#[test]
fn test_fused_wide_neuron_layout_bit_for_bit() {
    // A deliberately wide neuron with a spread of upstream squash types,
    // raw inputs, activations and weights.
    let upstream_squash = [
        SquashType::Identity as u8,
        SquashType::Relu as u8,
        SquashType::Tanh as u8,
        SquashType::Logistic as u8,
        SquashType::LeakyRelu as u8,
        SquashType::Gelu as u8,
    ];
    let hint_values = [0.5_f32, -2.0, 1.5, -0.3, 3.0, 0.1];
    let activations = [0.2_f32, -1.0, 0.8, 0.4, -0.6, 1.2];
    let weights = [1.0_f32, -0.5, 2.0, 0.3, -1.5, 0.7];

    let neuron_squash = SquashType::Tanh;
    let neuron_activation = 0.25_f32;
    let neuron_target = 0.75_f32;
    let neuron_hint = 0.25_f32;

    let result = apply_fused_error_distribution(
        neuron_squash,
        neuron_activation,
        neuron_target,
        neuron_hint,
        &upstream_squash,
        &hint_values,
        &activations,
        &weights,
    );

    let count = upstream_squash.len();
    assert_eq!(result.len(), 1 + 2 * count, "flat layout length");

    // Independently recompute the expected output.
    let error = apply_calculate_error(neuron_squash, neuron_activation, neuron_target, neuron_hint);
    assert_eq!(result[0], error, "error slot must match the primitive");
    assert_ne!(error, 0.0, "test input must exercise the non-zero path");

    let provisional = error / (count as f32);

    // Expected safe-zone factors at result[1 .. 1 + count].
    let mut expected_safe = Vec::with_capacity(count);
    for i in 0..count {
        let squash = SquashType::from(upstream_squash[i]);
        let weight = if weights[i].is_finite() {
            weights[i]
        } else {
            1.0
        };
        let factor = apply_safe_zone_adjustment(squash, hint_values[i], provisional, weight);
        expected_safe.push(factor);
        assert_eq!(
            result[1 + i],
            factor,
            "safe-zone factor at result[{}] must match primitive",
            1 + i
        );
    }

    // Expected scores and proportional shares at result[1 + count .. 1 + 2*count].
    let mut scores = Vec::with_capacity(count);
    let mut denom = 0.0_f32;
    for i in 0..count {
        let a = activations[i];
        let safe = expected_safe[i];
        let score = if !a.is_finite() || !safe.is_finite() {
            0.0
        } else {
            a * a * safe.clamp(0.0, 1.0)
        };
        scores.push(score);
        denom += score;
    }
    assert!(
        denom > 1e-12,
        "test input must exercise the score-based path"
    );

    let mut sum = 0.0_f32;
    let mut best_idx = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    let mut expected_shares = vec![0.0_f32; count];
    for i in 0..count {
        let share = error * (scores[i] / denom);
        expected_shares[i] = share;
        sum += share;
        if scores[i] > best_score {
            best_score = scores[i];
            best_idx = i;
        }
    }
    let residue = error - sum;
    if residue.abs() > 1e-12 {
        expected_shares[best_idx] += residue;
    }

    for i in 0..count {
        assert_eq!(
            result[1 + count + i],
            expected_shares[i],
            "share at result[{}] must match independent recomputation",
            1 + count + i
        );
    }

    // Shares must sum to error (residue cleanup invariant).
    let share_sum: f32 = result[1 + count..].iter().sum();
    assert!(
        (share_sum - error).abs() < 1e-5,
        "shares must sum to error: {} vs {}",
        share_sum,
        error
    );
}
