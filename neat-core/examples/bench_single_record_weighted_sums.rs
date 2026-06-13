//! Micro-benchmark for the single-record weighted-sum primitives (Issue #153).
//!
//! Measures the four single-record forward-pass primitives that `activate()` calls
//! per neuron. Run with:
//!
//! ```sh
//! cargo run --release --example bench_single_record_weighted_sums
//! ```
//!
//! Prints nanoseconds-per-call for each primitive so the native SIMD dispatch
//! (AVX2/FMA on x86_64, NEON on aarch64) can be compared against the scalar
//! baseline. This is a benchmark, not a unit test — correctness is covered by
//! `tests/simd_weighted_sums.rs`.

use neat_core::network::SynapseData;
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd,
};
use std::time::Instant;

fn make_synapse(from_index: u32, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
        _padding: [0; 3],
    }
}

fn main() {
    // Representative single-record neuron fan-in. 64 synapses is a realistic
    // upper-mid fan-in for a dense hidden neuron; the activation buffer is
    // sized to the number of distinct source neurons.
    let num_inputs = 64usize;
    let num_synapses = 64usize;
    let iters = 2_000_000u64;

    let activations: Vec<f32> = (0..num_inputs)
        .map(|i| ((i as f32) * 0.137).sin())
        .collect();
    let synapses: Vec<SynapseData> = (0..num_synapses)
        .map(|i| make_synapse((i % num_inputs) as u32, ((i as f32) * 0.0731).cos()))
        .collect();

    // `black_box` the slices and bias on every call so the optimiser cannot hoist
    // the (otherwise loop-invariant) result out of the timing loop.
    type Kernel = dyn Fn(&[SynapseData], &[f32], f32) -> f32;
    let bench = |name: &str, f: &Kernel| {
        // Warm-up to stabilise caches / branch predictor.
        let mut warm = 0.0f32;
        for _ in 0..50_000 {
            warm += f(
                std::hint::black_box(&synapses),
                std::hint::black_box(&activations),
                std::hint::black_box(0.5),
            );
        }
        std::hint::black_box(warm);

        let mut acc = 0.0f32;
        let start = Instant::now();
        for _ in 0..iters {
            acc += f(
                std::hint::black_box(&synapses),
                std::hint::black_box(&activations),
                std::hint::black_box(0.5),
            );
        }
        let elapsed = start.elapsed();
        std::hint::black_box(acc);
        let ns_per_call = elapsed.as_nanos() as f64 / iters as f64;
        println!("{name:<32} {ns_per_call:>8.3} ns/call");
    };

    println!("single-record weighted-sum benchmark ({num_synapses} synapses, {iters} iters)");
    bench("weighted_sum_simd", &|syn, act, bias| {
        weighted_sum_simd(syn, act, 0, syn.len(), bias)
    });
    bench("weighted_sum_no_bias_simd", &|syn, act, _bias| {
        weighted_sum_no_bias_simd(syn, act, 0, syn.len())
    });
    bench("weighted_sum_of_squares_simd", &|syn, act, _bias| {
        weighted_sum_of_squares_simd(syn, act, 0, syn.len())
    });
    bench("weighted_sum_of_squares_v2_simd", &|syn, act, bias| {
        weighted_sum_of_squares_v2_simd(syn, act, 0, syn.len(), bias)
    });
}
