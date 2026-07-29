//! Derivative tests (moved from `src/derivative.rs`).

use neat_core::squash::LEAKY_RELU_ALPHA;
use neat_core::{SquashType, apply_derivative};

#[test]
fn test_derivative_identity() {
    assert_eq!(apply_derivative(SquashType::Identity, 0.0), 1.0);
    assert_eq!(apply_derivative(SquashType::Identity, 5.0), 1.0);
    assert_eq!(apply_derivative(SquashType::Identity, -5.0), 1.0);
}

#[test]
fn test_derivative_relu() {
    assert_eq!(apply_derivative(SquashType::Relu, 1.0), 1.0);
    assert_eq!(apply_derivative(SquashType::Relu, -1.0), 0.0);
    assert_eq!(apply_derivative(SquashType::Relu, 0.0), 0.0);
}

#[test]
fn test_derivative_leaky_relu() {
    assert_eq!(apply_derivative(SquashType::LeakyRelu, 1.0), 1.0);
    assert_eq!(
        apply_derivative(SquashType::LeakyRelu, -1.0),
        LEAKY_RELU_ALPHA
    );
    assert_eq!(apply_derivative(SquashType::LeakyRelu, 0.0), 1.0);
}

#[test]
fn test_derivative_logistic() {
    // At x=0, sigmoid(0) = 0.5, so derivative = 0.5 * 0.5 = 0.25
    let result = apply_derivative(SquashType::Logistic, 0.0);
    assert!((result - 0.25).abs() < 1e-6);
}

#[test]
fn test_derivative_tanh() {
    // At x=0, tanh(0) = 0, so derivative = 1 - 0^2 = 1
    let result = apply_derivative(SquashType::Tanh, 0.0);
    assert!((result - 1.0).abs() < 1e-6);
}

#[test]
fn test_derivative_sine() {
    // sin'(x) = cos(x)
    let result = apply_derivative(SquashType::Sine, 0.0);
    assert!((result - 1.0).abs() < 1e-6); // cos(0) = 1
}

#[test]
fn test_derivative_cosine() {
    // cos'(x) = -sin(x)
    let result = apply_derivative(SquashType::Cosine, 0.0);
    assert!((result - 0.0).abs() < 1e-6); // -sin(0) = 0
}

#[test]
fn test_derivative_square() {
    // (x^2)' = 2x
    assert_eq!(apply_derivative(SquashType::Square, 0.0), 0.0);
    assert_eq!(apply_derivative(SquashType::Square, 2.0), 4.0);
    assert_eq!(apply_derivative(SquashType::Square, -3.0), -6.0);
}

#[test]
fn test_derivative_cube() {
    // (x^3)' = 3x^2
    assert_eq!(apply_derivative(SquashType::Cube, 0.0), 0.0);
    assert_eq!(apply_derivative(SquashType::Cube, 2.0), 12.0);
    assert_eq!(apply_derivative(SquashType::Cube, -2.0), 12.0);
}

#[test]
fn test_derivative_complement() {
    // (1-x)' = -1
    assert_eq!(apply_derivative(SquashType::Complement, 0.0), -1.0);
    assert_eq!(apply_derivative(SquashType::Complement, 5.0), -1.0);
}

#[test]
fn test_derivative_absolute() {
    assert_eq!(apply_derivative(SquashType::Absolute, 1.0), 1.0);
    assert_eq!(apply_derivative(SquashType::Absolute, -1.0), -1.0);
    assert_eq!(apply_derivative(SquashType::Absolute, 0.0), 0.0);
}

#[test]
fn test_derivative_arctan() {
    // atan'(x) = 1/(1+x^2)
    let result = apply_derivative(SquashType::ArcTan, 0.0);
    assert!((result - 1.0).abs() < 1e-6); // 1/(1+0) = 1

    let result2 = apply_derivative(SquashType::ArcTan, 1.0);
    assert!((result2 - 0.5).abs() < 1e-6); // 1/(1+1) = 0.5
}

#[test]
fn test_derivative_aggregate_functions() {
    // Aggregate functions return 0
    assert_eq!(apply_derivative(SquashType::Minimum, 1.0), 0.0);
    assert_eq!(apply_derivative(SquashType::Maximum, 1.0), 0.0);
    assert_eq!(apply_derivative(SquashType::If, 1.0), 0.0);
}
