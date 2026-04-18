//! Error calculation tests (moved from `src/error.rs`).

use neat_core::{SquashType, apply_calculate_error, apply_calculate_error_batch_4way};

#[test]
fn test_calculate_error_identity() {
    // Identity: error = rawError (slope = 1)
    let error = apply_calculate_error(SquashType::Identity, 0.5, 0.8, 0.5);
    assert!(
        (error - 0.3).abs() < 1e-5,
        "Identity error should be 0.3, got {}",
        error
    );

    // Tiny error should return 0
    let tiny_error = apply_calculate_error(SquashType::Identity, 0.5, 0.5 + 1e-8, 0.5);
    assert_eq!(tiny_error, 0.0, "Tiny error should return 0");
}

#[test]
fn test_calculate_error_complement() {
    // Complement: error = rawError / -1 = -rawError
    let error = apply_calculate_error(SquashType::Complement, 0.5, 0.8, 0.5);
    assert!(
        (error - (-0.3)).abs() < 1e-5,
        "Complement error should be -0.3, got {}",
        error
    );
}

#[test]
fn test_calculate_error_relu() {
    // ReLU: use raw error when active
    let error = apply_calculate_error(SquashType::Relu, 2.0, 3.0, 2.0);
    assert!(
        (error - 1.0).abs() < 1e-5,
        "ReLU active error should be 1.0, got {}",
        error
    );

    // ReLU: dead neuron uses unSquash fallback
    let dead_error = apply_calculate_error(SquashType::Relu, 0.0, 1.0, -1.0);
    assert!(dead_error.is_finite(), "ReLU dead error should be finite");
}

#[test]
fn test_calculate_error_tanh() {
    // TANH: use derivative when slope is strong
    let current_value = 0.0;
    let activation = (current_value as f32).tanh(); // 0.0
    let error = apply_calculate_error(SquashType::Tanh, activation, 0.5, current_value);
    // At x=0, derivative = 1, so error = rawError / 1 = 0.5
    assert!(
        (error - 0.5).abs() < 1e-4,
        "TANH error at x=0 should be ~0.5, got {}",
        error
    );
}

#[test]
fn test_calculate_error_absolute() {
    // ABSOLUTE: uses closest target approach
    // currentValue = -2, activation = 2, target = 1
    // Options: -1 or +1 as targets
    // closest to -2 is -1, so error = -1 - (-2) = 1
    let error = apply_calculate_error(SquashType::Absolute, 2.0, 1.0, -2.0);
    assert!(
        (error - 1.0).abs() < 1e-5,
        "Absolute error should be 1.0, got {}",
        error
    );
}

#[test]
fn test_calculate_error_clamping() {
    // Test that errors are clamped to +/-100
    let large_error = apply_calculate_error(SquashType::Identity, 0.0, 1000.0, 0.0);
    assert!(
        large_error.abs() <= 100.0,
        "Error should be clamped to +/-100, got {}",
        large_error
    );

    let neg_large_error = apply_calculate_error(SquashType::Identity, 1000.0, 0.0, 1000.0);
    assert!(
        neg_large_error.abs() <= 100.0,
        "Negative error should be clamped to +/-100, got {}",
        neg_large_error
    );
}

#[test]
fn test_calculate_error_aggregate_functions() {
    // Aggregate functions should return 0
    assert_eq!(
        apply_calculate_error(SquashType::Minimum, 0.5, 0.8, 0.5),
        0.0
    );
    assert_eq!(
        apply_calculate_error(SquashType::Maximum, 0.5, 0.8, 0.5),
        0.0
    );
    assert_eq!(apply_calculate_error(SquashType::If, 0.5, 0.8, 0.5), 0.0);
}

// Issue #1213 - SIMD batch error computation tests
#[test]
fn test_calculate_error_batch_4way_identity() {
    let curr_acts = [0.5, 0.3, 0.7, 0.1];
    let tgt_acts = [0.8, 0.5, 0.6, 0.9];
    let curr_vals = [0.5, 0.3, 0.7, 0.1];

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Identity, &curr_acts, &tgt_acts, &curr_vals);

    // Identity: error = rawError = target - current
    assert!(
        (e0 - 0.3).abs() < 1e-5,
        "Identity e0 should be 0.3, got {}",
        e0
    );
    assert!(
        (e1 - 0.2).abs() < 1e-5,
        "Identity e1 should be 0.2, got {}",
        e1
    );
    assert!(
        (e2 - (-0.1)).abs() < 1e-5,
        "Identity e2 should be -0.1, got {}",
        e2
    );
    assert!(
        (e3 - 0.8).abs() < 1e-5,
        "Identity e3 should be 0.8, got {}",
        e3
    );
}

#[test]
fn test_calculate_error_batch_4way_complement() {
    let curr_acts = [0.5, 0.3, 0.7, 0.1];
    let tgt_acts = [0.8, 0.5, 0.6, 0.9];
    let curr_vals = [0.5, 0.7, 0.3, 0.9];

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Complement, &curr_acts, &tgt_acts, &curr_vals);

    // Complement: error = rawError / -1 = -(target - current)
    assert!(
        (e0 - (-0.3)).abs() < 1e-5,
        "Complement e0 should be -0.3, got {}",
        e0
    );
    assert!(
        (e1 - (-0.2)).abs() < 1e-5,
        "Complement e1 should be -0.2, got {}",
        e1
    );
    assert!(
        (e2 - 0.1).abs() < 1e-5,
        "Complement e2 should be 0.1, got {}",
        e2
    );
    assert!(
        (e3 - (-0.8)).abs() < 1e-5,
        "Complement e3 should be -0.8, got {}",
        e3
    );
}

