//! Deterministic topology export from [`CompiledNetwork`] (Issue #22).
//!
//! This module provides pure-data exports — DOT (Graphviz) and a minimal
//! topology JSON — suitable for external tooling (Graphviz renderers, web
//! viewers, snapshot diffs) without pulling a GUI stack into `neat-core`.
//!
//! Output is deterministic for identical input: nodes are emitted in ascending
//! index order, synapses in the order they are stored on the network (which is
//! itself deterministic — compilation groups by target neuron and preserves
//! per-target synapse order), and weights are formatted with a fixed
//! precision. Two exports of the same network produce byte-identical output.
//!
//! # Example
//!
//! ```
//! use neat_core::{compile_creature, parse_creature_json};
//!
//! let json = r#"{
//!     "input": 1,
//!     "output": 1,
//!     "neurons": [
//!         {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
//!     ],
//!     "synapses": [
//!         {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}
//!     ],
//!     "forwardOnly": true
//! }"#;
//! let creature = parse_creature_json(json).unwrap();
//! let network = compile_creature(&creature).unwrap();
//!
//! let dot = network.to_dot(creature.output);
//! assert!(dot.starts_with("digraph "));
//!
//! let topology = network.to_topology_json(creature.output);
//! assert!(topology.contains("\"num_inputs\""));
//! ```

use std::fmt::Write;

use serde::Serialize;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::network::CompiledNetwork;
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// Node classification used by topology exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// Input neuron (index `0..num_inputs`).
    Input,
    /// Hidden (non-output, non-constant) neuron.
    Hidden,
    /// Output neuron (last `num_outputs` neurons).
    Output,
    /// Constant neuron (no inward connections).
    Constant,
}

impl NodeKind {
    /// Lowercase identifier used in DOT labels and JSON exports.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Input => "input",
            NodeKind::Hidden => "hidden",
            NodeKind::Output => "output",
            NodeKind::Constant => "constant",
        }
    }
}

/// Classify a neuron by its absolute index in the compiled network.
fn classify(network: &CompiledNetwork, index: usize, num_outputs: usize) -> NodeKind {
    if index < network.num_inputs {
        return NodeKind::Input;
    }
    let neuron = &network.neurons[index - network.num_inputs];
    if neuron.is_constant {
        return NodeKind::Constant;
    }
    let output_start = network.num_neurons.saturating_sub(num_outputs);
    if index >= output_start {
        NodeKind::Output
    } else {
        NodeKind::Hidden
    }
}

/// Canonical name for a squash (activation) function.
///
/// Names match those accepted by [`crate::creature::parse_squash_name`], so a
/// JSON export round-trips through the TypeScript contract where possible.
pub fn squash_name(ty: SquashType) -> &'static str {
    match ty {
        SquashType::Identity => "IDENTITY",
        SquashType::Relu => "RELU",
        SquashType::Relu6 => "ReLU6",
        SquashType::LeakyRelu => "LeakyReLU",
        SquashType::Selu => "SELU",
        SquashType::Elu => "ELU",
        SquashType::Logistic => "LOGISTIC",
        SquashType::Tanh => "TANH",
        SquashType::HardTanh => "HARD_TANH",
        SquashType::Softsign => "SOFTSIGN",
        SquashType::Softplus => "Softplus",
        SquashType::Swish => "Swish",
        SquashType::Mish => "Mish",
        SquashType::Gelu => "GELU",
        SquashType::Sine => "SINE",
        SquashType::Cosine => "Cosine",
        SquashType::Tan => "TAN",
        SquashType::ArcTan => "ArcTan",
        SquashType::Gaussian => "GAUSSIAN",
        SquashType::BentIdentity => "BENT_IDENTITY",
        SquashType::BipolarSigmoid => "BIPOLAR_SIGMOID",
        SquashType::Bipolar => "BIPOLAR",
        SquashType::Step => "STEP",
        SquashType::Complement => "COMPLEMENT",
        SquashType::Absolute => "ABSOLUTE",
        SquashType::Square => "SQUARE",
        SquashType::Cube => "Cube",
        SquashType::Sqrt => "SQRT",
        SquashType::StdInverse => "StdInverse",
        SquashType::Exponential => "Exponential",
        SquashType::LogSigmoid => "LogSigmoid",
        SquashType::Isru => "ISRU",
        SquashType::Minimum => "MINIMUM",
        SquashType::Maximum => "MAXIMUM",
        SquashType::If => "IF",
        SquashType::Hypotenuse => "HYPOT",
        SquashType::HypotenuseV2 => "HYPOTv2",
        SquashType::Mean => "MEAN",
    }
}

/// Canonical name for a synapse type.
pub fn synapse_type_name(ty: SynapseType) -> &'static str {
    match ty {
        SynapseType::Standard => "Standard",
        SynapseType::Condition => "Condition",
        SynapseType::Negative => "Negative",
        SynapseType::Positive => "Positive",
    }
}

// ---------------------------------------------------------------------------
// JSON export
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct NodeRecord {
    index: usize,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    bias: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    squash: Option<&'static str>,
}

#[derive(Serialize)]
struct SynapseRecord {
    from: u32,
    to: u32,
    weight: f32,
    #[serde(rename = "type")]
    synapse_type: &'static str,
}

#[derive(Serialize)]
struct TopologyExport {
    num_inputs: usize,
    num_outputs: usize,
    num_neurons: usize,
    nodes: Vec<NodeRecord>,
    synapses: Vec<SynapseRecord>,
}

