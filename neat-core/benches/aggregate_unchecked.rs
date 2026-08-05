//! Issue #510 — measurement harness for the experimental unchecked aggregate
//! kernels (`Minimum` / `Maximum` / `If`).
//!
//! Two layers, deliberately separate:
//!
//! - **`aggregate_kernel` / `aggregate_kernel_traced`** — isolated safe-vs-
//!   unchecked A/B *inside one process*, so the two kernels see the same cache
//!   state and the comparison is not a cross-session artefact. Only built with
//!   `--features experimental-aggregate-unchecked`, because the safe control
//!   lives in the prototype module.
//! - **`aggregate_forward_pass` / `aggregate_traced` / `aggregate_scoring`** —
//!   whole-creature inference at production scale across a sweep of aggregate
//!   frequencies. These always build; which kernel they exercise depends on the
//!   feature, so the control and prototype are compared through Criterion
//!   baselines:
//!
//! ```text
//! cargo bench -p neat-core --bench aggregate_unchecked -- --save-baseline safe
//! cargo bench -p neat-core --features experimental-aggregate-unchecked \
//!     --bench aggregate_unchecked -- --baseline safe
//! ```
//!
//! The fixture is the `production_exact` topology (2,461 inputs, 1,666
//! non-input neurons, exactly 21,513 synapses) with a deterministic fraction of
//! its neurons rewritten to `Minimum`/`Maximum`/`If`, so the aggregate
//! *frequency* is the swept variable.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use neat_core::network::CompiledNetwork;
use neat_core::squash::SquashType;

#[allow(dead_code)] // shared fixture module; this bench uses only a subset
mod common;
use common::{
    NETWORKS, NetSpec, PRODUCTION_SCORING_RECORDS, build_inputs, build_network, build_records,
};

/// The three prototyped aggregates, cycled across the rewritten neurons.
const AGGREGATES: [SquashType; 3] = [SquashType::Minimum, SquashType::Maximum, SquashType::If];

/// Aggregate frequencies swept by the end-to-end groups, in percent of
/// non-input neurons.
///
/// `10` is the frequency the crate itself already assumes for production
/// creatures: `CompiledNetwork::new` sizes its trace buffer from "~10% of
/// neurons have aggregate functions (MINIMUM, MAXIMUM, IF)" (Issue #1173).
/// `0` is the all-standard control, `100` the aggregate-heavy synthetic upper
/// bound that exists solely to prove the harness can detect a difference.
const AGGREGATE_PERCENTS: [usize; 4] = [0, 10, 50, 100];

/// The `production_exact` spec — the committed production creature topology.
fn production_exact() -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == "production_exact")
        .expect("production_exact spec")
}

/// Rewrite a deterministic `percent` of the network's non-input neurons to the
/// three prototyped aggregate squashes, cycling `Minimum`/`Maximum`/`If`.
///
/// Topology, weights, biases and fan-in are untouched — only `squash_type` and
/// (for `If` neurons) the synapse types change, so the gather pattern stays the
/// production one and the swept variable really is aggregate frequency.
fn with_aggregates(mut net: CompiledNetwork, percent: usize) -> CompiledNetwork {
    if percent == 0 {
        return net;
    }
    let num_non_inputs = net.neurons.len();
    let target = num_non_inputs * percent / 100;
    if target == 0 {
        return net;
    }
    // Evenly spread rather than clustered, so the aggregate neurons are
    // interleaved with standard ones as they are in a real creature.
    let stride = num_non_inputs / target;

    for n in (0..num_non_inputs).step_by(stride.max(1)) {
        let squash = AGGREGATES[(n / stride.max(1)) % AGGREGATES.len()];
        net.neurons[n].squash_type = squash as u8;

        if squash == SquashType::If {
            let start = net.neurons[n].start_synapse as usize;
            let end = start + net.neurons[n].num_synapses as usize;
            for (k, synapse) in net.synapses[start..end].iter_mut().enumerate() {
                // First synapse is the condition; the rest alternate the
                // positive and negative branches.
                synapse.synapse_type = match k {
                    0 => 1,             // Condition
                    _ if k % 2 == 1 => 3, // Positive
                    _ => 2,             // Negative
                };
            }
        }
    }
    net
}

