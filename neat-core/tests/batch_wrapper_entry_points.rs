//! Behavioural coverage for the batch/wrapper public entry points whose scalar
//! inner functions are already tested (Issue #480).
//!
//! Each wrapper below exists for the WASM boundary-batching contract and carries
//! real logic beyond delegation — packing strides, flag decoding, `start_index`
//! offset arithmetic, and fallback branches. These tests assert the observable
//! output of every wrapper against its already-tested scalar sibling, so a
//! stride slip, a mis-decoded flag, or a wrong offset fails here rather than
//! silently corrupting training state.

use neat_core::accumulate::{calculate_bias_batch_4way, calculate_weight_batch_4way};
use neat_core::elastic_distribution::apply_distribute_elastic_error;
use neat_core::{
    SquashType, accumulate_bias_batch_4way, accumulate_bias_batch_8way,
    accumulate_bias_persistent_8way, apply_safe_zone_adjustment, apply_safe_zone_adjustment_batch,
    calculate_bias, calculate_weight, distribute_elastic_error, free_training_state,
    init_training_state, read_neuron_state,
};

/// The default Planck constant `distribute_elastic_error` falls back to.
const DEFAULT_PLANK: f32 = 1e-12;

const PLANK: f64 = 1e-7;
const LEARNING_RATE: f64 = 1.0;
const MAX_ADJ_SCALE: f64 = 1.0;
const LIMIT_WEIGHT_SCALE: f64 = 100_000.0;
const LIMIT_BIAS_SCALE: f64 = 10_000.0;

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: expected {expected}, got {actual}"
    );
}

// ---------------------------------------------------------------------------
// accumulate_bias_batch_8way — neat-core/src/accumulate.rs
// ---------------------------------------------------------------------------

/// Eight neurons with distinct, finite state so every packed slot is distinct.
fn bias_inputs_8() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let targets = vec![2.0, -1.5, 0.8, 3.0, -4.25, 0.125, 7.5, -0.75];
    let pre_activations = vec![1.0, -0.5, 0.2, 2.5, -1.25, 0.0625, 3.5, -0.25];
    let biases = vec![0.5, -0.3, 1.2, 0.0, 2.5, -1.75, 0.25, 4.0];
    (targets, pre_activations, biases)
}

#[test]
fn bias_batch_8way_matches_the_4way_sibling_on_both_halves() {
    let (targets, pre_activations, biases) = bias_inputs_8();

    let eight = accumulate_bias_batch_8way(
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );
    assert_eq!(eight.len(), 24, "8-way packs 3 values per neuron");

    for half in 0..2 {
        let lo = half * 4;
        let four = accumulate_bias_batch_4way(
            &targets[lo..lo + 4],
            &pre_activations[lo..lo + 4],
            &biases[lo..lo + 4],
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_BIAS_SCALE,
        );

        for slot in 0..12 {
            assert_close(
                eight[half * 12 + slot],
                four[slot],
                &format!("half {half} slot {slot}"),
            );
        }
    }
}

#[test]
fn bias_batch_8way_packs_each_neuron_at_stride_three() {
    let (targets, pre_activations, biases) = bias_inputs_8();

    let packed = accumulate_bias_batch_8way(
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );

    for i in 0..8 {
        let expected_bias = biases[i] + (targets[i] - pre_activations[i]);
        assert_close(packed[i * 3], 1.0, &format!("neuron {i} count"));
        assert_close(
            packed[i * 3 + 1],
            expected_bias,
            &format!("neuron {i} total bias"),
        );
        assert_close(
            packed[i * 3 + 2],
            expected_bias,
            &format!("neuron {i} total adjusted bias"),
        );
    }
}

