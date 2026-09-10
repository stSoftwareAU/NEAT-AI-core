//! Issue #658 — mismatched parallel-array lengths must not abort the WASM module.
//!
//! The batch entry points below take two or more slices the body walks with a
//! single index. Before this guard a caller that passed a shorter secondary
//! slice reached an out-of-range index, and a panic on wasm aborts the whole
//! module instance (`catch_unwind` is unavailable there — see the note on
//! `crate::wasm_exports`). Each function now answers a malformed call with an
//! empty `Vec` — the sentinel a successful call can never produce — mirroring
//! `validate_topology_typed`'s `MALFORMED_BUFFER` and `loss.rs`'s
//! `packed_layout(...) -> Option<_>`.
//!
//! Every test drives the real exported function with a deliberately short
//! parallel slice and asserts the sentinel comes back, so the guard cannot be
//! removed without a red run.

use neat_core::accumulate::{calculate_bias_batch_4way, calculate_weight_batch_4way};
use neat_core::elastic_distribution::apply_distribute_elastic_error;
use neat_core::{
    SquashType, accumulate_bias_batch_4way, accumulate_bias_batch_8way,
    accumulate_weight_batch_4way, accumulate_weight_batch_8way, apply_fused_error_distribution,
    apply_safe_zone_adjustment_batch, distribute_elastic_error,
};

const PLANK: f64 = 1e-7;
const LEARNING_RATE: f64 = 1.0;
const MAX_ADJ_SCALE: f64 = 1.0;
const LIMIT_SCALE: f64 = 100_000.0;
const PLANK_F32: f32 = 1e-12;

fn ramp(n: usize) -> Vec<f64> {
    (0..n).map(|i| 0.5 + i as f64).collect()
}

fn ramp_f32(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 + i as f32).collect()
}

// ---------------------------------------------------------------------------
// accumulate.rs — fixed-arity batches
// ---------------------------------------------------------------------------

#[test]
fn accumulate_weight_batch_4way_rejects_a_short_parallel_slice() {
    let full = ramp(4);

    for short in 0..4 {
        let stub = ramp(short);
        assert!(
            accumulate_weight_batch_4way(
                &stub,
                &full,
                &full,
                PLANK,
                LEARNING_RATE,
                MAX_ADJ_SCALE,
                LIMIT_SCALE
            )
            .is_empty(),
            "short current_weights ({short}) must answer with the empty sentinel"
        );
        assert!(
            accumulate_weight_batch_4way(
                &full,
                &stub,
                &full,
                PLANK,
                LEARNING_RATE,
                MAX_ADJ_SCALE,
                LIMIT_SCALE
            )
            .is_empty(),
            "short target_values ({short}) must answer with the empty sentinel"
        );
        assert!(
            accumulate_weight_batch_4way(
                &full,
                &full,
                &stub,
                PLANK,
                LEARNING_RATE,
                MAX_ADJ_SCALE,
                LIMIT_SCALE
            )
            .is_empty(),
            "short activations ({short}) must answer with the empty sentinel"
        );
    }
}

#[test]
fn accumulate_weight_batch_4way_still_serves_well_formed_input() {
    let full = ramp(4);
    let result = accumulate_weight_batch_4way(
        &full,
        &full,
        &full,
        PLANK,
        LEARNING_RATE,
        MAX_ADJ_SCALE,
        LIMIT_SCALE,
    );
    assert_eq!(result.len(), 28, "7 packed values per synapse × 4");
}

