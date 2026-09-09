//! Issue #625 — `CompiledNetwork` holds the SIMD index invariant by construction.
//!
//! The forward and batched-scoring paths call the `simd::*_unchecked` kernels
//! and discharge their index contract from the load-time validation in
//! `CompiledNetwork::new` (Issue #613). That discharge only holds while nothing
//! outside the crate can rewrite the validated values afterwards, so the fields
//! are private and every construction path validates.
//!
//! These are "what" tests: they build networks through the public entry points
//! and assert on observable outcomes — the error a bad network is refused with,
//! and the state a good one exposes. The **refusal of the safe write itself** is
//! a compile-time outcome, so it is pinned by
//! `tests/scripts/compiled_network_encapsulation.bats`, which asks cargo to
//! build an out-of-crate probe performing exactly the Issue #625 mutation and
//! requires the compiler to reject it.

use neat_core::network::{CompiledNetwork, NetworkError, NeuronData, SynapseData};

/// Two inputs into one identity output neuron reading both of them.
fn valid_parts() -> (usize, Vec<NeuronData>, Vec<SynapseData>) {
    let synapses = vec![
        SynapseData {
            weight: 0.5,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: -0.25,
            from_index: 1,
            synapse_type: 0,
        },
    ];
    let neurons = vec![NeuronData {
        bias: 0.125,
        start_synapse: 0,
        num_synapses: 2,
        squash_type: 0, // IDENTITY
        is_constant: false,
    }];
    (2, neurons, synapses)
}

#[test]
fn from_parts_builds_a_network_that_activates() {
    let (num_inputs, neurons, synapses) = valid_parts();
    let mut net = CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("well-formed parts must assemble");

    assert_eq!(net.num_inputs(), 2);
    assert_eq!(net.num_neurons(), 3, "2 inputs + 1 output");
    // identity(1.0 * 0.5 + 2.0 * -0.25 + 0.125)
    let out = net.activate(&[1.0, 2.0], 1);
    assert_eq!(out.len(), 1);
    assert!((out[0] - 0.125).abs() < 1e-6, "{out:?}");
}

#[test]
fn from_parts_derives_the_hot_view_and_buffers_from_the_synapses_given() {
    let (num_inputs, neurons, synapses) = valid_parts();
    let net = CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("well-formed parts must assemble");

    // The struct-of-arrays view is derived, never taken from the caller, so it
    // cannot be handed in already drifted (Issue #533).
    assert_eq!(net.hot_from(), [0u16, 1]);
    assert_eq!(net.hot_weights(), [0.5f32, -0.25]);
    // Every buffer the unchecked gather indexes is sized to the node count.
    assert_eq!(net.activations().len(), net.num_neurons());
    assert_eq!(net.hint_values().len(), net.neurons().len());
}

#[test]
fn from_parts_refuses_a_source_index_past_the_last_neuron() {
    // The Issue #625 attack value: the write that used to be reachable in safe
    // code is now only expressible as an argument, and it is refused.
    let (num_inputs, neurons, mut synapses) = valid_parts();
    synapses[0].from_index = 60_000;

    let Err(err) = CompiledNetwork::from_parts(num_inputs, neurons, synapses) else {
        panic!("an out-of-range source index must be refused");
    };
    assert_eq!(
        err,
        NetworkError::InvalidSynapseIndex {
            from_index: 60_000,
            num_neurons: 3,
        }
    );
}

#[test]
fn from_parts_accepts_the_last_valid_source_index() {
    // Guard the boundary: `num_neurons - 1` is the last activation slot and must
    // still assemble, or "reject out of range" would quietly become "reject the
    // boundary".
    let (num_inputs, neurons, mut synapses) = valid_parts();
    synapses[0].from_index = 2; // num_neurons - 1

    let net = CompiledNetwork::from_parts(num_inputs, neurons, synapses)
        .expect("the boundary index must assemble");
    assert_eq!(net.synapses()[0].from_index, 2);
}

#[test]
fn from_parts_refuses_a_span_running_past_the_synapse_table() {
    // The other half of the unchecked contract: the kernels walk
    // `start_synapse..start_synapse + num_synapses` with `get_unchecked`, so an
    // overrunning span is an out-of-bounds read even when every `from_index` is
    // in range.
    let (num_inputs, mut neurons, synapses) = valid_parts();
    neurons[0].num_synapses = 5; // only 2 synapses exist

    let Err(err) = CompiledNetwork::from_parts(num_inputs, neurons, synapses) else {
        panic!("an overrunning synapse span must be refused");
    };
    assert_eq!(
        err,
        NetworkError::InvalidSynapseSpan {
            neuron: 0,
            end: 5,
            len: 2,
        }
    );
}

#[test]
fn a_loaded_network_exposes_its_state_read_only() {
    // The accessors that replaced the public fields hand out shared slices, and
    // they describe the same network `new` validated — so a consumer keeps the
    // read it had without keeping the write that broke the invariant.
    let (num_inputs, neurons, synapses) = valid_parts();
    let net = CompiledNetwork::from_parts(num_inputs, neurons, synapses).expect("assembles");

    let seen: &[SynapseData] = net.synapses();
    assert_eq!(seen.len(), net.num_synapses());
    for (i, synapse) in seen.iter().enumerate() {
        assert!(
            (synapse.from_index as usize) < net.activations().len(),
            "synapse {i} points outside the activation buffer"
        );
        assert_eq!(net.hot_from()[i], synapse.from_index);
        assert_eq!(net.hot_weights()[i].to_bits(), synapse.weight.to_bits());
    }
    for (i, neuron) in net.neurons().iter().enumerate() {
        let end = neuron.start_synapse as usize + neuron.num_synapses as usize;
        assert!(end <= net.synapses().len(), "neuron {i} span overruns");
    }
}