#[test]
fn bias_batch_8way_zeroes_only_the_non_finite_neuron() {
    let (mut targets, pre_activations, biases) = bias_inputs_8();
    targets[5] = f64::NAN;

    let packed = accumulate_bias_batch_8way(
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );

    for slot in 0..3 {
        assert_close(packed[5 * 3 + slot], 0.0, "skipped neuron 5");
    }
    for i in 0..8 {
        if i == 5 {
            continue;
        }
        assert_close(packed[i * 3], 1.0, &format!("neuron {i} still counted"));
    }
}

// ---------------------------------------------------------------------------
// calculate_weight_batch_4way — neat-core/src/accumulate.rs
// ---------------------------------------------------------------------------

/// Four synapses × 8 state values, packed at stride 8.
fn weight_state_4() -> Vec<Vec<f64>> {
    vec![
        // count, totalPosAct, totalNegAct, countPos, countNeg, totalPosAdj, totalNegAdj, weight
        vec![4.0, 3.0, 0.0, 3.0, 0.0, 6.0, 0.0, 0.5],
        vec![6.0, 0.0, 2.5, 0.0, 4.0, 0.0, -5.0, -0.25],
        vec![2.0, 1.5, 1.5, 1.0, 1.0, 3.0, -1.5, 1.25],
        vec![9.0, 8.0, 4.0, 5.0, 3.0, 12.0, -6.0, -2.0],
    ]
}

fn calculate_weight_direct(s: &[f64]) -> f64 {
    calculate_weight(
        s[0],
        s[1],
        s[2],
        s[3],
        s[4],
        s[5],
        s[6],
        s[7],
        3.0,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_WEIGHT_SCALE,
        0.0,
        0.0,
    )
}

fn calculate_weight_batch(state: &[Vec<f64>]) -> Vec<f64> {
    let packed: Vec<f64> = state.iter().flatten().copied().collect();
    calculate_weight_batch_4way(
        &packed,
        3.0,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_WEIGHT_SCALE,
        0.0,
        0.0,
    )
}

#[test]
fn weight_batch_4way_matches_the_scalar_sibling_for_every_synapse() {
    let state = weight_state_4();
    let batch = calculate_weight_batch(&state);

    assert_eq!(batch.len(), 4, "one weight per synapse");
    for (i, s) in state.iter().enumerate() {
        assert_close(
            batch[i],
            calculate_weight_direct(s),
            &format!("synapse {i}"),
        );
    }
}

#[test]
fn weight_batch_4way_unpacks_each_synapse_independently() {
    // Changing one synapse's current weight must move only that synapse's
    // output — a stride slip would leak into a neighbour.
    let baseline = weight_state_4();
    let mut altered = baseline.clone();
    altered[2][7] = 5.0;

    let before = calculate_weight_batch(&baseline);
    let after = calculate_weight_batch(&altered);

    assert!(
        (before[2] - after[2]).abs() > 1e-9,
        "synapse 2 must react to its own weight change"
    );
    for i in [0usize, 1, 3] {
        assert_close(after[i], before[i], &format!("synapse {i} unaffected"));
    }
}

#[test]
fn weight_batch_4way_returns_the_current_weight_when_a_synapse_has_no_count() {
    let mut state = weight_state_4();
    state[1][0] = 0.0; // count = 0 → no adjustment

    let batch = calculate_weight_batch(&state);
    assert_close(batch[1], state[1][7], "zero-count synapse keeps its weight");
    assert_close(
        batch[0],
        calculate_weight_direct(&state[0]),
        "neighbour still adjusted",
    );
}

// ---------------------------------------------------------------------------
// calculate_bias_batch_4way — neat-core/src/accumulate.rs
// ---------------------------------------------------------------------------

/// Four neurons × 3 state values `[count, totalAdjustedBias, currentBias]`.
fn bias_state_4() -> Vec<Vec<f64>> {
    vec![
        vec![4.0, 6.0, 0.5],
        vec![2.0, -3.0, -0.25],
        vec![8.0, 12.0, 1.5],
        vec![1.0, 0.75, -2.0],
    ]
}

fn calculate_bias_direct(s: &[f64], no_change: bool) -> f64 {
    calculate_bias(
        s[0],
        s[1],
        s[2],
        no_change,
        3.0,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
        0.0,
        0.0,
    )
}

