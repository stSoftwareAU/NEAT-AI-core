//! Unsquash tests (moved from `src/unsquash.rs`).

use neat_core::{SquashType, apply_squash, apply_unsquash};

#[test]
fn test_unsquash_identity() {
    // Identity: f^(-1)(y) = y
    assert_eq!(apply_unsquash(SquashType::Identity, 0.0, 0.0), 0.0);
    assert_eq!(apply_unsquash(SquashType::Identity, 5.0, 0.0), 5.0);
    assert_eq!(apply_unsquash(SquashType::Identity, -5.0, 0.0), -5.0);
}

#[test]
fn test_unsquash_relu() {
    // ReLU: f^(-1)(y) = y for y > 0, else hint
    assert_eq!(apply_unsquash(SquashType::Relu, 5.0, 0.0), 5.0);
    assert_eq!(apply_unsquash(SquashType::Relu, 0.0, -3.0), -3.0);
    assert_eq!(apply_unsquash(SquashType::Relu, 0.0, f32::NAN), 0.0);
}

#[test]
fn test_unsquash_leaky_relu() {
    // LeakyReLU: f^(-1)(y) = y >= 0 ? y : y / alpha
    assert_eq!(apply_unsquash(SquashType::LeakyRelu, 1.0, 0.0), 1.0);
    // For y = -0.01, x = -0.01 / 0.01 = -1.0
    let result = apply_unsquash(SquashType::LeakyRelu, -0.01, 0.0);
    assert!((result - (-1.0)).abs() < 1e-5);
}

#[test]
fn test_unsquash_logistic() {
    // Logistic: f^(-1)(y) = log(y / (1 - y))
    // At y = 0.5, x = log(0.5 / 0.5) = log(1) = 0
    let result = apply_unsquash(SquashType::Logistic, 0.5, 0.0);
    assert!(result.abs() < 1e-5);
}

#[test]
fn test_unsquash_tanh() {
    // TANH: f^(-1)(y) = 0.5 * log((1 + y) / (1 - y))
    // At y = 0, x = 0.5 * log(1/1) = 0
    let result = apply_unsquash(SquashType::Tanh, 0.0, 0.0);
    assert!(result.abs() < 1e-5);
}

#[test]
fn test_unsquash_complement() {
    // Complement: f^(-1)(y) = 1 - y
    assert_eq!(apply_unsquash(SquashType::Complement, 0.0, 0.0), 1.0);
    assert_eq!(apply_unsquash(SquashType::Complement, 0.5, 0.0), 0.5);
    assert_eq!(apply_unsquash(SquashType::Complement, 1.0, 0.0), 0.0);
}

#[test]
fn test_unsquash_cube() {
    // Cube: f^(-1)(y) = cbrt(y)
    assert_eq!(apply_unsquash(SquashType::Cube, 0.0, 0.0), 0.0);
    let result = apply_unsquash(SquashType::Cube, 8.0, 0.0);
    assert!((result - 2.0).abs() < 1e-5);
    let result2 = apply_unsquash(SquashType::Cube, -8.0, 0.0);
    assert!((result2 - (-2.0)).abs() < 1e-5);
}

#[test]
fn test_unsquash_square() {
    // Square: f^(-1)(y) = +/-sqrt(y) based on hint
    let result = apply_unsquash(SquashType::Square, 4.0, 2.0);
    assert!((result - 2.0).abs() < 1e-5);
    let result2 = apply_unsquash(SquashType::Square, 4.0, -2.0);
    assert!((result2 - (-2.0)).abs() < 1e-5);
}

#[test]
fn test_unsquash_roundtrip() {
    // Test roundtrip: squash(x) -> unsquash(activation, x) should give x back
    let test_values = [-2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0];

    for &x in &test_values {
        // Identity
        let activation = apply_squash(SquashType::Identity, x);
        let recovered = apply_unsquash(SquashType::Identity, activation, x);
        assert!(
            (recovered - x).abs() < 1e-5,
            "Identity roundtrip failed for x={}",
            x
        );

        // LeakyReLU
        let activation = apply_squash(SquashType::LeakyRelu, x);
        let recovered = apply_unsquash(SquashType::LeakyRelu, activation, x);
        assert!(
            (recovered - x).abs() < 1e-4,
            "LeakyReLU roundtrip failed for x={}",
            x
        );

        // Complement
        let activation = apply_squash(SquashType::Complement, x);
        let recovered = apply_unsquash(SquashType::Complement, activation, x);
        assert!(
            (recovered - x).abs() < 1e-5,
            "Complement roundtrip failed for x={}",
            x
        );

        // Cube
        let activation = apply_squash(SquashType::Cube, x);
        let recovered = apply_unsquash(SquashType::Cube, activation, x);
        assert!(
            (recovered - x).abs() < 1e-5,
            "Cube roundtrip failed for x={}",
            x
        );
    }
}

#[test]
fn test_unsquash_mish_roundtrip() {
    // Mish is non-invertible (it dips below zero around x = -1.19), so we assert
    // the achievable invariant: squash(unsquash(y)) approx y, with the raw x as
    // the hint to steer Newton-Raphson onto the correct branch. Issue #157.
    let xs = [-3.0_f32, -1.5, -0.5, 0.0, 0.25, 0.42, 1.0, 2.5, 5.0, 8.0];
    for &x in &xs {
        let activation = apply_squash(SquashType::Mish, x);
        let recovered = apply_unsquash(SquashType::Mish, activation, x);
        let reactivation = apply_squash(SquashType::Mish, recovered);
        assert!(
            (reactivation - activation).abs() < 1e-3,
            "Mish roundtrip failed for x={x}: activation={activation}, recovered={recovered}, reactivation={reactivation}"
        );
    }
}

#[test]
fn test_unsquash_gelu_roundtrip() {
    // Gelu is non-invertible near its negative dip, so assert squash(unsquash(y))
    // approx y using the raw x as the Newton-Raphson hint. Issue #157.
    let xs = [-3.0_f32, -1.0, -0.5, 0.25, 0.42, 1.0, 2.5, 5.0];
    for &x in &xs {
        let activation = apply_squash(SquashType::Gelu, x);
        let recovered = apply_unsquash(SquashType::Gelu, activation, x);
        let reactivation = apply_squash(SquashType::Gelu, recovered);
        assert!(
            (reactivation - activation).abs() < 1e-3,
            "Gelu roundtrip failed for x={x}: activation={activation}, recovered={recovered}, reactivation={reactivation}"
        );
    }
}

#[test]
fn test_unsquash_aggregate_functions() {
    // Aggregate functions return hint if provided
    assert_eq!(apply_unsquash(SquashType::Minimum, 1.0, 42.0), 42.0);
    assert_eq!(apply_unsquash(SquashType::Maximum, 1.0, 42.0), 42.0);
    assert_eq!(apply_unsquash(SquashType::If, 1.0, 42.0), 42.0);
}
