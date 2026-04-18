//! Elastic error distribution tests (moved from `src/elastic_distribution.rs`).

use neat_core::elastic_distribution::apply_distribute_elastic_error;

const PLANK_CONSTANT: f32 = 1e-12;

#[test]
fn test_prefers_higher_activation() {
    let shares =
        apply_distribute_elastic_error(10.0, &[1.0, 2.0], &[1.0, 1.0], &[1.0, 1.0], PLANK_CONSTANT);
    assert_eq!(shares.len(), 2);
    // activation² weighting: 1²=1, 2²=4
    // share0 = 10 * 1/5 = 2, share1 = 10 * 4/5 = 8
    assert!((shares[0] - 2.0).abs() < 1e-5, "share0={}", shares[0]);
    assert!((shares[1] - 8.0).abs() < 1e-5, "share1={}", shares[1]);
    assert!(
        (shares[0] + shares[1] - 10.0).abs() < 1e-5,
        "sum={}",
        shares[0] + shares[1]
    );
}

#[test]
fn test_honours_safe_zone_factor() {
    // safeZoneFactor=0 should block a link
    let shares =
        apply_distribute_elastic_error(10.0, &[1.0, 1.0], &[1.0, 0.0], &[1.0, 1.0], PLANK_CONSTANT);
    assert!((shares[0] - 10.0).abs() < 1e-5, "share0={}", shares[0]);
    assert!((shares[1]).abs() < 1e-5, "share1={}", shares[1]);
}

#[test]
fn test_weight_fallback_prefers_larger_weights() {
    // All activations zero → weight-based fallback
    let shares = apply_distribute_elastic_error(
        12.0,
        &[0.0, 0.0, 0.0],
        &[1.0, 1.0, 1.0],
        &[1.0, 2.0, 3.0],
        PLANK_CONSTANT,
    );
    assert_eq!(shares.len(), 3);
    // weight²: 1, 4, 9 → denom = 14
    let expected0 = 12.0 * (1.0 / 14.0);
    let expected1 = 12.0 * (4.0 / 14.0);
    let expected2 = 12.0 * (9.0 / 14.0);
    assert!(
        (shares[0] - expected0).abs() < 1e-4,
        "share0={}, expected={}",
        shares[0],
        expected0
    );
    assert!(
        (shares[1] - expected1).abs() < 1e-4,
        "share1={}, expected={}",
        shares[1],
        expected1
    );
    assert!(
        (shares[2] - expected2).abs() < 1e-4,
        "share2={}, expected={}",
        shares[2],
        expected2
    );
}

#[test]
fn test_equal_split_last_resort() {
    // Both activations and weights are zero
    let shares = apply_distribute_elastic_error(
        9.0,
        &[0.0, 0.0, 0.0],
        &[1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0],
        PLANK_CONSTANT,
    );
    assert_eq!(shares.len(), 3);
    for &s in &shares {
        assert!((s - 3.0).abs() < 1e-5, "share={}", s);
    }
}

#[test]
fn test_nan_error_returns_zeros() {
    let shares = apply_distribute_elastic_error(
        f32::NAN,
        &[1.0, 2.0],
        &[1.0, 1.0],
        &[1.0, 1.0],
        PLANK_CONSTANT,
    );
    assert_eq!(shares.len(), 2);
    for &s in &shares {
        assert_eq!(s, 0.0);
    }
}

#[test]
fn test_infinite_error_returns_zeros() {
    let shares =
        apply_distribute_elastic_error(f32::INFINITY, &[1.0], &[1.0], &[1.0], PLANK_CONSTANT);
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0], 0.0);
}

#[test]
fn test_empty_links() {
    let shares = apply_distribute_elastic_error(10.0, &[], &[], &[], PLANK_CONSTANT);
    assert!(shares.is_empty());
}

#[test]
fn test_single_link() {
    let shares = apply_distribute_elastic_error(5.0, &[3.0], &[1.0], &[1.0], PLANK_CONSTANT);
    assert_eq!(shares.len(), 1);
    assert!((shares[0] - 5.0).abs() < 1e-5, "share={}", shares[0]);
}

