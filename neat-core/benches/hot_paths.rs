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

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use neat_core::loss::mse_sum_batch_packed;
use neat_core::network::{CompiledNetwork, NeuronData, SynapseData};
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_simd,
    weighted_sum_simd_4records, weighted_sum_simd_8records,
};
use neat_core::squash::{SquashType, apply_squash};
use neat_core::topological_backprop::{
    NEURON_TYPE_HIDDEN, NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, NeuronInput, PropagateInput,
    SynapseInput, propagate_topological_loop,
};
use neat_core::unsquash::apply_unsquash;

/// Tiny deterministic PRNG (SplitMix64-style) so the harness produces fixed
/// topologies and weights without pulling in an `rand` dependency.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[-1.0, 1.0)`.
    fn next_signed(&mut self) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        unit * 2.0 - 1.0
    }

    /// Uniform integer in `[0, bound)`.
    fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Build a deterministic feedforward [`CompiledNetwork`].
///
/// Non-input neurons are emitted in topological order; each draws up to
/// `fan_in` incoming connections from strictly earlier neurons, giving a
/// realistic synapse density without recurrent edges.
fn build_network(
    num_neurons: usize,
    num_inputs: usize,
    fan_in: usize,
    seed: u64,
) -> CompiledNetwork {
    let mut rng = Lcg::new(seed);
    let num_non_inputs = num_neurons - num_inputs;
    let mut neurons = Vec::with_capacity(num_non_inputs);
    let mut synapses = Vec::new();

    for n in 0..num_non_inputs {
        let global_idx = num_inputs + n;
        let this_fan = fan_in.min(global_idx);
        let start_synapse = synapses.len() as u32;
        for _ in 0..this_fan {
            let from = rng.next_below(global_idx);
            synapses.push(SynapseData {
                weight: rng.next_signed() * 0.5,
                from_index: from as u32,
                synapse_type: 0,
                _padding: [0; 3],
            });
        }
        neurons.push(NeuronData {
            bias: rng.next_signed() * 0.1,
            start_synapse,
            num_synapses: this_fan as u16,
            squash_type: SquashType::Tanh as u8,
            is_constant: false,
        });
    }

    let estimated_trace_size = (num_non_inputs / 10).max(1) * 2 + 1;
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::with_capacity(estimated_trace_size),
    }
}

/// Deterministic input vector of length `n`.
fn build_inputs(n: usize, seed: u64) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    (0..n).map(|_| rng.next_signed()).collect()
}

/// Representative network sizes: (label, total neurons, inputs, outputs, fan-in).
const NETWORKS: [(&str, usize, usize, usize, usize); 3] = [
    ("small_50", 50, 8, 4, 12),
    ("medium_500", 500, 16, 8, 16),
    ("large_5000", 5000, 32, 16, 24),
];

