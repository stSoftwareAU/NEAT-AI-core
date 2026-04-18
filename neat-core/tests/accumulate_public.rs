//! Batch accumulation and calculate_weight / calculate_bias tests (moved from `src/accumulate.rs`).
//! Tests that need `limit_*` or `accumulate_*_single` stay in the library crate.

use neat_core::{
    accumulate_bias_batch_4way, accumulate_weight_batch_4way, calculate_bias, calculate_weight,
};

#[test]
fn test_batch_4way_weight() {
    let weights = vec![0.5, -0.3, 1.2, 0.0];
    let targets = vec![2.0, -1.5, 0.8, 3.0];
    let acts = vec![1.0, 0.5, -0.8, 2.0];

    let result = accumulate_weight_batch_4way(&weights, &targets, &acts, 1e-7, 1.0, 1.0, 100000.0);

    assert_eq!(result.len(), 28);
    // First synapse: positive activation
    assert_eq!(result[0], 1.0); // count
    assert_eq!(result[1], 1.0); // positive activation
}

#[test]
fn test_batch_4way_bias() {
    let targets = vec![2.0, -1.5, 0.8, 3.0];
    let pres = vec![1.0, -0.5, 0.2, 2.5];
    let biases = vec![0.5, -0.3, 1.2, 0.0];

    let result = accumulate_bias_batch_4way(&targets, &pres, &biases, 1e-7, 1.0, 1.0, 10000.0);

    assert_eq!(result.len(), 12);
    // First neuron: delta=1.0, target_bias=1.5
    assert_eq!(result[0], 1.0); // count
    assert_eq!(result[1], 1.5); // total_bias
}

#[test]
fn test_calculate_weight_basic() {
    // With only positive activations
    let result = calculate_weight(
        1.0,      // count
        1.0,      // total_positive_activation
        0.0,      // total_negative_activation
        1.0,      // count_positive
        0.0,      // count_negative
        2.0,      // total_positive_adjusted_value (adjusted_weight * activation)
        0.0,      // total_negative_adjusted_value
        0.5,      // current_weight
        0.0,      // generations
        1e-7,     // plank_constant
        1.0,      // learning_rate
        1.0,      // max_weight_adj_scale
        100000.0, // limit_weight_scale
        0.0,      // l1_weight_decay
        0.0,      // l2_weight_decay
    );

    assert!(result.is_finite());
}

#[test]
fn test_calculate_weight_with_l2_decay() {
    // Issue #1953: calculate_weight should apply L2 regularisation
    let without_decay = calculate_weight(
        1.0, 1.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.5, 0.0, 1e-7, 1.0, 1.0, 100000.0, 0.0, 0.0,
    );
    let with_decay = calculate_weight(
        1.0, 1.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.5, 0.0, 1e-7, 1.0, 1.0, 100000.0, 0.0, 0.01,
    );
    // L2 decay should produce a smaller absolute result
    assert!(with_decay.abs() <= without_decay.abs());
}

#[test]
fn test_calculate_bias_basic() {
    let result = calculate_bias(
        1.0,     // count
        1.5,     // total_adjusted_bias
        0.5,     // current_bias
        false,   // no_change
        0.0,     // generations
        1e-7,    // plank_constant
        1.0,     // learning_rate
        1.0,     // max_bias_adj_scale
        10000.0, // limit_bias_scale
        0.0,     // l1_bias_decay
        0.0,     // l2_bias_decay
    );

    assert!(result.is_finite());
}

#[test]
fn test_calculate_bias_with_l2_decay() {
    // Issue #1953: calculate_bias should apply L2 regularisation
    let without_decay =
        calculate_bias(1.0, 1.5, 0.5, false, 0.0, 1e-7, 1.0, 1.0, 10000.0, 0.0, 0.0);
    let with_decay = calculate_bias(
        1.0, 1.5, 0.5, false, 0.0, 1e-7, 1.0, 1.0, 10000.0, 0.0, 0.01,
    );
    assert!(with_decay.abs() <= without_decay.abs());
}

#[test]
fn test_calculate_bias_no_change() {
    let result = calculate_bias(
        10.0, 15.0, 0.5, true, 5.0, 1e-7, 1.0, 1.0, 10000.0, 0.0, 0.0,
    );
    assert_eq!(result, 0.5); // Should return current_bias
}

#[test]
fn test_calculate_bias_zero_count() {
    let result = calculate_bias(0.0, 0.0, 0.5, false, 5.0, 1e-7, 1.0, 1.0, 10000.0, 0.0, 0.0);
    assert_eq!(result, 0.5); // Should return current_bias
}