#[test]
fn test_negative_error() {
    let shares =
        apply_distribute_elastic_error(-6.0, &[1.0, 2.0], &[1.0, 1.0], &[1.0, 1.0], PLANK_CONSTANT);
    // 1²=1, 2²=4, denom=5
    assert!(
        (shares[0] - (-6.0 * 1.0 / 5.0)).abs() < 1e-5,
        "share0={}",
        shares[0]
    );
    assert!(
        (shares[1] - (-6.0 * 4.0 / 5.0)).abs() < 1e-5,
        "share1={}",
        shares[1]
    );
}

#[test]
fn test_zero_error() {
    let shares =
        apply_distribute_elastic_error(0.0, &[1.0, 2.0], &[1.0, 1.0], &[1.0, 1.0], PLANK_CONSTANT);
    for &s in &shares {
        assert_eq!(s, 0.0);
    }
}

#[test]
fn test_nan_activation_treated_as_zero() {
    let shares = apply_distribute_elastic_error(
        10.0,
        &[f32::NAN, 2.0],
        &[1.0, 1.0],
        &[1.0, 1.0],
        PLANK_CONSTANT,
    );
    // NaN activation → score = 0, so all error goes to link 1
    assert!((shares[0]).abs() < 1e-5, "share0={}", shares[0]);
    assert!((shares[1] - 10.0).abs() < 1e-5, "share1={}", shares[1]);
}

#[test]
fn test_safe_zone_clamped() {
    // safeZoneFactor > 1 should be clamped to 1
    let shares_clamped =
        apply_distribute_elastic_error(10.0, &[1.0, 1.0], &[2.0, 2.0], &[1.0, 1.0], PLANK_CONSTANT);
    let shares_normal =
        apply_distribute_elastic_error(10.0, &[1.0, 1.0], &[1.0, 1.0], &[1.0, 1.0], PLANK_CONSTANT);
    for i in 0..2 {
        assert!(
            (shares_clamped[i] - shares_normal[i]).abs() < 1e-5,
            "clamped[{}]={}, normal[{}]={}",
            i,
            shares_clamped[i],
            i,
            shares_normal[i]
        );
    }
}

#[test]
fn test_error_conservation_many_links() {
    let count = 50;
    let activations: Vec<f32> = (0..count).map(|i| (i as f32 * 0.7).sin()).collect();
    let safe_zones: Vec<f32> = (0..count)
        .map(|i| ((i as f32 * 0.3).cos() + 1.0) / 2.0)
        .collect();
    let weights: Vec<f32> = (0..count).map(|i| (i as f32 * 0.5).sin()).collect();
    let error = 42.0;

    let shares =
        apply_distribute_elastic_error(error, &activations, &safe_zones, &weights, PLANK_CONSTANT);

    let sum: f32 = shares.iter().sum();
    assert!((sum - error).abs() < 1e-3, "sum={}, error={}", sum, error);
}

#[test]
fn test_weight_fallback_negative_weights() {
    // Negative weights should use absolute value squared
    let shares = apply_distribute_elastic_error(
        10.0,
        &[0.0, 0.0],
        &[1.0, 1.0],
        &[-3.0, 3.0],
        PLANK_CONSTANT,
    );
    // Both |w|²=9, so equal split
    assert!((shares[0] - 5.0).abs() < 1e-5, "share0={}", shares[0]);
    assert!((shares[1] - 5.0).abs() < 1e-5, "share1={}", shares[1]);
}

#[test]
fn test_weight_fallback_nan_weight() {
    // NaN weight → weight score = 0
    let shares = apply_distribute_elastic_error(
        10.0,
        &[0.0, 0.0],
        &[1.0, 1.0],
        &[f32::NAN, 2.0],
        PLANK_CONSTANT,
    );
    // Only link 1 has weight score (4), so it gets all
    assert!((shares[0]).abs() < 1e-5, "share0={}", shares[0]);
    assert!((shares[1] - 10.0).abs() < 1e-5, "share1={}", shares[1]);
}
