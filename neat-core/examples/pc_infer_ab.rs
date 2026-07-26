//! Allocation + wall-clock A/B for the predictive-coding settling loop (Issue #389).
//!
//! Measures the shipped [`PredictiveCodingEngine::infer`] against a standalone
//! reproduction of the pre-#389 implementation — fresh `predictions`/`errors`
//! `Vec`s every step, array-of-structs inward connections, `Vec<Vec<_>>`
//! outward adjacency — on a production-shaped topology.
//!
//! Both sides must return **bit-identical** results: #389 is a memory-layout
//! and allocation change, never a numerics change. The settling loop's
//! per-edge derivative recompute is deliberately preserved (see the note in
//! `pc_inference.rs`), so the answer cannot move.
//!
//! The two sides are timed in **alternating rounds** inside one process and
//! reported as the *fastest* round each. Min-of-rounds is the noise-robust
//! statistic: a shared machine only ever adds time, so the minimum is the
//! closest estimate of the true cost, and alternating means both sides see the
//! same interference.
//!
//! Run with: `cargo run --release --example pc_infer_ab`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use neat_core::derivative::apply_derivative;
use neat_core::pc_inference::{PcConnection, PcInferenceResult, PcNeuron, PredictiveCodingEngine};
use neat_core::squash::{SquashType, apply_squash};

