//! Behavioural coverage for the packed-record scan rule (Issue #444).
//!
//! One rule says how a packed `[inputs…, targets…]` buffer is carved into
//! records: the record stride is `input_size + num_outputs`, only whole records
//! count, a record's targets start immediately after its inputs, and each
//! record is activated statelessly unless the caller declares the network
//! forward-only. Every packed loss entry point in `neat_core::loss` obeys it.
//!
//! These are "what" tests: they drive the public entry points with real
//! networks and assert observable numbers — a buffer's result equals the sum of
//! its records' individual results, a trailing partial record contributes
//! nothing, and targets are read from the slice that follows the inputs. A copy
//! of the arithmetic that drifted — a different stride, an off-by-one target
//! start, a dropped `reset_state()` — fails here no matter how the scan is
//! implemented.

use neat_core::loss::{
    categorical_error_sum_batch_packed, cross_entropy_sum_batch_packed, hinge_sum_batch_packed,
    mae_sum_batch_packed, mape_sum_batch_packed, mse_mean_record, mse_sum_batch_packed,
    msle_sum_batch_packed,
};
use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Identity squash — keeps every fixture's activation exact, so a mismatch is a
/// carve bug rather than an approximation.
const IDENTITY: u8 = 0;

/// Two inputs feeding two independent output neurons, no hidden state:
///   `out0 =  0.5*in0 - 0.3*in1 + 0.10`
///   `out1 = -0.2*in0 + 0.7*in1 + 0.05`
fn linear_network() -> CompiledNetwork {
    let synapses = vec![
        SynapseData {
            weight: 0.5,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: -0.3,
            from_index: 1,
            synapse_type: 0,
        },
        SynapseData {
            weight: -0.2,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: 0.7,
            from_index: 1,
            synapse_type: 0,
        },
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.10,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: IDENTITY,
            is_constant: false,
        },
        NeuronData {
            bias: 0.05,
            start_synapse: 2,
            num_synapses: 2,
            squash_type: IDENTITY,
            is_constant: false,
        },
    ];
    network(2, neurons, synapses)
}

/// One input into a single output neuron that also reads **its own** previous
/// activation, so the record loop's `reset_state()` is observable: with the
/// reset each record sees `out = in`, without it `out = in + 0.5 * previous`.
fn self_loop_network() -> CompiledNetwork {
    let synapses = vec![
        SynapseData {
            weight: 1.0,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: 0.5,
            from_index: 1,
            synapse_type: 0,
        },
    ];
    let neurons = vec![NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 2,
        squash_type: IDENTITY,
        is_constant: false,
    }];
    network(1, neurons, synapses)
}

fn network(
    num_inputs: usize,
    neurons: Vec<NeuronData>,
    synapses: Vec<SynapseData>,
) -> CompiledNetwork {
    CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("fixture must satisfy the load-time index invariant")
}

type PackedEntry = fn(&mut CompiledNetwork, &[f32], usize, usize, bool) -> f64;

/// `mse_mean_record` always resets, so it takes no `forward_only` flag; the
/// wrapper lets it join the table for the layout rules it shares.
fn mse_mean_record_entry(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    _forward_only: bool,
) -> f64 {
    mse_mean_record(network, records, input_size, num_outputs)
}

/// The seven entry points that return a **sum** over records.
const SUM_ENTRIES: [(&str, PackedEntry); 7] = [
    ("mse", mse_sum_batch_packed),
    ("mae", mae_sum_batch_packed),
    ("cross_entropy", cross_entropy_sum_batch_packed),
    ("mape", mape_sum_batch_packed),
    ("msle", msle_sum_batch_packed),
    ("hinge", hinge_sum_batch_packed),
    ("categorical_error", categorical_error_sum_batch_packed),
];

/// Every entry point carving a packed buffer — the seven sums plus the mean.
const ALL_ENTRIES: [(&str, PackedEntry); 8] = [
    ("mse", mse_sum_batch_packed),
    ("mae", mae_sum_batch_packed),
    ("cross_entropy", cross_entropy_sum_batch_packed),
    ("mape", mape_sum_batch_packed),
    ("msle", msle_sum_batch_packed),
    ("hinge", hinge_sum_batch_packed),
    ("categorical_error", categorical_error_sum_batch_packed),
    ("mse_mean_record", mse_mean_record_entry),
];

const INPUT_SIZE: usize = 2;
const NUM_OUTPUTS: usize = 2;

