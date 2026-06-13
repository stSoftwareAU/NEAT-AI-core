//! Creature JSON (de)serialisation for NEAT-AI neural networks.
//!
//! This module provides Rust structs matching the TypeScript `CreatureExport`,
//! `NeuronExport`, and `SynapseExport` interfaces, along with conversion to
//! `CompiledNetwork` for efficient activation.
//!
//! The structs derive both [`serde::Deserialize`] and [`serde::Serialize`], so
//! a parsed creature can be written back out to the same JSON shape. Round
//! tripping — `parse -> serialise -> parse` — preserves every field, and two
//! serialisations of the same `CreatureExport` produce byte-identical JSON
//! (serde emits fields in declaration order).
//!
//! The `#[serde(rename = "...")]` attributes (`semanticVersion`, `forwardOnly`,
//! `fromUUID`, `toUUID`, `type`) apply symmetrically on both input and output,
//! so the canonical TypeScript camelCase shape is preserved.
//!
//! Optional string fields are skipped when `None` to match the TypeScript
//! "optional field" convention (absent key rather than explicit `null`).
//!
//! The inverse helpers [`squash_name_from`] and [`synapse_type_name_from`]
//! provide a 1:1 inverse of [`parse_squash_name`] and [`parse_synapse_type`]
//! respectively, for callers constructing `NeuronExport` / `SynapseExport`
//! values in Rust from enum variants.
//!
//! Issues: #1965 (initial deserialisation), #30 (symmetric serialisation).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::network::{CompiledNetwork, NeuronData, SynapseData};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// Top-level creature export format matching the TypeScript `CreatureExport` interface.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CreatureExport {
    /// Number of input neurons.
    pub input: usize,
    /// Number of output neurons.
    pub output: usize,
    /// List of non-input neurons (hidden, output, constant).
    pub neurons: Vec<NeuronExport>,
    /// List of synapses connecting neurons.
    pub synapses: Vec<SynapseExport>,
    /// Optional semantic version string.
    #[serde(rename = "semanticVersion", skip_serializing_if = "Option::is_none")]
    pub semantic_version: Option<String>,
    /// When true, training rows are independent (no recurrent / feedback state).
    #[serde(rename = "forwardOnly", default)]
    pub forward_only: bool,
}

/// Neuron export format matching the TypeScript `NeuronExport` interface.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NeuronExport {
    /// Neuron type: "hidden", "output", or "constant".
    #[serde(rename = "type")]
    pub neuron_type: String,
    /// Unique identifier for the neuron.
    pub uuid: String,
    /// Bias value for the neuron.
    pub bias: f64,
    /// Activation function name (e.g. "TANH", "ReLU"). Defaults to IDENTITY.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub squash: Option<String>,
}

/// Synapse export format matching the TypeScript `SynapseExport` interface.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SynapseExport {
    /// UUID of the source neuron (e.g. "input-0" for input neurons).
    #[serde(rename = "fromUUID")]
    pub from_uuid: String,
    /// UUID of the destination neuron.
    #[serde(rename = "toUUID")]
    pub to_uuid: String,
    /// Connection weight.
    pub weight: f64,
    /// Optional synapse type: "positive", "negative", or "condition".
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub synapse_type: Option<String>,
}

/// Errors that can occur when parsing, serialising, or compiling a creature.
///
/// Replaces the previous `Result<_, String>` returns on the public creature
/// API (Issue #115). Mirrors the existing [`crate::training_data::TrainingDataError`]
/// idiom: implements [`std::error::Error`], can be matched on by variant, and
/// preserves the underlying [`serde_json::Error`] as the error `source()` so
/// callers can `?`-propagate into their own error type and programmatically
/// distinguish a JSON failure from a structural compile failure.
#[derive(Debug)]
pub enum CreatureError {
    /// JSON (de)serialisation failed. Exposes the underlying
    /// [`serde_json::Error`] via [`std::error::Error::source`].
    Json(serde_json::Error),
    /// An activation/squash function name was not recognised.
    UnknownSquash(String),
    /// A synapse referenced a source neuron UUID that does not exist.
    UnknownSourceUuid(String),
    /// The declared output count did not match the number of output neurons found.
    OutputCountMismatch {
        /// Output count declared by `CreatureExport::output`.
        expected: usize,
        /// Number of neurons of type `"output"` actually present.
        found: usize,
    },
}

impl std::fmt::Display for CreatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreatureError::Json(e) => write!(f, "Creature JSON error: {e}"),
            CreatureError::UnknownSquash(name) => write!(f, "Unknown squash function: {name}"),
            CreatureError::UnknownSourceUuid(uuid) => {
                write!(f, "Unknown source neuron UUID: {uuid}")
            }
            CreatureError::OutputCountMismatch { expected, found } => {
                write!(f, "Expected {expected} output neurons, found {found}")
            }
        }
    }
}

