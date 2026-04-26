//! Issue #36 — verifies the `#[wasm_bindgen]` annotations on
//! `CompiledNetwork`, `PredictiveCodingEngine`, and supporting types do not
//! regress the native API.
//!
//! These tests run on native targets (the bindgen attributes are gated to
//! `cfg(target_arch = "wasm32")`). They guard the contract that:
//!
//! - public fields on `CompiledNetwork` and `PredictiveCodingEngine` remain
//!   accessible to native consumers (`rust_scorer`, CLI tools);
//! - `activate_view` matches `activate` semantics on native;
//! - the constructors and getters still behave as plain Rust methods.

use neat_core::network::CompiledNetwork;
use neat_core::pc_inference::{PcConnection, PcNeuron, PredictiveCodingEngine};
use neat_core::squash::SquashType;

/// Minimal serialised network: 1 input, 1 identity output, weight 1.0, bias 0.5.
fn minimal_network_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    // Header: num_neurons=2, num_inputs=1
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    // Output neuron: bias=0.5 (f64), squash=IDENTITY (0), is_constant=0,
    // num_synapses=1
    data.extend_from_slice(&0.5_f64.to_le_bytes());
    data.push(0); // squash_type IDENTITY
    data.push(0); // is_constant false
    data.extend_from_slice(&1u16.to_le_bytes());
    // Synapse: from_index=0, synapse_type=0, padding=0, weight=1.0 (f64)
    data.extend_from_slice(&0u16.to_le_bytes());
    data.push(0); // synapse_type
    data.push(0); // padding
    data.extend_from_slice(&1.0_f64.to_le_bytes());
    data
}

#[test]
fn compiled_network_constructor_succeeds() {
    let bytes = minimal_network_bytes();
    let net = CompiledNetwork::new(&bytes).expect("network should parse");
    assert_eq!(net.num_neurons(), 2);
    assert_eq!(net.num_inputs(), 1);
    assert_eq!(net.num_synapses(), 1);
}

#[test]
fn compiled_network_public_fields_remain_accessible() {
    // Native consumers (e.g. NEAT-AI-scorer) read public fields directly.
    // The `#[wasm_bindgen(skip)]` annotations must not turn them private.
    let bytes = minimal_network_bytes();
    let net = CompiledNetwork::new(&bytes).expect("parse");

    // Reading these compiles only when the fields are still `pub`.
    let _: usize = net.num_neurons;
    let _: usize = net.num_inputs;
    let _: &Vec<_> = &net.neurons;
    let _: &Vec<_> = &net.synapses;
    let _: &Vec<f32> = &net.activations;
    let _: &Vec<f32> = &net.hint_values_buffer;
    let _: &Vec<f32> = &net.trace_data_buffer;
}

#[test]
fn activate_view_matches_activate() {
    // `activate_view` is the new method added to satisfy the canonical .d.ts.
    // On native it must behave like `activate` (same outputs).
    let bytes = minimal_network_bytes();
    let mut net_a = CompiledNetwork::new(&bytes).expect("parse");
    let mut net_b = CompiledNetwork::new(&bytes).expect("parse");

    let inputs = [2.0f32];
    let from_activate = net_a.activate(&inputs, 1);
    let from_view = net_b.activate_view(&inputs, 1);

    assert_eq!(from_activate.len(), from_view.len());
    for (a, b) in from_activate.iter().zip(from_view.iter()) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }
    // Verify the output is the expected identity(2.0 * 1.0 + 0.5) = 2.5.
    assert!((from_activate[0] - 2.5).abs() < 1e-5);
}

#[test]
fn to_dot_method_callable_on_compiled_network() {
    // Issue #43 — `to_dot` must remain accessible as a `CompiledNetwork`
    // method on native (and via `#[wasm_bindgen]` on wasm32). This guards
    // that the bindgen impl block does not regress the native surface.
    let bytes = minimal_network_bytes();
    let net = CompiledNetwork::new(&bytes).expect("parse");
    let dot = net.to_dot(1);
    assert!(dot.starts_with("digraph "));
    assert!(dot.trim_end().ends_with('}'));
}

