//! Issue #538 — the exported per-record MSE reduction (`mse_record`) and the
//! streaming directory helper (`mse_mean_streaming`).
//!
//! Oracles here reach the expected value by a route that does **not** share the
//! kernel under test (AGENTS.md rule 1): the reference walks records one at a
//! time through `CompiledNetwork::activate` and sums the squared differences in
//! the test, so a fault in the fused batch kernel or in the chunk/residual walk
//! moves only one side of the assertion.

use std::io::Write;
use std::path::Path;

use neat_core::network::CompiledNetwork;
use neat_core::{
    compile_creature, mse_mean_record, mse_mean_streaming, mse_record, mse_sum_batch_packed,
};

/// Float tolerance: the fused batch path re-associates its sums and uses the
/// vectorised squash approximations, so parity is stated, not bit-exact.
const TOL: f64 = 1e-6;

/// 3-input, 2-output forward-only creature with a non-identity squash on one
/// output, so the vectorised and scalar squash paths both carry real work.
fn creature_json() -> &'static str {
    r#"{
        "input": 3,
        "output": 2,
        "neurons": [
            {"type": "hidden", "uuid": "hidden-0", "bias": -0.2, "squash": "TANH"},
            {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"},
            {"type": "output", "uuid": "output-1", "bias": -0.4, "squash": "LOGISTIC"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "hidden-0", "weight": 0.5},
            {"fromUUID": "input-1", "toUUID": "hidden-0", "weight": -0.3},
            {"fromUUID": "input-2", "toUUID": "hidden-0", "weight": 0.7},
            {"fromUUID": "hidden-0", "toUUID": "output-0", "weight": 1.1},
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": -0.6},
            {"fromUUID": "hidden-0", "toUUID": "output-1", "weight": -0.9},
            {"fromUUID": "input-2", "toUUID": "output-1", "weight": 0.4}
        ],
        "forwardOnly": true
    }"#
}

const INPUT_SIZE: usize = 3;
const NUM_OUTPUTS: usize = 2;
const VALUES_PER_RECORD: usize = INPUT_SIZE + NUM_OUTPUTS;

fn network() -> CompiledNetwork {
    let creature = neat_core::parse_creature_json(creature_json()).expect("parse");
    compile_creature(&creature).expect("compile")
}

/// `count` deterministic packed `[inputs…, targets…]` records with alternating
/// signs, so no lane sees a uniformly positive input.
fn packed_records(count: usize) -> Vec<f32> {
    (0..count * VALUES_PER_RECORD)
        .map(|i| {
            let sign = if (i / VALUES_PER_RECORD) % 2 == 0 {
                1.0
            } else {
                -1.0
            };
            sign * (((i % 7) as f32) * 0.31 - 0.9)
        })
        .collect()
}

/// Write `records` as little-endian `f32` bytes split across `shard_bytes`
/// sized `.bin` files, so records may straddle a shard boundary.
fn write_bin_dir(dir: &Path, records: &[f32], shard_bytes: usize) {
    let bytes: Vec<u8> = records.iter().flat_map(|v| v.to_le_bytes()).collect();
    let chunks: Vec<&[u8]> = if bytes.is_empty() {
        Vec::new()
    } else {
        bytes.chunks(shard_bytes.max(1)).collect()
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let mut file = std::fs::File::create(dir.join(format!("{i}.bin"))).expect("create shard");
        file.write_all(chunk).expect("write shard");
    }
}

/// Independent oracle: mean per-record MSE computed one record at a time
/// through the scalar single-record forward pass, with the squared-error
/// reduction written out in the test rather than borrowed from `loss.rs`.
fn reference_mean_mse(records: &[f32], count: usize) -> f64 {
    let mut net = network();
    let mut sum = 0.0_f64;
    for r in 0..count {
        net.reset_state();
        let base = r * VALUES_PER_RECORD;
        let outputs = net.activate(&records[base..base + INPUT_SIZE], NUM_OUTPUTS);
        let targets = &records[base + INPUT_SIZE..base + VALUES_PER_RECORD];
        let mut sq = 0.0_f64;
        for (t, o) in targets.iter().zip(outputs.iter()) {
            let diff = (*t - *o) as f64;
            sq += diff * diff;
        }
        sum += sq / (NUM_OUTPUTS as f64);
    }
    sum / (count as f64)
}

