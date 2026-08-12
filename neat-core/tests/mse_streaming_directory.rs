//! Issue #538 — the streaming directory MSE helper and the exported
//! per-record MSE reduction.
//!
//! Oracle independence (AGENTS.md rule 1): the expected values here are
//! derived from the creature's closed-form arithmetic in `f64`
//! (`reference_output`), which shares no code path with the crate. The
//! `mse_sum_batch_packed` comparison is kept as a second, weaker check because
//! the issue names it as an acceptance criterion — it is never the only oracle.

use neat_core::creature::{compile_creature, parse_creature_json};
use neat_core::loss::{mse_mean_streaming, mse_record, mse_sum_batch_packed};
use neat_core::network::CompiledNetwork;
use std::path::Path;
use tempfile::TempDir;

const INPUT_SIZE: usize = 2;
const NUM_OUTPUTS: usize = 1;
const VALUES_PER_RECORD: usize = INPUT_SIZE + NUM_OUTPUTS;
const RECORD_BYTES: usize = VALUES_PER_RECORD * 4;
/// The batched path re-associates its sums and uses the vectorised squash
/// approximations, so parity against an independent oracle is a tolerance, not
/// bit-exactness (AGENTS.md rule 1).
const TOL: f64 = 1e-6;

/// 2-input, 1-output identity creature: `out = 0.5*in0 + (-0.3)*in1 + 0.1`.
fn linear_creature_json() -> &'static str {
    r#"{
        "input": 2,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 0.5},
            {"fromUUID": "input-1", "toUUID": "output-0", "weight": -0.3}
        ],
        "forwardOnly": true
    }"#
}

fn network() -> CompiledNetwork {
    let creature = parse_creature_json(linear_creature_json()).expect("parse");
    compile_creature(&creature).expect("compile")
}

/// Twelve `[in0, in1, target]` records — enough to cross the 8-record SIMD
/// group boundary inside `mse_sum_batch_packed`.
fn sample_records() -> Vec<[f32; VALUES_PER_RECORD]> {
    vec![
        [1.0, 0.5, 0.4],
        [0.0, 0.0, 0.0],
        [-1.0, 2.0, -1.0],
        [0.25, -0.75, 0.2],
        [2.0, 1.0, 0.8],
        [-0.5, -0.5, 0.0],
        [1.5, 0.5, 0.5],
        [0.1, 0.2, 0.15],
        [0.9, -0.1, 0.6],
        [-2.0, 0.75, -1.1],
        [0.6, 0.6, 0.05],
        [3.0, -1.5, 2.1],
    ]
}

/// Independent oracle — the creature's closed form, evaluated in `f64`.
/// Touches no crate code.
fn reference_output(record: &[f32; VALUES_PER_RECORD]) -> f64 {
    0.5 * record[0] as f64 - 0.3 * record[1] as f64 + 0.1
}

/// Independent oracle for the documented semantics: per-record mean over
/// outputs of `(target - output)^2`, then averaged over records. With one
/// output the per-record mean is just the squared difference.
fn reference_mean_mse(records: &[[f32; VALUES_PER_RECORD]]) -> f64 {
    if records.is_empty() {
        return 0.0;
    }
    let total: f64 = records
        .iter()
        .map(|r| {
            let diff = r[2] as f64 - reference_output(r);
            diff * diff
        })
        .sum();
    total / records.len() as f64
}

fn flatten(records: &[[f32; VALUES_PER_RECORD]]) -> Vec<f32> {
    records.iter().flat_map(|r| r.iter().copied()).collect()
}

fn to_le_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Write one `.bin` file per shard, named `0.bin`, `1.bin`, … so
/// `find_bin_files` reads them in the order given.
fn write_bin_dir(shards: &[Vec<u8>]) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for (idx, bytes) in shards.iter().enumerate() {
        std::fs::write(dir.path().join(format!("{idx}.bin")), bytes).expect("write shard");
    }
    dir
}

fn single_file_dir(records: &[[f32; VALUES_PER_RECORD]]) -> TempDir {
    write_bin_dir(&[to_le_bytes(&flatten(records))])
}