// ---------------------------------------------------------------------------
// Counting allocator — records allocation count and peak live bytes.
// ---------------------------------------------------------------------------

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged to the system allocator,
        // which is the documented delegation pattern for a wrapper allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr`/`layout` come from a matching `alloc` on this
        // allocator, which delegates to `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn reset_counters() {
    ALLOCS.store(0, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Deterministic fixture.
// ---------------------------------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn signed(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Topology shape: a wide input layer feeding multi-fan-in hidden layers.
const NUM_INPUTS: usize = 32;
const HIDDEN_LAYERS: [usize; 3] = [96, 96, 48];
const NUM_OUTPUTS: usize = 8;
const FAN_IN: usize = 16;
const STEPS: u32 = 50;
const RATE: f32 = 0.05;
/// Zero threshold: the supervised run never converges early, so every round
/// pays the full [`STEPS`] settling iterations.
const THRESHOLD: f32 = 0.0;

fn build_topology() -> (Vec<PcNeuron>, Vec<PcConnection>) {
    let mut lcg = Lcg(0x9C0D_E389);
    let mut neurons: Vec<PcNeuron> = Vec::new();
    let mut connections: Vec<PcConnection> = Vec::new();

    let mut prev: Vec<usize> = (0..NUM_INPUTS).collect();
    let mut next_full = NUM_INPUTS;

    let mut layer_sizes: Vec<usize> = HIDDEN_LAYERS.to_vec();
    layer_sizes.push(NUM_OUTPUTS);

    for (li, &size) in layer_sizes.iter().enumerate() {
        let is_hidden = li < HIDDEN_LAYERS.len();
        let squash = if is_hidden {
            SquashType::Tanh
        } else {
            SquashType::Identity
        };
        let mut this_layer: Vec<usize> = Vec::with_capacity(size);
        for _ in 0..size {
            let conn_start = connections.len();
            let k = FAN_IN.min(prev.len());
            for _ in 0..k {
                let src = prev[lcg.below(prev.len())];
                connections.push(PcConnection {
                    from: src,
                    weight: lcg.signed() * 0.5,
                });
            }
            neurons.push(PcNeuron {
                bias: lcg.signed() * 0.1,
                squash_type: squash,
                is_hidden,
                conn_start,
                conn_count: connections.len() - conn_start,
            });
            this_layer.push(next_full);
            next_full += 1;
        }
        prev = this_layer;
    }

    (neurons, connections)
}

// ---------------------------------------------------------------------------
// Pre-#389 reference: two `Vec`s per settling step, AoS inward connections,
// `Vec<Vec<_>>` outward adjacency, `neurons[target_rel]` chased per edge.
// ---------------------------------------------------------------------------

fn ref_pre(neurons: &[PcNeuron], connections: &[PcConnection], rel: usize, latents: &[f32]) -> f32 {
    let neuron = &neurons[rel];
    let mut sum = neuron.bias;
    for ci in neuron.conn_start..neuron.conn_start + neuron.conn_count {
        let c = &connections[ci];
        sum += c.weight * latents[c.from];
    }
    sum
}

fn ref_prediction(
    neurons: &[PcNeuron],
    connections: &[PcConnection],
    rel: usize,
    latents: &[f32],
) -> f32 {
    let neuron = &neurons[rel];
    apply_squash(
        neuron.squash_type,
        ref_pre(neurons, connections, rel, latents),
    )
}

/// The whole pre-#389 engine: adjacency is rebuilt per call only in
/// [`ReferenceEngine::new`], exactly as `new_from_parts` did, so the timed
/// `infer` below is a like-for-like comparison.
struct ReferenceEngine {
    num_neurons: usize,
    num_inputs: usize,
    num_outputs: usize,
    neurons: Vec<PcNeuron>,
    connections: Vec<PcConnection>,
    outward: Vec<Vec<(usize, f32)>>,
    hidden: Vec<usize>,
}

impl ReferenceEngine {
    fn new(neurons: Vec<PcNeuron>, connections: Vec<PcConnection>) -> Self {
        let num_neurons = NUM_INPUTS + neurons.len();
        let mut outward: Vec<Vec<(usize, f32)>> = vec![Vec::new(); num_neurons];
        for (ni, neuron) in neurons.iter().enumerate() {
            let actual = NUM_INPUTS + ni;
            for ci in neuron.conn_start..neuron.conn_start + neuron.conn_count {
                let c = &connections[ci];
                outward[c.from].push((actual, c.weight));
            }
        }
        let hidden: Vec<usize> = neurons
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_hidden)
            .map(|(i, _)| NUM_INPUTS + i)
            .collect();
        ReferenceEngine {
            num_neurons,
            num_inputs: NUM_INPUTS,
            num_outputs: NUM_OUTPUTS,
            neurons,
            connections,
            outward,
            hidden,
        }
    }

    fn compute_errors(&self, latents: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let n = self.neurons.len();
        let mut p = vec![0.0f32; n];
        let mut e = vec![0.0f32; n];
        for i in 0..n {
            let pred = ref_prediction(&self.neurons, &self.connections, i, latents);
            p[i] = pred;
            e[i] = latents[self.num_inputs + i] - pred;
        }
        (p, e)
    }

    fn infer(&self, input: &[f32], targets: Option<&[f32]>) -> PcInferenceResult {
        let mut latents = vec![0.0f32; self.num_neurons];
        let input_len = input.len().min(self.num_inputs);
        latents[..input_len].copy_from_slice(&input[..input_len]);
        for i in 0..self.neurons.len() {
            latents[self.num_inputs + i] =
                ref_prediction(&self.neurons, &self.connections, i, &latents);
        }
        if let Some(tgt) = targets {
            let output_start = self.num_neurons - self.num_outputs;
            for j in 0..self.num_outputs.min(tgt.len()) {
                latents[output_start + j] = tgt[j];
            }
        }

        let mut energy_history = Vec::with_capacity(STEPS as usize + 1);
        let mut converged = false;
        let mut steps_used = 0u32;

        let (mut predictions, mut errors) = self.compute_errors(&latents);
        let mut energy = energy_of(&errors);
        energy_history.push(energy);

        for t in 0..STEPS {
            steps_used = t + 1;
            if energy <= THRESHOLD {
                converged = true;
                break;
            }
            for &hidden_idx in &self.hidden {
                let hidden_rel = hidden_idx - self.num_inputs;
                let mut gradient = errors[hidden_rel];
                for &(to, weight) in &self.outward[hidden_idx] {
                    let target_rel = to - self.num_inputs;
                    let target_error = errors[target_rel];
                    let target_squash = self.neurons[target_rel].squash_type;
                    let pre = ref_pre(&self.neurons, &self.connections, target_rel, &latents);
                    let derivative = apply_derivative(target_squash, pre);
                    gradient -= weight * target_error * derivative;
                }
                latents[hidden_idx] -= RATE * gradient;
            }
            latents[..input_len].copy_from_slice(&input[..input_len]);
            if let Some(tgt) = targets {
                let output_start = self.num_neurons - self.num_outputs;
                for j in 0..self.num_outputs.min(tgt.len()) {
                    latents[output_start + j] = tgt[j];
                }
            }
            let (np, ne) = self.compute_errors(&latents);
            predictions = np;
            errors = ne;
            energy = energy_of(&errors);
            energy_history.push(energy);
        }
        if !converged && energy <= THRESHOLD {
            converged = true;
        }

        PcInferenceResult {
            latents,
            predictions,
            errors,
            final_energy: energy,
            energy_history,
            steps_used,
            converged,
        }
    }

    fn infer_batch(&self, inputs: &[&[f32]], targets: Option<&[&[f32]]>) -> Vec<PcInferenceResult> {
        inputs
            .iter()
            .enumerate()
            .map(|(i, input)| self.infer(input, targets.map(|t| t[i])))
            .collect()
    }
}