/// Forward pass — `CompiledNetwork::activate` across representative sizes.
fn bench_forward_pass(c: &mut Criterion) {
    let mut group = c.benchmark_group("forward_pass");
    for (label, num_neurons, num_inputs, num_outputs, fan_in) in NETWORKS {
        let mut net = build_network(num_neurons, num_inputs, fan_in, 0x5152_5354);
        let inputs = build_inputs(num_inputs, 0xA1B2_C3D4);
        group.throughput(Throughput::Elements(num_neurons as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &inputs, |b, inputs| {
            b.iter(|| {
                let out = net.activate(black_box(inputs), black_box(num_outputs));
                black_box(out);
            });
        });
    }
    group.finish();
}

/// Batched scoring — 4-way traced activation and the 8-record loss path.
fn bench_batched_scoring(c: &mut Criterion) {
    let mut group = c.benchmark_group("batched_scoring");

    for (label, num_neurons, num_inputs, num_outputs, fan_in) in NETWORKS {
        let net = build_network(num_neurons, num_inputs, fan_in, 0x5152_5354);

        // 4 records packed back-to-back for the traced batch path.
        let batch4 = build_inputs(num_inputs * 4, 0x0BAD_F00D);
        group.bench_with_input(
            BenchmarkId::new("trace_batch_4way", label),
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
            BenchmarkId::new("mse_sum_8records", label),
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

/// Owned backing storage for a [`PropagateInput`]; built once outside the loop.
struct BackpropData {
    neurons: Vec<NeuronInput>,
    synapses: Vec<SynapseInput>,
    inward_starts: Vec<u32>,
    inward_counts: Vec<u32>,
    inward_indices: Vec<u32>,
    reverse_topo_order: Vec<u32>,
    expected: Vec<f32>,
    input_count: u32,
    output_count: u32,
}

/// Build a deterministic feedforward backprop input of `num_neurons` neurons.
fn build_backprop_data(
    num_neurons: usize,
    num_inputs: usize,
    num_outputs: usize,
    fan_in: usize,
    seed: u64,
) -> BackpropData {
    let mut rng = Lcg::new(seed);
    let mut neurons = Vec::with_capacity(num_neurons);

    // Input neurons first.
    for _ in 0..num_inputs {
        neurons.push(make_neuron(
            SquashType::Identity,
            NEURON_TYPE_INPUT,
            rng.next_signed(),
        ));
    }

    // Synapses grouped by target neuron so the inward adjacency is contiguous.
    let mut synapses: Vec<SynapseInput> = Vec::new();
    let mut inward_starts = vec![0u32; num_neurons];
    let mut inward_counts = vec![0u32; num_neurons];
    let mut inward_indices: Vec<u32> = Vec::new();

    for global_idx in num_inputs..num_neurons {
        let is_output = global_idx >= num_neurons - num_outputs;
        let neuron_type = if is_output {
            NEURON_TYPE_OUTPUT
        } else {
            NEURON_TYPE_HIDDEN
        };
        neurons.push(make_neuron(
            SquashType::Tanh,
            neuron_type,
            rng.next_signed(),
        ));

        let this_fan = fan_in.min(global_idx);
        inward_starts[global_idx] = inward_indices.len() as u32;
        inward_counts[global_idx] = this_fan as u32;
        for _ in 0..this_fan {
            let from = rng.next_below(global_idx);
            let weight = rng.next_signed() * 0.5;
            inward_indices.push(synapses.len() as u32);
            synapses.push(SynapseInput {
                from: from as u32,
                to: global_idx as u32,
                original_weight: weight,
                adjusted_weight: weight,
                is_self_loop: false,
            });
        }
    }

    // Reverse topological order: non-input neurons from last back to first.
    let reverse_topo_order: Vec<u32> = (num_inputs..num_neurons).rev().map(|i| i as u32).collect();
    let expected = (0..num_outputs).map(|_| rng.next_signed()).collect();

    BackpropData {
        neurons,
        synapses,
        inward_starts,
        inward_counts,
        inward_indices,
        reverse_topo_order,
        expected,
        input_count: num_inputs as u32,
        output_count: num_outputs as u32,
    }
}

fn make_neuron(squash: SquashType, neuron_type: u8, adjusted_activation: f32) -> NeuronInput {
    NeuronInput {
        squash_type: squash as u8,
        neuron_type,
        propagate_needed: true,
        update_needed: true,
        hint_value: 0.0,
        range_low: -1.0e6,
        range_high: 1.0e6,
        adjusted_activation,
        adjusted_bias: 0.0,
    }
}

/// Backprop — one `propagate_topological_loop` step on representative sizes.
fn bench_backprop(c: &mut Criterion) {
    let mut group = c.benchmark_group("backprop");
    for (label, num_neurons, num_inputs, num_outputs, fan_in) in NETWORKS {
        let data = build_backprop_data(num_neurons, num_inputs, num_outputs, fan_in, 0x1357_9BDF);
        group.throughput(Throughput::Elements(num_neurons as u64));
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
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
            from_index: i as u32,
            synapse_type: 0,
            _padding: [0; 3],
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
}

criterion_group!(
    benches,
    bench_forward_pass,
    bench_batched_scoring,
    bench_backprop,
    bench_activation_primitives,
);
criterion_main!(benches);