#[test]
fn mse_record_is_the_mean_of_the_squared_differences() {
    let targets = [1.0_f32, -2.0, 0.5, 4.0];
    let outputs = [0.5_f32, -1.5, 0.5, 2.0];
    // diffs: 0.5, -0.5, 0.0, 2.0 → squares 0.25, 0.25, 0.0, 4.0 → sum 4.5
    // mean over 4 outputs = 1.125
    let actual = mse_record(&targets, &outputs);
    assert!(
        (actual - 1.125).abs() < TOL,
        "mse_record = {actual}, expected 1.125"
    );
}

#[test]
fn mse_record_returns_zero_for_no_outputs() {
    assert_eq!(mse_record(&[], &[]), 0.0);
    // Extra targets with no outputs to compare against contribute nothing.
    assert_eq!(mse_record(&[1.0, 2.0], &[]), 0.0);
}

#[test]
fn mse_record_ignores_targets_beyond_the_output_count() {
    // The reduction zips, so a longer target slice cannot inflate the mean.
    let full = mse_record(&[1.0, 3.0], &[0.0, 1.0]);
    let short = mse_record(&[1.0, 3.0, 9.0], &[0.0, 1.0]);
    assert!((full - short).abs() < TOL, "{full} != {short}");
    // (1^2 + 2^2) / 2 = 2.5
    assert!(
        (full - 2.5).abs() < TOL,
        "mse_record = {full}, expected 2.5"
    );
}

/// Delegation site 1: the scalar `mse_sum_batch_packed` closure. `forward_only
/// = false` keeps it off the fused SIMD path, and two outputs make the
/// per-record `1/num_outputs` factor observable.
#[test]
fn packed_sum_scalar_path_averages_over_the_output_count() {
    let count = 3;
    let records = packed_records(count);
    let mut net = network();
    let sum = mse_sum_batch_packed(&mut net, &records, INPUT_SIZE, NUM_OUTPUTS, false);
    let expected = reference_mean_mse(&records, count) * (count as f64);
    assert!(
        (sum - expected).abs() < TOL,
        "mse_sum_batch_packed = {sum}, per-record reference sum = {expected}"
    );
}

/// Delegation site 2: the `mse_mean_record` closure, likewise with two outputs.
#[test]
fn mean_record_averages_over_the_output_count() {
    let count = 5;
    let records = packed_records(count);
    let mut net = network();
    let mean = mse_mean_record(&mut net, &records, INPUT_SIZE, NUM_OUTPUTS);
    let expected = reference_mean_mse(&records, count);
    assert!(
        (mean - expected).abs() < TOL,
        "mse_mean_record = {mean}, per-record reference = {expected}"
    );
}

#[test]
fn streaming_mean_matches_a_per_record_reference() {
    let count = 11; // exercises the 8-way group, the 4-way remainder and the tail
    let records = packed_records(count);
    let dir = tempfile::tempdir().expect("tempdir");
    // Shard size deliberately not a whole number of records (record = 20 bytes),
    // so records straddle shard boundaries and the residual buffer is used.
    write_bin_dir(dir.path(), &records, 66);

    let mut net = network();
    let (mean, scored) =
        mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, None)
            .expect("stream");

    assert_eq!(scored, count as u64);
    let expected = reference_mean_mse(&records, count);
    assert!(
        (mean - expected).abs() < TOL,
        "mse_mean_streaming = {mean}, per-record reference = {expected}"
    );
}

#[test]
fn streaming_mean_equals_the_packed_sum_over_the_record_count() {
    let count = 9;
    let records = packed_records(count);
    let dir = tempfile::tempdir().expect("tempdir");
    write_bin_dir(dir.path(), &records, 4096);

    let mut stream_net = network();
    let (mean, scored) = mse_mean_streaming(
        &mut stream_net,
        dir.path(),
        INPUT_SIZE,
        NUM_OUTPUTS,
        true,
        None,
    )
    .expect("stream");

    let mut batch_net = network();
    let sum = mse_sum_batch_packed(&mut batch_net, &records, INPUT_SIZE, NUM_OUTPUTS, true);
    let expected = sum / (count as f64);

    assert_eq!(scored, count as u64);
    assert!(
        (mean - expected).abs() < TOL,
        "mse_mean_streaming = {mean}, mse_sum_batch_packed / {count} = {expected}"
    );
}

