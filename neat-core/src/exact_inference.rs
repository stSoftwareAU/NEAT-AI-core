//! **Experimental prototype** — validated exact-size inference (Issue #511).
//!
//! An A/B experiment, not a shipped path. It asks one question: if the public
//! boundary establishes `input.len() == num_inputs` **once**, does the hot path
//! get measurably faster because the optimiser can then see the copy length is
//! exactly the input count?
//!
//! The control ([`CompiledNetwork::activate_into`]) is deliberately untouched
//! and remains the only production entry point. Everything here lives behind
//! the non-default `experimental-exact-inference` feature and is removed if the
//! acceptance gate is not met — see
//! [`docs/research/exact-size-inference-entry-point.md`](https://github.com/stSoftwareAU/NEAT-AI-core/blob/Develop/docs/research/exact-size-inference-entry-point.md).
//!
//! Two differences from the control, and only two:
//!
//! 1. the copy length is `input.len()` (proven equal to `num_inputs` at the
//!    boundary) instead of `input.len().min(self.num_inputs)`, so the `min` and
//!    its branch are gone from the hot path;
//! 2. a wrong length is a typed [`NetworkError`] instead of a silent truncation
//!    (input) or an arithmetic-overflow panic (over-long output).
//!
//! No `unsafe`, no unchecked indexing: "validated" here means *the caller's
//! shapes were checked once*, not *bounds checks were removed*. The synapse
//! kernels are the same shared kernels the control calls.

use crate::batch_scoring::inline_squash;
use crate::network::{CompiledNetwork, NetworkError};
use crate::range::apply_limit_range;
use crate::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd,
};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

impl CompiledNetwork {
    /// Activate with exact-size validation, writing outputs into `output`.
    ///
    /// The safe public boundary of the Issue #511 prototype. It validates both
    /// dimensions once and then runs [`Self::activate_into_validated`], which
    /// assumes them.
    ///
    /// # Errors
    ///
    /// - [`NetworkError::InputLengthMismatch`] if `input.len() != num_inputs`.
    ///   The control truncates a long input and leaves the uncovered input
    ///   activations at whatever the previous call wrote; this rejects both.
    /// - [`NetworkError::OutputLengthMismatch`] if `output` is empty or longer
    ///   than `num_neurons - num_inputs`. The control panics on subtraction
    ///   overflow for the over-long case.
    ///
    /// Neither error mutates the network: both checks run before any write.
    pub fn activate_into_exact(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(), NetworkError> {
        if input.len() != self.num_inputs {
            return Err(NetworkError::InputLengthMismatch {
                expected: self.num_inputs,
                actual: input.len(),
            });
        }

        let max_outputs = self.num_neurons - self.num_inputs;
        if output.is_empty() || output.len() > max_outputs {
            return Err(NetworkError::OutputLengthMismatch {
                max: max_outputs,
                actual: output.len(),
            });
        }

        self.activate_into_validated(input, output);
        Ok(())
    }

    /// The validated hot path: identical to [`CompiledNetwork::activate_into`]
    /// except that the input copy takes `input.len()` directly.
    ///
    /// Callers must have established `input.len() == self.num_inputs` and
    /// `1 <= output.len() <= self.num_neurons - self.num_inputs`. Violating
    /// that is not unsound — every index here is still bounds-checked — it just
    /// panics or fills the wrong slots, which is exactly what the boundary
    /// exists to prevent.
    #[inline]
    fn activate_into_validated(&mut self, input: &[f32], output: &mut [f32]) {
        let num_outputs = output.len();

        // input.len() == self.num_inputs, so no `min` and no partial fill: the
        // source and destination lengths are the same expression.
        self.activations[..input.len()].copy_from_slice(input);

        for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
            let actual_idx = self.num_inputs + neuron_idx;

            if neuron.is_constant {
                self.activations[actual_idx] = apply_limit_range(SquashType::Identity, neuron.bias);
            } else {
                let squash = SquashType::from(neuron.squash_type);
                let start_synapse = neuron.start_synapse as usize;
                let end_synapse = start_synapse + neuron.num_synapses as usize;

                let activation = match squash {
                    SquashType::Minimum => {
                        let mut min_val = f32::INFINITY;
                        for synapse_idx in start_synapse..end_synapse {
                            let synapse = &self.synapses[synapse_idx];
                            let val =
                                self.activations[synapse.from_index as usize] * synapse.weight;
                            if val < min_val {
                                min_val = val;
                            }
                        }
                        if min_val == f32::INFINITY {
                            neuron.bias
                        } else {
                            min_val + neuron.bias
                        }
                    }
                    SquashType::Maximum => {
                        let mut max_val = f32::NEG_INFINITY;
                        for synapse_idx in start_synapse..end_synapse {
                            let synapse = &self.synapses[synapse_idx];
                            let val =
                                self.activations[synapse.from_index as usize] * synapse.weight;
                            if val > max_val {
                                max_val = val;
                            }
                        }
                        if max_val == f32::NEG_INFINITY {
                            neuron.bias
                        } else {
                            max_val + neuron.bias
                        }
                    }
                    SquashType::If => {
                        let mut condition_sum = 0.0f32;
                        let mut positive_sum = 0.0f32;
                        let mut negative_sum = 0.0f32;

                        for synapse_idx in start_synapse..end_synapse {
                            let synapse = &self.synapses[synapse_idx];
                            let val =
                                self.activations[synapse.from_index as usize] * synapse.weight;

                            match SynapseType::from(synapse.synapse_type) {
                                SynapseType::Condition => condition_sum += val,
                                SynapseType::Negative => negative_sum += val,
                                SynapseType::Positive | SynapseType::Standard => {
                                    positive_sum += val
                                }
                            }
                        }

                        if condition_sum > 0.0 {
                            positive_sum + neuron.bias
                        } else {
                            negative_sum + neuron.bias
                        }
                    }
                    SquashType::Hypotenuse => {
                        let sum_sq = weighted_sum_of_squares_simd(
                            &self.synapses,
                            &self.activations,
                            start_synapse,
                            end_synapse,
                        );
                        sum_sq.sqrt() + neuron.bias
                    }
                    SquashType::HypotenuseV2 => {
                        let sum_sq = weighted_sum_of_squares_v2_simd(
                            &self.synapses,
                            &self.activations,
                            start_synapse,
                            end_synapse,
                            neuron.bias,
                        );
                        sum_sq.sqrt()
                    }
                    SquashType::Mean => {
                        let n = (end_synapse - start_synapse) as f32;
                        if n <= 0.0 {
                            neuron.bias
                        } else {
                            let sum = weighted_sum_no_bias_simd(
                                &self.synapses,
                                &self.activations,
                                start_synapse,
                                end_synapse,
                            );
                            sum / n + neuron.bias
                        }
                    }
                    _ => {
                        let sum = weighted_sum_simd(
                            &self.synapses,
                            &self.activations,
                            start_synapse,
                            end_synapse,
                            neuron.bias,
                        );
                        inline_squash(neuron.squash_type, squash, sum)
                    }
                };

                self.activations[actual_idx] = apply_limit_range(squash, activation);
            }
        }

        let output_start = self.num_neurons - num_outputs;
        output.copy_from_slice(&self.activations[output_start..output_start + num_outputs]);
    }
}