fn stream(dir: &Path, forward_only: bool, max_records: Option<u64>) -> (f64, u64) {
    let mut net = network();
    mse_mean_streaming(
        &mut net,
        dir,
        INPUT_SIZE,
        NUM_OUTPUTS,
        forward_only,
        max_records,
    )
    .expect("streaming MSE should succeed")
}

// ----- mse_record: the exported per-record reduction -----

#[test]
fn mse_record_is_the_mean_squared_difference_over_outputs() {
    let targets = [1.0f32, -2.0, 0.5, 4.0];
    let outputs = [1.5f32, -1.0, 0.5, 2.0];

    // Derivation: diffs are -0.5, -1.0, 0.0, 2.0 → squares 0.25, 1.0, 0.0, 4.0
    // → sum 5.25 → mean over 4 outputs = 1.3125.
    let expected = 5.25f64 / 4.0;

    let actual = mse_record(&targets, &outputs);
    assert!(
        (actual - expected).abs() < 1e-12,
        "mse_record = {actual}, expected {expected}"
    );
}

#[test]
fn mse_record_returns_zero_for_empty_outputs() {
    assert_eq!(mse_record(&[], &[]), 0.0);
    // No outputs to average over, even with targets present.
    assert_eq!(mse_record(&[1.0, 2.0], &[]), 0.0);
}

#[test]
fn mse_record_squares_the_difference_in_both_directions() {
    // Sign of the difference must not survive the square.
    let over = mse_record(&[0.0], &[0.25]);
    let under = mse_record(&[0.25], &[0.0]);
    assert!((over - 0.0625).abs() < 1e-12, "over = {over}");
    assert!((under - 0.0625).abs() < 1e-12, "under = {under}");
}

// ----- mse_mean_streaming -----

#[test]
fn mse_mean_streaming_matches_the_independent_closed_form_reference() {
    let records = sample_records();
    let dir = single_file_dir(&records);

    let (mean, count) = stream(dir.path(), true, None);

    let expected = reference_mean_mse(&records);
    assert_eq!(count, records.len() as u64, "record count");
    assert!(
        (mean - expected).abs() < TOL,
        "mse_mean_streaming = {mean}, closed-form reference = {expected}"
    );
}

#[test]
fn mse_mean_streaming_equals_the_packed_sum_divided_by_records() {
    let records = sample_records();
    let dir = single_file_dir(&records);
    let packed = flatten(&records);

    let (mean, count) = stream(dir.path(), true, None);

    let mut net = network();
    let sum = mse_sum_batch_packed(&mut net, &packed, INPUT_SIZE, NUM_OUTPUTS, true);
    let expected = sum / records.len() as f64;

    assert_eq!(count, records.len() as u64);
    assert!(
        (mean - expected).abs() < TOL,
        "mse_mean_streaming = {mean}, mse_sum_batch_packed / {} = {expected}",
        records.len()
    );
}

#[test]
fn mse_mean_streaming_reads_every_bin_file_in_numeric_order() {
    let records = sample_records();
    // Three shards of four whole records each.
    let shards: Vec<Vec<u8>> = records
        .chunks(4)
        .map(|c| to_le_bytes(&flatten(c)))
        .collect();
    let dir = write_bin_dir(&shards);

    let (mean, count) = stream(dir.path(), true, None);

    let expected = reference_mean_mse(&records);
    assert_eq!(count, records.len() as u64);
    assert!(
        (mean - expected).abs() < TOL,
        "sharded mean = {mean}, expected {expected}"
    );
}

#[test]
fn mse_mean_streaming_rejoins_records_straddling_a_chunk_boundary() {
    let records = sample_records();
    let bytes = to_le_bytes(&flatten(&records));
    // Split mid-record: shard 0 ends 5 bytes into its last record, so the
    // residual has to be carried into the next chunk to be scored at all.
    let split = RECORD_BYTES * 2 + 5;
    let shards = vec![bytes[..split].to_vec(), bytes[split..].to_vec()];
    let dir = write_bin_dir(&shards);

    let (mean, count) = stream(dir.path(), true, None);

    let expected = reference_mean_mse(&records);
    assert_eq!(
        count,
        records.len() as u64,
        "the straddling record must still be counted"
    );
    assert!(
        (mean - expected).abs() < TOL,
        "straddled mean = {mean}, expected {expected}"
    );
}

