//! Deterministic fixtures shared by the `hot_paths` Criterion harness
//! (Issue #152) and the `bench_fixtures` integration test (Issue #176).
//!
//! Kept in `benches/common/` (a subdirectory, so Cargo does **not** treat it as
//! an auto-discovered bench target) and reused verbatim by the test via
//! `#[path = "../benches/common/mod.rs"]`. That makes the network-building logic
//! a single source of truth that is exercised by a real `cargo test` run rather
//! than only compiled inside the `harness = false` bench.

use neat_core::network::{CompiledNetwork, NeuronData, SynapseData};
use neat_core::squash::SquashType;
use neat_core::topological_backprop::{
    NEURON_TYPE_HIDDEN, NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, NeuronInput, SynapseInput,
};

/// Tiny deterministic PRNG (SplitMix64-style) so the harness produces fixed
/// topologies and weights without pulling in an `rand` dependency.
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[-1.0, 1.0)`.
    pub fn next_signed(&mut self) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        unit * 2.0 - 1.0
    }

    /// Uniform integer in `[0, bound)`.
    pub fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// How a neuron's incoming-connection count is chosen.
#[derive(Clone, Copy)]
pub enum FanIn {
    /// Every neuron draws exactly this many incoming connections (capped by the
    /// number of strictly earlier neurons). Used by the dense synthetic shapes,
    /// so their topologies — and therefore existing baselines — are unchanged.
    Fixed(usize),
    /// Per-neuron fan-in drawn uniformly in `[1, 2*avg - 1]` so the mean matches
    /// the production creature's sparse average (~13). Used by the wide/shallow
    /// `production` shapes, which are gather-bound rather than dense.
    VariedAround(usize),
}

impl FanIn {
    /// Number of incoming connections for a neuron that has `max_fan` strictly
    /// earlier neurons to draw from. Only [`FanIn::VariedAround`] consumes an
    /// RNG draw, so [`FanIn::Fixed`] shapes keep their exact prior sequence.
    pub fn draw(self, rng: &mut Lcg, max_fan: usize) -> usize {
        let target = match self {
            FanIn::Fixed(f) => f,
            FanIn::VariedAround(avg) => {
                let span = (2 * avg).saturating_sub(1).max(1);
                1 + rng.next_below(span)
            }
        };
        target.min(max_fan)
    }
}

/// A benchmark network shape.
pub struct NetSpec {
    pub label: &'static str,
    /// Total neurons, including the input layer.
    pub num_neurons: usize,
    pub num_inputs: usize,
    pub num_outputs: usize,
    pub fan_in: FanIn,
}

impl NetSpec {
    pub fn num_non_inputs(&self) -> usize {
        self.num_neurons - self.num_inputs
    }
}

/// Representative network shapes wired into every hot-path group.
///
/// The three synthetic shapes are dense feedforward nets with a fixed fan-in.
/// The two `production` shapes (Issue #176) mirror the real production creature:
/// a huge input layer, a modest neuron count and a sparse ~13 average fan-in,
/// which is gather-bound in a way the dense shapes are not. `production_2x`
/// doubles neurons and synapses to cover #175's "or larger creatures" clause.
pub const NETWORKS: [NetSpec; 5] = [
    NetSpec {
        label: "small_50",
        num_neurons: 50,
        num_inputs: 8,
        num_outputs: 4,
        fan_in: FanIn::Fixed(12),
    },
    NetSpec {
        label: "medium_500",
        num_neurons: 500,
        num_inputs: 16,
        num_outputs: 8,
        fan_in: FanIn::Fixed(16),
    },
    NetSpec {
        label: "large_5000",
        num_neurons: 5000,
        num_inputs: 32,
        num_outputs: 16,
        fan_in: FanIn::Fixed(24),
    },
    NetSpec {
        label: "production",
        // 2461 inputs + 1673 hidden/output neurons; ~13 avg fan-in ⇒ ~21.7k synapses.
        num_neurons: 4134,
        num_inputs: 2461,
        num_outputs: 1,
        fan_in: FanIn::VariedAround(13),
    },
    NetSpec {
        label: "production_2x",
        // ~2x the production neuron and synapse counts ("or larger creatures").
        num_neurons: 8268,
        num_inputs: 4922,
        num_outputs: 2,
        fan_in: FanIn::VariedAround(13),
    },
];

/// Build a deterministic feedforward [`CompiledNetwork`] from a [`NetSpec`].
///
/// Non-input neurons are emitted in topological order; each draws up to its
/// fan-in incoming connections from strictly earlier neurons, giving a realistic
/// synapse density without recurrent edges.
pub fn build_network(spec: &NetSpec, seed: u64) -> CompiledNetwork {
    let num_neurons = spec.num_neurons;
    let num_inputs = spec.num_inputs;
    let mut rng = Lcg::new(seed);
    let num_non_inputs = spec.num_non_inputs();
    let mut neurons = Vec::with_capacity(num_non_inputs);
    let mut synapses = Vec::new();

    for n in 0..num_non_inputs {
        let global_idx = num_inputs + n;
        let this_fan = spec.fan_in.draw(&mut rng, global_idx);
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
        // Issue #155 - 4-way batch scratch buffers
        batch_activations: [
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
        ],
        batch_hints: [
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
        ],
        batch_traces: [
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
        ],
    }
}

/// Deterministic input vector of length `n`.
pub fn build_inputs(n: usize, seed: u64) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    (0..n).map(|_| rng.next_signed()).collect()
}

/// Owned backing storage for a `PropagateInput`; built once outside the loop.
pub struct BackpropData {
    pub neurons: Vec<NeuronInput>,
    pub synapses: Vec<SynapseInput>,
    pub inward_starts: Vec<u32>,
    pub inward_counts: Vec<u32>,
    pub inward_indices: Vec<u32>,
    pub reverse_topo_order: Vec<u32>,
    pub expected: Vec<f32>,
    pub input_count: u32,
    pub output_count: u32,
}

/// Build a deterministic feedforward backprop input from a [`NetSpec`].
pub fn build_backprop_data(spec: &NetSpec, seed: u64) -> BackpropData {
    let num_neurons = spec.num_neurons;
    let num_inputs = spec.num_inputs;
    let num_outputs = spec.num_outputs;
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

        let this_fan = spec.fan_in.draw(&mut rng, global_idx);
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

pub fn make_neuron(squash: SquashType, neuron_type: u8, adjusted_activation: f32) -> NeuronInput {
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
