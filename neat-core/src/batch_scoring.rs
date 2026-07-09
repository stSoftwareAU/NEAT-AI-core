//! Batched record scoring through the 8-record SIMD path (Issue #230).
//!
//! The production topology is *wide, shallow, sparse, varied fan-in* —
//! gather-bound (see `benches/README.md`). The single-record forward pass
//! ([`CompiledNetwork::activate_into`]) re-reads each synapse's weight and
//! metadata once **per record**. This module drives the forward pass through
//! the existing across-records SIMD primitives
//! ([`weighted_sum_simd_8records`] / [`weighted_sum_simd_4records`]), loading
//! each synapse weight once and applying it across 8 (then 4) records. That
//! amortises the weight load and synapse-index read across the batch, cutting
//! gather traffic on the dominant standard-squash neurons.
//!
//! # Numerics
//!
//! Standard-squash neurons accumulate their weighted sum across records rather
//! than across synapses, so the summation is re-associated relative to the
//! single-record [`weighted_sum_simd`] kernel. In `f32` that yields results
//! that match the per-record reference **within a small tolerance**, not
//! bit-for-bit (Issue #230 acceptance criteria explicitly allow SIMD
//! reordering / `f32` accumulation differences). Every other numeric step is
//! identical to [`CompiledNetwork::activate_into`]:
//!
//! - The **squash** for the covered types is evaluated across all 8 (then 4)
//!   batch lanes at once through the vectorised `squash_x8` / `squash_x4`
//!   approximations (Issue #243, mirroring `mse_sum_batch_packed`). Those
//!   approximations stay within `SQUASH_SIMD_MAX_ABS_ERR` of scalar
//!   `apply_squash`, so results match the per-record reference within the same
//!   SIMD tolerance the batched weighted sums already introduce (Issue #230).
//!   Every other type keeps the scalar inline branch (Identity / ReLU /
//!   Logistic / Tanh, else `apply_squash`), so its squash stays bit-identical.
//! - **Aggregate** squashes (Minimum, Maximum, If, Hypotenuse, HypotenuseV2,
//!   Mean) and the **scalar tail** (`records.len() % 8` after the 4-way step)
//!   run the exact single-record path via `neuron_activation_scalar`, so those
//!   neurons and those records are bit-identical to the reference.
//!
//! # Determinism
//!
//! Weights are read through `&self`; all mutable state lives in a caller-owned
//! [`BatchScratch`]. Record `i`'s outputs are always written to
//! `out[i * num_outputs .. (i + 1) * num_outputs]`, so output order matches
//! input order regardless of batching or thread count.

use crate::network::{CompiledNetwork, NeuronData, SynapseData};
use crate::range::apply_limit_range;
use crate::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd, weighted_sum_simd_4records, weighted_sum_simd_8records,
};
use crate::squash::{SquashType, apply_squash};
use crate::squash_simd::{squash_x4, squash_x8};
use crate::synapse_type::SynapseType;

/// Reusable per-worker scratch: eight activation buffers, one per SIMD lane.
///
/// Owning the buffers here lets the sequential path allocate once for a whole
/// batch and the parallel path allocate once per rayon worker (via
/// `for_each_init`), instead of once per chunk.
pub struct BatchScratch {
    acts: [Vec<f32>; 8],
}

impl BatchScratch {
    /// Allocate eight zeroed activation buffers sized to the network.
    pub fn new(num_neurons: usize) -> Self {
        Self {
            acts: std::array::from_fn(|_| vec![0.0f32; num_neurons]),
        }
    }
}

/// Inline squash matching [`CompiledNetwork::activate_into`] exactly: the four
/// hot types are branched directly, everything else defers to [`apply_squash`].
#[inline]
fn inline_squash(squash_type: u8, squash: SquashType, sum: f32) -> f32 {
    match squash_type {
        0 => sum,                        // IDENTITY
        1 => sum.max(0.0),               // ReLU
        6 => 1.0 / (1.0 + (-sum).exp()), // LOGISTIC
        7 => sum.tanh(),                 // TANH
        _ => apply_squash(squash, sum),  // Other (fallback)
    }
}

