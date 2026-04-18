//! Range validation tests (moved from `src/range.rs`).

use neat_core::{SquashType, apply_get_range, apply_limit_range, apply_validate_range};

#[test]
fn test_get_range_bounded() {
    // LOGISTIC [0, 1]
    let (low, high) = apply_get_range(SquashType::Logistic);
    assert_eq!(low, 0.0, "Logistic low should be 0");
    assert_eq!(high, 1.0, "Logistic high should be 1");

    // TANH [-1, 1]
    let (low, high) = apply_get_range(SquashType::Tanh);
    assert_eq!(low, -1.0, "Tanh low should be -1");
    assert_eq!(high, 1.0, "Tanh high should be 1");

    // ReLU6 [0, 6]
    let (low, high) = apply_get_range(SquashType::Relu6);
    assert_eq!(low, 0.0, "ReLU6 low should be 0");
    assert_eq!(high, 6.0, "ReLU6 high should be 6");

    // GAUSSIAN [0, 1]
    let (low, high) = apply_get_range(SquashType::Gaussian);
    assert_eq!(low, 0.0, "Gaussian low should be 0");
    assert_eq!(high, 1.0, "Gaussian high should be 1");
}

#[test]
fn test_get_range_unbounded() {
    // IDENTITY - unbounded
    let (low, high) = apply_get_range(SquashType::Identity);
    assert!(low < -1e30, "Identity low should be very negative");
    assert!(high > 1e30, "Identity high should be very positive");

    // ReLU [0, inf)
    let (low, high) = apply_get_range(SquashType::Relu);
    assert_eq!(low, 0.0, "ReLU low should be 0");
    assert!(high > 1e30, "ReLU high should be very positive");
}

#[test]
fn test_get_range_special_bounds() {
    // ArcTan [-pi/2, pi/2]
    let (low, high) = apply_get_range(SquashType::ArcTan);
    assert!(
        (low - (-std::f32::consts::FRAC_PI_2)).abs() < 1e-5,
        "ArcTan low should be -pi/2"
    );
    assert!(
        (high - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
        "ArcTan high should be pi/2"
    );

    // LogSigmoid (-inf, 0]
    let (low, high) = apply_get_range(SquashType::LogSigmoid);
    assert!(low < -1e30, "LogSigmoid low should be very negative");
    assert_eq!(high, 0.0, "LogSigmoid high should be 0");

    // ELU [-1, inf)
    let (low, high) = apply_get_range(SquashType::Elu);
    assert_eq!(low, -1.0, "ELU low should be -1");
    assert!(high > 1e30, "ELU high should be very positive");

    // GELU has empirically determined minimum
    let (low, _) = apply_get_range(SquashType::Gelu);
    assert!(
        (low - (-0.17)).abs() < 0.01,
        "GELU low should be approximately -0.17"
    );
}

#[test]
fn test_validate_range_valid() {
    // Valid values within range
    assert!(
        apply_validate_range(SquashType::Logistic, 0.5),
        "0.5 should be valid for Logistic"
    );
    assert!(
        apply_validate_range(SquashType::Logistic, 0.0),
        "0.0 should be valid for Logistic"
    );
    assert!(
        apply_validate_range(SquashType::Logistic, 1.0),
        "1.0 should be valid for Logistic"
    );

    assert!(
        apply_validate_range(SquashType::Tanh, 0.0),
        "0.0 should be valid for Tanh"
    );
    assert!(
        apply_validate_range(SquashType::Tanh, -0.5),
        "-0.5 should be valid for Tanh"
    );

    assert!(
        apply_validate_range(SquashType::Relu, 0.0),
        "0.0 should be valid for ReLU"
    );
    assert!(
        apply_validate_range(SquashType::Relu, 100.0),
        "100.0 should be valid for ReLU"
    );

    // Unbounded functions accept any finite value
    assert!(
        apply_validate_range(SquashType::Identity, -1000.0),
        "-1000 should be valid for Identity"
    );
    assert!(
        apply_validate_range(SquashType::Identity, 1000.0),
        "1000 should be valid for Identity"
    );
}

#[test]
fn test_validate_range_invalid() {
    // Out of range values
    assert!(
        !apply_validate_range(SquashType::Logistic, -0.1),
        "-0.1 should be invalid for Logistic"
    );
    assert!(
        !apply_validate_range(SquashType::Logistic, 1.1),
        "1.1 should be invalid for Logistic"
    );

    assert!(
        !apply_validate_range(SquashType::Tanh, -1.5),
        "-1.5 should be invalid for Tanh"
    );
    assert!(
        !apply_validate_range(SquashType::Tanh, 1.5),
        "1.5 should be invalid for Tanh"
    );

    assert!(
        !apply_validate_range(SquashType::Relu, -1.0),
        "-1.0 should be invalid for ReLU"
    );

    // NaN and Infinity
    assert!(
        !apply_validate_range(SquashType::Logistic, f32::NAN),
        "NaN should be invalid"
    );
    assert!(
        !apply_validate_range(SquashType::Tanh, f32::INFINITY),
        "Infinity should be invalid"
    );
    assert!(
        !apply_validate_range(SquashType::Relu, f32::NEG_INFINITY),
        "-Infinity should be invalid"
    );
}

#[test]
fn test_limit_range_clamping() {
    // Values within range should pass through
    assert_eq!(apply_limit_range(SquashType::Logistic, 0.5), 0.5);
    assert_eq!(apply_limit_range(SquashType::Tanh, 0.0), 0.0);

    // Values outside range should be clamped
    assert_eq!(apply_limit_range(SquashType::Logistic, -0.5), 0.0);
    assert_eq!(apply_limit_range(SquashType::Logistic, 1.5), 1.0);
    assert_eq!(apply_limit_range(SquashType::Tanh, -2.0), -1.0);
    assert_eq!(apply_limit_range(SquashType::Tanh, 2.0), 1.0);
    assert_eq!(apply_limit_range(SquashType::Relu6, 10.0), 6.0);
    assert_eq!(apply_limit_range(SquashType::Relu6, -1.0), 0.0);

    // Infinity should be clamped to bounds
    assert_eq!(apply_limit_range(SquashType::Logistic, f32::INFINITY), 1.0);
    assert_eq!(
        apply_limit_range(SquashType::Logistic, f32::NEG_INFINITY),
        0.0
    );
    assert_eq!(apply_limit_range(SquashType::Tanh, f32::INFINITY), 1.0);
    assert_eq!(apply_limit_range(SquashType::Tanh, f32::NEG_INFINITY), -1.0);

    // NaN should return 0
    assert_eq!(apply_limit_range(SquashType::Logistic, f32::NAN), 0.0);
}

#[test]
fn test_range_aggregate_functions() {
    // Aggregate functions should have unbounded ranges
    let (low, high) = apply_get_range(SquashType::Minimum);
    assert!(low < -1e30, "Minimum low should be very negative");
    assert!(high > 1e30, "Minimum high should be very positive");

    let (low, high) = apply_get_range(SquashType::Maximum);
    assert!(low < -1e30, "Maximum low should be very negative");
    assert!(high > 1e30, "Maximum high should be very positive");

    let (low, high) = apply_get_range(SquashType::If);
    assert!(low < -1e30, "If low should be very negative");
    assert!(high > 1e30, "If high should be very positive");
}
