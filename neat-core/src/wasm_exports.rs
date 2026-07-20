//! Issue #36 — WASM-only `#[wasm_bindgen]` shims.
//!
//! Native modules expose their public API with idiomatic Rust signatures
//! (`SquashType` enums, tuple returns, `[f32; N]` array refs). `wasm-bindgen`
//! cannot bind those directly, so this module wraps them in thin shims that:
//!
//! - take `u8` activation codes and convert to `SquashType`,
//! - return `Vec<f32>` / `Vec<f64>` / `Vec<i32>` instead of tuples,
//! - decode the byte-packed `propagate_topological` ABI mirrored from
//!   NEAT-AI's `WasmTopologicalBackprop.ts`.
//!
//! All exports use `js_name` to match the canonical
//! `wasm_activation/pkg/wasm_activation.d.ts` surface that NEAT-AI consumes.
//! This module is gated entirely behind `cfg(target_arch = "wasm32")` so
//! native consumers (`rust_scorer`, CLI, native tests) never see it.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

use std::cell::RefCell;

use crate::network::CompiledNetwork;
use crate::training_data::TrainingDataConfig;
use crate::wasm_dataset::DatasetRegistry;

use crate::derivative::{apply_derivative, apply_derivative_simd_4way};
use crate::error::{apply_calculate_error, apply_calculate_error_batch_4way};
use crate::fused_error::apply_fused_error_distribution;
use crate::propagate_codec::{decode_propagate_buffer, encode_propagate_output};
use crate::range::{apply_get_range, apply_limit_range, apply_validate_range};
use crate::safe_zone::{apply_safe_zone_adjustment, apply_safe_zone_adjustment_batch};
use crate::score_scan::{compute_score_components, scan_max_bias, scan_max_weight};
use crate::squash::{SquashType, apply_squash};
use crate::topological_backprop::propagate_topological_loop;
use crate::unsquash::apply_unsquash;

// ---------------------------------------------------------------------------
// Activation-function scalar shims — apply_* in Rust, no `apply_` in JS.
// ---------------------------------------------------------------------------

/// JS `squash(squash_type, value)` → `apply_squash(SquashType, f32)`.
#[wasm_bindgen(js_name = squash)]
pub fn wasm_squash(squash_type: u8, value: f32) -> f32 {
    apply_squash(SquashType::from(squash_type), value)
}

/// JS `unsquash(squash_type, activation, hint)`.
#[wasm_bindgen(js_name = unsquash)]
pub fn wasm_unsquash(squash_type: u8, activation: f32, hint: f32) -> f32 {
    apply_unsquash(SquashType::from(squash_type), activation, hint)
}

/// JS `derivative(squash_type, value)`.
#[wasm_bindgen(js_name = derivative)]
pub fn wasm_derivative(squash_type: u8, value: f32) -> f32 {
    apply_derivative(SquashType::from(squash_type), value)
}

/// JS `derivative_batch_4way(squash_type, x0, x1, x2, x3) -> Float32Array`.
#[wasm_bindgen(js_name = derivative_batch_4way)]
pub fn wasm_derivative_batch_4way(squash_type: u8, x0: f32, x1: f32, x2: f32, x3: f32) -> Vec<f32> {
    let (d0, d1, d2, d3) =
        apply_derivative_simd_4way(SquashType::from(squash_type), x0, x1, x2, x3);
    vec![d0, d1, d2, d3]
}

/// JS `calculate_error(squash_type, current_activation, target_activation, current_value)`.
#[wasm_bindgen(js_name = calculate_error)]
pub fn wasm_calculate_error(
    squash_type: u8,
    current_activation: f32,
    target_activation: f32,
    current_value: f32,
) -> f32 {
    apply_calculate_error(
        SquashType::from(squash_type),
        current_activation,
        target_activation,
        current_value,
    )
}

