//! Criterion benchmark harness for neat-core hot paths (Issue #152).
//!
//! Opt-in only: `harness = false` plus the `[[bench]]` target in
//! `neat-core/Cargo.toml` keeps these out of `cargo test` / the `quality.sh`
//! gate. Run them explicitly with:
//!
//! ```text
//! cargo bench -p neat-core
//! ```
//!
//! The harness is deterministic — networks are built from a fixed-seed LCG and
//! fixed topologies — so before/after comparisons across a code change are
//! meaningful. See `neat-core/benches/README.md` for the comparison workflow.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use neat_core::loss::mse_sum_batch_packed;
use neat_core::network::SynapseData;
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_simd,
    weighted_sum_simd_4records, weighted_sum_simd_8records,
};
use neat_core::squash::{SquashType, apply_squash};
use neat_core::squash_simd::squash_x4;
use neat_core::topological_backprop::{PropagateInput, propagate_topological_loop};
use neat_core::unsquash::apply_unsquash;

/// Deterministic network/backprop fixtures, shared with the `bench_fixtures`
/// integration test (Issue #176) so the production-scale builders are exercised
/// by a real `cargo test` run as well as the harness.
mod common;
use common::{Lcg, NETWORKS, build_backprop_data, build_inputs, build_network};

/// Forward pass — `CompiledNetwork::activate` across representative sizes.
fn bench_forward_pass(c: &mut Criterion) {
    let mut group = c.benchmark_group("forward_pass");
    for spec in &NETWORKS {
        let mut net = build_network(spec, 0x5152_5354);
        let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
        let num_outputs = spec.num_outputs;
        group.throughput(Throughput::Elements(spec.num_neurons as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(spec.label),
            &inputs,
            |b, inputs| {
                b.iter(|| {
                    let out = net.activate(black_box(inputs), black_box(num_outputs));
                    black_box(out);
                });
            },
        );
    }
    group.finish();
}

/// Batched scoring — 4-way traced activation and the 8-record loss path.
fn bench_batched_scoring(c: &mut Criterion) {
    let mut group = c.benchmark_group("batched_scoring");

    for spec in &NETWORKS {
        let num_inputs = spec.num_inputs;
        let num_outputs = spec.num_outputs;
        // `mut` so the 4-way traced batch path can reuse its scratch buffers (Issue #155).
        let mut net = build_network(spec, 0x5152_5354);

        // 4 records packed back-to-back for the traced batch path.
        let batch4 = build_inputs(num_inputs * 4, 0x0BAD_F00D);
        group.bench_with_input(
            BenchmarkId::new("trace_batch_4way", spec.label),
            &batch4,
            |b, batch| {
                b.iter(|| {
                    let out = net.activate_and_trace_batch_4way(
                        black_box(batch),
                        black_box(num_inputs),
                        black_box(num_outputs),
                    );
                    black_box(out);
                });
            },
        );

        // 8 records of [inputs..., targets...] for the fused MSE loss path.
        let mut loss_net = net.clone();
        let records = build_inputs((num_inputs + num_outputs) * 8, 0xFEED_BEEF);
        group.bench_with_input(
            BenchmarkId::new("mse_sum_8records", spec.label),
            &records,
            |b, records| {
                b.iter(|| {
                    let sum = mse_sum_batch_packed(
                        &mut loss_net,
                        black_box(records),
                        black_box(num_inputs),
                        black_box(num_outputs),
                        black_box(true),
                    );
                    black_box(sum);
                });
            },
        );
    }
    group.finish();
}

/// Backprop — one `propagate_topological_loop` step on representative sizes.
fn bench_backprop(c: &mut Criterion) {
    let mut group = c.benchmark_group("backprop");
    for spec in &NETWORKS {
        let data = build_backprop_data(spec, 0x1357_9BDF);
        group.throughput(Throughput::Elements(spec.num_neurons as u64));
        group.bench_function(BenchmarkId::from_parameter(spec.label), |b| {
            b.iter(|| {
                let input = PropagateInput {
                    neurons: &data.neurons,
                    synapses: &data.synapses,
                    inward_starts: &data.inward_starts,
                    inward_counts: &data.inward_counts,
                    inward_synapse_indices: &data.inward_indices,
                    reverse_topo_order: &data.reverse_topo_order,
                    expected: &data.expected,
                    input_count: data.input_count,
                    output_count: data.output_count,
                    plank_constant: 1e-7,
                    normalise_gradients: false,
                };
                let out = propagate_topological_loop(black_box(&input));
                black_box(out);
            });
        });
    }
    group.finish();
}

/// Representative spread of activation functions for the squash primitives.
const SQUASH_SPREAD: [SquashType; 10] = [
    SquashType::Identity,
    SquashType::Relu,
    SquashType::LeakyRelu,
    SquashType::Logistic,
    SquashType::Tanh,
    SquashType::Gelu,
    SquashType::Swish,
    SquashType::Mish,
    SquashType::Sine,
    SquashType::Gaussian,
];

/// Activation primitives — `weighted_sum_simd` family plus squash/unsquash.
fn bench_activation_primitives(c: &mut Criterion) {
    // weighted_sum_simd family over a representative synapse block.
    let mut sum_group = c.benchmark_group("weighted_sum_simd");
    let synapse_count = 64usize;
    let mut rng = Lcg::new(0x2468_ACE0);
    let synapses: Vec<SynapseData> = (0..synapse_count)
        .map(|i| SynapseData {
            weight: rng.next_signed(),
            from_index: i as u16,
            synapse_type: 0,
        })
        .collect();
    let activations: Vec<f32> = (0..synapse_count).map(|_| rng.next_signed()).collect();
    let (a0, a1, a2, a3) = (
        activations.clone(),
        activations.clone(),
        activations.clone(),
        activations.clone(),
    );
    let (a4, a5, a6, a7) = (
        activations.clone(),
        activations.clone(),
        activations.clone(),
        activations.clone(),
    );
    let end = synapse_count;

    sum_group.bench_function("single", |b| {
        b.iter(|| {
            black_box(weighted_sum_simd(
                black_box(&synapses),
                black_box(&activations),
                0,
                end,
                0.25,
            ))
        });
    });
    sum_group.bench_function("no_bias", |b| {
        b.iter(|| {
            black_box(weighted_sum_no_bias_simd(
                black_box(&synapses),
                black_box(&activations),
                0,
                end,
            ))
        });
    });
    sum_group.bench_function("of_squares", |b| {
        b.iter(|| {
            black_box(weighted_sum_of_squares_simd(
                black_box(&synapses),
                black_box(&activations),
                0,
                end,
            ))
        });
    });
    sum_group.bench_function("batch_4records", |b| {
        b.iter(|| {
            black_box(weighted_sum_simd_4records(
                black_box(&synapses),
                &a0,
                &a1,
                &a2,
                &a3,
                0,
                end,
                0.25,
            ))
        });
    });
    sum_group.bench_function("batch_8records", |b| {
        b.iter(|| {
            black_box(weighted_sum_simd_8records(
                black_box(&synapses),
                &a0,
                &a1,
                &a2,
                &a3,
                &a4,
                &a5,
                &a6,
                &a7,
                0,
                end,
                0.25,
            ))
        });
    });
    sum_group.finish();

    // apply_squash / apply_unsquash across a spread of SquashTypes.
    let mut squash_group = c.benchmark_group("squash");
    for squash_type in SQUASH_SPREAD {
        let label = format!("{squash_type:?}");
        squash_group.bench_with_input(
            BenchmarkId::new("apply_squash", label.clone()),
            &squash_type,
            |b, &st| {
                b.iter(|| black_box(apply_squash(black_box(st), black_box(0.42))));
            },
        );
        squash_group.bench_with_input(
            BenchmarkId::new("apply_unsquash", label),
            &squash_type,
            |b, &st| {
                b.iter(|| {
                    black_box(apply_unsquash(
                        black_box(st),
                        black_box(0.42),
                        black_box(0.0),
                    ))
                });
            },
        );
    }
    squash_group.finish();

    // Lane-parallel squash (Issue #180): vectorised 4-lane approximation versus
    // four scalar `apply_squash` calls, for the hot transcendental squashes.
    let mut squash4_group = c.benchmark_group("squash_x4");
    let lanes = [0.42_f32, -1.3, 2.7, -0.05];
    for squash_type in [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
    ] {
        let label = format!("{squash_type:?}");
        squash4_group.bench_with_input(
            BenchmarkId::new("scalar_x4", label.clone()),
            &squash_type,
            |b, &st| {
                b.iter(|| {
                    let x = black_box(lanes);
                    black_box([
                        apply_squash(st, x[0]),
                        apply_squash(st, x[1]),
                        apply_squash(st, x[2]),
                        apply_squash(st, x[3]),
                    ])
                });
            },
        );
        squash4_group.bench_with_input(
            BenchmarkId::new("simd_x4", label),
            &squash_type,
            |b, &st| {
                b.iter(|| black_box(squash_x4(black_box(st), black_box(lanes))));
            },
        );
    }
    squash4_group.finish();
}

criterion_group!(
    benches,
    bench_forward_pass,
    bench_batched_scoring,
    bench_backprop,
    bench_activation_primitives,
);
criterion_main!(benches);
