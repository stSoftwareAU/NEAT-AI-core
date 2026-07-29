//! Error calculation tests (moved from `src/error.rs`).

use neat_core::{SquashType, apply_calculate_error};

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
