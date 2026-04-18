//! Crate-root integration tests (same role as `tests/integration.rs` in NEAT-AI-Discovery).

mod common;

use neat_core::{
    SquashType, SynapseType, apply_calculate_error, apply_derivative, apply_get_range,
    apply_limit_range, apply_safe_zone_adjustment, apply_squash, apply_unsquash,
    apply_validate_range, compile_creature, parse_creature_json,
};

#[test]
fn parses_and_compiles_minimal_creature() {
    let creature = parse_creature_json(common::minimal_creature_json()).expect("parse JSON");
    let net = compile_creature(&creature).expect("compile creature");
    assert!(net.num_neurons() > 0);
    assert!(net.num_synapses() >= 3);
}

#[test]
fn squash_identity_round_trip() {
    assert_eq!(apply_squash(SquashType::Identity, 3.0_f32), 3.0_f32);
}

// --- Crate-root API tests (moved from `src/lib.rs`) ---

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
fn test_unsquash_identity() {
    assert_eq!(apply_unsquash(SquashType::Identity, 0.0, 0.0), 0.0);
    assert_eq!(apply_unsquash(SquashType::Identity, 5.0, 0.0), 5.0);
    assert_eq!(apply_unsquash(SquashType::Identity, -5.0, 0.0), -5.0);
}

#[test]
fn test_calculate_error() {
    let error = apply_calculate_error(SquashType::Identity, 0.5, 1.0, 0.5);
    assert!((error - 0.5).abs() < 1e-5);
}

#[test]
fn test_safe_zone_adjustment() {
    let factor = apply_safe_zone_adjustment(SquashType::Identity, 0.5, 0.1, 1.0);
    assert!((0.0..=1.0).contains(&factor));
}

#[test]
fn test_get_range() {
    let (low, high) = apply_get_range(SquashType::Logistic);
    assert!((low - 0.0).abs() < 1e-6);
    assert!((high - 1.0).abs() < 1e-6);
}

#[test]
fn test_validate_range() {
    assert!(apply_validate_range(SquashType::Logistic, 0.5));
    assert!(!apply_validate_range(SquashType::Logistic, -0.5));
}

#[test]
fn test_limit_range() {
    let result = apply_limit_range(SquashType::Logistic, 2.0);
    assert!((result - 1.0).abs() < 1e-6);
}

#[test]
fn test_version_constant() {
    // Verify the crate compiles and basic types are accessible
    let _ = SquashType::Relu;
    let _ = SynapseType::Standard;
}