fn energy_of(errors: &[f32]) -> f32 {
    let mut s = 0.0f32;
    for &e in errors {
        s += e * e;
    }
    0.5 * s
}

// ---------------------------------------------------------------------------
// Comparison helpers.
// ---------------------------------------------------------------------------

/// Bit-level comparison — `to_bits` so a `NaN` or a signed zero cannot slip
/// through an `==` that would call them equal (or unequal) by accident.
fn assert_bit_identical(label: &str, a: &PcInferenceResult, b: &PcInferenceResult) {
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
    assert_eq!(bits(&a.latents), bits(&b.latents), "{label}: latents");
    assert_eq!(
        bits(&a.predictions),
        bits(&b.predictions),
        "{label}: predictions"
    );
    assert_eq!(bits(&a.errors), bits(&b.errors), "{label}: errors");
    assert_eq!(
        a.final_energy.to_bits(),
        b.final_energy.to_bits(),
        "{label}: final_energy"
    );
    assert_eq!(
        bits(&a.energy_history),
        bits(&b.energy_history),
        "{label}: energy_history"
    );
    assert_eq!(a.steps_used, b.steps_used, "{label}: steps_used");
    assert_eq!(a.converged, b.converged, "{label}: converged");
}

fn pct(before: f64, after: f64) -> f64 {
    (before - after) / before * 100.0
}

