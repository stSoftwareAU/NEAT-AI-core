//! Packed-record indexing in the persistent training state (Issue #606).
//!
//! Every entry point takes a caller-supplied record index and multiplies it by
//! the record width to find the record's start. `[profile.release]` does not
//! enable `overflow-checks`, so that multiply wraps silently in release — and a
//! wrapped start lands *inside* the buffer, where the `base + FIELDS <= len`
//! guard waves it through and the call reads or corrupts a different synapse's
//! record instead of refusing an out-of-range one.
//!
//! The indices below are derived, not magic: with `SYNAPSE_FIELDS == 7`,
//! `usize::MAX / 7 + 1` is the smallest index whose record start overflows, and
//! `(usize::MAX / 7 + 1) * 7 mod 2^64 == 5` — offset 5 of a two-synapse buffer,
//! which is a live record. The neuron form is the same shape with
//! `NEURON_FIELDS == 3`, landing on offset 2.

use neat_core::{
    accumulate_bias_persistent_4way, accumulate_weight_persistent_4way, init_training_state,
    read_all_neuron_state, read_all_synapse_state, read_neuron_state, read_synapse_state,
};

const SYNAPSE_FIELDS: usize = 7;
const NEURON_FIELDS: usize = 3;

/// Smallest synapse index whose record start overflows `usize`.
fn wrapping_synapse_index() -> usize {
    usize::MAX / SYNAPSE_FIELDS + 1
}

/// Smallest neuron index whose record start overflows `usize`.
fn wrapping_neuron_index() -> usize {
    usize::MAX / NEURON_FIELDS + 1
}

/// The four-lane weight fixture the in-module tests use, known to produce a
/// non-zero `count` for lane 0 — without it the write guard is never reached
/// and the assertion below would pass vacuously.
fn weight_lanes() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    (
        vec![0.5, -0.3, 1.2, 0.0],
        vec![2.0, -1.5, 0.8, 3.0],
        vec![1.0, 0.5, -0.8, 2.0],
    )
}

#[test]
fn a_synapse_index_whose_record_start_wraps_reads_as_out_of_range() {
    init_training_state(2, 1);
    let (weights, targets, acts) = weight_lanes();
    accumulate_weight_persistent_4way(0, &weights, &targets, &acts, 1e-7, 1.0, 1.0, 100_000.0);

    // The window the wrapped start would have aliased must hold real data, so
    // this test cannot pass by reading zeroes that were never written.
    let aliased = &read_all_synapse_state()[5..5 + SYNAPSE_FIELDS];
    assert!(
        aliased.iter().any(|v| *v != 0.0),
        "offset 5 must hold accumulated data for the aliasing to be observable"
    );

    assert_eq!(
        read_synapse_state(wrapping_synapse_index()),
        vec![0.0; SYNAPSE_FIELDS],
        "an index past the buffer reads as out of range, never as offset 5"
    );
}

#[test]
fn a_neuron_index_whose_record_start_wraps_reads_as_out_of_range() {
    init_training_state(0, 2);
    accumulate_bias_persistent_4way(
        0,
        &[2.0, -1.5, 0.8, 3.0],
        &[0.5, -0.3, 1.2, 0.0],
        &[0.1, 0.1, 0.1, 0.1],
        1e-7,
        1.0,
        1.0,
        100_000.0,
    );

    let aliased = &read_all_neuron_state()[2..2 + NEURON_FIELDS];
    assert!(
        aliased.iter().any(|v| *v != 0.0),
        "offset 2 must hold accumulated data for the aliasing to be observable"
    );

    assert_eq!(
        read_neuron_state(wrapping_neuron_index()),
        vec![0.0; NEURON_FIELDS],
        "an index past the buffer reads as out of range, never as offset 2"
    );
}

#[test]
fn accumulating_at_a_wrapping_start_index_leaves_every_record_untouched() {
    init_training_state(2, 1);
    let (weights, targets, acts) = weight_lanes();

    accumulate_weight_persistent_4way(
        wrapping_synapse_index(),
        &weights,
        &targets,
        &acts,
        1e-7,
        1.0,
        1.0,
        100_000.0,
    );

    assert_eq!(
        read_all_synapse_state(),
        vec![0.0; 2 * SYNAPSE_FIELDS],
        "a start index past the buffer writes nothing, least of all offset 5"
    );
}

#[test]
fn accumulating_bias_at_a_wrapping_start_index_leaves_every_record_untouched() {
    init_training_state(0, 2);

    accumulate_bias_persistent_4way(
        wrapping_neuron_index(),
        &[2.0, -1.5, 0.8, 3.0],
        &[0.5, -0.3, 1.2, 0.0],
        &[0.1, 0.1, 0.1, 0.1],
        1e-7,
        1.0,
        1.0,
        100_000.0,
    );

    assert_eq!(
        read_all_neuron_state(),
        vec![0.0; 2 * NEURON_FIELDS],
        "a start index past the buffer writes nothing, least of all offset 2"
    );
}

#[test]
#[should_panic(expected = "does not fit a packed state buffer")]
fn a_synapse_count_whose_buffer_size_wraps_fails_loud() {
    // Silently wrapping to a three-element buffer would leave every later
    // accumulation dropped by the length guard — an epoch that trains nothing
    // and reports success.
    init_training_state(wrapping_synapse_index(), 0);
}

#[test]
#[should_panic(expected = "does not fit a packed state buffer")]
fn a_neuron_count_whose_buffer_size_wraps_fails_loud() {
    init_training_state(0, wrapping_neuron_index());
}
