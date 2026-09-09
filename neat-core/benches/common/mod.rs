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
    /// Distribute *exactly* this many synapses across the non-input neurons, as
    /// evenly as possible. Used by the `production_exact` shape (Issue #286) so
    /// the fixture reproduces the committed production creature's synapse
    /// count to the synapse, not just its ~13 average. See
    /// [`FanIn::exact_schedule`].
    ExactTotal(usize),
}

impl FanIn {
    /// Number of incoming connections for a neuron that has `max_fan` strictly
    /// earlier neurons to draw from. Only [`FanIn::VariedAround`] consumes an
    /// RNG draw, so [`FanIn::Fixed`] shapes keep their exact prior sequence.
    /// [`FanIn::ExactTotal`] is not drawn per-neuron — its per-neuron counts come
    /// from [`FanIn::exact_schedule`] — so `draw` treats it as a no-connection
    /// fallback that is never reached on the exact path.
    pub fn draw(self, rng: &mut Lcg, max_fan: usize) -> usize {
        let target = match self {
            FanIn::Fixed(f) => f,
            FanIn::VariedAround(avg) => {
                let span = (2 * avg).saturating_sub(1).max(1);
                1 + rng.next_below(span)
            }
            FanIn::ExactTotal(_) => 0,
        };
        target.min(max_fan)
    }