fn calculate_bias_batch(state: &[Vec<f64>], flags: &[u8]) -> Vec<f64> {
    let packed: Vec<f64> = state.iter().flatten().copied().collect();
    calculate_bias_batch_4way(
        &packed,
        flags,
        3.0,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
        0.0,
        0.0,
    )
}

#[test]
fn bias_batch_4way_matches_the_scalar_sibling_for_every_neuron() {
    let state = bias_state_4();
    let batch = calculate_bias_batch(&state, &[0, 0, 0, 0]);

    assert_eq!(batch.len(), 4, "one bias per neuron");
    for (i, s) in state.iter().enumerate() {
        assert_close(
            batch[i],
            calculate_bias_direct(s, false),
            &format!("neuron {i}"),
        );
    }
}

#[test]
fn bias_batch_4way_reads_every_non_zero_flag_as_no_change() {
    let state = bias_state_4();
    // 0 → false; any non-zero byte → true, including 2 and 255.
    let batch = calculate_bias_batch(&state, &[0, 1, 2, 255]);

    assert_close(
        batch[0],
        calculate_bias_direct(&state[0], false),
        "flag 0 adjusts",
    );
    for i in [1usize, 2, 3] {
        assert_close(
            batch[i],
            state[i][2],
            &format!("flag byte at {i} pins the current bias"),
        );
    }
}

#[test]
fn bias_batch_4way_unpacks_each_neuron_independently() {
    let baseline = bias_state_4();
    let mut altered = baseline.clone();
    altered[1][1] = 20.0; // total adjusted bias

    let before = calculate_bias_batch(&baseline, &[0, 0, 0, 0]);
    let after = calculate_bias_batch(&altered, &[0, 0, 0, 0]);

    assert!(
        (before[1] - after[1]).abs() > 1e-9,
        "neuron 1 must react to its own accumulator"
    );
    for i in [0usize, 2, 3] {
        assert_close(after[i], before[i], &format!("neuron {i} unaffected"));
    }
}

#[test]
fn bias_batch_4way_treats_a_missing_flag_as_change_allowed() {
    let state = bias_state_4();
    // Only two flags supplied — the absent ones must not be read as "no change".
    let batch = calculate_bias_batch(&state, &[1, 0]);

    assert_close(batch[0], state[0][2], "supplied flag honoured");
    for i in [1usize, 2, 3] {
        assert_close(
            batch[i],
            calculate_bias_direct(&state[i], false),
            &format!("neuron {i} adjusted"),
        );
    }
}

// ---------------------------------------------------------------------------
// accumulate_bias_persistent_8way — neat-core/src/training_state.rs
// ---------------------------------------------------------------------------

#[test]
fn bias_persistent_8way_writes_exactly_the_eight_slots_from_start_index() {
    const NUM_NEURONS: usize = 16;
    const START: usize = 5;

    init_training_state(0, NUM_NEURONS);

    let (targets, pre_activations, biases) = bias_inputs_8();
    accumulate_bias_persistent_8way(
        START,
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );

    let expected = accumulate_bias_batch_8way(
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );

    for neuron in 0..NUM_NEURONS {
        let state = read_neuron_state(neuron);
        if (START..START + 8).contains(&neuron) {
            let lane = neuron - START;
            for slot in 0..3 {
                assert_close(
                    state[slot],
                    expected[lane * 3 + slot],
                    &format!("neuron {neuron} slot {slot}"),
                );
            }
        } else {
            for slot in 0..3 {
                assert_close(state[slot], 0.0, &format!("neuron {neuron} untouched"));
            }
        }
    }

    free_training_state();
}