#[test]
fn streaming_mean_is_stateless_when_not_forward_only() {
    let count = 10;
    let records = packed_records(count);
    let dir = tempfile::tempdir().expect("tempdir");
    write_bin_dir(dir.path(), &records, 4096);

    let mut net = network();
    let (mean, scored) =
        mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, false, None)
            .expect("stream");

    assert_eq!(scored, count as u64);
    // The fixture is forward-only, so resetting state per record must land on
    // the same numbers as the fused path.
    let expected = reference_mean_mse(&records, count);
    assert!(
        (mean - expected).abs() < TOL,
        "recurrent-path mean = {mean}, per-record reference = {expected}"
    );
}

#[test]
fn streaming_mean_of_an_empty_directory_is_a_silent_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut net = network();
    let result = mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect("an empty directory is not an error");
    assert_eq!(result, (0.0, 0));
}

#[test]
fn streaming_mean_of_empty_shards_is_a_silent_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::File::create(dir.path().join("0.bin")).expect("create empty shard");
    let mut net = network();
    let result = mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect("zero-length shards yield no records, not an error");
    assert_eq!(result, (0.0, 0));
}

#[test]
fn streaming_max_records_truncates_to_the_first_n_records() {
    let count = 20;
    let records = packed_records(count);
    let dir = tempfile::tempdir().expect("tempdir");
    // 46 bytes per shard against a 20-byte record: caps land mid-shard.
    write_bin_dir(dir.path(), &records, 46);

    for cap in [1_u64, 5, 8, 13, 20] {
        let mut net = network();
        let (mean, scored) = mse_mean_streaming(
            &mut net,
            dir.path(),
            INPUT_SIZE,
            NUM_OUTPUTS,
            true,
            Some(cap),
        )
        .expect("stream");

        assert_eq!(scored, cap, "cap {cap} scored {scored} records");
        let expected = reference_mean_mse(&records, cap as usize);
        assert!(
            (mean - expected).abs() < TOL,
            "cap {cap}: mean = {mean}, first-{cap} reference = {expected}"
        );
    }
}

#[test]
fn streaming_max_records_above_the_corpus_scores_every_record() {
    let count = 6;
    let records = packed_records(count);
    let dir = tempfile::tempdir().expect("tempdir");
    write_bin_dir(dir.path(), &records, 4096);

    let mut net = network();
    let (mean, scored) = mse_mean_streaming(
        &mut net,
        dir.path(),
        INPUT_SIZE,
        NUM_OUTPUTS,
        true,
        Some(1_000),
    )
    .expect("stream");

    assert_eq!(scored, count as u64);
    let expected = reference_mean_mse(&records, count);
    assert!((mean - expected).abs() < TOL, "{mean} != {expected}");
}

#[test]
fn streaming_max_records_of_zero_scores_nothing() {
    let records = packed_records(4);
    let dir = tempfile::tempdir().expect("tempdir");
    write_bin_dir(dir.path(), &records, 4096);

    let mut net = network();
    let result = mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, Some(0))
        .expect("stream");
    assert_eq!(result, (0.0, 0));
}

#[test]
fn streaming_mean_fails_loud_on_a_missing_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("not-here");
    let mut net = network();
    let err = mse_mean_streaming(&mut net, &missing, INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect_err("a missing directory must not be reported as zero records");
    assert!(
        err.contains("not-here"),
        "error should name the offending path: {err}"
    );
}

#[test]
fn streaming_mean_fails_loud_on_a_trailing_partial_record() {
    let records = packed_records(3);
    let dir = tempfile::tempdir().expect("tempdir");
    // Drop the last two floats so the final record is incomplete.
    let truncated = &records[..records.len() - 2];
    write_bin_dir(dir.path(), truncated, 4096);

    let mut net = network();
    let err = mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect_err("a truncated record must not be silently dropped");
    assert!(
        err.contains("incomplete record"),
        "error should name the incomplete record: {err}"
    );
}

#[test]
fn streaming_mean_of_a_zero_width_record_is_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Bytes are present, but a zero-width record can never carve a record out
    // of them — the guard must fire before any read is attempted.
    write_bin_dir(dir.path(), &packed_records(2), 4096);
    let mut net = network();
    let result =
        mse_mean_streaming(&mut net, dir.path(), 0, 0, true, None).expect("zero-width record");
    assert_eq!(result, (0.0, 0));
}
