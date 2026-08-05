//! Load-time index validation as the `gather4` safety contract (Issue #509).
//!
//! The wasm32 `gather4` scaffold helper reads four synapses and four indirect
//! activations with `get_unchecked`. Its safety contract is that a loaded
//! `CompiledNetwork` can never carry a `from_index` outside its activation
//! buffer — `CompiledNetwork::new` rejects such a network with
//! `NetworkError::InvalidSynapseIndex`.
//!
//! `network.rs`'s own unit tests already pin that rejection for a *single*
//! synapse. A 4-wide gather adds an obligation those do not cover: the check
//! must hold for **every** synapse in a span, wherever the malformed one sits
//! in a lane group, and it must hold for the whole loaded structure rather than
//! for the first synapse the loader happens to look at. This file pins that
//! stronger reading, plus the buffer-length half of the contract
//! (`activations.len() == num_neurons`), so removing the load-time guard as
//! "redundant" fails a test instead of shipping undefined behaviour.
//!
//! These are architecture-neutral assertions about `CompiledNetwork`, so they
//! run in the native CI suite even though the kernel they protect is
//! wasm32-only.

use neat_core::network::{CompiledNetwork, NetworkError};
use neat_core::squash::SquashType;

/// Serialise a network of one non-input neuron carrying `from_indices.len()`
/// synapses, in the compiled `.bin` layout `CompiledNetwork::new` parses.
fn serialise(num_neurons: u32, num_inputs: u32, from_indices: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&num_neurons.to_le_bytes());
    bytes.extend_from_slice(&num_inputs.to_le_bytes());
    // Exactly one non-input neuron; callers pass num_neurons - num_inputs == 1.
    bytes.extend_from_slice(&0.0f64.to_le_bytes()); // bias
    bytes.push(SquashType::Identity as u8);
    bytes.push(0); // is_constant
    bytes.extend_from_slice(&(from_indices.len() as u16).to_le_bytes());
    for (i, from_index) in from_indices.iter().enumerate() {
        bytes.extend_from_slice(&from_index.to_le_bytes());
        bytes.push(0); // synapse_type
        bytes.push(0); // padding
        bytes.extend_from_slice(&(1.0 + i as f64).to_le_bytes()); // weight
    }
    bytes
}

/// A span wide enough for two dual-accumulator chunks of eight, a trailing
/// chunk of four and a scalar remainder — every lane position a gather can put
/// a synapse in.
const SPAN: usize = 21;

#[test]
fn an_out_of_range_index_is_rejected_from_every_lane_position() {
    // 24 inputs + 1 output neuron: indices 0..=24 are valid, 25 is one past.
    let num_inputs = 24u32;
    let num_neurons = num_inputs + 1;
    let out_of_range = num_neurons as u16;

    for position in 0..SPAN {
        let mut from_indices: Vec<u16> = (0..SPAN)
            .map(|i| (i % num_inputs as usize) as u16)
            .collect();
        from_indices[position] = out_of_range;

        let bytes = serialise(num_neurons, num_inputs, &from_indices);
        match CompiledNetwork::new(&bytes) {
            Err(NetworkError::InvalidSynapseIndex {
                from_index,
                num_neurons: n,
            }) => {
                assert_eq!(from_index, out_of_range, "lane position {position}");
                assert_eq!(n, num_neurons as usize, "lane position {position}");
            }
            Err(other) => {
                panic!("lane position {position}: expected InvalidSynapseIndex, got {other:?}")
            }
            Ok(_) => panic!(
                "lane position {position}: malformed network loaded — the gather4 \
                 safety contract is no longer enforced"
            ),
        }
    }
}

#[test]
fn every_synapse_of_a_loaded_network_indexes_its_activation_buffer() {
    // The positive half of the contract: a well-formed network of the same
    // shape loads, and the structure it produces satisfies both obligations the
    // unchecked gather relies on.
    let num_inputs = 24u32;
    let num_neurons = num_inputs + 1;
    let from_indices: Vec<u16> = (0..SPAN)
        .map(|i| (i % num_inputs as usize) as u16)
        .collect();

    let net = CompiledNetwork::new(&serialise(num_neurons, num_inputs, &from_indices))
        .expect("well-formed network must load");

    // Obligation 2a: the activation buffer is sized to exactly num_neurons.
    assert_eq!(net.activations.len(), net.num_neurons);
    // Obligation 2b: every from_index indexes that buffer.
    for (i, synapse) in net.synapses.iter().enumerate() {
        assert!(
            (synapse.from_index as usize) < net.activations.len(),
            "synapse {i} has from_index {} outside the {}-long activation buffer",
            synapse.from_index,
            net.activations.len()
        );
    }
    // Obligation 1: the neuron's declared span lies inside `synapses`.
    let neuron = &net.neurons[0];
    let end = neuron.start_synapse as usize + neuron.num_synapses as usize;
    assert!(end <= net.synapses.len());
    assert_eq!(end, SPAN);
}

#[test]
fn the_largest_in_range_index_still_loads() {
    // The guard must reject only genuinely out-of-bounds indices: num_neurons-1
    // is the last valid activation slot and has to survive, otherwise the
    // "reject out of range" rule would quietly become "reject the boundary".
    let num_inputs = 24u32;
    let num_neurons = num_inputs + 1;
    let last_valid = (num_neurons - 1) as u16;
    let from_indices = vec![last_valid; SPAN];

    let net = CompiledNetwork::new(&serialise(num_neurons, num_inputs, &from_indices))
        .expect("boundary index must load");
    assert!(
        net.synapses
            .iter()
            .all(|s| (s.from_index as usize) < net.num_neurons)
    );
}