#[test]
fn test_calculate_error_batch_4way_tiny_error() {
    let curr_acts = [0.5, 0.3, 0.7, 0.1];
    let tgt_acts = [0.5 + 1e-8, 0.3 + 1e-9, 0.7 - 1e-8, 0.1];
    let curr_vals = [0.5, 0.3, 0.7, 0.1];

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Identity, &curr_acts, &tgt_acts, &curr_vals);

    // Tiny errors should be zeroed
    assert_eq!(e0, 0.0, "Tiny error e0 should be 0");
    assert_eq!(e1, 0.0, "Tiny error e1 should be 0");
    assert_eq!(e2, 0.0, "Tiny error e2 should be 0");
    assert_eq!(e3, 0.0, "Tiny error e3 should be 0");
}

#[test]
fn test_calculate_error_batch_4way_relu_active() {
    // All neurons are active (current_value > 0)
    let curr_acts = [2.0, 3.0, 1.0, 5.0];
    let tgt_acts = [3.0, 4.0, 2.0, 6.0];
    let curr_vals = [2.0, 3.0, 1.0, 5.0]; // All positive = active

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Relu, &curr_acts, &tgt_acts, &curr_vals);

    // Active ReLU: error = rawError
    assert!(
        (e0 - 1.0).abs() < 1e-5,
        "ReLU active e0 should be 1.0, got {}",
        e0
    );
    assert!(
        (e1 - 1.0).abs() < 1e-5,
        "ReLU active e1 should be 1.0, got {}",
        e1
    );
    assert!(
        (e2 - 1.0).abs() < 1e-5,
        "ReLU active e2 should be 1.0, got {}",
        e2
    );
    assert!(
        (e3 - 1.0).abs() < 1e-5,
        "ReLU active e3 should be 1.0, got {}",
        e3
    );
}

#[test]
fn test_calculate_error_batch_4way_clamping() {
    let curr_acts = [0.0, 0.0, 0.0, 0.0];
    let tgt_acts = [1000.0, -1000.0, 500.0, -500.0];
    let curr_vals = [0.0, 0.0, 0.0, 0.0];

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Identity, &curr_acts, &tgt_acts, &curr_vals);

    // Errors should be clamped to +/-100
    assert!(e0.abs() <= 100.0, "Error e0 should be clamped, got {}", e0);
    assert!(e1.abs() <= 100.0, "Error e1 should be clamped, got {}", e1);
    assert!(e2.abs() <= 100.0, "Error e2 should be clamped, got {}", e2);
    assert!(e3.abs() <= 100.0, "Error e3 should be clamped, got {}", e3);
}

#[test]
fn test_calculate_error_batch_4way_matches_scalar() {
    // Test that batch results match scalar results for various squash types
    let curr_acts = [0.5, 0.3, 0.7, 0.9];
    let tgt_acts = [0.8, 0.5, 0.4, 0.2];
    let curr_vals = [0.5, 0.3, 0.7, 0.9];

    for squash_type in [
        SquashType::Identity,
        SquashType::Relu,
        SquashType::Complement,
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::LeakyRelu,
    ] {
        let (b0, b1, b2, b3) =
            apply_calculate_error_batch_4way(squash_type, &curr_acts, &tgt_acts, &curr_vals);

        let s0 = apply_calculate_error(squash_type, curr_acts[0], tgt_acts[0], curr_vals[0]);
        let s1 = apply_calculate_error(squash_type, curr_acts[1], tgt_acts[1], curr_vals[1]);
        let s2 = apply_calculate_error(squash_type, curr_acts[2], tgt_acts[2], curr_vals[2]);
        let s3 = apply_calculate_error(squash_type, curr_acts[3], tgt_acts[3], curr_vals[3]);

        assert!(
            (b0 - s0).abs() < 1e-5,
            "{:?}: b0={} != s0={}",
            squash_type,
            b0,
            s0
        );
        assert!(
            (b1 - s1).abs() < 1e-5,
            "{:?}: b1={} != s1={}",
            squash_type,
            b1,
            s1
        );
        assert!(
            (b2 - s2).abs() < 1e-5,
            "{:?}: b2={} != s2={}",
            squash_type,
            b2,
            s2
        );
        assert!(
            (b3 - s3).abs() < 1e-5,
            "{:?}: b3={} != s3={}",
            squash_type,
            b3,
            s3
        );
    }
}

#[test]
fn test_calculate_error_batch_4way_aggregate_functions() {
    let curr_acts = [0.5, 0.3, 0.7, 0.9];
    let tgt_acts = [0.8, 0.5, 0.4, 0.2];
    let curr_vals = [0.5, 0.3, 0.7, 0.9];

    // Aggregate functions should return 0
    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Minimum, &curr_acts, &tgt_acts, &curr_vals);
    assert_eq!((e0, e1, e2, e3), (0.0, 0.0, 0.0, 0.0));

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::Maximum, &curr_acts, &tgt_acts, &curr_vals);
    assert_eq!((e0, e1, e2, e3), (0.0, 0.0, 0.0, 0.0));

    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::If, &curr_acts, &tgt_acts, &curr_vals);
    assert_eq!((e0, e1, e2, e3), (0.0, 0.0, 0.0, 0.0));
}
