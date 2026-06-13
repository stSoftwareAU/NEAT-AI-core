//! Issue #1377 - Fused backward pass error distribution in WASM.
//!
//! Combines three steps of the backward pass into a single WASM call:
//! 1. `calculateError()` - compute neuron error in value-space
//! 2. `safeZoneAdjustment()` per synapse - compute gradient flow safety
//! 3. Elastic error distribution - allocate error proportional to activation²×safeZone
//!
//! This eliminates S+1 WASM boundary crossings per neuron and keeps all
//! intermediate float values in WASM linear memory.

use crate::error::apply_calculate_error;
use crate::safe_zone::apply_safe_zone_adjustment;
use crate::squash::SquashType;

/// Planck constant for floating-point comparisons (matches TypeScript default).
const PLANK_CONSTANT: f32 = 1e-12;

/// Fused error distribution result.
/// Layout: [error, safeZone_0..safeZone_N, perLinkError_0..perLinkError_N]
/// Total length: 1 + 2*N
///
/// This flat layout avoids allocating multiple Vec/Float32Array objects across
/// the WASM boundary.
pub fn apply_fused_error_distribution(
    neuron_squash_type: SquashType,
    neuron_activation: f32,
    neuron_target_activation: f32,
    neuron_hint_value: f32,
    upstream_squash_types: &[u8],
    upstream_hint_values: &[f32],
    upstream_activations: &[f32],
    synapse_weights: &[f32],
) -> Vec<f32> {
    let count = upstream_squash_types.len();

    // Step 1: Calculate error in value-space
    let error = apply_calculate_error(
        neuron_squash_type,
        neuron_activation,
        neuron_target_activation,
        neuron_hint_value,
    );

    // Result buffer: [error, safeZoneFactors..., perLinkErrors...]
    let mut result = Vec::with_capacity(1 + 2 * count);
    result.push(error);

    if count == 0 {
        return result;
    }

    // Early exit: if error is zero, all shares are zero
    if error == 0.0 {
        // safeZoneFactors (all 1.0 as default)
        for _ in 0..count {
            result.push(1.0);
        }
        // perLinkErrors (all 0.0)
        for _ in 0..count {
            result.push(0.0);
        }
        return result;
    }

    let provisional_error_per_link = error / (count as f32);

    // Step 2: Safe zone adjustment per synapse.
    // Push each factor straight into `result` (no separate `safe_zone_factors`
    // Vec or copy loop — Issue #156). The factors live at `result[1 .. 1 + count]`
    // and are read back in Step 3.
    for i in 0..count {
        let squash = SquashType::from(upstream_squash_types[i]);
        let raw_input = upstream_hint_values[i];
        let weight = if synapse_weights[i].is_finite() {
            synapse_weights[i]
        } else {
            1.0
        };
        let factor =
            apply_safe_zone_adjustment(squash, raw_input, provisional_error_per_link, weight);
        result.push(factor);
    }

    // Step 3: Elastic error distribution
    // score[i] = activation[i]² × clamp(safeZoneFactor[i], 0, 1)
    let mut scores = Vec::with_capacity(count);
    let mut denom: f32 = 0.0;
    for i in 0..count {
        let activation = upstream_activations[i];
        // Safe-zone factor written above at result[1 + i].
        let safe = result[1 + i];

        if !activation.is_finite() || !safe.is_finite() {
            scores.push(0.0);
            continue;
        }

        let clamped_safe = safe.clamp(0.0, 1.0);
        let a2 = activation * activation;
        let score = a2 * clamped_safe;
        scores.push(score);
        denom += score;
    }

    if denom <= PLANK_CONSTANT {
        // Issue #1519 - Weight-based fallback when activations are near zero.
        // Links with larger weights carry more influence and should absorb
        // proportionally more error (weight²).
        let mut weight_denom: f32 = 0.0;
        let mut weight_scores = Vec::with_capacity(count);
        for i in 0..count {
            let w = synapse_weights[i];
            let w2 = if w.is_finite() { w * w } else { 0.0 };
            weight_scores.push(w2);
            weight_denom += w2;
        }

        if weight_denom > PLANK_CONSTANT {
            let mut sum: f32 = 0.0;
            let mut best_idx: usize = 0;
            for i in 0..count {
                let share = error * (weight_scores[i] / weight_denom);
                result.push(share);
                sum += share;
                if weight_scores[i] > weight_scores[best_idx] {
                    best_idx = i;
                }
            }
            let residue = error - sum;
            if residue.abs() > PLANK_CONSTANT {
                result[1 + count + best_idx] += residue;
            }
        } else {
            // Last resort: equal split when both activations and weights are zero.
            let per = error / (count as f32);
            for _ in 0..count {
                result.push(per);
            }
        }
    } else {
        // Distribute proportionally to scores
        let mut sum: f32 = 0.0;
        let mut best_idx: usize = 0;
        let mut best_score: f32 = -f32::INFINITY;

        for i in 0..count {
            let share = error * (scores[i] / denom);
            result.push(share);
            sum += share;
            if scores[i] > best_score {
                best_score = scores[i];
                best_idx = i;
            }
        }

        // Floating-point cleanup: add residue to highest-score link
        let residue = error - sum;
        if residue.abs() > PLANK_CONSTANT {
            // Offset into result: 1 (error) + count (safeZones) + best_idx
            result[1 + count + best_idx] += residue;
        }
    }

    result
}
