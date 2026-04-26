//! Shared computation library for NEAT-AI neural network operations.
//!
//! This crate contains the core neural network logic extracted from the
//! `wasm_activation` crate. Native targets omit `wasm-bindgen`; on
//! `wasm32-unknown-unknown`, `accumulate` exports use `wasm-bindgen` behind
//! `cfg_attr` so the same sources build for CLI tools and WASM.
//!
//! Issue #1964 - Extract shared Rust library crate from wasm_activation.

// Core computation modules
pub mod accumulate;
pub mod creature;
pub mod derivative;
pub mod elastic_distribution;
pub mod error;
pub mod fused_error;
pub mod loss;
pub mod network;
pub mod pc_inference;
pub mod pc_learning;
pub mod range;
pub mod safe_zone;
pub mod score_scan;
pub mod simd;
pub mod squash;
pub mod synapse_type;
pub mod topological_backprop;
pub mod topology_export;
pub mod topology_ops;
pub mod training_bin_stream;
pub mod training_data;
pub mod training_state;
pub mod unsquash;

// Issue #36 — WASM-only `#[wasm_bindgen]` shims that wrap apply_* helpers,
// tuple returns, and the byte-packed `propagate_topological` ABI. Native
// targets do not see this module.
#[cfg(target_arch = "wasm32")]
pub mod wasm_exports;

// Re-export key types for convenience
pub use creature::{
    CreatureExport, NeuronExport, SynapseExport, compile_creature, creature_to_json,
    creature_to_json_pretty, parse_creature_json, parse_squash_name, parse_synapse_type,
    squash_name_from, synapse_type_name_from,
};
pub use network::{CompiledNetwork, NeuronData, SynapseData};
pub use pc_inference::PredictiveCodingEngine;
pub use squash::SquashType;
pub use synapse_type::SynapseType;
pub use training_data::{
    SeekingRecordReader, TrainingDataConfig, TrainingDataError, TrainingDataIterator,
    TrainingRecord, find_bin_files, read_dir as read_training_dir, read_file as read_training_file,
};

// Re-export core functions
pub use accumulate::{
    accumulate_bias_batch_4way, accumulate_bias_batch_8way, accumulate_weight_batch_4way,
    accumulate_weight_batch_8way, calculate_bias, calculate_weight,
};
pub use derivative::{apply_derivative, apply_derivative_simd_4way};
pub use elastic_distribution::distribute_elastic_error;
pub use error::{apply_calculate_error, apply_calculate_error_batch_4way};
pub use fused_error::apply_fused_error_distribution;
pub use loss::{
    cross_entropy_sum_batch_packed, hinge_sum_batch_packed, mae_sum_batch_packed,
    mape_sum_batch_packed, mse_mean_record, mse_sum_batch_packed, msle_sum_batch_packed,
};
pub use range::{apply_get_range, apply_limit_range, apply_validate_range};
pub use safe_zone::{apply_safe_zone_adjustment, apply_safe_zone_adjustment_batch};
pub use score_scan::{compute_score_components, scan_max_bias, scan_max_weight};
pub use squash::apply_squash;
pub use topological_backprop::{
    NEURON_TYPE_CONSTANT, NEURON_TYPE_HIDDEN, NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, NeuronInput,
    NeuronType, PropagateInput, PropagateOutcome, PropagateOutput, StandardOutcome, SynapseDelta,
    SynapseInput, propagate_topological_loop,
};
pub use topology_export::{NodeKind, squash_name, synapse_type_name, to_dot, to_topology_json};
pub use topology_ops::{
    BACKWARD_CONNECTION, DUPLICATE_CONNECTION, SELF_CONNECTION, SORT_ERROR_FROM, SORT_ERROR_TO,
    STRUCTURAL_BIAS_NOT_FINITE, STRUCTURAL_CONSTANT_HAS_INWARD, STRUCTURAL_HIDDEN_NO_INWARD,
    STRUCTURAL_HIDDEN_NO_OUTWARD, STRUCTURAL_IF_MISSING_CONDITION, STRUCTURAL_IF_MISSING_NEGATIVE,
    STRUCTURAL_IF_MISSING_POSITIVE, STRUCTURAL_IF_TOO_FEW_INWARD, STRUCTURAL_SYNAPSE_TARGETS_INPUT,
    STRUCTURAL_VALID, VALID, compute_reverse_topological_order, detect_cycles,
    scan_available_connections, validate_structural_integrity, validate_topology,
    validate_topology_batch,
};
pub use training_state::{
    accumulate_bias_persistent_4way, accumulate_bias_persistent_8way,
    accumulate_weight_persistent_4way, accumulate_weight_persistent_8way, free_training_state,
    get_training_state_num_neurons, get_training_state_num_synapses, init_training_state,
    read_all_neuron_state, read_all_synapse_state, read_neuron_state, read_synapse_state,
    reset_training_state,
};
pub use unsquash::apply_unsquash;