fn main() {
    const REPS: usize = 8;
    const ROUNDS: usize = 400;
    const BATCH_ROUNDS: usize = 40;
    const BATCH: usize = 8;

    let (neurons, connections) = build_topology();
    let reference = ReferenceEngine::new(neurons.clone(), connections.clone());
    let engine = PredictiveCodingEngine::new_from_parts(
        NUM_INPUTS,
        NUM_OUTPUTS,
        neurons,
        connections,
        STEPS,
        RATE,
        THRESHOLD,
    );

    let mut lcg = Lcg(0x1234_5678);
    let input: Vec<f32> = (0..NUM_INPUTS).map(|_| lcg.signed()).collect();
    let targets: Vec<f32> = (0..NUM_OUTPUTS).map(|_| lcg.signed()).collect();
    let batch: Vec<Vec<f32>> = (0..BATCH)
        .map(|_| (0..NUM_INPUTS).map(|_| lcg.signed()).collect())
        .collect();
    let batch_refs: Vec<&[f32]> = batch.iter().map(|v| v.as_slice()).collect();
    let batch_targets: Vec<&[f32]> = (0..BATCH).map(|_| &targets[..]).collect();

    println!(
        "topology: {NUM_INPUTS} inputs + {} non-inputs ({NUM_OUTPUTS} outputs), \
         {} synapses, fan-in {FAN_IN}, {STEPS} settling steps\n",
        engine.num_neurons() - NUM_INPUTS,
        engine.connections.len()
    );

    // --- Correctness: the change must not move a single bit. ---
    let want = reference.infer(&input, Some(&targets));
    let got = engine.infer(&input, Some(&targets));
    assert_bit_identical("supervised", &want, &got);
    assert_eq!(
        got.steps_used, STEPS,
        "fixture converged early — it would not measure the settling loop"
    );
    let want_unsup = reference.infer(&input, None);
    let got_unsup = engine.infer(&input, None);
    assert_bit_identical("unsupervised", &want_unsup, &got_unsup);
    println!(
        "results bit-identical (supervised: {} steps, energy {:e}; unsupervised: {} steps)\n",
        got.steps_used, got.final_energy, got_unsup.steps_used
    );

    // --- Allocation counts: one `infer` call each. ---
    reset_counters();
    let out = reference.infer(&input, Some(&targets));
    let ref_allocs = ALLOCS.load(Ordering::Relaxed);
    let ref_peak = PEAK.load(Ordering::Relaxed);
    std::hint::black_box(&out);

    reset_counters();
    let out = engine.infer(&input, Some(&targets));
    let new_allocs = ALLOCS.load(Ordering::Relaxed);
    let new_peak = PEAK.load(Ordering::Relaxed);
    std::hint::black_box(&out);

    println!("allocations for one supervised `infer` ({STEPS} steps):");
    println!("  pre-#389 : {ref_allocs:>5} allocations, peak {ref_peak:>7} bytes");
    println!("  shipped  : {new_allocs:>5} allocations, peak {new_peak:>7} bytes");
    println!(
        "  delta    : {:>5} fewer allocations ({:.1}%)\n",
        ref_allocs.saturating_sub(new_allocs),
        pct(ref_allocs as f64, new_allocs as f64)
    );

    // --- Wall clock: tight A/B alternation, fastest round on each side.
    //
    // One call per side per round, alternating, so the two sides sit inside the
    // same scheduling window. Reported as the minimum over ROUNDS: on a loaded
    // machine (this harness is expected to run alongside other work) every
    // sample is the true cost *plus* interference, so the minimum is the
    // closest available estimate of the true cost and the mean is not. ---
    let mut ref_best = f64::INFINITY;
    let mut new_best = f64::INFINITY;
    let mut ref_batch_best = f64::INFINITY;
    let mut new_batch_best = f64::INFINITY;

    // Warm up both sides so first-touch page faults land outside the timing.
    for _ in 0..REPS {
        std::hint::black_box(reference.infer(&input, Some(&targets)));
        std::hint::black_box(engine.infer(&input, Some(&targets)));
    }

    for _ in 0..ROUNDS {
        let start = Instant::now();
        std::hint::black_box(reference.infer(&input, Some(&targets)));
        ref_best = ref_best.min(start.elapsed().as_secs_f64());

        let start = Instant::now();
        std::hint::black_box(engine.infer(&input, Some(&targets)));
        new_best = new_best.min(start.elapsed().as_secs_f64());
    }

    for _ in 0..BATCH_ROUNDS {
        let start = Instant::now();
        std::hint::black_box(reference.infer_batch(&batch_refs, Some(&batch_targets)));
        ref_batch_best = ref_batch_best.min(start.elapsed().as_secs_f64());

        let start = Instant::now();
        std::hint::black_box(engine.infer_batch(&batch_refs, Some(&batch_targets)));
        new_batch_best = new_batch_best.min(start.elapsed().as_secs_f64());
    }

    println!("wall clock — fastest of {ROUNDS} tightly alternated rounds:");
    println!(
        "  infer            pre-#389 {:>8.3} ms   shipped {:>8.3} ms   {:+.1}%",
        ref_best * 1e3,
        new_best * 1e3,
        -pct(ref_best, new_best)
    );
    println!(
        "  infer_batch/{BATCH}   pre-#389 {:>8.3} ms   shipped {:>8.3} ms   {:+.1}%  \
         (fastest of {BATCH_ROUNDS})",
        ref_batch_best * 1e3,
        new_batch_best * 1e3,
        -pct(ref_batch_best, new_batch_best)
    );
}
