//! Squash activation tests (moved from `src/squash.rs`).

use neat_core::squash::{SquashType, apply_squash, apply_squash_f64};

#[test]
fn test_relu() {
    assert_eq!(apply_squash(SquashType::Relu, 1.0), 1.0);
    assert_eq!(apply_squash(SquashType::Relu, -1.0), 0.0);
    assert_eq!(apply_squash(SquashType::Relu, 0.0), 0.0);
}

#[test]
fn test_tanh() {
    let result = apply_squash(SquashType::Tanh, 0.0);
    assert!((result - 0.0).abs() < 1e-6);
}

#[test]
fn test_logistic() {
    let result = apply_squash(SquashType::Logistic, 0.0);
    assert!((result - 0.5).abs() < 1e-6);
}

// --- Output clamping tests (#2151) ---

#[test]
fn test_tan_clamped_f32() {
    // Near asymptote: should be clamped to ±1000
    let near_pi_half = std::f32::consts::FRAC_PI_2 - 1e-7;
    let result = apply_squash(SquashType::Tan, near_pi_half);
    assert!(result.is_finite(), "TAN f32 near asymptote not finite");
    assert!(
        result.abs() <= 1000.0,
        "TAN f32 near asymptote exceeded 1000: {}",
        result
    );
}

#[test]
fn test_tan_clamped_f64() {
    let near_pi_half = std::f64::consts::FRAC_PI_2 - 1e-15;
    let result = apply_squash_f64(SquashType::Tan, near_pi_half);
    assert!(result.is_finite(), "TAN f64 near asymptote not finite");
    assert!(
        result.abs() <= 1000.0,
        "TAN f64 near asymptote exceeded 1000: {}",
        result
    );
}

#[test]
fn test_square_clamped_f32() {
    let result = apply_squash(SquashType::Square, 1e15);
    assert!(result.is_finite(), "SQUARE f32 large input not finite");
    assert!(result <= 1e6, "SQUARE f32 exceeded 1e6: {}", result);
}

#[test]
fn test_square_clamped_f64() {
    let result = apply_squash_f64(SquashType::Square, 1e30);
    assert!(result.is_finite(), "SQUARE f64 large input not finite");
    assert!(result <= 1e6, "SQUARE f64 exceeded 1e6: {}", result);
}

#[test]
fn test_cube_clamped_f32() {
    let result = apply_squash(SquashType::Cube, 1e10);
    assert!(result.is_finite(), "CUBE f32 large positive not finite");
    assert!(result <= 1e6, "CUBE f32 positive exceeded 1e6: {}", result);

    let result_neg = apply_squash(SquashType::Cube, -1e10);
    assert!(result_neg.is_finite(), "CUBE f32 large negative not finite");
    assert!(
        result_neg >= -1e6,
        "CUBE f32 negative below -1e6: {}",
        result_neg
    );
}

#[test]
fn test_cube_clamped_f64() {
    let result = apply_squash_f64(SquashType::Cube, 1e30);
    assert!(result.is_finite(), "CUBE f64 large positive not finite");
    assert!(result <= 1e6, "CUBE f64 positive exceeded 1e6: {}", result);

    let result_neg = apply_squash_f64(SquashType::Cube, -1e30);
    assert!(result_neg.is_finite(), "CUBE f64 large negative not finite");
    assert!(
        result_neg >= -1e6,
        "CUBE f64 negative below -1e6: {}",
        result_neg
    );
}