impl std::error::Error for CreatureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CreatureError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for CreatureError {
    fn from(e: serde_json::Error) -> Self {
        CreatureError::Json(e)
    }
}

/// Parse a squash function name string into a `SquashType` enum value.
///
/// Handles all activation function names from the TypeScript codebase,
/// including aliases (CLIPPED, RELU, INVERSE, SINUSOID).
pub fn parse_squash_name(name: &str) -> Result<SquashType, CreatureError> {
    match name {
        "IDENTITY" => Ok(SquashType::Identity),
        "ReLU" | "RELU" => Ok(SquashType::Relu),
        "ReLU6" => Ok(SquashType::Relu6),
        "LeakyReLU" => Ok(SquashType::LeakyRelu),
        "SELU" => Ok(SquashType::Selu),
        "ELU" => Ok(SquashType::Elu),
        "LOGISTIC" => Ok(SquashType::Logistic),
        "TANH" => Ok(SquashType::Tanh),
        "HARD_TANH" | "CLIPPED" => Ok(SquashType::HardTanh),
        "SOFTSIGN" => Ok(SquashType::Softsign),
        "Softplus" => Ok(SquashType::Softplus),
        "Swish" => Ok(SquashType::Swish),
        "Mish" => Ok(SquashType::Mish),
        "GELU" => Ok(SquashType::Gelu),
        "SINE" | "SINUSOID" => Ok(SquashType::Sine),
        "Cosine" => Ok(SquashType::Cosine),
        "TAN" => Ok(SquashType::Tan),
        "ArcTan" => Ok(SquashType::ArcTan),
        "GAUSSIAN" => Ok(SquashType::Gaussian),
        "BENT_IDENTITY" => Ok(SquashType::BentIdentity),
        "BIPOLAR_SIGMOID" => Ok(SquashType::BipolarSigmoid),
        "BIPOLAR" => Ok(SquashType::Bipolar),
        "STEP" => Ok(SquashType::Step),
        "COMPLEMENT" | "INVERSE" => Ok(SquashType::Complement),
        "ABSOLUTE" => Ok(SquashType::Absolute),
        "SQUARE" => Ok(SquashType::Square),
        "Cube" => Ok(SquashType::Cube),
        "SQRT" => Ok(SquashType::Sqrt),
        "StdInverse" => Ok(SquashType::StdInverse),
        "Exponential" => Ok(SquashType::Exponential),
        "LogSigmoid" => Ok(SquashType::LogSigmoid),
        "ISRU" => Ok(SquashType::Isru),
        "MINIMUM" => Ok(SquashType::Minimum),
        "MAXIMUM" => Ok(SquashType::Maximum),
        "IF" => Ok(SquashType::If),
        "HYPOT" => Ok(SquashType::Hypotenuse),
        "HYPOTv2" => Ok(SquashType::HypotenuseV2),
        "MEAN" => Ok(SquashType::Mean),
        _ => Err(CreatureError::UnknownSquash(name.to_string())),
    }
}

/// Parse a synapse type string into a `SynapseType` enum value.
///
/// Maps the TypeScript synapse type strings ("positive", "negative", "condition")
/// to the Rust `SynapseType` enum. When no type is specified, returns `Standard`.
pub fn parse_synapse_type(type_str: Option<&str>) -> SynapseType {
    match type_str {
        Some("condition") => SynapseType::Condition,
        Some("negative") => SynapseType::Negative,
        Some("positive") => SynapseType::Positive,
        _ => SynapseType::Standard,
    }
}

/// Canonical activation name for a [`SquashType`].
///
/// This is the 1:1 inverse of [`parse_squash_name`]: for every variant `v`,
/// `parse_squash_name(squash_name_from(v)) == Ok(v)` holds. Where
/// [`parse_squash_name`] accepts aliases (e.g. `"ReLU"` and `"RELU"` both map
/// to [`SquashType::Relu`]), this function returns the canonical variant used
/// by the TypeScript emitter.
///
/// Use this when populating [`NeuronExport::squash`] from a Rust
/// [`SquashType`] before serialising back to JSON.
pub fn squash_name_from(ty: SquashType) -> &'static str {
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

/// Canonical JSON type string for a [`SynapseType`].
///
/// Inverse of [`parse_synapse_type`]: returns `None` for
/// [`SynapseType::Standard`] (omitted in the JSON export by convention) and
/// the canonical lowercase TypeScript names for the other variants.
/// For every variant `v`, `parse_synapse_type(synapse_type_name_from(v)) == v`
/// holds.
pub fn synapse_type_name_from(ty: SynapseType) -> Option<&'static str> {
    match ty {
        SynapseType::Standard => None,
        SynapseType::Condition => Some("condition"),
        SynapseType::Negative => Some("negative"),
        SynapseType::Positive => Some("positive"),
    }
}