#[test]
fn bias_persistent_8way_accumulates_across_iterations() {
    const START: usize = 2;
    init_training_state(0, 12);

    let (targets, pre_activations, biases) = bias_inputs_8();
    for _ in 0..3 {
        accumulate_bias_persistent_8way(
            START,
            &targets,
            &pre_activations,
            &biases,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_BIAS_SCALE,
        );
    }

    for lane in 0..8 {
        let state = read_neuron_state(START + lane);
        let expected_bias = biases[lane] + (targets[lane] - pre_activations[lane]);
        assert_close(state[0], 3.0, &format!("lane {lane} count"));
        assert_close(
            state[1],
            expected_bias * 3.0,
            &format!("lane {lane} total bias"),
        );
    }

    free_training_state();
}

#[test]
fn bias_persistent_8way_drops_lanes_past_the_end_of_the_buffer() {
    // Six neurons, starting at 2 — lanes 0..4 land in range, lanes 4..8 do not.
    init_training_state(0, 6);

    let (targets, pre_activations, biases) = bias_inputs_8();
    accumulate_bias_persistent_8way(
        2,
        &targets,
        &pre_activations,
        &biases,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_BIAS_SCALE,
    );

    for neuron in 0..6 {
        let state = read_neuron_state(neuron);
        let expected_count = if neuron >= 2 { 1.0 } else { 0.0 };
        assert_close(
            state[0],
            expected_count,
            &format!("neuron {neuron} count after clipped write"),
        );
    }

    free_training_state();
}

// ---------------------------------------------------------------------------
// apply_safe_zone_adjustment_batch — neat-core/src/safe_zone.rs
// ---------------------------------------------------------------------------

#[test]
fn safe_zone_batch_matches_the_scalar_call_for_every_synapse() {
    let squash_types: Vec<u8> = vec![
        SquashType::Identity as u8,
        SquashType::Relu as u8,
        SquashType::Selu as u8,
        SquashType::Sqrt as u8,
        SquashType::Square as u8,
    ];
    let raw_inputs = vec![1.0f32, -2.0, 0.0, 15.0, 7.5];
    let weights = vec![1.0f32, 0.5, -3.0, 2.0, 0.25];
    let error = -1.0f32;

    let batch = apply_safe_zone_adjustment_batch(&squash_types, &raw_inputs, error, &weights);

    assert_eq!(batch.len(), squash_types.len(), "one factor per synapse");
    for i in 0..squash_types.len() {
        let expected = apply_safe_zone_adjustment(
            SquashType::from(squash_types[i]),
            raw_inputs[i],
            error,
            weights[i],
        );
        assert!(
            (batch[i] - expected).abs() < 1e-6,
            "synapse {i}: expected {expected}, got {}",
            batch[i]
        );
    }
}

#[test]
fn safe_zone_batch_substitutes_one_for_a_non_finite_weight() {
    // Selu inside its band with an infinite weight that the error is already
    // correcting: the scalar call blocks the gradient, the batch wrapper
    // substitutes a healthy 1.0 and lets it flow.
    let squash_types = vec![SquashType::Selu as u8];
    let raw_inputs = vec![0.0f32];
    let error = -1.0f32;

    let direct_infinite = apply_safe_zone_adjustment(SquashType::Selu, 0.0, error, f32::INFINITY);
    let direct_substitute = apply_safe_zone_adjustment(SquashType::Selu, 0.0, error, 1.0);
    assert!(
        (direct_infinite - direct_substitute).abs() > 1e-6,
        "the two weights must be observably different for this test to bite"
    );

    for weight in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        let batch = apply_safe_zone_adjustment_batch(&squash_types, &raw_inputs, error, &[weight]);
        assert!(
            (batch[0] - direct_substitute).abs() < 1e-6,
            "weight {weight} should behave as 1.0, got {}",
            batch[0]
        );
    }
}