/// Single-record activation for one neuron, byte-for-byte identical to the body
/// of [`CompiledNetwork::activate_into`]. Reused for constant neurons, aggregate
/// squashes and the scalar tail so those results exactly match the reference.
#[inline]
fn neuron_activation_scalar(synapses: &[SynapseData], act: &[f32], neuron: &NeuronData) -> f32 {
    if neuron.is_constant {
        return apply_limit_range(SquashType::Identity, neuron.bias);
    }

    let squash = SquashType::from(neuron.squash_type);
    let start = neuron.start_synapse as usize;
    let end = start + neuron.num_synapses as usize;

    let activation = match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
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
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
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
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
                match SynapseType::from(s.synapse_type) {
                    SynapseType::Condition => condition_sum += val,
                    SynapseType::Negative => negative_sum += val,
                    SynapseType::Positive | SynapseType::Standard => positive_sum += val,
                }
            }
            if condition_sum > 0.0 {
                positive_sum + neuron.bias
            } else {
                negative_sum + neuron.bias
            }
        }
        SquashType::Hypotenuse => {
            let sum_sq = weighted_sum_of_squares_simd(synapses, act, start, end);
            sum_sq.sqrt() + neuron.bias
        }
        SquashType::HypotenuseV2 => {
            let sum_sq = weighted_sum_of_squares_v2_simd(synapses, act, start, end, neuron.bias);
            sum_sq.sqrt()
        }
        SquashType::Mean => {
            let n = (end - start) as f32;
            if n <= 0.0 {
                neuron.bias
            } else {
                let sum = weighted_sum_no_bias_simd(synapses, act, start, end);
                sum / n + neuron.bias
            }
        }
        _ => {
            let sum = weighted_sum_simd(synapses, act, start, end, neuron.bias);
            inline_squash(neuron.squash_type, squash, sum)
        }
    };

    apply_limit_range(squash, activation)
}

/// Copy record inputs into a lane buffer, matching [`CompiledNetwork::activate_into`]
/// (which copies `min(len, num_inputs)` input values). Any input slot the record
/// does not cover is zeroed so each record is scored statelessly — buffers are
/// reused across batches, and every non-input neuron is overwritten during the
/// forward pass, so no further reset is required.
#[inline]
fn load_record(act: &mut [f32], record: &[f32], num_inputs: usize) {
    let in_len = record.len().min(num_inputs);
    act[..in_len].copy_from_slice(&record[..in_len]);
    act[in_len..num_inputs].fill(0.0);
}

impl CompiledNetwork {
    /// Score a contiguous slice of records through the batched SIMD path.
    ///
    /// Record `i` (0-based within `records`) writes its outputs to
    /// `out[i * num_outputs .. (i + 1) * num_outputs]`; `out` must be exactly
    /// `records.len() * num_outputs` long. Records are grouped into 8s and
    /// driven through [`weighted_sum_simd_8records`], then a 4-record group via
    /// [`weighted_sum_simd_4records`], then a scalar tail — matching the
    /// remainder handling of `mse_sum_batch_packed`.
    pub(crate) fn score_batch_into(
        &self,
        scratch: &mut BatchScratch,
        records: &[Vec<f32>],
        num_outputs: usize,
        out: &mut [f32],
    ) {
        let num_neurons = self.num_neurons;
        let num_inputs = self.num_inputs;
        let output_start = num_neurons - num_outputs;
        let n = records.len();

        let [act0, act1, act2, act3, act4, act5, act6, act7] = &mut scratch.acts;

        let mut base = 0usize;

        // ---- 8-record batches ------------------------------------------------
        while base + 8 <= n {
            load_record(act0, &records[base], num_inputs);
            load_record(act1, &records[base + 1], num_inputs);
            load_record(act2, &records[base + 2], num_inputs);
            load_record(act3, &records[base + 3], num_inputs);
            load_record(act4, &records[base + 4], num_inputs);
            load_record(act5, &records[base + 5], num_inputs);
            load_record(act6, &records[base + 6], num_inputs);
            load_record(act7, &records[base + 7], num_inputs);

            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;

                if neuron.is_constant {
                    let v = apply_limit_range(SquashType::Identity, neuron.bias);
                    act0[actual_idx] = v;
                    act1[actual_idx] = v;
                    act2[actual_idx] = v;
                    act3[actual_idx] = v;
                    act4[actual_idx] = v;
                    act5[actual_idx] = v;
                    act6[actual_idx] = v;
                    act7[actual_idx] = v;
                    continue;
                }

                let squash = SquashType::from(neuron.squash_type);
                match squash {
                    SquashType::Minimum
                    | SquashType::Maximum
                    | SquashType::If
                    | SquashType::Hypotenuse
                    | SquashType::HypotenuseV2
                    | SquashType::Mean => {
                        // Aggregate squashes stay on the exact single-record path.
                        act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
                        act1[actual_idx] = neuron_activation_scalar(&self.synapses, act1, neuron);
                        act2[actual_idx] = neuron_activation_scalar(&self.synapses, act2, neuron);
                        act3[actual_idx] = neuron_activation_scalar(&self.synapses, act3, neuron);
                        act4[actual_idx] = neuron_activation_scalar(&self.synapses, act4, neuron);
                        act5[actual_idx] = neuron_activation_scalar(&self.synapses, act5, neuron);
                        act6[actual_idx] = neuron_activation_scalar(&self.synapses, act6, neuron);
                        act7[actual_idx] = neuron_activation_scalar(&self.synapses, act7, neuron);
                    }
                    _ => {
                        let start = neuron.start_synapse as usize;
                        let end = start + neuron.num_synapses as usize;
                        let (s0, s1, s2, s3, s4, s5, s6, s7) = weighted_sum_simd_8records(
                            &self.synapses,
                            act0,
                            act1,
                            act2,
                            act3,
                            act4,
                            act5,
                            act6,
                            act7,
                            start,
                            end,
                            neuron.bias,
                        );
                        // Vectorised squash across all 8 lanes for the covered
                        // types (Issue #243); scalar inline fallback otherwise.
                        let sums = [s0, s1, s2, s3, s4, s5, s6, s7];
                        let squashed = squash_x8(squash, sums).unwrap_or_else(|| {
                            let st = neuron.squash_type;
                            sums.map(|s| inline_squash(st, squash, s))
                        });
                        act0[actual_idx] = apply_limit_range(squash, squashed[0]);
                        act1[actual_idx] = apply_limit_range(squash, squashed[1]);
                        act2[actual_idx] = apply_limit_range(squash, squashed[2]);
                        act3[actual_idx] = apply_limit_range(squash, squashed[3]);
                        act4[actual_idx] = apply_limit_range(squash, squashed[4]);
                        act5[actual_idx] = apply_limit_range(squash, squashed[5]);
                        act6[actual_idx] = apply_limit_range(squash, squashed[6]);
                        act7[actual_idx] = apply_limit_range(squash, squashed[7]);
                    }
                }
            }

            for r in 0..8 {
                let src = match r {
                    0 => &*act0,
                    1 => &*act1,
                    2 => &*act2,
                    3 => &*act3,
                    4 => &*act4,
                    5 => &*act5,
                    6 => &*act6,
                    _ => &*act7,
                };
                let dst = (base + r) * num_outputs;
                out[dst..dst + num_outputs]
                    .copy_from_slice(&src[output_start..output_start + num_outputs]);
            }

            base += 8;
        }