/// Parse a creature JSON string into a `CreatureExport` struct.
pub fn parse_creature_json(json: &str) -> Result<CreatureExport, CreatureError> {
    Ok(serde_json::from_str(json)?)
}

/// Serialise a [`CreatureExport`] to canonical JSON text.
///
/// Output is deterministic: fields are emitted in struct declaration order,
/// so two calls with the same input produce byte-identical output. This is
/// the symmetric counterpart to [`parse_creature_json`].
pub fn creature_to_json(creature: &CreatureExport) -> Result<String, CreatureError> {
    Ok(serde_json::to_string(creature)?)
}

/// Pretty-printed variant of [`creature_to_json`].
pub fn creature_to_json_pretty(creature: &CreatureExport) -> Result<String, CreatureError> {
    Ok(serde_json::to_string_pretty(creature)?)
}

/// Convert a `CreatureExport` into a `CompiledNetwork` for activation.
///
/// This performs the following steps:
/// 1. Assigns integer indices to all neurons (inputs first, then non-inputs in order)
/// 2. Maps neuron UUIDs to their indices
/// 3. Resolves synapse UUID references to index-based connections
/// 4. Maps squash function names and synapse type strings to enum values
pub fn compile_creature(creature: &CreatureExport) -> Result<CompiledNetwork, CreatureError> {
    let num_inputs = creature.input;
    let num_outputs = creature.output;

    // Build UUID-to-index mapping using owned Strings.
    // Input neurons use "input-N" UUIDs.
    let mut uuid_to_index: HashMap<String, usize> =
        HashMap::with_capacity(num_inputs + creature.neurons.len());
    for i in 0..num_inputs {
        uuid_to_index.insert(format!("input-{i}"), i);
    }

    // Validate neuron counts
    let mut output_count = 0;
    for neuron in &creature.neurons {
        if neuron.neuron_type == "output" {
            output_count += 1;
        }
    }
    if output_count != num_outputs {
        return Err(CreatureError::OutputCountMismatch {
            expected: num_outputs,
            found: output_count,
        });
    }

    // Assign indices to non-input neurons (they follow input neurons)
    for (i, neuron) in creature.neurons.iter().enumerate() {
        let index = num_inputs + i;
        uuid_to_index.insert(neuron.uuid.clone(), index);
    }

    let num_neurons = num_inputs + creature.neurons.len();

    // Group synapses by destination neuron UUID for ordered construction
    let mut synapses_by_target: HashMap<&str, Vec<&SynapseExport>> = HashMap::new();
    for synapse in &creature.synapses {
        synapses_by_target
            .entry(synapse.to_uuid.as_str())
            .or_default()
            .push(synapse);
    }

    // Build neuron and synapse data arrays
    let mut neurons: Vec<NeuronData> = Vec::with_capacity(creature.neurons.len());
    let mut synapses: Vec<SynapseData> = Vec::new();

    for neuron in &creature.neurons {
        let is_constant = neuron.neuron_type == "constant";
        let squash_name = neuron.squash.as_deref().unwrap_or("IDENTITY");
        let squash_type = parse_squash_name(squash_name)?;

        let start_synapse = synapses.len() as u32;
        let neuron_synapses = synapses_by_target.get(neuron.uuid.as_str());

        let mut num_synapses: u16 = 0;
        if let Some(neuron_syn) = neuron_synapses {
            for syn in neuron_syn {
                let from_index = *uuid_to_index
                    .get(syn.from_uuid.as_str())
                    .ok_or_else(|| CreatureError::UnknownSourceUuid(syn.from_uuid.clone()))?;
                let synapse_type = parse_synapse_type(syn.synapse_type.as_deref());

                synapses.push(SynapseData {
                    weight: syn.weight as f32,
                    from_index: from_index as u32,
                    synapse_type: synapse_type as u8,
                    _padding: [0; 3],
                });
                num_synapses += 1;
            }
        }

        neurons.push(NeuronData {
            bias: neuron.bias as f32,
            start_synapse,
            num_synapses,
            squash_type: squash_type as u8,
            is_constant,
        });
    }

    let num_non_inputs = creature.neurons.len();

    // Estimate trace data buffer capacity (same heuristic as binary deserialisation)
    let estimated_trace_size = (num_non_inputs / 10).max(1) * 2 + 1;

    Ok(CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::with_capacity(estimated_trace_size),
        // Issue #155 - 4-way batch scratch buffers
        batch_activations: [
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
        ],
        batch_hints: [
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
        ],
        batch_traces: [
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
        ],
    })
}