#[test]
fn mse_mean_streaming_empty_directory_returns_zero_and_no_records() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(stream(dir.path(), true, None), (0.0, 0));

    // A directory holding a non-`.bin` file is equally empty to this helper.
    std::fs::write(dir.path().join("notes.txt"), b"ignored").expect("write");
    assert_eq!(stream(dir.path(), true, None), (0.0, 0));
}

#[test]
fn mse_mean_streaming_max_records_truncates_to_the_first_n() {
    let records = sample_records();
    // Shard so the cap lands inside the second file, not on a file boundary.
    let shards: Vec<Vec<u8>> = records
        .chunks(4)
        .map(|c| to_le_bytes(&flatten(c)))
        .collect();
    let dir = write_bin_dir(&shards);

    for cap in [1u64, 5, 8, 11] {
        let (mean, count) = stream(dir.path(), true, Some(cap));
        let expected = reference_mean_mse(&records[..cap as usize]);
        assert_eq!(count, cap, "cap {cap} record count");
        assert!(
            (mean - expected).abs() < TOL,
            "cap {cap}: mean = {mean}, expected mean over first {cap} records = {expected}"
        );
    }
}

#[test]
fn mse_mean_streaming_max_records_above_the_corpus_reads_everything() {
    let records = sample_records();
    let dir = single_file_dir(&records);

    let (mean, count) = stream(dir.path(), true, Some(1_000));

    assert_eq!(count, records.len() as u64);
    assert!((mean - reference_mean_mse(&records)).abs() < TOL);
}

#[test]
fn mse_mean_streaming_max_records_zero_reads_nothing() {
    let records = sample_records();
    let dir = single_file_dir(&records);

    assert_eq!(stream(dir.path(), true, Some(0)), (0.0, 0));
}

#[test]
fn mse_mean_streaming_recurrent_route_matches_the_forward_only_route() {
    // The creature is stateless, so `forward_only = false` — which resets the
    // network between records instead of taking the fused batch path — must
    // reach the same numbers.
    let records = sample_records();
    let dir = single_file_dir(&records);

    let (fused, fused_count) = stream(dir.path(), true, None);
    let (reset, reset_count) = stream(dir.path(), false, None);

    let expected = reference_mean_mse(&records);
    assert_eq!(fused_count, reset_count);
    assert!(
        (reset - expected).abs() < TOL,
        "forward_only=false mean = {reset}, expected {expected}"
    );
    assert!(
        (fused - reset).abs() < TOL,
        "fused = {fused}, reset-per-record = {reset}"
    );
}

#[test]
fn mse_mean_streaming_missing_directory_fails_loud() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("no-such-directory");

    let mut net = network();
    let err = mse_mean_streaming(&mut net, &missing, INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect_err("a missing directory must not be reported as an empty corpus");
    assert!(
        err.contains("no-such-directory"),
        "error should name the path: {err}"
    );
}

#[test]
fn mse_mean_streaming_trailing_partial_record_fails_loud() {
    let records = sample_records();
    let mut bytes = to_le_bytes(&flatten(&records));
    bytes.truncate(bytes.len() - 5); // last record is 5 bytes short
    let dir = write_bin_dir(&[bytes]);

    let mut net = network();
    let err = mse_mean_streaming(&mut net, dir.path(), INPUT_SIZE, NUM_OUTPUTS, true, None)
        .expect_err("a trailing partial record must not be silently dropped");
    assert!(
        err.contains("trailing"),
        "error should describe the trailing bytes: {err}"
    );
}

#[test]
fn mse_mean_streaming_zero_width_records_return_zero() {
    let dir = single_file_dir(&sample_records());
    let mut net = network();
    // Degenerate layout: no inputs and no outputs is not a record.
    assert_eq!(
        mse_mean_streaming(&mut net, dir.path(), 0, 0, true, None).expect("degenerate layout"),
        (0.0, 0)
    );
}