/// JS `calculate_error_batch_4way(squash_type, current_activations, target_activations, current_values)`.
///
/// Inputs must each have length 4; the function reads the first 4 lanes.
#[wasm_bindgen(js_name = calculate_error_batch_4way)]
pub fn wasm_calculate_error_batch_4way(
    squash_type: u8,
    current_activations: &[f32],
    target_activations: &[f32],
    current_values: &[f32],
) -> Vec<f32> {
    fn first_four(s: &[f32]) -> [f32; 4] {
        [
            *s.first().unwrap_or(&0.0),
            *s.get(1).unwrap_or(&0.0),
            *s.get(2).unwrap_or(&0.0),
            *s.get(3).unwrap_or(&0.0),
        ]
    }
    let curr = first_four(current_activations);
    let tgt = first_four(target_activations);
    let vals = first_four(current_values);
    let (e0, e1, e2, e3) =
        apply_calculate_error_batch_4way(SquashType::from(squash_type), &curr, &tgt, &vals);
    vec![e0, e1, e2, e3]
}

/// JS `safe_zone_adjustment(squash_type, raw_input, error, weight)`.
#[wasm_bindgen(js_name = safe_zone_adjustment)]
pub fn wasm_safe_zone_adjustment(squash_type: u8, raw_input: f32, error: f32, weight: f32) -> f32 {
    apply_safe_zone_adjustment(SquashType::from(squash_type), raw_input, error, weight)
}

/// JS `safe_zone_adjustment_batch(squash_types, raw_inputs, error, weights)`.
#[wasm_bindgen(js_name = safe_zone_adjustment_batch)]
pub fn wasm_safe_zone_adjustment_batch(
    squash_types: &[u8],
    raw_inputs: &[f32],
    error: f32,
    weights: &[f32],
) -> Vec<f32> {
    apply_safe_zone_adjustment_batch(squash_types, raw_inputs, error, weights)
}

/// JS `fused_error_distribution(...)`.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = fused_error_distribution)]
pub fn wasm_fused_error_distribution(
    neuron_squash_type: u8,
    neuron_activation: f32,
    neuron_target_activation: f32,
    neuron_hint_value: f32,
    upstream_squash_types: &[u8],
    upstream_hint_values: &[f32],
    upstream_activations: &[f32],
    synapse_weights: &[f32],
) -> Vec<f32> {
    apply_fused_error_distribution(
        SquashType::from(neuron_squash_type),
        neuron_activation,
        neuron_target_activation,
        neuron_hint_value,
        upstream_squash_types,
        upstream_hint_values,
        upstream_activations,
        synapse_weights,
    )
}

// ---------------------------------------------------------------------------
// Range helpers — apply_* in Rust, no `apply_` in JS.
// ---------------------------------------------------------------------------

/// JS `get_range(squash_type) -> Float32Array of [low, high]`.
#[wasm_bindgen(js_name = get_range)]
pub fn wasm_get_range(squash_type: u8) -> Vec<f32> {
    let (low, high) = apply_get_range(SquashType::from(squash_type));
    vec![low, high]
}

/// JS `validate_range(squash_type, activation) -> boolean`.
#[wasm_bindgen(js_name = validate_range)]
pub fn wasm_validate_range(squash_type: u8, activation: f32) -> bool {
    apply_validate_range(SquashType::from(squash_type), activation)
}

/// JS `limit_range(squash_type, value) -> number`.
#[wasm_bindgen(js_name = limit_range)]
pub fn wasm_limit_range(squash_type: u8, value: f32) -> f32 {
    apply_limit_range(SquashType::from(squash_type), value)
}

// ---------------------------------------------------------------------------
// Score-scan tuple shims — return Float64Array.
// ---------------------------------------------------------------------------

/// JS `compute_score_components(weights, biases) -> Float64Array of length 4`.
#[wasm_bindgen(js_name = compute_score_components)]
pub fn wasm_compute_score_components(weights: &[f64], biases: &[f64]) -> Vec<f64> {
    let (total, count, max, second_max) = compute_score_components(weights, biases);
    vec![total, count as f64, max, second_max]
}

