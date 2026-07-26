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
use neat_core::topology_ops::compute_reverse_topological_order;
use neat_core::unsquash::apply_unsquash;
use neat_core::wasm_dataset::TrainingDataset;

/// Deterministic network/backprop fixtures, shared with the `bench_fixtures`
/// integration test (Issue #176) so the production-scale builders are exercised
/// by a real `cargo test` run as well as the harness.
mod common;
use common::{
    Lcg, NETWORKS, PRODUCTION_SCORING_RECORDS, build_backprop_data, build_inputs, build_network,
    build_records,
};

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

        // Production-sized fused MSE batch (Issue #384). The 8-record case above
        // over-weights the per-call buffer setup; scoring a full
        // `PRODUCTION_SCORING_RECORDS` batch measures the steady-state gather
        // cost that the #287 interleaved reroute targets. Only the gather-bound
        // `production*` shapes are representative, so restrict the heavy case to
        // them (reachable via `--bench hot_paths -- mse_sum_production`).
        if spec.label.starts_with("production") {
            let mut loss_net_prod = net.clone();
            let prod_records = build_inputs(
                (num_inputs + num_outputs) * PRODUCTION_SCORING_RECORDS,
                0xFEED_BEEF,
            );
            group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));
            group.bench_with_input(
                BenchmarkId::new("mse_sum_production", spec.label),
                &prod_records,
                |b, records| {
                    b.iter(|| {
                        let sum = mse_sum_batch_packed(
                            &mut loss_net_prod,
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

/// Backprop setup — `compute_reverse_topological_order` over a creature's full
/// synapse list (Issue #388). This runs once per creature per generation to
/// order the backprop walk, so its allocation behaviour is on the
/// per-generation path.
fn bench_reverse_topological_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("reverse_topological_order");
    for spec in &NETWORKS {
        let data = build_backprop_data(spec, 0x1357_9BDF);
        let from: Vec<u32> = data.synapses.iter().map(|s| s.from).collect();
        let to: Vec<u32> = data.synapses.iter().map(|s| s.to).collect();
        let num_neurons = spec.num_neurons as u32;
        let num_inputs = spec.num_inputs as u32;
        group.throughput(Throughput::Elements(from.len() as u64));
        group.bench_function(BenchmarkId::from_parameter(spec.label), |b| {
            b.iter(|| {
                let order = compute_reverse_topological_order(
                    black_box(&from),
                    black_box(&to),
                    black_box(num_neurons),
                    black_box(num_inputs),
                );
                black_box(order);
            });
        });
    }
    group.finish();
}

/// Scoring throughput — a full production-shaped record batch pushed through
/// one creature via `score_records` (Issue #228). Sized to
/// [`PRODUCTION_SCORING_RECORDS`] so the single-core scoring figure in
/// `hot_paths` is measured at production record volume, matching the
/// `parallel_scoring` harness. Only the production shapes are gather-bound in
/// the way real scoring is, so this group covers just `production` /
/// `production_2x` and is reachable via `--bench hot_paths -- production`.
fn bench_scoring(c: &mut Criterion) {
    let mut group = c.benchmark_group("scoring");
    for spec in NETWORKS
        .iter()
        .filter(|s| s.label.starts_with("production"))
    {
        let net = build_network(spec, 0x5EED);
        let records = build_records(net.num_inputs(), PRODUCTION_SCORING_RECORDS);
        let num_outputs = spec.num_outputs;
        group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(spec.label),
            &records,
            |b, records| {
                b.iter(|| {
                    let out = net.score_records(black_box(records), black_box(num_outputs));
                    black_box(out);
                });
            },
        );
    }
    group.finish();
}

/// Flat-slice record **input** scoring — `score_records_flat` over the same
/// production shard the `scoring` group scores as `&[Vec<f32>]` (Issue #386).
///
/// Same kernel, same records, same output layout; only the input layout differs,
/// so the delta against `scoring` is the cost of the per-record `Vec` header
/// pointer-chase on each lane load. The one-heap-allocation-per-record the
/// `&[Vec<f32>]` signature forces on the *caller* is not measured here (both
/// fixtures are built outside the timed loop) — it is pure additional saving for
/// callers that already hold a contiguous buffer.
fn bench_scoring_flat(c: &mut Criterion) {
    let mut group = c.benchmark_group("scoring_flat");
    for spec in NETWORKS
        .iter()
        .filter(|s| s.label.starts_with("production"))
    {
        let net = build_network(spec, 0x5EED);
        let stride = net.num_inputs();
        let inputs: Vec<f32> = build_records(stride, PRODUCTION_SCORING_RECORDS)
            .into_iter()
            .flatten()
            .collect();
        let num_outputs = spec.num_outputs;
        group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(spec.label),
            &inputs,
            |b, inputs| {
                b.iter(|| {
                    let out = net.score_records_flat(
                        black_box(inputs),
                        black_box(stride),
                        black_box(num_outputs),
                    );
                    black_box(out);
                });
            },
        );
    }
    group.finish();
}

/// Dataset-offload evaluation — `TrainingDataset::evaluate_mse` over a
/// production-sized batch (Issue #386).
///
/// The WASM offload lane (Issue #298) already stores its inputs contiguously in
/// SoA layout, so this group measures the cost of the whole
/// bounds-check → forward-pass → MSE-accumulate call at
/// [`PRODUCTION_SCORING_RECORDS`] volume — the per-generation unit of work the
/// Memory64 lane performs. Sized identically to the `scoring` group so the two
/// are directly comparable.
fn bench_dataset_evaluate_mse(c: &mut Criterion) {
    let mut group = c.benchmark_group("dataset_evaluate_mse");
    for spec in NETWORKS
        .iter()
        .filter(|s| s.label.starts_with("production"))
    {
        let mut net = build_network(spec, 0x5EED);
        let inputs: Vec<f32> = build_records(spec.num_inputs, PRODUCTION_SCORING_RECORDS)
            .into_iter()
            .flatten()
            .collect();
        let targets = build_inputs(
            PRODUCTION_SCORING_RECORDS * spec.num_outputs,
            0x7A46_0E75_0000,
        );
        let dataset = TrainingDataset::from_soa(
            inputs,
            targets,
            TrainingDataConfig::new(spec.num_inputs, spec.num_outputs),
        )
        .expect("dataset fixture should be well-formed");

        group.throughput(Throughput::Elements(PRODUCTION_SCORING_RECORDS as u64));
        group.bench_function(BenchmarkId::from_parameter(spec.label), |b| {
            b.iter(|| {
                let mse = dataset
                    .evaluate_mse(&mut net, 0, black_box(PRODUCTION_SCORING_RECORDS))
                    .expect("batch is in range");
                black_box(mse);
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

/// A deterministic sorted forward-only topology for the topology-ops group.
///
/// Emits exactly `num_synapses` synapses spread as evenly as possible over the
/// non-input neurons, each drawing sources from strictly earlier neurons, so the
/// `(from, to)` list comes out ascending-sorted exactly as `validate_topology`
/// requires. Mirrors the production creature's shape at the neuron count the
/// mutation-time scan actually sees.
struct TopologySpec {
    label: &'static str,
    num_neurons: usize,
    num_inputs: usize,
    num_synapses: usize,
}

/// Topology shapes for `scan_available_connections` (Issue #387).
///
/// `n1666_21513` is the production anchor named in the issue: 1,666 neurons
/// carrying 21,513 synapses — a fill factor under 0.8%, which is what made the
/// old dense `n × n` matrix so wasteful.
const TOPOLOGIES: [TopologySpec; 2] = [
    TopologySpec {
        label: "n1666_21513",
        num_neurons: 1666,
        num_inputs: 100,
        num_synapses: 21_513,
    },
    TopologySpec {
        label: "n4127_21513",
        num_neurons: 4127,
        num_inputs: 2461,
        num_synapses: 21_513,
    },
];

/// Build `(from_indices, to_indices, is_constant)` for a [`TopologySpec`].
fn build_topology(spec: &TopologySpec) -> (Vec<u32>, Vec<u32>, Vec<u8>) {
    let mut rng = Lcg::new(0x7061_7468);
    let num_non_inputs = spec.num_neurons - spec.num_inputs;
    // Per-target fan-in, distributed as evenly as the synapse budget allows.
    let base = spec.num_synapses / num_non_inputs;
    let remainder = spec.num_synapses % num_non_inputs;

    let mut from_indices = Vec::with_capacity(spec.num_synapses);
    let mut to_indices = Vec::with_capacity(spec.num_synapses);
    // Collect per-`from` target lists so the emitted pairs come out sorted by
    // `from` then `to`, matching the ordering contract the scan relies on.
    let mut per_from: Vec<Vec<u32>> = vec![Vec::new(); spec.num_neurons];

    for offset in 0..num_non_inputs {
        let to = spec.num_inputs + offset;
        let want = base + usize::from(offset < remainder);
        let fan = want.min(to);
        let mut drawn = 0usize;
        let mut attempts = 0usize;
        while drawn < fan && attempts < fan * 8 {
            attempts += 1;
            let from = rng.next_below(to);
            if per_from[from].contains(&(to as u32)) {
                continue;
            }
            per_from[from].push(to as u32);
            drawn += 1;
        }
    }

    for (from, targets) in per_from.iter_mut().enumerate() {
        targets.sort_unstable();
        for &to in targets.iter() {
            from_indices.push(from as u32);
            to_indices.push(to);
        }
    }

    // A handful of constant neurons, matching the production mix.
    let mut is_constant = vec![0u8; spec.num_neurons];
    for i in 0..spec.num_neurons {
        if i >= spec.num_inputs && i % 97 == 0 {
            is_constant[i] = 1;
        }
    }

    (from_indices, to_indices, is_constant)
}

/// Mutation-time topology ops — `scan_available_connections` (Issue #387).
fn bench_topology_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("topology_ops");
    for spec in &TOPOLOGIES {
        let (from_indices, to_indices, is_constant) = build_topology(spec);
        let num_neurons = spec.num_neurons as u32;
        let num_inputs = spec.num_inputs as u32;
        // One element per candidate slot the scan has to consider.
        group.throughput(Throughput::Elements(
            (spec.num_neurons * spec.num_neurons) as u64,
        ));
        group.bench_function(
            BenchmarkId::new("scan_available_connections", spec.label),
            |b| {
                b.iter(|| {
                    let out = scan_available_connections(
                        black_box(&from_indices),
                        black_box(&to_indices),
                        black_box(&is_constant),
                        black_box(num_neurons),
                        black_box(num_inputs),
                    );
                    black_box(out);
                });
            },
        );

        // Backprop-ordering setup — `compute_reverse_topological_order`
        // (Issue #388). Runs once per creature per generation, so its
        // per-neuron adjacency allocations sat on the hot path.
        group.throughput(Throughput::Elements(
            (spec.num_neurons + spec.num_synapses) as u64,
        ));
        group.bench_function(
            BenchmarkId::new("compute_reverse_topological_order", spec.label),
            |b| {
                b.iter(|| {
                    let out = compute_reverse_topological_order(
                        black_box(&from_indices),
                        black_box(&to_indices),
                        black_box(num_neurons),
                        black_box(num_inputs),
                    );
                    black_box(out);
                });
            },
        );
    }
    group.finish();
}

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
    bench_reverse_topological_order,
    bench_scoring,
    bench_scoring_flat,
    bench_dataset_evaluate_mse,
    bench_activation_primitives,
    bench_topology_ops,
);
criterion_main!(benches);
