//! Range validation tests (moved from `src/range.rs`).

use neat_core::range::{apply_limit_range_bounds, apply_limit_range_f64};
use neat_core::{SquashType, apply_get_range, apply_limit_range, apply_validate_range};

/// Every squash type, so the hoisted-bounds helper can be checked against the
/// squash-dispatching [`apply_limit_range`] for all 37 ranges (Issue #245).
const ALL_SQUASH: [SquashType; 37] = [
    SquashType::Identity,
    SquashType::Logistic,
    SquashType::Tanh,
    SquashType::Relu,
    SquashType::LeakyRelu,
    SquashType::Sine,
    SquashType::Cosine,
    SquashType::Tan,
    SquashType::ArcTan,
    SquashType::Gaussian,
    SquashType::BentIdentity,
    SquashType::Bipolar,
    SquashType::BipolarSigmoid,
    SquashType::HardTanh,
    SquashType::Absolute,
    SquashType::Relu6,
    SquashType::Selu,
    SquashType::Gelu,
    SquashType::Swish,
    SquashType::Mish,
    SquashType::Elu,
    SquashType::Softsign,
    SquashType::Softplus,
    SquashType::Square,
    SquashType::Cube,
    SquashType::Sqrt,
    SquashType::Exponential,
    SquashType::LogSigmoid,
    SquashType::StdInverse,
    SquashType::Complement,
    SquashType::Step,
    SquashType::Isru,
    SquashType::Minimum,
    SquashType::Maximum,
    SquashType::If,
    SquashType::Hypotenuse,
    SquashType::HypotenuseV2,
];

#[test]
fn limit_range_bounds_matches_apply_limit_range() {
    // The Issue #245 hoist resolves `(low, high)` once per neuron and clamps
    // every lane through `apply_limit_range_bounds`. That must be byte-for-byte
    // identical to the per-value `apply_limit_range` for every squash type and
    // every edge value, or the batched scoring numerics would change.
    let values = [
        0.0f32,
        -0.5,
        0.5,
        1.5,
        -1.5,
        6.5,
        -6.5,
        1e6,
        -1e6,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    for squash in ALL_SQUASH {
        let (low, high) = apply_get_range(squash);
        for &v in &values {
            let want = apply_limit_range(squash, v);
            let got = apply_limit_range_bounds(low, high, v);
            assert_eq!(
                want.to_bits(),
                got.to_bits(),
                "{squash:?} value {v}: apply_limit_range={want} but bounds helper={got}"
            );
        }
    }
}

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
fn test_limit_range_f64_clamping() {
    // Values within range pass through unchanged.
    assert_eq!(apply_limit_range_f64(SquashType::Logistic, 0.5), 0.5);
    assert_eq!(apply_limit_range_f64(SquashType::Tanh, 0.0), 0.0);

    // Out-of-range values are clamped to the activation bounds.
    assert_eq!(apply_limit_range_f64(SquashType::Logistic, -0.5), 0.0);
    assert_eq!(apply_limit_range_f64(SquashType::Logistic, 1.5), 1.0);
    assert_eq!(apply_limit_range_f64(SquashType::Relu6, 10.0), 6.0);

    // NaN maps to 0.
    assert_eq!(apply_limit_range_f64(SquashType::Logistic, f64::NAN), 0.0);

    // Bounded ranges clamp infinities to the finite bounds.
    assert_eq!(
        apply_limit_range_f64(SquashType::Logistic, f64::INFINITY),
        1.0
    );
    assert_eq!(
        apply_limit_range_f64(SquashType::Logistic, f64::NEG_INFINITY),
        0.0
    );

    // Unbounded ranges clamp infinities to ±F32_LARGE (no overflow to inf).
    let hi = apply_limit_range_f64(SquashType::Identity, f64::INFINITY);
    assert!(hi.is_finite(), "Identity +inf should be finite");
    assert!(hi > 1e30, "Identity +inf should clamp to a large positive");
    let lo = apply_limit_range_f64(SquashType::Identity, f64::NEG_INFINITY);
    assert!(lo.is_finite(), "Identity -inf should be finite");
    assert!(lo < -1e30, "Identity -inf should clamp to a large negative");
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