/// Build a deterministic JSON topology description.
///
/// The returned string is pretty-printed JSON using `serde_json`'s default
/// formatter. Nodes are listed in ascending index order; synapses are listed
/// in network storage order (grouped by target neuron, then by per-target
/// compilation order). See the module docs for the determinism contract.
pub fn to_topology_json(network: &CompiledNetwork, num_outputs: usize) -> String {
    let mut nodes = Vec::with_capacity(network.num_neurons);

    for i in 0..network.num_inputs {
        nodes.push(NodeRecord {
            index: i,
            kind: NodeKind::Input.as_str(),
            bias: None,
            squash: None,
        });
    }

    for (offset, neuron) in network.neurons.iter().enumerate() {
        let index = network.num_inputs + offset;
        let kind = classify(network, index, num_outputs);
        let squash = SquashType::from(neuron.squash_type);
        nodes.push(NodeRecord {
            index,
            kind: kind.as_str(),
            bias: Some(neuron.bias),
            squash: Some(squash_name(squash)),
        });
    }

    let mut synapses = Vec::with_capacity(network.synapses.len());
    for (offset, neuron) in network.neurons.iter().enumerate() {
        let to_index = (network.num_inputs + offset) as u32;
        let start = neuron.start_synapse as usize;
        let end = start + neuron.num_synapses as usize;
        for synapse in &network.synapses[start..end] {
            let synapse_type = SynapseType::from(synapse.synapse_type);
            synapses.push(SynapseRecord {
                // Issue #177 - from_index narrowed to u16; widen for the export record.
                from: synapse.from_index as u32,
                to: to_index,
                weight: synapse.weight,
                synapse_type: synapse_type_name(synapse_type),
            });
        }
    }

    let export = TopologyExport {
        num_inputs: network.num_inputs,
        num_outputs,
        num_neurons: network.num_neurons,
        nodes,
        synapses,
    };

    // `serde_json` serialises struct fields in declaration order, so this is
    // deterministic. `to_string_pretty` is stable across calls for the same
    // input.
    serde_json::to_string_pretty(&export).expect("TopologyExport is always serialisable to JSON")
}

// ---------------------------------------------------------------------------
// DOT export
// ---------------------------------------------------------------------------

/// Emit a deterministic DOT (Graphviz) representation of the network.
///
/// Node shape convention:
/// - inputs: `ellipse`
/// - hidden: `box`
/// - outputs: `doublecircle`
/// - constants: `diamond`
///
/// Each edge is labelled with `"<weight> <SynapseType>"` where `<weight>` is
/// formatted with six-decimal-place precision for stable output.
pub fn to_dot(network: &CompiledNetwork, num_outputs: usize) -> String {
    // Pre-allocate a reasonable buffer to avoid intermediate reallocations —
    // roughly 64 bytes per node + 48 bytes per edge.
    let mut out =
        String::with_capacity(64 + network.num_neurons * 64 + network.synapses.len() * 48);

    out.push_str("digraph CompiledNetwork {\n");
    out.push_str("  rankdir=LR;\n");

    // Node declarations in ascending index order.
    for i in 0..network.num_inputs {
        writeln!(out, "  n{i} [label=\"n{i}\\ninput\", shape=ellipse];",).unwrap();
    }

    for (offset, neuron) in network.neurons.iter().enumerate() {
        let index = network.num_inputs + offset;
        let kind = classify(network, index, num_outputs);
        let shape = match kind {
            NodeKind::Input => "ellipse",
            NodeKind::Hidden => "box",
            NodeKind::Output => "doublecircle",
            NodeKind::Constant => "diamond",
        };
        let squash = squash_name(SquashType::from(neuron.squash_type));
        let kind_label = kind.as_str();
        let bias = neuron.bias;
        writeln!(
            out,
            "  n{index} [label=\"n{index}\\n{kind_label}\\n{squash}\\nbias={bias:.6}\", shape={shape}];",
        )
        .unwrap();
    }

    // Edges in storage order.
    for (offset, neuron) in network.neurons.iter().enumerate() {
        let to_index = network.num_inputs + offset;
        let start = neuron.start_synapse as usize;
        let end = start + neuron.num_synapses as usize;
        for synapse in &network.synapses[start..end] {
            let from_index = synapse.from_index as usize;
            let weight = synapse.weight;
            let synapse_type = synapse_type_name(SynapseType::from(synapse.synapse_type));
            writeln!(
                out,
                "  n{from_index} -> n{to_index} [label=\"{weight:.6} {synapse_type}\"];",
            )
            .unwrap();
        }
    }

    out.push_str("}\n");
    out
}

// ---------------------------------------------------------------------------
// Methods on CompiledNetwork
// ---------------------------------------------------------------------------
//
// Issue #43 — annotate the impl block with `#[wasm_bindgen]` on `wasm32`
// targets so `to_dot` and `to_topology_json` appear as methods on the
// `CompiledNetwork` JS class in `wasm_activation/pkg/wasm_activation.{js,d.ts}`,
// mirroring the pattern used for `activate` / `activate_view` / `reset_state`
// in `network.rs`. NEAT-AI's TypeScript wrapper depends on these exports.

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl CompiledNetwork {
    /// Export this network as a DOT (Graphviz) string.
    ///
    /// See [`to_dot`] for format details and determinism guarantees.
    ///
    /// `num_outputs` is required because [`CompiledNetwork`] itself does not
    /// record the output count — outputs are the last `num_outputs` neurons
    /// by construction.
    pub fn to_dot(&self, num_outputs: usize) -> String {
        to_dot(self, num_outputs)
    }

    /// Export this network as a topology JSON string.
    ///
    /// See [`to_topology_json`] for format details and determinism guarantees.
    pub fn to_topology_json(&self, num_outputs: usize) -> String {
        to_topology_json(self, num_outputs)
    }
}
