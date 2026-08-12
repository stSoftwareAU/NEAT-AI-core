//! Records that straddle a read-chunk boundary must still be scored (Issue #538).
//!
//! `mse_mean_streaming` sizes its reads through
//! `training_bin_stream::training_read_tuning_from_env`, so setting
//! `NEAT_SCORER_READ_BYTES` to a value that is *not* a whole number of records
//! forces every chunk to end mid-record. The result must be unchanged: the same
//! record count and the same mean as the single-record `activate` reference.
//!
//! This lives in its own test binary because it mutates a process-wide env var.

use neat_core::network::CompiledNetwork;
use neat_core::training_bin_stream::READ_BYTES_ENV;
use neat_core::{compile_creature, mse_mean_streaming, parse_creature_json};
use std::io::Write;
use tempfile::TempDir;

const TOL: f64 = 1e-6;
const INPUT_SIZE: usize = 2;
const NUM_OUTPUTS: usize = 1;
const VALUES_PER_RECORD: usize = INPUT_SIZE + NUM_OUTPUTS;

fn linear_network() -> CompiledNetwork {
    let creature = parse_creature_json(
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
        }"#,
    )
    .expect("parse creature");
    compile_creature(&creature).expect("compile creature")
}

fn packed_records(num_records: usize) -> Vec<f32> {
    (0..num_records)
        .flat_map(|i| {
            let f = i as f32;
            [0.2 * f - 2.0, 1.0 - 0.1 * f, 0.3 * ((i % 4) as f32) - 0.4]
        })
        .collect()
}

fn write_bin(path: &std::path::Path, values: &[f32]) {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let mut file = std::fs::File::create(path).expect("create .bin");
    file.write_all(&bytes).expect("write .bin");
}

/// Independent oracle — one `activate` call per record, arithmetic written out.
fn scalar_mean_reference(
    network: &mut CompiledNetwork,
    records: &[f32],
    limit: usize,
) -> (f64, u64) {
    let num_records = (records.len() / VALUES_PER_RECORD).min(limit);
    if num_records == 0 {
        return (0.0, 0);
    }
    let mut sum = 0.0f64;
    for idx in 0..num_records {
        network.reset_state();
        let base = idx * VALUES_PER_RECORD;
        let outputs = network.activate(&records[base..base + INPUT_SIZE], NUM_OUTPUTS);
        let mut sq_sum = 0.0f64;
        for (t, o) in records[base + INPUT_SIZE..base + VALUES_PER_RECORD]
            .iter()
            .zip(outputs.iter())
        {
            let diff = (*t - *o) as f64;
            sq_sum += diff * diff;
        }
        sum += sq_sum / (NUM_OUTPUTS as f64);
    }
    (sum / (num_records as f64), num_records as u64)
}

/// A single test drives every case: the env var is process-wide, so splitting
/// these into separate `#[test]` functions would let them race each other.
#[test]
fn records_straddling_a_read_chunk_boundary_are_scored_once() {
    let records = packed_records(23);
    let dir = TempDir::new().expect("tempdir");
    // Two shards, split off a record boundary in the *stream* sense: shard 0
    // holds 10 whole records, shard 1 the remaining 13.
    write_bin(
        &dir.path().join("0.bin"),
        &records[..10 * VALUES_PER_RECORD],
    );
    write_bin(
        &dir.path().join("1.bin"),
        &records[10 * VALUES_PER_RECORD..],
    );

    // 20 bytes is neither a multiple of the 12-byte record nor large enough to
    // hold two, so every chunk ends mid-record.
    // SAFETY: this test binary contains no other test, so nothing else in the
    // process can observe the mutation.
    unsafe { std::env::set_var(READ_BYTES_ENV, "20") };

    let mut streamed_net = linear_network();
    let streamed = mse_mean_streaming(
        &mut streamed_net,
        dir.path(),
        INPUT_SIZE,
        NUM_OUTPUTS,
        true,
        None,
    );

    let mut capped_net = linear_network();
    let capped = mse_mean_streaming(
        &mut capped_net,
        dir.path(),
        INPUT_SIZE,
        NUM_OUTPUTS,
        true,
        Some(13),
    );

    // SAFETY: as above.
    unsafe { std::env::remove_var(READ_BYTES_ENV) };

    let (mean, count) = streamed.expect("streaming succeeds");
    let (capped_mean, capped_count) = capped.expect("capped streaming succeeds");

    let mut reference_net = linear_network();
    let (expected, expected_count) =
        scalar_mean_reference(&mut reference_net, &records, usize::MAX);
    assert_eq!(count, expected_count);
    assert_eq!(count, 23, "every record must be scored exactly once");
    assert!(
        (mean - expected).abs() < TOL,
        "streamed mean = {mean}, expected {expected}"
    );

    let mut capped_reference_net = linear_network();
    let (capped_expected, _) = scalar_mean_reference(&mut capped_reference_net, &records, 13);
    assert_eq!(
        capped_count, 13,
        "the cap must hold across chunk boundaries"
    );
    assert!(
        (capped_mean - capped_expected).abs() < TOL,
        "capped mean = {capped_mean}, expected {capped_expected}"
    );
}
