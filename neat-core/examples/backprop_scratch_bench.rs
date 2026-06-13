//! Throughput benchmark for the reverse-topological backprop loop (Issue #154).
//!
//! Builds a dense feed-forward network and runs many backward passes, timing
//! the total. Used to validate that reusing the per-neuron scratch buffers
//! (instead of allocating nine fresh `Vec`s per neuron per pass) improves
//! throughput on dense networks. Pure measurement — no behavioural change.
//!
//! Run with: `cargo run --release --example backprop_scratch_bench`

use std::time::Instant;

use neat_core::{
    NEURON_TYPE_HIDDEN, NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, NeuronInput, PropagateInput,
    SynapseInput, propagate_topological_loop,
};

// Identity squash constant (0) — kept local so the example does not depend on
// the SquashType enum's numeric layout.
const SQUASH_IDENTITY: u8 = 0;

fn make_neuron(neuron_type: u8, activation: f32) -> NeuronInput {
    NeuronInput {
        squash_type: SQUASH_IDENTITY,
        neuron_type,
        propagate_needed: true,
        update_needed: true,
        hint_value: 0.0,
        range_low: -1.0e6,
        range_high: 1.0e6,
        adjusted_activation: activation,
        adjusted_bias: 0.0,
    }
}

fn main() {
    // Dense multi-layer perceptron: INPUTS → H1 → H2 → OUTPUTS, fully connected
    // between adjacent layers. Each hidden/output neuron has a wide inward list,
    // which is exactly where the nine-Vec-per-neuron churn was heaviest.
    let inputs = 32usize;
    let hidden1 = 48usize;
    let hidden2 = 48usize;
    let outputs = 16usize;

    let layer_sizes = [inputs, hidden1, hidden2, outputs];
    let layer_types = [
        NEURON_TYPE_INPUT,
        NEURON_TYPE_HIDDEN,
        NEURON_TYPE_HIDDEN,
        NEURON_TYPE_OUTPUT,
    ];

    let mut neurons: Vec<NeuronInput> = Vec::new();
    let mut layer_offsets: Vec<usize> = Vec::new();
    for (li, &size) in layer_sizes.iter().enumerate() {
        layer_offsets.push(neurons.len());
        for i in 0..size {
            // Deterministic pseudo-activations — vary by position.
            let act = 0.25 + ((i % 7) as f32) * 0.05;
            neurons.push(make_neuron(layer_types[li], act));
        }
    }

    // Fully connect adjacent layers.
    let mut synapses: Vec<SynapseInput> = Vec::new();
    for li in 0..layer_sizes.len() - 1 {
        let from_base = layer_offsets[li];
        let to_base = layer_offsets[li + 1];
        for f in 0..layer_sizes[li] {
            for t in 0..layer_sizes[li + 1] {
                let w = 0.1 + (((f + t) % 9) as f32) * 0.05;
                synapses.push(SynapseInput {
                    from: (from_base + f) as u32,
                    to: (to_base + t) as u32,
                    original_weight: w,
                    adjusted_weight: w,
                    is_self_loop: false,
                });
            }
        }
    }

    // Build inward (CSR) mapping per neuron.
    let neuron_count = neurons.len();
    let mut inward_starts = vec![0u32; neuron_count];
    let mut inward_counts = vec![0u32; neuron_count];
    let mut inward_indices: Vec<u32> = Vec::new();
    for nidx in 0..neuron_count {
        inward_starts[nidx] = inward_indices.len() as u32;
        let mut c = 0u32;
        for (si, s) in synapses.iter().enumerate() {
            if s.to as usize == nidx {
                inward_indices.push(si as u32);
                c += 1;
            }
        }
        inward_counts[nidx] = c;
    }

    // Reverse-topological order: outputs, then hidden2, then hidden1.
    let mut order: Vec<u32> = Vec::new();
    for li in (1..layer_sizes.len()).rev() {
        let base = layer_offsets[li];
        for i in 0..layer_sizes[li] {
            order.push((base + i) as u32);
        }
    }

    let expected: Vec<f32> = (0..outputs).map(|i| 0.5 + ((i % 5) as f32) * 0.1).collect();

    let input = PropagateInput {
        neurons: &neurons,
        synapses: &synapses,
        inward_starts: &inward_starts,
        inward_counts: &inward_counts,
        inward_synapse_indices: &inward_indices,
        reverse_topo_order: &order,
        expected: &expected,
        input_count: inputs as u32,
        output_count: outputs as u32,
        plank_constant: 1e-7,
        normalise_gradients: false,
    };

    let passes = 50_000u32;
    // Warm-up.
    let mut checksum = 0u64;
    for _ in 0..1_000 {
        let out = propagate_topological_loop(&input);
        checksum = checksum.wrapping_add(out.synapses.len() as u64);
    }

    let start = Instant::now();
    for _ in 0..passes {
        let out = propagate_topological_loop(&input);
        checksum = checksum.wrapping_add(out.synapses.len() as u64);
    }
    let elapsed = start.elapsed();

    let per_pass_us = elapsed.as_secs_f64() * 1e6 / passes as f64;
    println!(
        "neurons={neuron_count} synapses={} passes={passes}",
        synapses.len()
    );
    println!(
        "total={:?}  per_pass={per_pass_us:.2}us  ({:.0} passes/s)  checksum={checksum}",
        elapsed,
        passes as f64 / elapsed.as_secs_f64()
    );
}