/// JS `scan_max_weight(weights, biases, exclude_idx, new_weight) -> Float64Array of [max, second_max]`.
#[wasm_bindgen(js_name = scan_max_weight)]
pub fn wasm_scan_max_weight(
    weights: &[f64],
    biases: &[f64],
    exclude_idx: usize,
    new_weight: f64,
) -> Vec<f64> {
    let (max, second_max) = scan_max_weight(weights, biases, exclude_idx, new_weight);
    vec![max, second_max]
}

/// JS `scan_max_bias(weights, biases, exclude_idx, new_bias) -> Float64Array of [max, second_max]`.
#[wasm_bindgen(js_name = scan_max_bias)]
pub fn wasm_scan_max_bias(
    weights: &[f64],
    biases: &[f64],
    exclude_idx: usize,
    new_bias: f64,
) -> Vec<f64> {
    let (max, second_max) = scan_max_bias(weights, biases, exclude_idx, new_bias);
    vec![max, second_max]
}

// ---------------------------------------------------------------------------
// Crate version shim.
// ---------------------------------------------------------------------------

/// JS `version() -> string` — returns the `neat-core` Cargo package version.
#[wasm_bindgen(js_name = version)]
pub fn wasm_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// propagate_topological — byte-packed ABI mirror of
// NEAT-AI's `WasmTopologicalBackprop.ts`. The decoder/encoder live in
// `propagate_codec` so they can be unit-tested natively (Issue #2463).
//
// Buffer layout (mirrors the canonical TS contract from Issue #1954):
//   Header (36 bytes):
//     u32: neuron_count
//     u32: input_count
//     u32: output_count
//     u32: synapse_count
//     u32: order_length
//     u32: total_inward_entries
//     f64: plank_constant
//     u8:  normalise_gradients
//     [3 bytes padding]
//   Per neuron (24 bytes each):
//     u8 squash_type, u8 neuron_type, u8 propagate_needed, u8 update_needed,
//     f32 hint_value, f32 range_low, f32 range_high, f32 adjusted_activation,
//     f32 adjusted_bias
//   Per synapse (20 bytes each):
//     u32 from, u32 to, f32 original_weight, f32 adjusted_weight,
//     u8 is_self_loop, [3 bytes padding]
//   Inward mapping (8 bytes per neuron): u32 start, u32 count
//   Inward indices (4 bytes each): u32 synapse_index
//   Reverse topo order (4 bytes each): u32 neuron_index
//   Expected outputs (4 bytes each): f32
//
// Output (Float64Array):
//   Section 1 (neuron_count × 7 f64): per-neuron deltas with sentinel encoding:
//     -Infinity in cached_activation → NoChange
//     +Infinity in cached_activation → Special (target_activation in trace slot)
//     finite → Standard or Skipped (Skipped has all NaN for cached/trace)
//   Section 2 (synapse_count × 7 f64): per-synapse accumulator deltas.
// ---------------------------------------------------------------------------

/// JS `propagate_topological(data: Uint8Array) -> Float64Array`.
///
/// Decodes the byte-packed buffer, runs the reverse-topological backprop
/// loop, and re-encodes the result with the TS↔WASM sentinel contract.
#[wasm_bindgen(js_name = propagate_topological)]
pub fn wasm_propagate_topological(data: &[u8]) -> Vec<f64> {
    let Ok(decoded) = decode_propagate_buffer(data) else {
        // Header too short or buffer truncated — return an empty
        // Float64Array. The TS wrapper treats `undefined`/empty as a
        // signal to fall back to the TS path.
        return Vec::new();
    };
    let output = propagate_topological_loop(&decoded.as_input());
    encode_propagate_output(&output)
}

// ---------------------------------------------------------------------------
// Training-data offload (Issue #298) — the dataset is owned in neat-core's
// linear memory and JS holds only a `u32` handle. Byte counts and record
// indices are `u64` (JS `BigInt`) so the surface is Memory64-ready for the
// >4 GB jobs the wasm64 milestone (#295) targets. The dataset bytes cross the
// boundary once, at `training_data_load`; evaluation passes only the handle,
// the (small) network buffer, and the batch bounds.
// ---------------------------------------------------------------------------

