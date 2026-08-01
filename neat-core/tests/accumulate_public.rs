//! Batch accumulation and calculate_weight / calculate_bias tests (moved from `src/accumulate.rs`).
//! Tests that need `limit_*` or `accumulate_*_single` stay in the library crate.

use neat_core::{
    accumulate_bias_batch_4way, accumulate_weight_batch_4way, calculate_bias, calculate_weight,
};

/// Slots per synapse in the weight accumulation layout:
/// `[count, totalPosAct, totalNegAct, countPos, countNeg, totalPosAdj, totalNegAdj]`.
const WEIGHT_SLOTS_PER_SYNAPSE: usize = 7;
/// Slots per neuron in the bias accumulation layout:
/// `[count, totalBias, totalAdjustedBias]`.
const BIAS_SLOTS_PER_NEURON: usize = 3;
/// Items processed by the 4-way batch entry points.
const BATCH_4WAY: usize = 4;

/// Assert two f64s agree to within a tolerance that survives f64 rounding of
/// the derived expected value but still fails on a wrong formula.
#[track_caller]
fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-12,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn test_batch_4way_weight() {
    let weights = vec![0.5, -0.3, 1.2, 0.0];
    let targets = vec![2.0, -1.5, 0.8, 3.0];
    let acts = vec![1.0, 0.5, -0.8, 2.0];

    let result = accumulate_weight_batch_4way(&weights, &targets, &acts, 1e-7, 1.0, 1.0, 100000.0);

    // Flat layout: 4 synapses × 7 slots.
    assert_eq!(result.len(), BATCH_4WAY * WEIGHT_SLOTS_PER_SYNAPSE);

    // Per synapse the rule is: tmpWeight = target / activation (both already
    // above the plank constant here), then the activation's sign selects the
    // positive or the negative accumulator.
    //   [count, totalPosAct, totalNegAct, countPos, countNeg, totalPosAdj, totalNegAdj]
    let expected: [[f64; WEIGHT_SLOTS_PER_SYNAPSE]; BATCH_4WAY] = [
        // act +1.0 ⇒ tmpWeight = 2.0 / 1.0 = 2.0, posAdj = 2.0 × 1.0 = 2.0
        [1.0, 1.0, 0.0, 1.0, 0.0, 2.0, 0.0],
        // act +0.5 ⇒ tmpWeight = -1.5 / 0.5 = -3.0, posAdj = -3.0 × 0.5 = -1.5
        [1.0, 0.5, 0.0, 1.0, 0.0, -1.5, 0.0],
        // act -0.8 ⇒ tmpWeight = 0.8 / -0.8 = -1.0, negAct = |−0.8| = 0.8,
        //            negAdj = -1.0 × -0.8 = 0.8
        [1.0, 0.0, 0.8, 0.0, 1.0, 0.0, 0.8],
        // act +2.0 ⇒ tmpWeight = 3.0 / 2.0 = 1.5, posAdj = 1.5 × 2.0 = 3.0
        [1.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0],
    ];

    for (synapse, slots) in expected.iter().enumerate() {
        let base = synapse * WEIGHT_SLOTS_PER_SYNAPSE;
        for (slot, &want) in slots.iter().enumerate() {
            assert_close(
                result[base + slot],
                want,
                &format!("synapse {synapse} slot {slot}"),
            );
        }
    }
}

#[test]
fn test_batch_4way_bias() {
    let targets = vec![2.0, -1.5, 0.8, 3.0];
    let pre_activations = vec![1.0, -0.5, 0.2, 2.5];
    let biases = vec![0.5, -0.3, 1.2, 0.0];

    let result =
        accumulate_bias_batch_4way(&targets, &pre_activations, &biases, 1e-7, 1.0, 1.0, 10000.0);

    // Flat layout: 4 neurons × 3 slots.
    assert_eq!(result.len(), BATCH_4WAY * BIAS_SLOTS_PER_NEURON);

    // Per neuron: targetBias = currentBias + (targetPreActivation − preActivation),
    // accumulated raw into both the total and the adjusted total (the limit is
    // applied later, in calculate_bias). Layout: [count, totalBias, totalAdjustedBias].
    let expected: [[f64; BIAS_SLOTS_PER_NEURON]; BATCH_4WAY] = [
        // delta = 2.0 − 1.0 = 1.0  ⇒ 0.5 + 1.0 = 1.5
        [1.0, 1.5, 1.5],
        // delta = -1.5 − -0.5 = -1.0 ⇒ -0.3 + -1.0 = -1.3
        [1.0, -1.3, -1.3],
        // delta = 0.8 − 0.2 = 0.6  ⇒ 1.2 + 0.6 = 1.8
        [1.0, 1.8, 1.8],
        // delta = 3.0 − 2.5 = 0.5  ⇒ 0.0 + 0.5 = 0.5
        [1.0, 0.5, 0.5],
    ];

    for (neuron, slots) in expected.iter().enumerate() {
        let base = neuron * BIAS_SLOTS_PER_NEURON;
        for (slot, &want) in slots.iter().enumerate() {
            assert_close(
                result[base + slot],
                want,
                &format!("neuron {neuron} slot {slot}"),
            );
        }
    }
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

    // Derivation from the documented formula:
    //   positiveWeight = totalPosAdj / totalPosAct = 2.0 / 1.0 = 2.0
    //   negativeWeight = 0 (no negative activation above the plank constant)
    //   totalActivationCount = 1 + 0 = 1
    //   synapseAverageWeightTotal = 2.0 × 1 = 2.0
    //   cappedGenerations = min(0 + 1 − 1, 1 × 2) = 0 ⇒ generational term = 0
    //   averageWeight = (2.0 + 0) / (1 + 0) = 2.0
    // limit_weight(2.0, current 0.5): difference = 1.0 × (2.0 − 0.5) = 1.5,
    // which exceeds max_weight_adj_scale 1.0, so the result clamps to
    // current + 1.0 = 1.5 (inside limit_weight_scale, no decay).
    assert_close(result, 1.5, "calculate_weight");
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

    // Derivation from the documented formula:
    //   effectiveGenerations = min(0, 1 × 2) = 0
    //   totalBias = 1.5 + 0.5 × 0 = 1.5, samples = 1 + 0 = 1
    //   adjustedBias = 1.5 / 1 = 1.5
    // limit_bias(1.5, current 0.5): difference = 1.0 × (1.5 − 0.5) = 1.0, which
    // does not *exceed* max_bias_adj_scale 1.0, so no adjustment clamp; 1.5 is
    // inside limit_bias_scale 10000 and both decays are zero.
    assert_close(result, 1.5, "calculate_bias");
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