#[test]
fn to_topology_json_method_callable_on_compiled_network() {
    // Issue #43 — `to_topology_json` must remain accessible as a
    // `CompiledNetwork` method on native (and via `#[wasm_bindgen]` on wasm32).
    let bytes = minimal_network_bytes();
    let net = CompiledNetwork::new(&bytes).expect("parse");
    let json = net.to_topology_json(1);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("to_topology_json output must be valid JSON");
    assert_eq!(parsed["num_inputs"], 1);
    assert_eq!(parsed["num_outputs"], 1);
    assert_eq!(parsed["num_neurons"], 2);
}

#[test]
fn reset_state_clears_non_input_activations() {
    let bytes = minimal_network_bytes();
    let mut net = CompiledNetwork::new(&bytes).expect("parse");
    let _ = net.activate(&[1.0], 1);
    // Output activation should now be non-zero.
    assert!(net.activations[1].abs() > 0.0);
    net.reset_state();
    assert_eq!(net.activations[1], 0.0);
}

#[test]
fn pc_engine_constructor_and_getters() {
    // Build a minimal serialised PC engine: 1 input, 1 output, no hidden.
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes()); // num_inputs
    data.extend_from_slice(&1u32.to_le_bytes()); // num_outputs
    data.extend_from_slice(&2u32.to_le_bytes()); // num_neurons_total
    data.extend_from_slice(&1u32.to_le_bytes()); // inference_steps
    data.extend_from_slice(&0.1_f32.to_le_bytes()); // inference_rate
    data.extend_from_slice(&1e-3_f32.to_le_bytes()); // energy_threshold
    // Output neuron: bias=0.0, squash=IDENTITY, is_hidden=0, num_conn=1
    data.extend_from_slice(&0.0_f32.to_le_bytes());
    data.push(0); // squash IDENTITY
    data.push(0); // is_hidden false
    data.extend_from_slice(&1u16.to_le_bytes());
    // Connection: from=0, weight=1.0
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1.0_f32.to_le_bytes());

    let engine = PredictiveCodingEngine::new(&data).expect("parse");
    assert_eq!(engine.num_neurons(), 2);
    assert_eq!(engine.num_inputs(), 1);
    assert_eq!(engine.num_outputs(), 1);
}

#[test]
fn pc_engine_public_fields_remain_accessible() {
    // Build directly via `new_from_parts` to dodge the byte format.
    let neuron = PcNeuron {
        bias: 0.0,
        squash_type: SquashType::Identity,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    };
    let conn = PcConnection {
        from: 0,
        weight: 1.0,
    };
    let engine =
        PredictiveCodingEngine::new_from_parts(1, 1, vec![neuron], vec![conn], 1, 0.1, 1e-3);

    // Public-field access must still compile (rust_scorer relies on this).
    let _: usize = engine.num_neurons;
    let _: usize = engine.num_inputs;
    let _: usize = engine.num_outputs;
    let _: &Vec<_> = &engine.neurons;
    let _: &Vec<_> = &engine.connections;
}

#[test]
fn pc_engine_infer_wasm_packs_header_and_body() {
    let neuron = PcNeuron {
        bias: 0.0,
        squash_type: SquashType::Identity,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    };
    let conn = PcConnection {
        from: 0,
        weight: 1.0,
    };
    let engine =
        PredictiveCodingEngine::new_from_parts(1, 1, vec![neuron], vec![conn], 1, 0.1, 1e-3);

    let packed = engine.infer_wasm(&[0.5], None);
    // Header is 6 floats; we then carry latents (num_neurons=2),
    // predictions (1), errors (1), and energy history (>=1).
    assert!(packed.len() > 6 + 2 + 1 + 1);
    // Header[3] is num_neurons, Header[4] is num_non_inputs.
    assert_eq!(packed[3], 2.0);
    assert_eq!(packed[4], 1.0);
}