thread_local! {
    /// Process-wide handle table. wasm32/wasm64 modules are single-threaded, so
    /// a thread-local `RefCell` is the whole synchronisation story.
    static DATASET_REGISTRY: RefCell<DatasetRegistry> = RefCell::new(DatasetRegistry::new());
}

/// Convert a `u64` boundary value to `usize`, failing loud if it cannot fit the
/// current address space (only possible on a wasm32 module).
fn usize_from_u64(value: u64, what: &str) -> Result<usize, JsError> {
    usize::try_from(value)
        .map_err(|_| JsError::new(&format!("{what} {value} exceeds addressable range")))
}

/// JS `training_data_load(data: Uint8Array, num_inputs, num_outputs) -> handle`.
///
/// Loads the packed `.bin` byte stream into linear memory once and returns the
/// handle JS retains in place of the bytes.
#[wasm_bindgen(js_name = training_data_load)]
pub fn wasm_training_data_load(
    data: &[u8],
    num_inputs: u32,
    num_outputs: u32,
) -> Result<u32, JsError> {
    let config = TrainingDataConfig::new(num_inputs as usize, num_outputs as usize);
    DATASET_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .load_packed(data, config)
            .map_err(|err| JsError::new(&err.to_string()))
    })
}

/// JS `training_data_free(handle)` — release the dataset's linear memory.
#[wasm_bindgen(js_name = training_data_free)]
pub fn wasm_training_data_free(handle: u32) -> Result<(), JsError> {
    DATASET_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .free(handle)
            .map(|_freed| ())
            .map_err(|err| JsError::new(&err.to_string()))
    })
}

/// JS `training_data_num_records(handle) -> BigInt`.
#[wasm_bindgen(js_name = training_data_num_records)]
pub fn wasm_training_data_num_records(handle: u32) -> Result<u64, JsError> {
    DATASET_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(handle)
            .map(|ds| ds.num_records() as u64)
            .map_err(|err| JsError::new(&err.to_string()))
    })
}

/// JS `training_data_byte_len(handle) -> BigInt` — footprint in linear memory.
#[wasm_bindgen(js_name = training_data_byte_len)]
pub fn wasm_training_data_byte_len(handle: u32) -> Result<u64, JsError> {
    DATASET_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(handle)
            .map(|ds| ds.byte_len() as u64)
            .map_err(|err| JsError::new(&err.to_string()))
    })
}

/// JS `training_data_evaluate_mse(handle, network: Uint8Array, start, count) -> number`.
///
/// Evaluates `network` over the `[start, start + count)` record batch and
/// returns the mean squared error. The dataset is read by index from linear
/// memory — only the small network buffer and the batch bounds cross the
/// boundary, never the training arrays.
#[wasm_bindgen(js_name = training_data_evaluate_mse)]
pub fn wasm_training_data_evaluate_mse(
    handle: u32,
    network: &[u8],
    start: u64,
    count: u64,
) -> Result<f32, JsError> {
    let start = usize_from_u64(start, "batch start")?;
    let count = usize_from_u64(count, "batch count")?;
    let mut net =
        CompiledNetwork::new(network).map_err(|err| JsError::new(&format!("network: {err}")))?;
    DATASET_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let dataset = registry
            .get(handle)
            .map_err(|err| JsError::new(&err.to_string()))?;
        dataset
            .evaluate_mse(&mut net, start, count)
            .map_err(|err| JsError::new(&err.to_string()))
    })
}

/// JS `training_data_live_bytes() -> BigInt` — bytes held by live datasets.
#[wasm_bindgen(js_name = training_data_live_bytes)]
pub fn wasm_training_data_live_bytes() -> u64 {
    DATASET_REGISTRY.with(|registry| registry.borrow().live_bytes() as u64)
}

/// JS `training_data_peak_bytes() -> BigInt` — linear-memory high-water mark.
#[wasm_bindgen(js_name = training_data_peak_bytes)]
pub fn wasm_training_data_peak_bytes() -> u64 {
    DATASET_REGISTRY.with(|registry| registry.borrow().peak_bytes() as u64)
}
