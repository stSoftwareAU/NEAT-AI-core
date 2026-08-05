//! Control-vs-prototype A/B for the validated exact-size inference entry point
//! (Issue #511).
//!
//! ```text
//! cargo bench -p neat-core --bench exact_inference                                    # control only (baseline)
//! cargo bench -p neat-core --bench exact_inference --features experimental-exact-inference
//! ```
//!
//! Both arms are compiled into the **same binary** when the feature is on, so
//! the A/B is measured inside one process and one thermal state — the
//! cross-session drift that dominated the Issue #510 experiment (a 19–40% noise
//! floor) cannot contaminate it.
//!
//! Three arms per group, deliberately:
//!
//! - `control` — the shipped [`CompiledNetwork::activate_into`];
//! - `exact` — the prototype `activate_into_exact` (validate once, then the
//!   validated hot path);
//! - `control_b` — the control a second time. It runs byte-identical work to
//!   `control`, so the `control` vs `control_b` gap **is** the resolution limit
//!   of the harness. A prototype delta smaller than that gap is noise.
//!
//! The fixture is `production_exact` (2,461 inputs, 1,666 non-input neurons,
//! 21,513 synapses) and the throughput group scores one production shard's
//! worth of records (`PRODUCTION_SCORING_RECORDS`) one at a time — the
//! per-record entry point this prototype is about, as used by
//! `loss.rs::mse_mean_record` and `wasm-bench`.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

#[allow(dead_code)] // shared fixture module; this bench uses only a subset
mod common;
use common::{NETWORKS, NetSpec, PRODUCTION_SCORING_RECORDS, build_inputs, build_network};

/// The `production_exact` spec — the committed production creature topology.
fn production_exact() -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == "production_exact")
        .expect("production_exact spec")
}

/// Single production-sized forward pass through each entry point.
fn bench_forward_pass(c: &mut Criterion) {
    let spec = production_exact();
    let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
    let mut group = c.benchmark_group("exact_forward_pass");
    group.throughput(Throughput::Elements(spec.num_neurons as u64));

    let mut net = build_network(spec, 0x5152_5354);
    let mut out = vec![0.0f32; spec.num_outputs];

    group.bench_function(BenchmarkId::from_parameter("control"), |b| {
        b.iter(|| {
            net.activate_into(black_box(&inputs), black_box(&mut out));
            black_box(&out);
        });
    });

    #[cfg(feature = "experimental-exact-inference")]
    group.bench_function(BenchmarkId::from_parameter("exact"), |b| {
        b.iter(|| {
            net.activate_into_exact(black_box(&inputs), black_box(&mut out))
                .expect("production-shaped call");
            black_box(&out);
        });
    });

    // Null control: identical work to `control`, so its delta is the noise floor.
    group.bench_function(BenchmarkId::from_parameter("control_b"), |b| {
        b.iter(|| {
            net.activate_into(black_box(&inputs), black_box(&mut out));
            black_box(&out);
        });
    });

    group.finish();
}

/// End-to-end: one production shard scored a record at a time through each
/// entry point. This is the complete-records/second metric.
fn bench_record_throughput(c: &mut Criterion) {
    let spec = production_exact();
    let stride = spec.num_inputs;
    let records: Vec<f32> = (0..PRODUCTION_SCORING_RECORDS)
        .flat_map(|r| build_inputs(stride, 0x5EED_0000 ^ r as u64))
        .collect();

    let mut group = c.benchmark_group("exact_record_throughput");
    group.sample_size(20);
    group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));

    let mut net = build_network(spec, 0x5152_5354);
    let mut out = vec![0.0f32; spec.num_outputs];

    group.bench_function(BenchmarkId::from_parameter("control"), |b| {
        b.iter(|| {
            for record in records.chunks_exact(stride) {
                net.activate_into(black_box(record), black_box(&mut out));
                black_box(&out);
            }
        });
    });

    #[cfg(feature = "experimental-exact-inference")]
    group.bench_function(BenchmarkId::from_parameter("exact"), |b| {
        b.iter(|| {
            for record in records.chunks_exact(stride) {
                net.activate_into_exact(black_box(record), black_box(&mut out))
                    .expect("production-shaped call");
                black_box(&out);
            }
        });
    });

    group.bench_function(BenchmarkId::from_parameter("control_b"), |b| {
        b.iter(|| {
            for record in records.chunks_exact(stride) {
                net.activate_into(black_box(record), black_box(&mut out));
                black_box(&out);
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_forward_pass, bench_record_throughput);
criterion_main!(benches);