    /// Pre-computed per-neuron fan-in schedule for variants whose total synapse
    /// count is fixed exactly. Returns `None` for the RNG-drawn variants
    /// ([`FanIn::Fixed`] / [`FanIn::VariedAround`]) so their existing draw
    /// sequence — and therefore every committed baseline — is left untouched.
    ///
    /// [`FanIn::ExactTotal`] spreads `total` synapses as evenly as possible
    /// across `num_non_inputs` neurons: each neuron gets `total / num_non_inputs`
    /// synapses, and the `total % num_non_inputs` remainder is handed out
    /// one-per-neuron using a Bresenham stride so the extra-synapse neurons are
    /// interleaved across the range rather than clustered at the front. The
    /// schedule therefore sums to **exactly** `total` by construction, with at
    /// most two distinct fan-in values (`base` and `base + 1`).
    pub fn exact_schedule(self, num_non_inputs: usize) -> Option<Vec<usize>> {
        match self {
            FanIn::ExactTotal(total) => {
                if num_non_inputs == 0 {
                    return Some(Vec::new());
                }
                let base = total / num_non_inputs;
                let remainder = total % num_non_inputs;
                let schedule = (0..num_non_inputs)
                    .map(|n| {
                        // Bresenham even distribution: exactly `remainder` neurons
                        // across the whole range receive one extra synapse.
                        let extra =
                            (n + 1) * remainder / num_non_inputs - n * remainder / num_non_inputs;
                        base + extra
                    })
                    .collect();
                Some(schedule)
            }
            FanIn::Fixed(_) | FanIn::VariedAround(_) => None,
        }
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
/// `production_exact` (Issue #286) pins the fixture to the committed production
/// creature topology — 1,666 non-input neurons, 21,513
/// synapses, 2,461 inputs — to the synapse, giving the Criterion baseline a
/// reproducible production-topology anchor.
pub const NETWORKS: [NetSpec; 6] = [
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
    NetSpec {
        label: "production_exact",
        // Exact production creature topology (Issue #286): 2461 inputs +
        // 1666 non-input neurons = 4127 total, exactly 21,513 synapses. Unlike
        // `production`'s ~13-average VariedAround, ExactTotal pins the synapse
        // count to the committed production model so the baseline is anchored to
        // the real topology rather than an approximation of it.
        num_neurons: 4127,
        num_inputs: 2461,
        num_outputs: 1,
        fan_in: FanIn::ExactTotal(21_513),
    },
];

/// Build a deterministic feedforward [`CompiledNetwork`] from a [`NetSpec`].
///
/// Non-input neurons are emitted in topological order; each draws up to its
/// fan-in incoming connections from strictly earlier neurons, giving a realistic
/// synapse density without recurrent edges.
pub fn build_network(spec: &NetSpec, seed: u64) -> CompiledNetwork {
    let num_inputs = spec.num_inputs;
    let mut rng = Lcg::new(seed);
    let num_non_inputs = spec.num_non_inputs();
    // `Some` only for exact-count shapes; the RNG-drawn shapes stay `None` so
    // their per-neuron `draw` sequence — and every committed baseline — is
    // byte-for-byte unchanged.
    let schedule = spec.fan_in.exact_schedule(num_non_inputs);
    let mut neurons = Vec::with_capacity(num_non_inputs);
    let mut synapses = Vec::new();

    for n in 0..num_non_inputs {
        let global_idx = num_inputs + n;
        let this_fan = match &schedule {
            // Cap by strictly-earlier neurons for soundness; never truncates on
            // the production_exact shape (num_inputs ≫ per-neuron fan-in).
            Some(sched) => sched[n].min(global_idx),
            None => spec.fan_in.draw(&mut rng, global_idx),
        };
        let start_synapse = synapses.len() as u32;
        for _ in 0..this_fan {
            let from = rng.next_below(global_idx);
            synapses.push(SynapseData {
                weight: rng.next_signed() * 0.5,
                from_index: from as u16,
                synapse_type: 0,
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

    CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("fixture must satisfy the load-time index invariant")
}

/// Deterministic input vector of length `n`.
pub fn build_inputs(n: usize, seed: u64) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    (0..n).map(|_| rng.next_signed()).collect()
}

/// Production-representative record count for the scoring throughput benches
/// (Issue #228), calibrated to committed production-run telemetry rather than
/// an arbitrary token batch.
///
/// Derivation (from the 32-generation production
/// run whose totals match the run summary):
///
/// - `training_data_size_bytes` = 22,097,375,712 across
///   `training_data_files` = 520 shards.
/// - Per-record width = `num_inputs * size_of::<f32>()` = 2461 * 4 = 9844 bytes.
/// - Whole-corpus records ≈ 22,097,375,712 / 9844 ≈ **2.24 million** — the count
///   a single creature forward-passes per generation to compute its fitness.
/// - Per training shard ≈ 2,244,755 / 520 ≈ **4,317 records** — the natural
///   granularity the streaming scorer holds in flight.
///
/// Materialising the whole 2.24 M-record corpus in memory (~21 GiB at
/// production width) is infeasible for a micro-benchmark, so the harness scores
/// **one production shard's worth**, rounded down to the nearest power of two
/// (4096). At ~9.8 KiB/record this batch is ~40 MiB — already far larger than
/// any CPU cache, so its memory-traffic behaviour is production-representative,
/// and records/sec extrapolates directly to the full corpus pass (throughput is
/// size-invariant once the pool/allocation overhead is amortised). This is 2x
/// the prior token batch of 2048.
pub const PRODUCTION_SCORING_RECORDS: usize = 4096;

/// Build `count` distinct input records of length `num_inputs`, each drawn from
/// the fixed-seed PRNG with a per-record seed so the batch is reproducible yet
/// not a degenerate run of identical rows. Shared by the `parallel_scoring` and
/// `hot_paths` scoring groups so both measure the same synthesised workload.
pub fn build_records(num_inputs: usize, count: usize) -> Vec<Vec<f32>> {
    (0..count)
        .map(|i| build_inputs(num_inputs, 0x5C0E_0000 + i as u64))
        .collect()
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
    // Exact-count shapes use a pre-computed schedule; RNG-drawn shapes stay
    // `None` so their existing draw sequence is untouched (mirrors
    // `build_network`).
    let schedule = spec.fan_in.exact_schedule(spec.num_non_inputs());
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

        let this_fan = match &schedule {
            Some(sched) => sched[global_idx - num_inputs].min(global_idx),
            None => spec.fan_in.draw(&mut rng, global_idx),
        };
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

/// The three aggregate squashes swept by the `aggregate_frequency` bench
/// (Issue #510), cycled across the rewritten neurons.
pub const AGGREGATES: [SquashType; 3] = [SquashType::Minimum, SquashType::Maximum, SquashType::If];

/// Rewrite a deterministic `percent` of a network's non-input neurons to the
/// three aggregate squashes, cycling `Minimum`/`Maximum`/`If` and spreading them
/// evenly rather than clustering them (Issue #510).
///
/// Topology, weights, biases and fan-in are untouched — only `squash_type` and,
/// for `If` neurons, the synapse types change. That keeps the gather pattern at
/// production sparsity so aggregate **frequency** is the only swept variable.
/// The committed `production*` fixtures are homogeneous `Tanh`, so this is the
/// only way to put aggregate neurons in a production-sized creature.
///
/// Issue #625 - `CompiledNetwork`'s fields are private, so the rewrite happens
/// on copies of the neuron and synapse tables and the result is rebuilt through
/// `from_parts`, which re-runs the load-time index validation.
pub fn with_aggregates(net: CompiledNetwork, percent: usize) -> CompiledNetwork {
    if percent == 0 {
        return net;
    }
    let num_non_inputs = net.neurons().len();
    let target = num_non_inputs * percent / 100;
    if target == 0 {
        return net;
    }
    let stride = (num_non_inputs / target).max(1);

    let mut neurons = net.neurons().to_vec();
    let mut synapses = net.synapses().to_vec();
    for n in (0..num_non_inputs).step_by(stride) {
        let squash = AGGREGATES[(n / stride) % AGGREGATES.len()];
        neurons[n].squash_type = squash as u8;

        if squash == SquashType::If {
            let start = neurons[n].start_synapse as usize;
            let end = start + neurons[n].num_synapses as usize;
            for (k, synapse) in synapses[start..end].iter_mut().enumerate() {
                // First synapse is the condition; the rest alternate the
                // positive and negative branches.
                synapse.synapse_type = match k {
                    0 => 1,               // Condition
                    _ if k % 2 == 1 => 3, // Positive
                    _ => 2,               // Negative
                };
            }
        }
    }
    CompiledNetwork::from_parts(net.num_inputs(), neurons, synapses)
        .expect("rewriting squash types cannot break the index invariant")
}

/// Count of non-constant neurons using an aggregate squash.
pub fn aggregate_count(net: &CompiledNetwork) -> usize {
    net.neurons()
        .iter()
        .filter(|n| !n.is_constant && SquashType::from(n.squash_type).is_aggregate())
        .count()
}