        // ---- 4-record batch (0 or 1 of them) ---------------------------------
        if base + 4 <= n {
            load_record(act0, &records[base], num_inputs);
            load_record(act1, &records[base + 1], num_inputs);
            load_record(act2, &records[base + 2], num_inputs);
            load_record(act3, &records[base + 3], num_inputs);

            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;

                if neuron.is_constant {
                    let v = apply_limit_range(SquashType::Identity, neuron.bias);
                    act0[actual_idx] = v;
                    act1[actual_idx] = v;
                    act2[actual_idx] = v;
                    act3[actual_idx] = v;
                    continue;
                }

                let squash = SquashType::from(neuron.squash_type);
                match squash {
                    SquashType::Minimum
                    | SquashType::Maximum
                    | SquashType::If
                    | SquashType::Hypotenuse
                    | SquashType::HypotenuseV2
                    | SquashType::Mean => {
                        act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
                        act1[actual_idx] = neuron_activation_scalar(&self.synapses, act1, neuron);
                        act2[actual_idx] = neuron_activation_scalar(&self.synapses, act2, neuron);
                        act3[actual_idx] = neuron_activation_scalar(&self.synapses, act3, neuron);
                    }
                    _ => {
                        let start = neuron.start_synapse as usize;
                        let end = start + neuron.num_synapses as usize;
                        let (s0, s1, s2, s3) = weighted_sum_simd_4records(
                            &self.synapses,
                            act0,
                            act1,
                            act2,
                            act3,
                            start,
                            end,
                            neuron.bias,
                        );
                        // Vectorised squash across all 4 lanes for the covered
                        // types (Issue #243); scalar inline fallback otherwise.
                        let sums = [s0, s1, s2, s3];
                        let squashed = squash_x4(squash, sums).unwrap_or_else(|| {
                            let st = neuron.squash_type;
                            sums.map(|s| inline_squash(st, squash, s))
                        });
                        act0[actual_idx] = apply_limit_range(squash, squashed[0]);
                        act1[actual_idx] = apply_limit_range(squash, squashed[1]);
                        act2[actual_idx] = apply_limit_range(squash, squashed[2]);
                        act3[actual_idx] = apply_limit_range(squash, squashed[3]);
                    }
                }
            }

            for r in 0..4 {
                let src = match r {
                    0 => &*act0,
                    1 => &*act1,
                    2 => &*act2,
                    _ => &*act3,
                };
                let dst = (base + r) * num_outputs;
                out[dst..dst + num_outputs]
                    .copy_from_slice(&src[output_start..output_start + num_outputs]);
            }

            base += 4;
        }

        // ---- scalar tail (records.len() % 4) ---------------------------------
        // Exact single-record path so tail records are bit-identical to the
        // reference.
        while base < n {
            load_record(act0, &records[base], num_inputs);
            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;
                act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
            }
            let dst = base * num_outputs;
            out[dst..dst + num_outputs]
                .copy_from_slice(&act0[output_start..output_start + num_outputs]);
            base += 1;
        }
    }
}