/// Count of aggregate neurons, reported as the benchmark's fixture metadata.
fn aggregate_count(net: &CompiledNetwork) -> usize {
    net.neurons
        .iter()
        .filter(|n| !n.is_constant && SquashType::from(n.squash_type).is_aggregate())
        .count()
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

/// Isolated safe-vs-unchecked kernel A/B, in one process.
#[cfg(feature = "experimental-aggregate-unchecked")]
fn bench_kernels(c: &mut Criterion) {
    use neat_core::aggregate_experiment::{
        aggregate_forward_safe, aggregate_forward_safe_indexed, aggregate_forward_unchecked,
        aggregate_traced_safe, aggregate_traced_safe_span, aggregate_traced_unchecked,
    };

    let spec = production_exact();
    // 100% aggregate so every neuron in the sweep exercises the kernel: this is
    // the *isolated* measurement, deliberately the most favourable case.
    let net = with_aggregates(build_network(spec, 0x5152_5354), 100);
    let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
    let mut act = vec![0.0f32; net.num_neurons];
    act[..spec.num_inputs].copy_from_slice(&inputs);
    let synapse_total: u64 = net.neurons.iter().map(|n| n.num_synapses as u64).sum();

    for (label, squash) in [
        ("minimum", SquashType::Minimum),
        ("maximum", SquashType::Maximum),
        ("if", SquashType::If),
    ] {
        // Homogeneous per-type fixture so each kernel is measured on its own.
        let mut typed = net.clone();
        for neuron in &mut typed.neurons {
            neuron.squash_type = squash as u8;
        }

        let mut group = c.benchmark_group("aggregate_kernel");
        group.throughput(Throughput::Elements(synapse_total));
        group.bench_function(BenchmarkId::new(label, "safe"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    acc += aggregate_forward_safe(
                        black_box(&typed.synapses),
                        black_box(&act),
                        neuron,
                        squash,
                    )
                    .unwrap_or(0.0);
                }
                black_box(acc)
            });
        });
        // Attribution control: safe, but walking `start..end` and indexing the
        // synapse array — the form `activate` / `activate_into` actually ship.
        group.bench_function(BenchmarkId::new(label, "safe_indexed"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    acc += aggregate_forward_safe_indexed(
                        black_box(&typed.synapses),
                        black_box(&act),
                        neuron,
                        squash,
                    )
                    .unwrap_or(0.0);
                }
                black_box(acc)
            });
        });
        group.bench_function(BenchmarkId::new(label, "unchecked"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    // SAFETY: the fixture's `from_index` values are all drawn
                    // below the neuron's own global index, so every gather is
                    // within `act` (the same invariant `CompiledNetwork::new`
                    // enforces for loaded networks).
                    acc += unsafe {
                        aggregate_forward_unchecked(
                            black_box(&typed.synapses),
                            black_box(&act),
                            neuron,
                            squash,
                        )
                    }
                    .unwrap_or(0.0);
                }
                black_box(acc)
            });
        });
        group.finish();

        let mut group = c.benchmark_group("aggregate_kernel_traced");
        group.throughput(Throughput::Elements(synapse_total));
        group.bench_function(BenchmarkId::new(label, "safe"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    let (v, t) = aggregate_traced_safe(
                        black_box(&typed.synapses),
                        black_box(&act),
                        neuron,
                        squash,
                    )
                    .unwrap_or((0.0, 0.0));
                    acc += v + t;
                }
                black_box(acc)
            });
        });
        // Attribution control: safe, span-slice iteration with `enumerate`.
        group.bench_function(BenchmarkId::new(label, "safe_span"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    let (v, t) = aggregate_traced_safe_span(
                        black_box(&typed.synapses),
                        black_box(&act),
                        neuron,
                        squash,
                    )
                    .unwrap_or((0.0, 0.0));
                    acc += v + t;
                }
                black_box(acc)
            });
        });
        group.bench_function(BenchmarkId::new(label, "unchecked"), |b| {
            b.iter(|| {
                let mut acc = 0.0f32;
                for neuron in &typed.neurons {
                    // SAFETY: as above.
                    let (v, t) = unsafe {
                        aggregate_traced_unchecked(
                            black_box(&typed.synapses),
                            black_box(&act),
                            neuron,
                            squash,
                        )
                    }
                    .unwrap_or((0.0, 0.0));
                    acc += v + t;
                }
                black_box(acc)
            });
        });
        group.finish();
    }
}

#[cfg(not(feature = "experimental-aggregate-unchecked"))]
fn bench_kernels(_c: &mut Criterion) {
    // The safe control lives in the prototype module, so the isolated A/B is
    // only available with the feature on. The end-to-end groups still run.
}

criterion_group!(
    benches,
    bench_kernels,
    bench_forward_pass,
    bench_traced,
    bench_scoring
);
criterion_main!(benches);
