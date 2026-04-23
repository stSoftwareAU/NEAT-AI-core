//! Tests for CompiledNetwork topology export (DOT / JSON).
//!
//! Issue #22 — machine-readable topology export for debugging and visualisation.

use neat_core::{compile_creature, parse_creature_json};

/// A small network with two inputs, one hidden neuron, and one output neuron.
/// Weights and biases are chosen so the resulting strings are easy to inspect.
fn small_network_json() -> &'static str {
    r#"{
        "input": 2,
        "output": 1,
        "neurons": [
            {"type": "hidden", "uuid": "hidden-1", "bias": 0.25, "squash": "TANH"},
            {"type": "output", "uuid": "output-0", "bias": -0.5, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "hidden-1", "weight": 1.0},
            {"fromUUID": "input-1", "toUUID": "hidden-1", "weight": 0.5},
            {"fromUUID": "hidden-1", "toUUID": "output-0", "weight": 1.5}
        ],
        "forwardOnly": true
    }"#
}

/// The minimal possible non-trivial network: a single input feeding a single
/// output through one synapse.
fn minimal_network_json() -> &'static str {
    r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}
        ],
        "forwardOnly": true
    }"#
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn to_dot_emits_valid_digraph_header() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    assert!(
        dot.starts_with("digraph "),
        "DOT output must begin with `digraph`, got: {dot}"
    );
    assert!(
        dot.trim_end().ends_with('}'),
        "DOT output must close with `}}`"
    );
}

#[test]
fn to_dot_contains_node_for_every_neuron() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    // Two inputs + one hidden + one output = 4 nodes declared as `n0`..`n3`.
    for i in 0..network.num_neurons {
        let needle = format!("n{i} ");
        assert!(
            dot.contains(&needle),
            "DOT output is missing node declaration `n{i}`:\n{dot}"
        );
    }
}

#[test]
fn to_dot_classifies_nodes_as_input_hidden_output() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    assert!(dot.contains("input"), "DOT output should label input nodes");
    assert!(
        dot.contains("hidden"),
        "DOT output should label hidden nodes"
    );
    assert!(
        dot.contains("output"),
        "DOT output should label output nodes"
    );
}

#[test]
fn to_dot_labels_activation_function_for_non_inputs() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    assert!(
        dot.contains("TANH"),
        "hidden neuron squash should be labelled"
    );
    assert!(
        dot.contains("IDENTITY"),
        "output neuron squash should be labelled"
    );
}

#[test]
fn to_dot_declares_one_edge_per_synapse() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    let edge_count = dot.matches(" -> ").count();
    assert_eq!(
        edge_count,
        network.synapses.len(),
        "DOT edge count must match synapse count"
    );
}

#[test]
fn to_dot_edges_include_weight_and_type() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    // Each of the three synapse weights should appear in the DOT output.
    for expected in ["1.000000", "0.500000", "1.500000"] {
        assert!(
            dot.contains(expected),
            "DOT output is missing expected weight `{expected}`:\n{dot}"
        );
    }
    assert!(
        dot.contains("Standard"),
        "DOT output should label synapse type"
    );
}

#[test]
fn to_topology_json_round_trips_as_valid_json() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let json = network.to_topology_json(creature.output);

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("to_topology_json output must be valid JSON");

    assert_eq!(parsed["num_inputs"], 2);
    assert_eq!(parsed["num_outputs"], 1);
    assert_eq!(parsed["num_neurons"], 4);

    let nodes = parsed["nodes"].as_array().expect("nodes must be an array");
    assert_eq!(nodes.len(), 4, "node count must match num_neurons");

    assert_eq!(nodes[0]["kind"], "input");
    assert_eq!(nodes[1]["kind"], "input");
    assert_eq!(nodes[2]["kind"], "hidden");
    assert_eq!(nodes[2]["squash"], "TANH");
    assert_eq!(nodes[3]["kind"], "output");
    assert_eq!(nodes[3]["squash"], "IDENTITY");

    let synapses = parsed["synapses"]
        .as_array()
        .expect("synapses must be an array");
    assert_eq!(synapses.len(), 3);

    // First synapse: input-0 -> hidden-1 with weight 1.0, type Standard.
    assert_eq!(synapses[0]["from"], 0);
    assert_eq!(synapses[0]["to"], 2);
    assert_eq!(synapses[0]["type"], "Standard");
}

#[test]
fn to_topology_json_includes_bias_for_non_input_nodes() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let json = network.to_topology_json(creature.output);

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = parsed["nodes"].as_array().unwrap();

    // Inputs should not expose a bias (they have none).
    assert!(nodes[0].get("bias").is_none());
    // Hidden neuron bias 0.25 and output neuron bias -0.5 should be present.
    assert!((nodes[2]["bias"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!((nodes[3]["bias"].as_f64().unwrap() + 0.5).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Edge case: minimal network
// ---------------------------------------------------------------------------

#[test]
fn minimal_network_exports_to_dot_cleanly() {
    let creature = parse_creature_json(minimal_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let dot = network.to_dot(creature.output);

    assert!(dot.starts_with("digraph "));
    assert_eq!(
        dot.matches(" -> ").count(),
        1,
        "minimal network has one edge"
    );
    assert!(dot.contains("n0"));
    assert!(dot.contains("n1"));
    assert!(dot.contains("input"));
    assert!(dot.contains("output"));
}

#[test]
fn minimal_network_exports_to_topology_json_cleanly() {
    let creature = parse_creature_json(minimal_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();
    let json = network.to_topology_json(creature.output);

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["num_inputs"], 1);
    assert_eq!(parsed["num_outputs"], 1);
    assert_eq!(parsed["num_neurons"], 2);
    assert_eq!(parsed["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["synapses"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn to_dot_is_deterministic_across_calls() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();

    let a = network.to_dot(creature.output);
    let b = network.to_dot(creature.output);

    assert_eq!(
        a, b,
        "to_dot must return byte-identical output for identical input"
    );
}

#[test]
fn to_topology_json_is_deterministic_across_calls() {
    let creature = parse_creature_json(small_network_json()).unwrap();
    let network = compile_creature(&creature).unwrap();

    let a = network.to_topology_json(creature.output);
    let b = network.to_topology_json(creature.output);

    assert_eq!(
        a, b,
        "to_topology_json must return byte-identical output for identical input"
    );
}

#[test]
fn to_dot_is_deterministic_across_fresh_compiles() {
    // Recompile the same creature JSON twice and confirm both exports match byte-for-byte.
    let creature_a = parse_creature_json(small_network_json()).unwrap();
    let creature_b = parse_creature_json(small_network_json()).unwrap();
    let network_a = compile_creature(&creature_a).unwrap();
    let network_b = compile_creature(&creature_b).unwrap();

    assert_eq!(
        network_a.to_dot(creature_a.output),
        network_b.to_dot(creature_b.output)
    );
    assert_eq!(
        network_a.to_topology_json(creature_a.output),
        network_b.to_topology_json(creature_b.output)
    );
}