#[test]
fn accumulate_weight_batch_8way_rejects_a_short_parallel_slice() {
    let full = ramp(8);
    let stub = ramp(7);

    assert!(
        accumulate_weight_batch_8way(
            &full,
            &full,
            &stub,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .is_empty(),
        "short activations must answer with the empty sentinel"
    );
    assert_eq!(
        accumulate_weight_batch_8way(
            &full,
            &full,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .len(),
        56,
        "well-formed input still returns 7 values per synapse × 8"
    );
}

#[test]
fn accumulate_bias_batch_4way_rejects_a_short_parallel_slice() {
    let full = ramp(4);
    let stub = ramp(3);

    assert!(
        accumulate_bias_batch_4way(
            &stub,
            &full,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .is_empty(),
        "short target_pre_activations must answer with the empty sentinel"
    );
    assert!(
        accumulate_bias_batch_4way(
            &full,
            &stub,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .is_empty(),
        "short pre_activations must answer with the empty sentinel"
    );
    assert!(
        accumulate_bias_batch_4way(
            &full,
            &full,
            &stub,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .is_empty(),
        "short current_biases must answer with the empty sentinel"
    );
    assert_eq!(
        accumulate_bias_batch_4way(
            &full,
            &full,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .len(),
        12,
        "well-formed input still returns 3 values per neuron × 4"
    );
}

#[test]
fn accumulate_bias_batch_8way_rejects_a_short_parallel_slice() {
    let full = ramp(8);
    let stub = ramp(1);

    assert!(
        accumulate_bias_batch_8way(
            &full,
            &stub,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .is_empty(),
        "short pre_activations must answer with the empty sentinel"
    );
    assert_eq!(
        accumulate_bias_batch_8way(
            &full,
            &full,
            &full,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE
        )
        .len(),
        24,
        "well-formed input still returns 3 values per neuron × 8"
    );
}

#[test]
fn calculate_weight_batch_4way_rejects_a_short_packed_state() {
    // 8 packed f64 per synapse × 4 synapses.
    for short in [0usize, 1, 8, 24, 31] {
        assert!(
            calculate_weight_batch_4way(
                &ramp(short),
                3.0,
                PLANK,
                LEARNING_RATE,
                MAX_ADJ_SCALE,
                LIMIT_SCALE,
                0.0,
                0.0
            )
            .is_empty(),
            "packed_state of {short} f64 is short of the 32 the stride needs"
        );
    }

    assert_eq!(
        calculate_weight_batch_4way(
            &ramp(32),
            3.0,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE,
            0.0,
            0.0
        )
        .len(),
        4,
        "a full 32-value packed state still returns one weight per synapse"
    );
}

#[test]
fn calculate_bias_batch_4way_rejects_a_short_packed_state() {
    // 3 packed f64 per neuron × 4 neurons.
    for short in [0usize, 3, 11] {
        assert!(
            calculate_bias_batch_4way(
                &ramp(short),
                &[0u8; 4],
                3.0,
                PLANK,
                LEARNING_RATE,
                MAX_ADJ_SCALE,
                LIMIT_SCALE,
                0.0,
                0.0
            )
            .is_empty(),
            "packed_state of {short} f64 is short of the 12 the stride needs"
        );
    }

    assert_eq!(
        calculate_bias_batch_4way(
            &ramp(12),
            &[0u8; 4],
            3.0,
            PLANK,
            LEARNING_RATE,
            MAX_ADJ_SCALE,
            LIMIT_SCALE,
            0.0,
            0.0
        )
        .len(),
        4,
        "a full 12-value packed state still returns one bias per neuron"
    );
}

// ---------------------------------------------------------------------------
// safe_zone.rs — count driven by squash_types
// ---------------------------------------------------------------------------

#[test]
fn safe_zone_adjustment_batch_rejects_a_short_parallel_slice() {
    let squash_types = vec![SquashType::Identity as u8; 4];
    let full = ramp_f32(4);
    let stub = ramp_f32(2);

    assert!(
        apply_safe_zone_adjustment_batch(&squash_types, &stub, 1.0, &full).is_empty(),
        "short raw_inputs must answer with the empty sentinel"
    );
    assert!(
        apply_safe_zone_adjustment_batch(&squash_types, &full, 1.0, &stub).is_empty(),
        "short weights must answer with the empty sentinel"
    );
    assert_eq!(
        apply_safe_zone_adjustment_batch(&squash_types, &full, 1.0, &full).len(),
        4,
        "well-formed input still returns one factor per synapse"
    );
}

// ---------------------------------------------------------------------------
// fused_error.rs — count driven by upstream_squash_types
// ---------------------------------------------------------------------------

fn fused(hints: &[f32], activations: &[f32], weights: &[f32]) -> Vec<f32> {
    apply_fused_error_distribution(
        SquashType::Identity,
        0.5,
        0.9,
        0.5,
        &[SquashType::Identity as u8; 4],
        hints,
        activations,
        weights,
    )
}

#[test]
fn fused_error_distribution_rejects_a_short_parallel_slice() {
    let full = ramp_f32(4);
    let stub = ramp_f32(3);

    assert!(
        fused(&stub, &full, &full).is_empty(),
        "short upstream_hint_values must answer with the empty sentinel"
    );
    assert!(
        fused(&full, &stub, &full).is_empty(),
        "short upstream_activations must answer with the empty sentinel"
    );
    assert!(
        fused(&full, &full, &stub).is_empty(),
        "short synapse_weights must answer with the empty sentinel"
    );
    assert_eq!(
        fused(&full, &full, &full).len(),
        9,
        "well-formed input still returns [error, safeZone × 4, perLink × 4]"
    );
}

#[test]
fn fused_error_distribution_rejects_a_short_slice_on_the_zero_error_path() {
    // Error is zero (activation already equals the target), which returns early
    // before the per-link loops — the guard must still fire ahead of it.
    let result = apply_fused_error_distribution(
        SquashType::Identity,
        0.5,
        0.5,
        0.5,
        &[SquashType::Identity as u8; 4],
        &ramp_f32(4),
        &ramp_f32(4),
        &ramp_f32(1),
    );
    assert!(
        result.is_empty(),
        "a short slice is malformed whichever branch the error value would take"
    );
}

// ---------------------------------------------------------------------------
// elastic_distribution.rs — count driven by activations
// ---------------------------------------------------------------------------

#[test]
fn distribute_elastic_error_rejects_a_short_parallel_slice() {
    let full = ramp_f32(8);
    let stub = ramp_f32(4);

    assert!(
        distribute_elastic_error(3.0, &full, &stub, &full, PLANK_F32).is_empty(),
        "short safe_zone_factors must answer with the empty sentinel"
    );
    assert!(
        distribute_elastic_error(3.0, &full, &full, &stub, PLANK_F32).is_empty(),
        "short weights must answer with the empty sentinel"
    );
    assert_eq!(
        distribute_elastic_error(3.0, &full, &full, &full, PLANK_F32).len(),
        8,
        "well-formed input still returns one share per link"
    );
}

#[test]
fn distribute_elastic_error_rejects_a_short_weights_slice_on_the_fallback_path() {
    // Zero activations force the weight-based fallback branch — the exploit
    // sketch in Issue #658 — which indexes `weights[i]` for i in 0..count.
    let activations = vec![0.0f32; 8];
    assert!(
        apply_distribute_elastic_error(3.0, &activations, &[1.0f32; 8], &ramp_f32(4), PLANK_F32)
            .is_empty(),
        "the weight fallback must not index past a short weights slice"
    );
}

#[test]
fn distribute_elastic_error_rejects_a_short_slice_when_the_error_is_not_finite() {
    // The non-finite early return must not mask a malformed call.
    assert!(
        apply_distribute_elastic_error(
            f32::NAN,
            &ramp_f32(4),
            &ramp_f32(4),
            &ramp_f32(2),
            PLANK_F32
        )
        .is_empty(),
        "a short slice is malformed even when the error is not finite"
    );
}

#[test]
fn empty_input_is_not_treated_as_malformed() {
    // count == 0 with all-empty parallel slices is a well-formed no-op, not a
    // length mismatch.
    assert!(apply_safe_zone_adjustment_batch(&[], &[], 1.0, &[]).is_empty());
    assert!(apply_distribute_elastic_error(1.0, &[], &[], &[], PLANK_F32).is_empty());
    assert_eq!(
        apply_fused_error_distribution(SquashType::Identity, 0.5, 0.9, 0.5, &[], &[], &[], &[])
            .len(),
        1,
        "an empty fused call still reports the neuron error"
    );
}