/// Packed `[in0, in1, t0, t1]` records with distinct, strictly positive targets
/// — positive keeps MAPE and MSLE away from their epsilon clamps, distinct
/// makes a record mix-up visible.
fn packed_records(num_records: usize) -> Vec<f32> {
    let values_per_record = INPUT_SIZE + NUM_OUTPUTS;
    let mut records = vec![0.0f32; num_records * values_per_record];
    for r in 0..num_records {
        let base = r * values_per_record;
        let f = r as f32;
        records[base] = 0.9 - 0.11 * f;
        records[base + 1] = 0.2 + 0.07 * f;
        records[base + 2] = 0.3 + 0.05 * f;
        records[base + 3] = 0.8 - 0.04 * f;
    }
    records
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    let tolerance = 1e-6 * expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected}, got {actual}"
    );
}

/// The record stride is `input_size + num_outputs` and only whole records are
/// scanned, so trailing bytes that cannot complete a record are invisible.
#[test]
fn every_packed_entry_point_ignores_a_trailing_partial_record() {
    for (name, entry) in ALL_ENTRIES {
        for forward_only in [true, false] {
            let whole = packed_records(9);
            for extra in 1..(INPUT_SIZE + NUM_OUTPUTS) {
                let mut ragged = whole.clone();
                ragged.extend(std::iter::repeat_n(0.42f32, extra));

                let mut net = linear_network();
                let want = entry(&mut net, &whole, INPUT_SIZE, NUM_OUTPUTS, forward_only);
                let mut net = linear_network();
                let got = entry(&mut net, &ragged, INPUT_SIZE, NUM_OUTPUTS, forward_only);

                assert_close(
                    got,
                    want,
                    &format!("{name} (forward_only={forward_only}) with {extra} trailing values"),
                );
            }
        }
    }
}

/// A buffer holding no whole record — empty, short, or a zero-width record —
/// has no error to report.
#[test]
fn every_packed_entry_point_returns_zero_without_a_whole_record() {
    let empty: Vec<f32> = Vec::new();
    let short = vec![0.5f32; INPUT_SIZE + NUM_OUTPUTS - 1];
    let non_empty = packed_records(3);

    for (name, entry) in ALL_ENTRIES {
        for forward_only in [true, false] {
            let mut net = linear_network();
            assert_eq!(
                entry(&mut net, &empty, INPUT_SIZE, NUM_OUTPUTS, forward_only),
                0.0,
                "{name}: empty buffer"
            );
            assert_eq!(
                entry(&mut net, &short, INPUT_SIZE, NUM_OUTPUTS, forward_only),
                0.0,
                "{name}: buffer shorter than one record"
            );
            // Zero stride: no record can be carved out of any buffer.
            assert_eq!(
                entry(&mut net, &non_empty, 0, 0, forward_only),
                0.0,
                "{name}: zero-width record"
            );
        }
    }
}

/// Records are independent: scanning a buffer of N records gives the same total
/// as scanning each record on its own. This pins the stride, the per-record
/// target start and the stateless activation protocol together — and, at nine
/// records, that the SIMD-batched buffer agrees with the scalar single-record
/// path.
#[test]
fn every_sum_entry_point_equals_the_sum_of_its_single_record_scans() {
    let values_per_record = INPUT_SIZE + NUM_OUTPUTS;
    for (name, entry) in SUM_ENTRIES {
        for forward_only in [true, false] {
            for num_records in [1usize, 3, 4, 8, 9] {
                let records = packed_records(num_records);

                let mut net = linear_network();
                let whole = entry(&mut net, &records, INPUT_SIZE, NUM_OUTPUTS, forward_only);

                let mut per_record = 0.0f64;
                for r in 0..num_records {
                    let base = r * values_per_record;
                    let mut net = linear_network();
                    per_record += entry(
                        &mut net,
                        &records[base..base + values_per_record],
                        INPUT_SIZE,
                        NUM_OUTPUTS,
                        forward_only,
                    );
                }

                assert_close(
                    whole,
                    per_record,
                    &format!("{name} (forward_only={forward_only}, {num_records} records)"),
                );
            }
        }
    }
}

/// `mse_mean_record` carves the same records and then averages them.
#[test]
fn mse_mean_record_equals_the_mean_of_its_single_record_scans() {
    let values_per_record = INPUT_SIZE + NUM_OUTPUTS;
    for num_records in [1usize, 3, 9] {
        let records = packed_records(num_records);

        let mut net = linear_network();
        let whole = mse_mean_record(&mut net, &records, INPUT_SIZE, NUM_OUTPUTS);

        let mut per_record = 0.0f64;
        for r in 0..num_records {
            let base = r * values_per_record;
            let mut net = linear_network();
            per_record += mse_mean_record(
                &mut net,
                &records[base..base + values_per_record],
                INPUT_SIZE,
                NUM_OUTPUTS,
            );
        }

        assert_close(
            whole,
            per_record / num_records as f64,
            &format!("mse_mean_record over {num_records} records"),
        );
    }
}

