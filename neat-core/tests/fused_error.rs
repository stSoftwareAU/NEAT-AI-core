//! Fused error distribution tests (moved from `src/fused_error.rs`).

use neat_core::{SquashType, apply_fused_error_distribution};

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