#[test]
fn safe_zone_batch_substitutes_per_synapse_only() {
    let squash_types = vec![SquashType::Selu as u8; 3];
    let raw_inputs = vec![0.0f32; 3];
    let error = -1.0f32;
    let weights = vec![f32::INFINITY, f32::INFINITY, 1e5];

    let batch = apply_safe_zone_adjustment_batch(&squash_types, &raw_inputs, error, &weights);

    let substituted = apply_safe_zone_adjustment(SquashType::Selu, 0.0, error, 1.0);
    let untouched = apply_safe_zone_adjustment(SquashType::Selu, 0.0, error, 1e5);
    assert!((batch[0] - substituted).abs() < 1e-6, "lane 0 substituted");
    assert!((batch[1] - substituted).abs() < 1e-6, "lane 1 substituted");
    assert!(
        (batch[2] - untouched).abs() < 1e-6,
        "a finite weight is passed through unchanged"
    );
}

#[test]
fn safe_zone_batch_of_zero_synapses_returns_no_factors() {
    let batch = apply_safe_zone_adjustment_batch(&[], &[], 1.0, &[]);
    assert!(batch.is_empty(), "empty batch yields empty result");
}

// ---------------------------------------------------------------------------
// distribute_elastic_error — neat-core/src/elastic_distribution.rs
// ---------------------------------------------------------------------------

/// Activations small enough that the score denominator sits below the default
/// Planck constant, so the chosen constant decides which pass runs.
const TINY_ACTIVATIONS: [f32; 2] = [1e-7, 1e-7];
const SAFE_ZONES: [f32; 2] = [1.0, 1.0];
const FALLBACK_WEIGHTS: [f32; 2] = [1.0, 3.0];

#[test]
fn elastic_error_passes_a_valid_plank_constant_through() {
    let plank = 1e-20f32;
    let shares = distribute_elastic_error(
        10.0,
        &TINY_ACTIVATIONS,
        &SAFE_ZONES,
        &FALLBACK_WEIGHTS,
        plank,
    );
    let expected = apply_distribute_elastic_error(
        10.0,
        &TINY_ACTIVATIONS,
        &SAFE_ZONES,
        &FALLBACK_WEIGHTS,
        plank,
    );

    assert_eq!(shares.len(), 2);
    for i in 0..2 {
        assert!(
            (shares[i] - expected[i]).abs() < 1e-5,
            "share {i}: expected {}, got {}",
            expected[i],
            shares[i]
        );
    }
    // With this constant the activation-proportional pass runs: an even split.
    assert!((shares[0] - 5.0).abs() < 1e-3, "share0={}", shares[0]);
    assert!((shares[1] - 5.0).abs() < 1e-3, "share1={}", shares[1]);
}

#[test]
fn elastic_error_falls_back_to_the_default_plank_constant_when_invalid() {
    let expected = apply_distribute_elastic_error(
        10.0,
        &TINY_ACTIVATIONS,
        &SAFE_ZONES,
        &FALLBACK_WEIGHTS,
        DEFAULT_PLANK,
    );

    for invalid in [0.0f32, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let shares = distribute_elastic_error(
            10.0,
            &TINY_ACTIVATIONS,
            &SAFE_ZONES,
            &FALLBACK_WEIGHTS,
            invalid,
        );
        for i in 0..2 {
            assert!(
                (shares[i] - expected[i]).abs() < 1e-5,
                "plank {invalid} share {i}: expected {}, got {}",
                expected[i],
                shares[i]
            );
        }
        // The default constant routes to the weight² fallback: 1² : 3² = 1 : 9.
        assert!((shares[0] - 1.0).abs() < 1e-3, "share0={}", shares[0]);
        assert!((shares[1] - 9.0).abs() < 1e-3, "share1={}", shares[1]);
    }
}

#[test]
fn elastic_error_shares_still_sum_to_the_error_after_the_fallback() {
    let shares = distribute_elastic_error(
        7.5,
        &[2.0, 1.0, 0.5],
        &[1.0, 1.0, 1.0],
        &[1.0, 1.0, 1.0],
        f32::NAN,
    );
    let total: f32 = shares.iter().sum();
    assert!((total - 7.5).abs() < 1e-4, "sum={total}");
}
