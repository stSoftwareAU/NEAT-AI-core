//! Behavioural tests for data-parallel record scoring (Issue #179, #230).
//!
//! These assert on observable outcomes — the returned output vectors — rather
//! than on threading mechanics. They run in both feature modes: with the
//! `parallel` feature off, `score_records_parallel` is the sequential fallback;
//! with `--features parallel` (as `quality.sh` runs via `--all-features`), the
//! same assertions exercise the rayon path.
//!
//! Since Issue #230 the forward pass is driven through the across-records SIMD
//! path (8-record / 4-record batches, scalar tail). Standard-squash neurons now
//! sum across records rather than across synapses, so their `f32` results match
//! the per-record reference **within [`TOL`]**, not bit-for-bit — the
//! acceptance criteria explicitly allow SIMD reordering / `f32` accumulation
//! differences. A genuine bug (cross-lane weight mix-up, dropped/duplicated
//! record, mis-ordered output) differs by O(1), far above `TOL`. The scalar
//! tail and the sequential-vs-parallel comparison remain **exact**.

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{NETWORKS, NetSpec, build_inputs, build_network};
use neat_core::network::CompiledNetwork;

/// Absolute tolerance for the batched SIMD path vs the per-record reference.
/// Outputs are bounded (Tanh + range-limit), so ~1e-6 reassociation noise sits
/// well under this bound while any real lane/order bug (O(1) error) trips it.
const TOL: f32 = 1e-3;

/// Assert two flat output buffers agree within [`TOL`], reporting the worst
/// element on failure so a real divergence is easy to spot.
fn assert_close(actual: &[f32], expected: &[f32], ctx: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{ctx}: length mismatch ({} vs {})",
        actual.len(),
        expected.len()
    );
    let mut worst = 0.0f32;
    let mut worst_at = 0usize;
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        let diff = (a - e).abs();
        if diff > worst {
            worst = diff;
            worst_at = i;
        }
    }
    assert!(
        worst <= TOL,
        "{ctx}: max abs diff {worst} at index {worst_at} exceeds tol {TOL} \
         (actual={}, expected={})",
        actual.get(worst_at).copied().unwrap_or(f32::NAN),
        expected.get(worst_at).copied().unwrap_or(f32::NAN),
    );
}

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
/// a time via per-record `activate` (which still allocates its own output
/// `Vec`), flattened to the flat `[record * num_outputs]` layout the scoring
/// path now returns. Deliberately does not call `score_records`, so the parity
/// assertion does not assume the two share an implementation.
fn reference(net: &CompiledNetwork, records: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
    let mut scratch = net.clone();
    records
        .iter()
        .flat_map(|r| scratch.activate(r, num_outputs))
        .collect()
}

#[test]
fn parallel_scoring_matches_sequential_on_production_fixture() {
    let s = spec("production");
    let net = build_network(s, 0xC0FFEE);
    let records = build_records(&net, 257); // not a multiple of any core count

    let expected = reference(&net, &records, s.num_outputs);
    let actual = net.score_records_parallel(&records, s.num_outputs);

    assert_eq!(actual.len(), records.len() * s.num_outputs);
    assert_close(&actual, &expected, "production fixture");
}

#[test]
fn parallel_scoring_matches_sequential_across_shapes() {
    for label in ["small_50", "medium_500", "production", "production_2x"] {
        let s = spec(label);
        let net = build_network(s, 0xABCD_1234);
        let records = build_records(&net, 128);

        let expected = reference(&net, &records, s.num_outputs);
        let actual = net.score_records_parallel(&records, s.num_outputs);

        assert_close(&actual, &expected, &format!("shape {label}"));
    }
}

#[test]
fn score_records_matches_reference() {
    let s = spec("medium_500");
    let net = build_network(s, 0x1357);
    let records = build_records(&net, 64);

    let expected = reference(&net, &records, s.num_outputs);
    assert_close(
        &net.score_records(&records, s.num_outputs),
        &expected,
        "score_records medium_500",
    );
}

#[test]
fn output_order_is_preserved() {
    let s = spec("small_50");
    let net = build_network(s, 0x2468);
    let records = build_records(&net, 200);

    // Each record is distinct, so a re-ordered result would not match the
    // index-aligned reference within tolerance (a swap is an O(1) error).
    let expected = reference(&net, &records, s.num_outputs);
    let actual = net.score_records_parallel(&records, s.num_outputs);
    for (i, (a, e)) in actual
        .chunks_exact(s.num_outputs)
        .zip(expected.chunks_exact(s.num_outputs))
        .enumerate()
    {
        assert_close(a, e, &format!("record {i} out of order or incorrect"));
    }
}

#[test]
fn each_output_has_num_outputs_elements() {
    let s = spec("medium_500");
    let net = build_network(s, 0x99);
    let records = build_records(&net, 16);

    let out = net.score_records_parallel(&records, s.num_outputs);
    // Flat buffer: 16 records each contributing exactly num_outputs elements.
    assert_eq!(out.len(), 16 * s.num_outputs);
    assert_eq!(out.chunks_exact(s.num_outputs).count(), 16);
    assert!(out.chunks_exact(s.num_outputs).remainder().is_empty());
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

    // Single record: it runs the scalar tail, which is the exact single-record
    // path — so the flat buffer is bit-for-bit that record's outputs.
    assert_eq!(via_parallel, direct);
}

/// Issue #230 — exercise the 8 / 4 / scalar tail boundaries. The remainder path
/// is where a batched rewrite most likely silently drops or duplicates records,
/// so we assert both the exact output length (no record lost or added) and
/// per-record parity with the reference across counts straddling every boundary.
#[test]
fn tail_boundary_record_counts_match_reference() {
    let s = spec("production");
    let net = build_network(s, 0x0BAD_C0DE);

    for &count in &[0usize, 1, 2, 3, 4, 5, 7, 8, 9, 12, 15, 16, 17, 24, 31, 33] {
        let records = build_records(&net, count);
        let expected = reference(&net, &records, s.num_outputs);

        let seq = net.score_records(&records, s.num_outputs);
        let par = net.score_records_parallel(&records, s.num_outputs);

        assert_eq!(
            seq.len(),
            count * s.num_outputs,
            "seq length for count {count}"
        );
        assert_eq!(
            par.len(),
            count * s.num_outputs,
            "par length for count {count}"
        );
        assert_close(&seq, &expected, &format!("score_records count {count}"));
        assert_close(
            &par,
            &expected,
            &format!("score_records_parallel count {count}"),
        );
    }
}

/// The sequential and parallel paths run the identical batched forward pass and
/// each record's result is independent of its batch-mates; the parallel chunk
/// size is a multiple of the SIMD batch, so every record lands on the same
/// primitive (8-way / 4-way / scalar) either way. They must therefore agree
/// **bit-for-bit**, even though each only matches the per-record reference
/// within tolerance.
#[test]
fn sequential_and_parallel_are_bit_identical() {
    for label in ["small_50", "medium_500", "production"] {
        let s = spec(label);
        let net = build_network(s, 0x5AFE_5EED);
        for &count in &[7usize, 8, 9, 65, 130, 257] {
            let records = build_records(&net, count);
            let seq = net.score_records(&records, s.num_outputs);
            let par = net.score_records_parallel(&records, s.num_outputs);
            assert_eq!(
                seq, par,
                "seq vs parallel diverged for {label} count {count}"
            );
        }
    }
}
