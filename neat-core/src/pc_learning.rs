//! Predictive Coding local Hebbian learning rules.
//!
//! Issue #1560: Implements the "slow" learning phase — weight updates
//! based on locally available pre-synaptic activity and post-synaptic error.
//! Each synapse update depends only on the error at its target neuron,
//! the activation of its source neuron, and the local derivative.
//!
//! Weight update rule (after inference has settled):
//!   ΔW(j→i) = η_learn · f'(a(i)) · ε(i) · x(j)
//!
//! Bias update rule:
//!   Δb(i) = η_learn · f'(a(i)) · ε(i)
//!
//! Matches the TypeScript reference implementation in
//! src/predictiveCoding/PredictiveCodingLearning.ts.

use crate::derivative::apply_derivative;
use crate::pc_inference::PredictiveCodingEngine;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Gradient result containing weight and bias deltas.
#[derive(Clone, Debug)]
pub struct PcGradientResult {
    /// Per-synapse weight deltas (indexed by a flat connection index).
    /// Format: [(neuron_rel_idx, conn_local_idx, delta), ...]
    pub weight_deltas: Vec<(usize, usize, f32)>,
    /// Per-neuron bias deltas (indexed by neuron_rel_idx).
    pub bias_deltas: Vec<f32>,
}

impl PredictiveCodingEngine {
    /// Computes local Hebbian weight and bias gradients from settled inference state.
    ///
    /// For each connection j→i:
    ///   ΔW = η_learn · f'(a(i)) · ε(i) · x(j)
    ///
    /// For each non-input neuron i:
    ///   Δb(i) = η_learn · f'(a(i)) · ε(i)
    ///
    /// # Arguments
    /// * `latents` - Final settled latent values from inference.
    /// * `errors` - Per-neuron prediction errors from inference (indexed by neuron_rel_idx).
    /// * `learning_rate` - The learning rate η_learn.
    pub fn compute_gradients(
        &self,
        latents: &[f32],
        errors: &[f32],
        learning_rate: f32,
    ) -> PcGradientResult {
        let num_non_inputs = self.neurons.len();
        let mut bias_deltas = vec![0.0f32; num_non_inputs];
        let mut weight_deltas = Vec::new();

        for (ni, neuron) in self.neurons.iter().enumerate() {
            // Compute pre-activation for this neuron.
            let pre_activation = self.compute_pre_activation(ni, latents);
            let derivative = apply_derivative(neuron.squash_type, pre_activation);

            let error = errors[ni];

            // Bias gradient: Δb = η · f'(a) · ε
            bias_deltas[ni] = learning_rate * derivative * error;

            // Weight gradients: ΔW = η · f'(a) · ε · x(source)
            for ci in 0..neuron.conn_count {
                let conn = &self.connections[neuron.conn_start + ci];
                let source_latent = latents[conn.from];
                let delta = learning_rate * derivative * error * source_latent;
                weight_deltas.push((ni, ci, delta));
            }
        }

        PcGradientResult {
            weight_deltas,
            bias_deltas,
        }
    }
}

// ---------------------------------------------------------------------------
// Packed gradient computation
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PredictiveCodingEngine {
    /// Computes weight and bias gradients from settled inference state.
    ///
    /// # Arguments
    /// * `latents` - Float32Array of settled latent values (length = num_neurons).
    /// * `errors` - Float32Array of prediction errors for non-input neurons.
    /// * `learning_rate` - The learning rate for weight updates.
    ///
    /// # Returns
    /// Packed Float32Array:
    /// - \[0\]: num_non_inputs (number of bias deltas)
    /// - \[1\]: num_weight_entries (number of weight delta triples)
    /// - \[2..2+num_non_inputs\): bias deltas
    /// - \[2+num_non_inputs..\]: weight delta triples (neuron_rel_idx, conn_local_idx, delta)
    pub fn compute_gradients_wasm(
        &self,
        latents: &[f32],
        errors: &[f32],
        learning_rate: f32,
    ) -> Vec<f32> {
        let result = self.compute_gradients(latents, errors, learning_rate);

        let num_non_inputs = result.bias_deltas.len();
        let num_weight_entries = result.weight_deltas.len();
        let total = 2 + num_non_inputs + num_weight_entries * 3;
        let mut packed = Vec::with_capacity(total);

        packed.push(num_non_inputs as f32);
        packed.push(num_weight_entries as f32);

        // Bias deltas
        packed.extend_from_slice(&result.bias_deltas);

        // Weight deltas as triples
        for (ni, ci, delta) in &result.weight_deltas {
            packed.push(*ni as f32);
            packed.push(*ci as f32);
            packed.push(*delta);
        }

        packed
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