/// A record's targets are the values packed *after* its own inputs. Swapping
/// the two records' target blocks pairs each input with the other record's
/// targets, which must change the total — an entry point reading targets from a
/// fixed offset, or from the wrong record, would not notice.
///
/// MSLE is excluded by mathematics, not by exemption: its per-record reduction
/// is `Σ(ln t) - Σ(ln o)`, so permuting whole target blocks across records
/// cannot change its total. The next test covers MSLE's target reads.
#[test]
fn every_packed_entry_point_pairs_a_record_with_its_own_targets() {
    #[rustfmt::skip]
    let paired: Vec<f32> = vec![
        0.9, 0.1, /* targets */ 0.2, 0.9,
        0.1, 0.8, /* targets */ 0.8, 0.3,
    ];
    #[rustfmt::skip]
    let swapped: Vec<f32> = vec![
        0.9, 0.1, /* targets */ 0.8, 0.3,
        0.1, 0.8, /* targets */ 0.2, 0.9,
    ];

    for (name, entry) in ALL_ENTRIES {
        if name == "msle" {
            continue;
        }
        let mut net = linear_network();
        let as_packed = entry(&mut net, &paired, INPUT_SIZE, NUM_OUTPUTS, true);
        let mut net = linear_network();
        let as_swapped = entry(&mut net, &swapped, INPUT_SIZE, NUM_OUTPUTS, true);

        assert!(
            (as_packed - as_swapped).abs() > 1e-6,
            "{name}: swapping the records' target blocks left the result unchanged \
             ({as_packed} vs {as_swapped}) — targets are not being read per record"
        );
    }
}

/// Every record's target block is read from that record's own offset: editing
/// the targets of *any* single record — first, middle or last — must move the
/// result. An entry point that read targets from one fixed offset, or that
/// mis-strided past a record, would leave at least one edit invisible.
#[test]
fn every_packed_entry_point_reads_the_target_block_of_every_record() {
    const NUM_RECORDS: usize = 3;
    // Inputs chosen so the network's output argmax is 1 for every record; the
    // edited target block below flips the target argmax, which `categorical_error`
    // needs in order to notice at all.
    let mut base: Vec<f32> = Vec::new();
    for r in 0..NUM_RECORDS {
        let f = r as f32;
        base.extend([0.1 + 0.05 * f, 0.9 - 0.05 * f, 0.2, 0.9]);
    }
    let values_per_record = INPUT_SIZE + NUM_OUTPUTS;

    for (name, entry) in ALL_ENTRIES {
        let mut net = linear_network();
        let unedited = entry(&mut net, &base, INPUT_SIZE, NUM_OUTPUTS, true);

        for r in 0..NUM_RECORDS {
            let mut edited = base.clone();
            let target_start = r * values_per_record + INPUT_SIZE;
            edited[target_start] = 0.95;
            edited[target_start + 1] = 0.1;

            let mut net = linear_network();
            let got = entry(&mut net, &edited, INPUT_SIZE, NUM_OUTPUTS, true);
            assert!(
                (got - unedited).abs() > 1e-6,
                "{name}: editing record {r}'s target block left the result unchanged \
                 ({got} vs {unedited})"
            );
        }
    }
}

/// The per-record `reset_state()` is conditional on `forward_only`: with
/// `forward_only=false` a stateful network is driven statelessly (records stay
/// independent), and with `forward_only=true` the reset is skipped, so the
/// previous record's activation carries over.
#[test]
fn stateless_reset_is_conditional_on_forward_only() {
    // Single-output records `[in, target]` on the self-loop network.
    let records: Vec<f32> = vec![0.6, 0.2, 0.5, 0.3, 0.4, 0.1];
    let num_records = 3;

    let mut net = self_loop_network();
    let stateless = mse_sum_batch_packed(&mut net, &records, 1, 1, false);

    let mut independent = 0.0f64;
    for r in 0..num_records {
        let mut net = self_loop_network();
        independent += mse_sum_batch_packed(&mut net, &records[r * 2..r * 2 + 2], 1, 1, false);
    }
    assert_close(
        stateless,
        independent,
        "forward_only=false must reset between records",
    );

    let mut net = self_loop_network();
    let stateful = mse_sum_batch_packed(&mut net, &records, 1, 1, true);
    assert!(
        (stateful - stateless).abs() > 1e-6,
        "forward_only=true must skip the reset, leaving the self-loop's state to \
         carry over (got {stateful}, stateless {stateless})"
    );

    // `mse_mean_record` has no flag — it always resets.
    let mut net = self_loop_network();
    let mean = mse_mean_record(&mut net, &records, 1, 1);
    assert_close(
        mean,
        independent / num_records as f64,
        "mse_mean_record must reset between records",
    );
}
