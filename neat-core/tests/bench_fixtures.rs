//! Behavioural tests for the deterministic fixtures behind the `hot_paths`
//! Criterion harness (Issue #176). The builders live in `benches/common/mod.rs`
//! and are reused here verbatim via `#[path]`, so the production-scale shape is
//! validated by a real `cargo test` run rather than only compiled in the bench.

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{FanIn, NETWORKS, NetSpec, build_backprop_data, build_inputs, build_network};

fn spec(label: &str) -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
}

#[test]
fn production_shapes_are_registered_with_expected_dimensions() {
    let prod = spec("production");
    assert_eq!(prod.num_inputs, 2461);
    assert_eq!(prod.num_outputs, 1);
    assert_eq!(prod.num_non_inputs(), 1673);
    assert_eq!(prod.num_neurons, 4134);

    let prod2x = spec("production_2x");
    // "or larger creatures": ~2x neurons.
    assert!(
        prod2x.num_non_inputs() >= 2 * prod.num_non_inputs(),
        "production_2x should have at least double the non-input neurons"
    );
    assert!(prod2x.num_inputs >= prod.num_inputs);
}

#[test]
fn build_network_matches_production_neuron_and_synapse_counts() {
    let prod = spec("production");
    let net = build_network(prod, 0x5152_5354);

    // One NeuronData per non-input neuron; total count includes the input layer.
    assert_eq!(net.neurons.len(), prod.num_non_inputs());
    assert_eq!(net.num_neurons, prod.num_neurons);
    assert_eq!(net.num_inputs, prod.num_inputs);

    // Sparse ~13 average fan-in ⇒ roughly 21.7k synapses for the real creature.
    let total_synapses = net.synapses.len();
    let avg_fan_in = total_synapses as f64 / prod.num_non_inputs() as f64;
    assert!(
        (12.0..=14.0).contains(&avg_fan_in),
        "average fan-in {avg_fan_in} should sit near the production ~13"
    );

    // num_synapses on each neuron must sum to the flat synapse buffer length.
    let summed: usize = net.neurons.iter().map(|n| n.num_synapses as usize).sum();
    assert_eq!(summed, total_synapses);
}

#[test]
fn varied_fan_in_actually_varies_unlike_fixed_shapes() {
    // The wide/shallow production shape must draw a non-constant fan-in.
    let prod = spec("production");
    let net = build_network(prod, 0x5152_5354);
    let distinct: std::collections::BTreeSet<u16> =
        net.neurons.iter().map(|n| n.num_synapses).collect();
    assert!(
        distinct.len() > 1,
        "production fan-in should vary, saw only {distinct:?}"
    );

    // A fixed-shape network keeps a single fan-in (away from the early cap).
    let small = spec("small_50");
    if let FanIn::Fixed(f) = small.fan_in {
        let net = build_network(small, 0x5152_5354);
        let tail = &net.neurons[small.num_inputs..];
        assert!(
            tail.iter().all(|n| n.num_synapses as usize == f),
            "fixed shapes should keep a constant fan-in past the early ramp"
        );
    }
}

#[test]
fn build_network_is_deterministic_for_a_fixed_seed() {
    let prod = spec("production");
    let a = build_network(prod, 0x5152_5354);
    let b = build_network(prod, 0x5152_5354);

    assert_eq!(a.synapses.len(), b.synapses.len());
    assert!(
        a.synapses
            .iter()
            .zip(&b.synapses)
            .all(|(x, y)| x.weight == y.weight && x.from_index == y.from_index),
        "same seed must reproduce identical synapses"
    );
    assert!(
        a.neurons
            .iter()
            .zip(&b.neurons)
            .all(|(x, y)| x.bias == y.bias && x.num_synapses == y.num_synapses),
        "same seed must reproduce identical neurons"
    );
}

#[test]
fn production_network_activates_to_finite_outputs() {
    let prod = spec("production");
    let mut net = build_network(prod, 0x5152_5354);
    let inputs = build_inputs(prod.num_inputs, 0xA1B2_C3D4);

    let out = net.activate(&inputs, prod.num_outputs);
    assert_eq!(out.len(), prod.num_outputs);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "production forward pass must produce finite outputs"
    );
}

#[test]
fn backprop_data_has_consistent_inward_adjacency_at_production_scale() {
    let prod = spec("production");
    let data = build_backprop_data(prod, 0x1357_9BDF);

    assert_eq!(data.neurons.len(), prod.num_neurons);
    assert_eq!(data.input_count as usize, prod.num_inputs);
    assert_eq!(data.output_count as usize, prod.num_outputs);
    assert_eq!(data.reverse_topo_order.len(), prod.num_non_inputs());

    // Inward counts must sum to the flat synapse buffer and index list length.
    let summed: usize = data.inward_counts.iter().map(|&c| c as usize).sum();
    assert_eq!(summed, data.synapses.len());
    assert_eq!(summed, data.inward_indices.len());
}
