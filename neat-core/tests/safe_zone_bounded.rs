//! Behavioural coverage for the bounded safe-zone rule shared by twelve
//! `SquashType` arms of `apply_safe_zone_adjustment` (Issue #442).
//!
//! Each arm below differs only in its safe band `[safe_min, safe_max]` and the
//! fade width either side of it. These tests assert the observable factor
//! returned for every branch of that rule, so the shared policy stays pinned
//! regardless of how the arms are implemented.

use neat_core::{SquashType, apply_safe_zone_adjustment};

/// `(squash, safe_min, safe_max, fade)` for every arm using the bounded rule.
const BOUNDED_ARMS: &[(SquashType, f32, f32, f32)] = &[
    (SquashType::LeakyRelu, -50.0, 50.0, 20.0),
    (SquashType::Selu, -10.0, 10.0, 10.0),
    (SquashType::Elu, -10.0, 10.0, 10.0),
    (SquashType::Softsign, -10.0, 10.0, 10.0),
    (SquashType::Softplus, -10.0, 20.0, 10.0),
    (SquashType::Swish, -10.0, 10.0, 10.0),
    (SquashType::Mish, -10.0, 10.0, 10.0),
    (SquashType::Gelu, -6.0, 6.0, 10.0),
    (SquashType::StdInverse, -10.0, 10.0, 10.0),
    (SquashType::Exponential, -10.0, 30.0, 10.0),
    (SquashType::LogSigmoid, -20.0, 20.0, 10.0),
    (SquashType::Isru, -10.0, 10.0, 10.0),
];

/// A weight comfortably inside the `[1e-3, 1e3]` guard band.
const HEALTHY_WEIGHT: f32 = 1.0;

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn inside_band_with_healthy_weight_flows_fully() {
    for &(squash, safe_min, safe_max, _) in BOUNDED_ARMS {
        let centre = (safe_min + safe_max) / 2.0;
        for raw in [safe_min, centre, safe_max] {
            for error in [-1.0, 0.0, 1.0] {
                let factor = apply_safe_zone_adjustment(squash, raw, error, HEALTHY_WEIGHT);
                assert_close(factor, 1.0, &format!("{squash:?} raw={raw} error={error}"));
            }
        }
    }
}

#[test]
fn outside_band_with_worsening_error_blocks_gradient() {
    for &(squash, safe_min, safe_max, _) in BOUNDED_ARMS {
        // Below the band and the error pushes it further down.
        let below = apply_safe_zone_adjustment(squash, safe_min - 1.0, -1.0, HEALTHY_WEIGHT);
        assert_close(below, 0.0, &format!("{squash:?} below band, worsening"));

        // Above the band and the error pushes it further up.
        let above = apply_safe_zone_adjustment(squash, safe_max + 1.0, 1.0, HEALTHY_WEIGHT);
        assert_close(above, 0.0, &format!("{squash:?} above band, worsening"));
    }
}

#[test]
fn inside_band_defers_to_a_correcting_tiny_weight() {
    for &(squash, safe_min, safe_max, _) in BOUNDED_ARMS {
        let centre = (safe_min + safe_max) / 2.0;
        // |weight| < 1e-3 and weight * error > 0 -- let the weight grow first.
        let factor = apply_safe_zone_adjustment(squash, centre, 1.0, 1e-4);
        assert_close(factor, 0.0, &format!("{squash:?} tiny weight correcting"));

        // Same tiny weight but the error is not correcting it -- gradient flows.
        let factor = apply_safe_zone_adjustment(squash, centre, -1.0, 1e-4);
        assert_close(
            factor,
            1.0,
            &format!("{squash:?} tiny weight not correcting"),
        );
    }
}

#[test]
fn inside_band_defers_to_a_correcting_huge_weight() {
    for &(squash, safe_min, safe_max, _) in BOUNDED_ARMS {
        let centre = (safe_min + safe_max) / 2.0;
        // |weight| > 1e3 and weight * error < 0 -- let the weight shrink first.
        let factor = apply_safe_zone_adjustment(squash, centre, -1.0, 1e4);
        assert_close(factor, 0.0, &format!("{squash:?} huge weight correcting"));

        let factor = apply_safe_zone_adjustment(squash, centre, 1.0, 1e4);
        assert_close(
            factor,
            1.0,
            &format!("{squash:?} huge weight not correcting"),
        );
    }
}

#[test]
fn beyond_band_fades_linearly_to_zero() {
    for &(squash, safe_min, safe_max, fade) in BOUNDED_ARMS {
        for fraction in [0.25_f32, 0.5, 0.75, 1.0] {
            let expected = 1.0 - fraction;

            // Upper fade: error must not be worsening (error <= 0).
            let raw = safe_max + fade * fraction;
            let factor = apply_safe_zone_adjustment(squash, raw, -1.0, HEALTHY_WEIGHT);
            assert_close(factor, expected, &format!("{squash:?} upper fade at {raw}"));

            // Lower fade: error must not be worsening (error >= 0).
            let raw = safe_min - fade * fraction;
            let factor = apply_safe_zone_adjustment(squash, raw, 1.0, HEALTHY_WEIGHT);
            assert_close(factor, expected, &format!("{squash:?} lower fade at {raw}"));
        }
    }
}

#[test]
fn past_the_fade_width_no_gradient_flows() {
    for &(squash, safe_min, safe_max, fade) in BOUNDED_ARMS {
        let above = apply_safe_zone_adjustment(squash, safe_max + fade + 1.0, -1.0, HEALTHY_WEIGHT);
        assert_close(above, 0.0, &format!("{squash:?} past upper fade"));

        let below = apply_safe_zone_adjustment(squash, safe_min - fade - 1.0, 1.0, HEALTHY_WEIGHT);
        assert_close(below, 0.0, &format!("{squash:?} past lower fade"));
    }
}

#[test]
fn non_finite_raw_input_is_never_safe() {
    for &(squash, ..) in BOUNDED_ARMS {
        for raw in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let factor = apply_safe_zone_adjustment(squash, raw, 1.0, HEALTHY_WEIGHT);
            assert_close(factor, 0.0, &format!("{squash:?} raw={raw}"));
        }
    }
}
