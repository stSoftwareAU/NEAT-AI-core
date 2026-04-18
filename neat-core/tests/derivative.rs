//! Derivative tests (moved from `src/derivative.rs`).

use neat_core::squash::LEAKY_RELU_ALPHA;
use neat_core::{SquashType, apply_derivative, apply_derivative_simd_4way};

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

// Issue #1213 - SIMD batch derivative tests
#[test]
fn test_derivative_simd_4way_identity() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Identity, 0.0, 5.0, -5.0, 1.0);
    assert_eq!(d0, 1.0);
    assert_eq!(d1, 1.0);
    assert_eq!(d2, 1.0);
    assert_eq!(d3, 1.0);
}

#[test]
fn test_derivative_simd_4way_relu() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Relu, 1.0, -1.0, 0.0, 5.0);
    assert_eq!(d0, 1.0);
    assert_eq!(d1, 0.0);
    assert_eq!(d2, 0.0);
    assert_eq!(d3, 1.0);
}

#[test]
fn test_derivative_simd_4way_leaky_relu() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::LeakyRelu, 1.0, -1.0, 0.0, -5.0);
    assert_eq!(d0, 1.0);
    assert_eq!(d1, LEAKY_RELU_ALPHA);
    assert_eq!(d2, 1.0);
    assert_eq!(d3, LEAKY_RELU_ALPHA);
}

#[test]
fn test_derivative_simd_4way_logistic() {
    // At x=0, sigmoid(0) = 0.5, so derivative = 0.5 * 0.5 = 0.25
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Logistic, 0.0, 0.0, 0.0, 0.0);
    assert!((d0 - 0.25).abs() < 1e-6);
    assert!((d1 - 0.25).abs() < 1e-6);
    assert!((d2 - 0.25).abs() < 1e-6);
    assert!((d3 - 0.25).abs() < 1e-6);
}

#[test]
fn test_derivative_simd_4way_tanh() {
    // At x=0, tanh(0) = 0, so derivative = 1 - 0^2 = 1
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Tanh, 0.0, 0.0, 0.0, 0.0);
    assert!((d0 - 1.0).abs() < 1e-6);
    assert!((d1 - 1.0).abs() < 1e-6);
    assert!((d2 - 1.0).abs() < 1e-6);
    assert!((d3 - 1.0).abs() < 1e-6);
}

#[test]
fn test_derivative_simd_4way_hard_tanh() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::HardTanh, 0.5, -2.0, 2.0, 0.0);
    assert_eq!(d0, 1.0); // Inside [-1, 1]
    assert_eq!(d1, 0.0); // Outside range
    assert_eq!(d2, 0.0); // Outside range
    assert_eq!(d3, 1.0); // Inside [-1, 1]
}

#[test]
fn test_derivative_simd_4way_complement() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Complement, 0.0, 5.0, -5.0, 1.0);
    assert_eq!(d0, -1.0);
    assert_eq!(d1, -1.0);
    assert_eq!(d2, -1.0);
    assert_eq!(d3, -1.0);
}

#[test]
fn test_derivative_simd_4way_square() {
    // (x^2)' = 2x
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Square, 0.0, 2.0, -3.0, 1.0);
    assert_eq!(d0, 0.0);
    assert_eq!(d1, 4.0);
    assert_eq!(d2, -6.0);
    assert_eq!(d3, 2.0);
}

#[test]
fn test_derivative_simd_4way_cube() {
    // (x^3)' = 3x^2
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Cube, 0.0, 2.0, -2.0, 1.0);
    assert_eq!(d0, 0.0);
    assert_eq!(d1, 12.0);
    assert_eq!(d2, 12.0);
    assert_eq!(d3, 3.0);
}

#[test]
fn test_derivative_simd_4way_absolute() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Absolute, 1.0, -1.0, 0.0, 5.0);
    assert_eq!(d0, 1.0);
    assert_eq!(d1, -1.0);
    assert_eq!(d2, 0.0);
    assert_eq!(d3, 1.0);
}

#[test]
fn test_derivative_simd_4way_arctan() {
    // atan'(x) = 1/(1+x^2)
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::ArcTan, 0.0, 1.0, -1.0, 2.0);
    assert!((d0 - 1.0).abs() < 1e-6); // 1/(1+0) = 1
    assert!((d1 - 0.5).abs() < 1e-6); // 1/(1+1) = 0.5
    assert!((d2 - 0.5).abs() < 1e-6); // 1/(1+1) = 0.5
    assert!((d3 - 0.2).abs() < 1e-6); // 1/(1+4) = 0.2
}

#[test]
fn test_derivative_simd_4way_softsign() {
    // f(x) = x / (1 + |x|), f'(x) = 1 / (1 + |x|)^2
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Softsign, 0.0, 1.0, -1.0, 3.0);
    assert!((d0 - 1.0).abs() < 1e-6); // 1/(1+0)^2 = 1
    assert!((d1 - 0.25).abs() < 1e-6); // 1/(1+1)^2 = 0.25
    assert!((d2 - 0.25).abs() < 1e-6); // 1/(1+1)^2 = 0.25
    assert!((d3 - 0.0625).abs() < 1e-6); // 1/(1+3)^2 = 0.0625
}

#[test]
fn test_derivative_simd_4way_relu6() {
    let (d0, d1, d2, d3) = apply_derivative_simd_4way(SquashType::Relu6, 3.0, -1.0, 7.0, 0.5);
    assert_eq!(d0, 1.0); // Inside (0, 6)
    assert_eq!(d1, 0.0); // Below 0
    assert_eq!(d2, 0.0); // Above 6
    assert_eq!(d3, 1.0); // Inside (0, 6)
}

#[test]
fn test_derivative_simd_4way_matches_scalar() {
    // Test that SIMD results match scalar results for various squash types
    let test_values = [(-2.0_f32, -1.0_f32, 0.5_f32, 2.0_f32)];

    for squash_type in [
        SquashType::Identity,
        SquashType::Relu,
        SquashType::Relu6,
        SquashType::LeakyRelu,
        SquashType::Selu,
        SquashType::Elu,
        SquashType::Logistic,
        SquashType::Tanh,
        SquashType::HardTanh,
        SquashType::Softsign,
        SquashType::Complement,
        SquashType::Absolute,
        SquashType::Square,
        SquashType::Cube,
        SquashType::ArcTan,
        SquashType::Gelu,
        SquashType::Swish,
    ] {
        for (x0, x1, x2, x3) in test_values {
            let (d0, d1, d2, d3) = apply_derivative_simd_4way(squash_type, x0, x1, x2, x3);
            let s0 = apply_derivative(squash_type, x0);
            let s1 = apply_derivative(squash_type, x1);
            let s2 = apply_derivative(squash_type, x2);
            let s3 = apply_derivative(squash_type, x3);

            assert!(
                (d0 - s0).abs() < 1e-5,
                "{:?}: d0={} != s0={}",
                squash_type,
                d0,
                s0
            );
            assert!(
                (d1 - s1).abs() < 1e-5,
                "{:?}: d1={} != s1={}",
                squash_type,
                d1,
                s1
            );
            assert!(
                (d2 - s2).abs() < 1e-5,
                "{:?}: d2={} != s2={}",
                squash_type,
                d2,
                s2
            );
            assert!(
                (d3 - s3).abs() < 1e-5,
                "{:?}: d3={} != s3={}",
                squash_type,
                d3,
                s3
            );
        }
    }
}
