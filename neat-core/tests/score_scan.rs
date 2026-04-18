//! Score scan tests (moved from `src/score_scan.rs`).

use neat_core::{compute_score_components, scan_max_bias, scan_max_weight};

#[test]
fn test_compute_components_basic() {
    let weights = [1.0f64, -2.0, 3.0, -4.0];
    let biases = [0.5f64, -1.5];

    let (total, count, max, second_max) = compute_score_components(&weights, &biases);

    assert_eq!(count, 6);
    assert!((total - 12.0).abs() < 1e-10, "total={}", total);
    assert!((max - 4.0).abs() < 1e-10, "max={}", max);
    assert!(
        (second_max - 3.0).abs() < 1e-10,
        "second_max={}",
        second_max
    );
}

#[test]
fn test_compute_components_empty_weights() {
    let weights: [f64; 0] = [];
    let biases = [2.0f64, -3.0];

    let (total, count, max, second_max) = compute_score_components(&weights, &biases);

    assert_eq!(count, 2);
    assert!((total - 5.0).abs() < 1e-10);
    assert!((max - 3.0).abs() < 1e-10);
    assert!((second_max - 2.0).abs() < 1e-10);
}

#[test]
fn test_compute_components_empty_biases() {
    let weights = [5.0f64, -1.0];
    let biases: [f64; 0] = [];

    let (total, count, max, second_max) = compute_score_components(&weights, &biases);

    assert_eq!(count, 2);
    assert!((total - 6.0).abs() < 1e-10);
    assert!((max - 5.0).abs() < 1e-10);
    assert!((second_max - 1.0).abs() < 1e-10);
}

#[test]
fn test_compute_components_single_element() {
    let weights = [7.0f64];
    let biases: [f64; 0] = [];

    let (total, count, max, second_max) = compute_score_components(&weights, &biases);

    assert_eq!(count, 1);
    assert!((total - 7.0).abs() < 1e-10);
    assert!((max - 7.0).abs() < 1e-10);
    assert!((second_max - 0.0).abs() < 1e-10);
}

#[test]
fn test_compute_components_many_elements() {
    let weights: Vec<f64> = (1..=10).map(|x| x as f64 * 0.5).collect();
    let biases = [-3.0f64, 2.5, -1.0, 0.5, -4.0];

    let (total, count, max, second_max) = compute_score_components(&weights, &biases);

    assert_eq!(count, 15);
    assert!((total - 38.5).abs() < 1e-10, "total={}", total);
    assert!((max - 5.0).abs() < 1e-10, "max={}", max);
    assert!(
        (second_max - 4.5).abs() < 1e-10,
        "second_max={}",
        second_max
    );
}

#[test]
fn test_scan_max_weight_exclude() {
    let weights = [1.0f64, -5.0, 3.0, -2.0];
    let biases = [0.5f64, -1.0];

    let (max, second_max) = scan_max_weight(&weights, &biases, 1, 0.1);

    assert!((max - 3.0).abs() < 1e-10, "max={}", max);
    assert!(
        (second_max - 2.0).abs() < 1e-10,
        "second_max={}",
        second_max
    );
}

#[test]
fn test_scan_max_bias_exclude() {
    let weights = [1.0f64, -2.0];
    let biases = [0.5f64, -7.0, 3.0];

    let (max, second_max) = scan_max_bias(&weights, &biases, 1, 0.2);

    assert!((max - 3.0).abs() < 1e-10, "max={}", max);
    assert!(
        (second_max - 2.0).abs() < 1e-10,
        "second_max={}",
        second_max
    );
}

#[test]
fn test_scan_max_weight_new_is_largest() {
    let weights = [1.0f64, 2.0, 3.0];
    let biases = [0.5f64];

    let (max, second_max) = scan_max_weight(&weights, &biases, 0, 10.0);

    assert!((max - 10.0).abs() < 1e-10, "max={}", max);
    assert!(
        (second_max - 3.0).abs() < 1e-10,
        "second_max={}",
        second_max
    );
}
