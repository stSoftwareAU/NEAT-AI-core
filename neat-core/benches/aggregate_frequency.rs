//! Aggregate-frequency sweep for production-scale inference (Issue #510).
//!
//! Three groups — `aggregate_forward_pass`, `aggregate_traced` and
//! `aggregate_scoring` — push the `production_exact` creature (2,461 inputs,
//! 1,666 non-input neurons, exactly 21,513 synapses) through forward-only,
//! traced and batched scoring while sweeping the **fraction of neurons using an
//! aggregate squash** (`Minimum`/`Maximum`/`If`). Topology, weights and biases
//! are untouched by the rewrite, so aggregate frequency is the only variable.
//!
//! ```text
//! cargo bench -p neat-core --bench aggregate_frequency
//! ```
//!
//! This exists because the committed `production` fixtures are homogeneous
//! `Tanh` (0% aggregate) while real creatures run ~10% (see
//! `docs/research/aggregate-unchecked-kernels-2026-08-05.md`), and because the
//! `agg0pct` point is a useful **null control**: it contains no aggregate
//! neuron, so its session-to-session spread measures the host's noise floor.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

#[allow(dead_code)] // shared fixture module; this bench uses only a subset
mod common;
use common::{
    NETWORKS, NetSpec, PRODUCTION_SCORING_RECORDS, aggregate_count, build_inputs, build_network,
    build_records, with_aggregates,
};

/// Aggregate frequencies swept by every group, in percent of non-input neurons.
///
/// `10` is the frequency the crate itself already assumes for production
/// creatures: `CompiledNetwork::new` sizes its trace buffer from "~10% of
/// neurons have aggregate functions (MINIMUM, MAXIMUM, IF)" (Issue #1173).
/// `0` is the all-standard null control, `100` the aggregate-heavy synthetic
/// upper bound that exists solely to prove the harness can detect a difference.
const AGGREGATE_PERCENTS: [usize; 4] = [0, 10, 50, 100];

/// The `production_exact` spec — the committed production creature topology.
fn production_exact() -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == "production_exact")
        .expect("production_exact spec")
}

/// Whole-creature forward pass (`activate`) at each aggregate frequency.
fn bench_forward_pass(c: &mut Criterion) {
    let spec = production_exact();
    let mut group = c.benchmark_group("aggregate_forward_pass");
    for percent in AGGREGATE_PERCENTS {
        let mut net = with_aggregates(build_network(spec, 0x5152_5354), percent);
        let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
        let num_outputs = spec.num_outputs;
        let label = format!("agg{percent}pct_n{}", aggregate_count(&net));
        group.throughput(Throughput::Elements(spec.num_neurons as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &inputs, |b, inputs| {
            b.iter(|| {
                let out = net.activate(black_box(inputs), black_box(num_outputs));
                black_box(out);
            });
        });
    }
    group.finish();
}

/// Whole-creature traced pass (`activate_and_trace`) at each aggregate
/// frequency — the training-side inference path.
fn bench_traced(c: &mut Criterion) {
    let spec = production_exact();
    let mut group = c.benchmark_group("aggregate_traced");
    for percent in AGGREGATE_PERCENTS {
        let mut net = with_aggregates(build_network(spec, 0x5152_5354), percent);
        let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
        let num_outputs = spec.num_outputs;
        let label = format!("agg{percent}pct_n{}", aggregate_count(&net));
        group.throughput(Throughput::Elements(spec.num_neurons as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &inputs, |b, inputs| {
            b.iter(|| {
                let out = net.activate_and_trace(black_box(inputs), black_box(num_outputs));
                black_box(out);
            });
        });
    }
    group.finish();
}

/// End-to-end inference throughput: one production shard's worth of records
/// (`PRODUCTION_SCORING_RECORDS`) scored through the batched path.
fn bench_scoring(c: &mut Criterion) {
    let spec = production_exact();
    let mut group = c.benchmark_group("aggregate_scoring");
    group.sample_size(20);
    for percent in AGGREGATE_PERCENTS {
        let net = with_aggregates(build_network(spec, 0x5EED), percent);
        let stride = net.num_inputs;
        let inputs: Vec<f32> = build_records(stride, PRODUCTION_SCORING_RECORDS)
            .into_iter()
            .flatten()
            .collect();
        let num_outputs = spec.num_outputs;
        let label = format!("agg{percent}pct_n{}", aggregate_count(&net));
        group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &inputs, |b, inputs| {
            b.iter(|| {
                let out = net.score_records_flat(
                    black_box(inputs),
                    black_box(stride),
                    black_box(num_outputs),
                );
                black_box(out);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_forward_pass, bench_traced, bench_scoring);
criterion_main!(benches);
