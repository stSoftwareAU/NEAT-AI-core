//! Shared computation library for NEAT-AI neural network operations.
//!
//! This crate contains the core neural network logic extracted from the
//! `wasm_activation` crate. Native targets omit `wasm-bindgen`; on the
//! **wasm target family** (`wasm32-unknown-unknown` *and*
//! `wasm64-unknown-unknown`, Issue #541), `accumulate` exports use
//! `wasm-bindgen` behind `cfg_attr` so the same sources build for CLI tools and
//! both WASM address sizes.
//!
//! Issue #1964 - Extract shared Rust library crate from wasm_activation.

// Issue #541 - `core::arch::wasm64` is unstable (`simd_wasm64`,
// rust-lang/rust#90599). `wasm64-unknown-unknown` is a Tier 3 target that
// already requires nightly + `-Z build-std`, so enabling the feature there
// costs the stable wasm32 and native builds nothing: the attribute is inert
// unless `target_arch = "wasm64"`.
#![cfg_attr(target_arch = "wasm64", feature(simd_wasm64))]

// Core computation modules
pub mod accumulate;
pub mod batch_scoring;
pub mod creature;
pub mod creature_validate;
pub mod decision_tree;
pub mod derivative;
pub mod elastic_distribution;
pub mod error;
pub mod fused_error;
pub mod if_graft;
pub mod loss;
pub mod network;
pub mod parallel_scoring;
pub mod propagate_codec;
pub mod range;
pub mod safe_zone;
pub mod score_scan;
pub mod simd;
pub mod squash;
pub mod squash_simd;
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
#[cfg(target_family = "wasm")]
pub mod wasm_exports;

// Issue #541 — one home for the `core::arch::wasm32` / `core::arch::wasm64`
// split, so both wasm SIMD kernels reach the SIMD128 intrinsics by one path.
#[cfg(target_family = "wasm")]
pub mod wasm_arch;

// Re-export key types for convenience
pub use creature::{
    CreatureError, CreatureExport, MemeticExport, MemeticWeightExport, NeuronExport, SynapseExport,
    compile_creature, creature_to_json, creature_to_json_pretty, parse_creature_json,
    parse_squash_name, parse_synapse_type, squash_name_from, synapse_type_name_from,
    validate_creature_width, validate_no_duplicate_synapses,
};
// Issue #559 — the shared creature-validation contract.
pub use creature_validate::{
    FailureClass, ValidateOptions, ValidationFailure, ValidationStats, creature_validate,
    validate_synapse_and_memetic_rules,
};
// Issue #555 — canonical IF decision-tree fixtures and the graft helper.
pub use decision_tree::{
    DecisionCase, depth2_tree_creature, linear_base_creature, residual_correction_creature,
    stump_creature,
};
pub use if_graft::{
    GraftError, IfCorrectionSpec, IfNodeSpec, graft_if_correction, graft_if_node, graft_if_tree,
    validate_creature_topology,
};
pub use network::{CompiledNetwork, NetworkError, NeuronData, SynapseData, hot_synapse_soa};
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
pub use derivative::apply_derivative;
pub use elastic_distribution::distribute_elastic_error;
pub use error::apply_calculate_error;
pub use fused_error::apply_fused_error_distribution;
pub use loss::{
    categorical_error_sum_batch_packed, cross_entropy_sum_batch_packed, hinge_sum_batch_packed,
    mae_sum_batch_packed, mape_sum_batch_packed, mse_mean_record, mse_mean_streaming, mse_record,
    mse_sum_batch_packed, msle_sum_batch_packed,
};
pub use range::{
    apply_get_range, apply_limit_range, apply_limit_range_bounds, apply_validate_range,
};
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
    scan_available_connections, structural_error_message, topology_error_message,
    validate_structural_integrity, validate_topology, validate_topology_batch,
};
pub use training_state::{
    accumulate_bias_persistent_4way, accumulate_bias_persistent_8way,
    accumulate_weight_persistent_4way, accumulate_weight_persistent_8way, free_training_state,
    init_training_state, read_all_neuron_state, read_all_synapse_state, read_neuron_state,
    read_synapse_state, reset_training_state,
};
pub use unsquash::apply_unsquash;
