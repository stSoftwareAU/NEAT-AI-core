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
//! This module is gated entirely behind `cfg(target_family = "wasm")` so
//! native consumers (`rust_scorer`, CLI, native tests) never see it.

#![cfg(target_family = "wasm")]

use wasm_bindgen::prelude::*;

use crate::creature_validate_json::creature_validate_json;
use crate::creature_validate_packed::creature_validate_packed;
use crate::derivative::apply_derivative;
use crate::error::apply_calculate_error;
use crate::fused_error::apply_fused_error_distribution;
use crate::propagate_codec::{decode_propagate_buffer, encode_propagate_output};
use crate::prune_json::{prune_neuron_json, prune_synapse_json};
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
// creature_validate — JSON in, JSON out (Issue #562).
//
// The whole ABI, and why it is JSON rather than a packed buffer, is documented
// on `crate::creature_validate_json`; this shim only renames it for JS. Both
// halves of the contract live in that native module so the boundary is covered
// by `cargo test` rather than only in a browser:
//
//   In:  { "creature": <CreatureExport>, "options"?: { neurons?, connections?,
//         feedbackLoop?, forwardOnly? } }
//        { "runtimeCreature": <RuntimeCreature>, "options"?: ... } — the
//        in-memory shape a host holds (NEAT-AI#3803); a request names exactly
//        one of the two creature keys
//   Out: { "ok": true,  "stats": { input, constant, hidden, output, connections } }
//        { "ok": false, "failure": { class, reason, message, neuronIndex,
//                                    synapseIndex, malformed } }
//
// Malformed input answers with `malformed: true` and a `MALFORMED_REQUEST:`
// message — the JSON twin of `topology_ops`' `MALFORMED_BUFFER` — because a
// panic here aborts the module and `catch_unwind` is unavailable on wasm.
// ---------------------------------------------------------------------------

/// JS `creature_validate(request: string) -> string`.
#[wasm_bindgen(js_name = creature_validate)]
pub fn wasm_creature_validate(request: &str) -> String {
    creature_validate_json(request)
}

// ---------------------------------------------------------------------------
// creature_validate_packed — packed buffer in, JSON out (NEAT-AI#3832).
//
// The same rules over the typed arrays a host already holds, because the JSON
// wire form costs more than the rules on a large creature: 850 KB and 3.5 ms
// of `JSON.stringify` before a rule has looked at anything, on a creature
// validated after every mutation, breed and discovery step. The layout, and
// why the buffer carries no strings, are documented on
// `crate::creature_validate_packed`; this shim only renames it for JS.
//
//   In:  the packed request buffer, and the memetic record as JSON ("" for a
//        creature carrying none)
//   Out: { "ok": true,  "stats": { input, constant, hidden, output, connections } }
//        { "ok": false, "detailRequired": true } — a rule was broken; ask
//        `creature_validate` for the class, reason and message
//        { "ok": false, "failure": { …, "malformed": true } } — the buffer was
//        never a request, which is never a verdict on the creature
// ---------------------------------------------------------------------------

/// JS `creature_validate_packed(request: Uint8Array, memetic: string) -> string`.
#[wasm_bindgen(js_name = creature_validate_packed)]
pub fn wasm_creature_validate_packed(request: &[u8], memetic: &str) -> String {
    creature_validate_packed(request, memetic)
}

// ---------------------------------------------------------------------------
// prune_neuron / prune_synapse — JSON in, JSON out (Issue #592).
//
// One Rust implementation, two entry surfaces. The rewrites themselves are
// `crate::prune_neuron` and `crate::prune_synapse`; `crate::prune_json` is the
// only wire form, and these shims rename it for JS — so nothing about pruning
// is decided on this side of the boundary and there is no second
// implementation to drift.
//
//   In:  { "creature": <CreatureExport>, "uuid": "h-1", "stats"?: { … } }
//        { "creature": <CreatureExport>,
//          "synapse": { "fromUUID": …, "toUUID": …, "type"?: … },
//          "stats"?: { … } }
//   Out: { "ok": true,  "creature": <CreatureExport>, "transform": "exact",
//          "passes": 2, "biasFolds": [ … ], "uncompensated": [ … ] }
//        { "ok": false, "failure": { "reason", "message", "malformed" } }
//
// `malformed: true` marks a payload that never reached the rewrite, led by
// `MALFORMED_REQUEST:` — the same convention `creature_validate` uses, and for
// the same reason: a panic here aborts the module and `catch_unwind` is
// unavailable on wasm.
//
// No scorer and no acceptance policy live here or in the native crate: a
// successful call answers with a valid creature and an honest `transform`
// label, and what to do with it is the caller's decision (Issue #587).
// ---------------------------------------------------------------------------

/// JS `prune_neuron(request: string) -> string`.
#[wasm_bindgen(js_name = prune_neuron)]
pub fn wasm_prune_neuron(request: &str) -> String {
    prune_neuron_json(request)
}

/// JS `prune_synapse(request: string) -> string`.
#[wasm_bindgen(js_name = prune_synapse)]
pub fn wasm_prune_synapse(request: &str) -> String {
    prune_synapse_json(request)
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
