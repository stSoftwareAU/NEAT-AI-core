//! Behavioural tests for data-parallel record scoring (Issue #179).
//!
//! These assert on observable outcomes — the returned output vectors — rather
//! than on threading mechanics. They run in both feature modes: with the
//! `parallel` feature off, `score_records_parallel` is the sequential fallback;
//! with `--features parallel` (as `quality.sh` runs via `--all-features`), the
//! same assertions exercise the rayon path. Either way the contract is the same:
//! results identical to the sequential reference, in input order.

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{NETWORKS, NetSpec, build_inputs, build_network};
use neat_core::network::CompiledNetwork;

fn spec(label: &str) -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
}

/// Build `count` distinct input records sized to the network's input layer.
fn build_records(net: &CompiledNetwork, count: usize) -> Vec<Vec<f32>> {
    (0..count)
        .map(|i| build_inputs(net.num_inputs(), 0x51A7_0000 + i as u64))
        .collect()
}

/// Independent sequential reference: a fresh scratch clone scored one record at
/// a time. Deliberately does not call `score_records`, so the parity assertion
/// does not assume the two share an implementation.
fn reference(net: &CompiledNetwork, records: &[Vec<f32>], num_outputs: usize) -> Vec<Vec<f32>> {
    let mut scratch = net.clone();
    records
        .iter()
        .map(|r| scratch.activate(r, num_outputs))
        .collect()
}

#[test]
fn parallel_scoring_matches_sequential_on_production_fixture() {
    let s = spec("production");
    let net = build_network(s, 0xC0FFEE);
    let records = build_records(&net, 257); // not a multiple of any core count

    let expected = reference(&net, &records, s.num_outputs);
    let actual = net.score_records_parallel(&records, s.num_outputs);

    assert_eq!(actual.len(), records.len());
    assert_eq!(actual, expected, "parallel results must equal sequential");
}

#[test]
fn parallel_scoring_matches_sequential_across_shapes() {
    for label in ["small_50", "medium_500", "production", "production_2x"] {
        let s = spec(label);
        let net = build_network(s, 0xABCD_1234);
        let records = build_records(&net, 128);

        let expected = reference(&net, &records, s.num_outputs);
        let actual = net.score_records_parallel(&records, s.num_outputs);

        assert_eq!(actual, expected, "mismatch for shape {label}");
    }
}

#[test]
fn score_records_matches_reference() {
    let s = spec("medium_500");
    let net = build_network(s, 0x1357);
    let records = build_records(&net, 64);

    let expected = reference(&net, &records, s.num_outputs);
    assert_eq!(net.score_records(&records, s.num_outputs), expected);
}

#[test]
fn output_order_is_preserved() {
    let s = spec("small_50");
    let net = build_network(s, 0x2468);
    let records = build_records(&net, 200);

    // Each record is distinct, so a re-ordered result would not match the
    // index-aligned reference.
    let expected = reference(&net, &records, s.num_outputs);
    let actual = net.score_records_parallel(&records, s.num_outputs);
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(a, e, "record {i} out of order or incorrect");
    }
}

#[test]
fn each_output_has_num_outputs_elements() {
    let s = spec("medium_500");
    let net = build_network(s, 0x99);
    let records = build_records(&net, 16);

    let out = net.score_records_parallel(&records, s.num_outputs);
    assert_eq!(out.len(), 16);
    for row in &out {
        assert_eq!(row.len(), s.num_outputs);
    }
}

#[test]
fn empty_records_yields_empty_output() {
    let s = spec("small_50");
    let net = build_network(s, 0x7);
    let empty: Vec<Vec<f32>> = Vec::new();

    assert!(net.score_records_parallel(&empty, s.num_outputs).is_empty());
    assert!(net.score_records(&empty, s.num_outputs).is_empty());
}

#[test]
fn single_record_matches_direct_activate() {
    let s = spec("production");
    let net = build_network(s, 0x42);
    let record = build_inputs(net.num_inputs(), 0xDEAD);

    let mut scratch = net.clone();
    let direct = scratch.activate(&record, s.num_outputs);
    let via_parallel = net.score_records_parallel(std::slice::from_ref(&record), s.num_outputs);

    assert_eq!(via_parallel, vec![direct]);
}
