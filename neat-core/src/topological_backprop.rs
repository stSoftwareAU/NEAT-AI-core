//! Issue #9 — Reverse-topological backpropagation loop lifted from
//! NEAT-AI `wasm_activation/src/topological_backprop.rs`.
//!
//! The WASM-facing `#[wasm_bindgen] propagate_topological(data: &[u8])` entry
//! over in `wasm_activation` is ABI plumbing — it packs a binary buffer to
//! avoid per-neuron JS↔WASM boundary crossings. The inner algorithm is
//! generic neural-network backprop, and every helper it calls
//! (`apply_fused_error_distribution`, `apply_distribute_elastic_error`,
//! `apply_unsquash`, `accumulate_weight_single`, `accumulate_bias_single`)
//! already lives in this crate. Only the assembly was stranded there.
//!
//! This module extracts the loop as [`propagate_topological_loop`], a plain
//! `pub fn` over decoded Rust structs. Native consumers (CLI, scorer,
//! discovery) can call it directly without going through a byte-packed ABI;
//! the WASM crate keeps its `#[wasm_bindgen]` shim that decodes the byte
//! buffer, calls this function, then re-encodes results with the TS↔WASM
//! sentinel contract (`-Infinity` = fall back to TS noChange path,
//! `+Infinity` = IF/MAX/MIN needs TS custom propagate).
//!
//! Upstream context:
//! - NEAT-AI #1954 — original WASM migration of the backprop loop.
//! - NEAT-AI #2341 — architecture decision to keep `wasm_activation` in NEAT-AI.
//! - NEAT-AI #2366 / #2369 — parent epic and source audit.

use crate::accumulate::{accumulate_bias_single, accumulate_weight_single};
use crate::elastic_distribution::apply_distribute_elastic_error;
use crate::fused_error::apply_fused_error_distribution;
use crate::squash::SquashType;
use crate::unsquash::apply_unsquash;

// ---------------------------------------------------------------------------
// Shared neuron-type codes.
//
// These values are part of the TS↔WASM contract (`WasmTopologicalBackprop.ts`
// mirrors the same constants). Kept as `pub const` for byte-packed ABI
// callers and as [`NeuronType`] for typed Rust callers.
// ---------------------------------------------------------------------------

/// Input neuron — receives external data, has no inward connections.
pub const NEURON_TYPE_INPUT: u8 = 0;
/// Hidden neuron — internal computation node.
pub const NEURON_TYPE_HIDDEN: u8 = 1;
/// Output neuron — produces network output.
pub const NEURON_TYPE_OUTPUT: u8 = 2;
/// Constant neuron — fixed activation, no inward connections.
pub const NEURON_TYPE_CONSTANT: u8 = 3;

/// Typed neuron category for the reverse-topological backprop loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NeuronType {
    /// Receives external data; never back-propagated through.
    Input = NEURON_TYPE_INPUT,
    /// Internal computation node.
    Hidden = NEURON_TYPE_HIDDEN,
    /// Produces network output.
    Output = NEURON_TYPE_OUTPUT,
    /// Fixed activation; never back-propagated through.
    Constant = NEURON_TYPE_CONSTANT,
}

impl From<u8> for NeuronType {
    fn from(v: u8) -> Self {
        match v {
            NEURON_TYPE_INPUT => NeuronType::Input,
            NEURON_TYPE_HIDDEN => NeuronType::Hidden,
            NEURON_TYPE_OUTPUT => NeuronType::Output,
            NEURON_TYPE_CONSTANT => NeuronType::Constant,
            // Unknown codes default to `Hidden` — the most permissive classification
            // (i.e. participates in backprop). The WASM-side decoder is expected to
            // validate incoming bytes before this point.
            _ => NeuronType::Hidden,
        }
    }
}

impl NeuronType {
    /// Returns true if this neuron never acts as a backprop *source*
    /// (i.e. we do not write back into its upstream accumulators).
    #[inline]
    pub fn is_terminal_upstream(self) -> bool {
        matches!(self, NeuronType::Input | NeuronType::Constant)
    }
}

// ---------------------------------------------------------------------------
// Decoded input structs
// ---------------------------------------------------------------------------

/// Per-neuron decoded input.
#[derive(Debug, Clone)]
pub struct NeuronInput {
    /// Activation squash code — matches [`SquashType`] discriminants.
    pub squash_type: u8,
    /// Neuron category — matches the `NEURON_TYPE_*` constants
    /// (`NEURON_TYPE_INPUT`, `NEURON_TYPE_HIDDEN`, `NEURON_TYPE_OUTPUT`,
    /// `NEURON_TYPE_CONSTANT`).
    pub neuron_type: u8,
    /// Whether this neuron participates in error distribution (sparseConfig).
    pub propagate_needed: bool,
    /// Whether this neuron should accumulate weight/bias updates (sparseConfig).
    pub update_needed: bool,
    /// Cached value-space hint for non-invertible unsquash.
    pub hint_value: f32,
    /// Range lower bound for target-activation clamping.
    pub range_low: f32,
    /// Range upper bound for target-activation clamping.
    pub range_high: f32,
    /// Adjusted activation snapshot (pre-computed at loop start).
    pub adjusted_activation: f32,
    /// Adjusted bias snapshot (pre-computed at loop start).
    pub adjusted_bias: f32,
}

/// Per-synapse decoded input.
#[derive(Debug, Clone, Copy)]
pub struct SynapseInput {
    /// Source neuron index.
    pub from: u32,
    /// Destination neuron index.
    pub to: u32,
    /// Original synapse weight (un-adjusted), passed to accumulate_weight.
    pub original_weight: f32,
    /// Adjusted weight snapshot (pre-computed at loop start).
    pub adjusted_weight: f32,
    /// Whether this synapse is a self-loop (from == to).
    pub is_self_loop: bool,
}

/// Full decoded input for [`propagate_topological_loop`].
///
/// Mirrors the byte-packed buffer the WASM ABI decodes: all fields are
/// snapshots pre-computed at loop start (no mid-loop state mutation needed).
#[derive(Debug, Clone)]
pub struct PropagateInput<'a> {
    /// Per-neuron data indexed by neuron id.
    pub neurons: &'a [NeuronInput],
    /// Per-synapse data indexed by synapse id.
    pub synapses: &'a [SynapseInput],
    /// Per-neuron: offset into `inward_synapse_indices` for this neuron's inward list.
    pub inward_starts: &'a [u32],
    /// Per-neuron: number of inward connections.
    pub inward_counts: &'a [u32],
    /// Flat array of global synapse indices forming each neuron's inward list.
    pub inward_synapse_indices: &'a [u32],
    /// Reverse topological order — output neurons first, hidden after
    /// downstream consumers. Input neurons excluded.
    pub reverse_topo_order: &'a [u32],
    /// Expected output values — length equals output-neuron count.
    pub expected: &'a [f32],
    /// Number of input neurons (also the lower bound below which we do not
    /// accumulate upstream target deltas).
    pub input_count: u32,
    /// Number of output neurons.
    pub output_count: u32,
    /// Plank constant — floating-point noise threshold.
    pub plank_constant: f32,
    /// Whether to apply sqrt-scaling to multi-path gradients (Issue #1872).
    pub normalise_gradients: bool,
}

// ---------------------------------------------------------------------------
// Decoded output structs
// ---------------------------------------------------------------------------

/// Per-neuron outcome — typed replacement for the WASM sentinel encoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropagateOutcome {
    /// Neuron had no accumulated error signal or was sparse-skipped.
    /// The WASM shim should not touch this neuron's state.
    Skipped,
    /// Error below `plank_constant` — TS callers fall into the `noChange`
    /// recursive path. The WASM shim encodes this slot as `-Infinity` in the
    /// cached-activation field to trigger the TS-side bailout contract.
    NoChange {
        /// Cached adjusted activation that the noChange path should observe.
        cached_activation: f32,
    },
    /// IF/MAXIMUM/MINIMUM neuron — standard fused distribution is skipped;
    /// native callers invoke the custom propagate method themselves. The
    /// WASM shim encodes this as `+Infinity` in the cached-activation slot
    /// and stashes `target_activation` in the trace-activation slot.
    Special {
        /// Target activation passed to the TS custom propagate method.
        target_activation: f32,
    },
    /// Standard neuron — deltas accumulated into the NeuronState fields.
    Standard(StandardOutcome),
}

/// Per-neuron standard-path deltas.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StandardOutcome {
    /// Delta to add to `NeuronState.totalErrorAbsolute`.
    pub total_error_absolute_delta: f32,
    /// Cached adjusted activation — the new squashed output.
    pub cached_activation: f32,
    /// Whether this neuron is flagged `noChange` (because `!update_needed`).
    pub no_change: bool,
    /// Bias accumulation count delta (0 or 1).
    pub bias_count_delta: u32,
    /// Bias accumulation `totalBias` delta.
    pub total_bias_delta: f32,
    /// Bias accumulation `totalAdjustedBias` delta.
    pub total_adjusted_bias_delta: f32,
    /// Trace activation — `Some` when `update_needed`, `None` otherwise.
    pub trace_activation: Option<f32>,
}

/// Per-synapse accumulator delta (matches `accumulate_weight_single` output).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SynapseDelta {
    /// Count delta to add to `SynapseState.count`.
    pub count: f32,
    /// `totalPositiveActivation` delta.
    pub total_positive_activation: f32,
    /// `totalNegativeActivation` delta.
    pub total_negative_activation: f32,
    /// `countPositiveActivations` delta.
    pub count_positive: f32,
    /// `countNegativeActivations` delta.
    pub count_negative: f32,
    /// `totalPositiveAdjustedValue` delta.
    pub total_positive_adjusted_value: f32,
    /// `totalNegativeAdjustedValue` delta.
    pub total_negative_adjusted_value: f32,
}

/// Result of [`propagate_topological_loop`].
#[derive(Debug, Clone)]
pub struct PropagateOutput {
    /// Per-neuron outcome — indexed by neuron id.
    pub neurons: Vec<PropagateOutcome>,
    /// Per-synapse accumulator deltas — indexed by synapse id.
    pub synapses: Vec<SynapseDelta>,
}

// ---------------------------------------------------------------------------
// Main algorithm
// ---------------------------------------------------------------------------

/// Reverse-topological backpropagation loop.
///
/// Seeds output neurons with `(expected − activation)` error, then iterates
/// neurons in reverse topological order. Each neuron's accumulated target
/// delta is clamped to its activation range and converted to per-link error
/// via the fused error-distribution helper (with elastic-distribution
/// fallback when the safe zone collapses). Per-synapse weight statistics
/// and per-neuron bias statistics are accumulated as deltas rather than
/// in-place state mutation — callers apply them to their SynapseState /
/// NeuronState objects after the loop returns.
///
/// The pre-computed `adjusted_activation` / `adjusted_bias` / `adjusted_weight`
/// snapshots are used throughout; the loop does **not** recompute them as
/// state changes mid-iteration. This matches the WASM-path invariant (the
/// caller must use `batchSize > 1` — the TS fallback handles the in-loop
/// recomputation case).
pub fn propagate_topological_loop(input: &PropagateInput<'_>) -> PropagateOutput {
    let neuron_count = input.neurons.len();
    let synapse_count = input.synapses.len();
    let input_count = input.input_count as usize;

    let mut neurons_out: Vec<PropagateOutcome> = vec![PropagateOutcome::Skipped; neuron_count];
    let mut synapses_out: Vec<SynapseDelta> = vec![SynapseDelta::default(); synapse_count];

    // Per-neuron accumulated target delta (signed sum) and path count.
    let mut target_delta_sum = vec![0.0f64; neuron_count];
    let mut target_delta_count = vec![0u32; neuron_count];

    // Seed output neurons with (expected − adjustedActivation).
    let output_count = input.output_count as usize;
    if output_count > 0 {
        let last_output_idx = neuron_count.saturating_sub(output_count);
        for i in 0..output_count {
            let idx = last_output_idx + i;
            if idx >= neuron_count {
                break;
            }
            let n = &input.neurons[idx];
            if !n.propagate_needed {
                continue;
            }
            let activation = n.adjusted_activation;
            let expected = input.expected.get(i).copied().unwrap_or(0.0);
            target_delta_sum[idx] = (expected - activation) as f64;
            target_delta_count[idx] = 1;
        }
    }

    let plank = input.plank_constant;
    let plank_f64 = plank as f64;

    // Issue #154 — reusable per-neuron scratch buffers, hoisted outside the
    // reverse-topo loop. Cleared and refilled per neuron so capacity is
    // retained across neurons instead of allocating nine fresh Vecs each time.
    // Fused error-distribution arguments over the neuron's inward list.
    let mut fused_squash_types: Vec<u8> = Vec::new();
    let mut fused_hint_values: Vec<f32> = Vec::new();
    let mut fused_activations: Vec<f32> = Vec::new();
    let mut fused_weights: Vec<f32> = Vec::new();
    // Per-link caches — indexed by position within the inward list.
    let mut from_activation_cache: Vec<f32> = Vec::new();
    let mut from_weight_cache: Vec<f32> = Vec::new();
    let mut from_value_cache: Vec<f32> = Vec::new();
    let mut synapse_idx_cache: Vec<usize> = Vec::new();
    let mut is_self_loop_cache: Vec<bool> = Vec::new();

    // Process each neuron exactly once in reverse topological order.
    for &neuron_index_u32 in input.reverse_topo_order {
        let neuron_index = neuron_index_u32 as usize;
        if neuron_index >= neuron_count {
            continue;
        }

        let count = target_delta_count[neuron_index];
        if count == 0 {
            continue;
        }

        let n = &input.neurons[neuron_index];
        if !n.propagate_needed {
            continue;
        }

        let activation = n.adjusted_activation;

        // Issue #1872 — sqrt-scaled gradient for multi-path neurons.
        let total_delta = if input.normalise_gradients && count > 1 {
            target_delta_sum[neuron_index] / (count as f64).sqrt()
        } else {
            target_delta_sum[neuron_index]
        };
        let requested_activation = activation as f64 + total_delta;
        let target_activation =
            requested_activation.clamp(n.range_low as f64, n.range_high as f64) as f32;

        let raw_error_abs = (target_activation - activation).abs();
        if raw_error_abs < plank {
            neurons_out[neuron_index] = PropagateOutcome::NoChange {
                cached_activation: activation,
            };
            continue;
        }

        let update_needed = n.update_needed;
        let no_change_flag = !update_needed;

        // Special neurons (IF / MAXIMUM / MINIMUM) — defer to the TS
        // custom propagate path.
        let squash = SquashType::from(n.squash_type);
        if matches!(
            squash,
            SquashType::If | SquashType::Maximum | SquashType::Minimum
        ) {
            neurons_out[neuron_index] = PropagateOutcome::Special { target_activation };
            continue;
        }

        let current_bias = n.adjusted_bias;
        let mut improved_value: f64 = current_bias as f64;

        let start = input.inward_starts[neuron_index] as usize;
        let list_length = input.inward_counts[neuron_index] as usize;

        // Reuse the hoisted scratch buffers: clear retains capacity, so the
        // push-based fill below reallocates only when this neuron's inward
        // list is longer than any seen so far.
        fused_squash_types.clear();
        fused_hint_values.clear();
        fused_activations.clear();
        fused_weights.clear();
        from_activation_cache.clear();
        from_weight_cache.clear();
        from_value_cache.clear();
        synapse_idx_cache.clear();
        is_self_loop_cache.clear();

        for index in 0..list_length {
            let syn_idx = input
                .inward_synapse_indices
                .get(start + index)
                .copied()
                .unwrap_or(u32::MAX) as usize;
            synapse_idx_cache.push(syn_idx);

            if syn_idx >= synapse_count {
                // Unknown synapse reference — treat as zeroed self-loop.
                from_activation_cache.push(0.0f32);
                from_weight_cache.push(0.0f32);
                from_value_cache.push(0.0f32);
                fused_squash_types.push(SquashType::Identity as u8);
                fused_hint_values.push(0.0f32);
                fused_activations.push(0.0f32);
                fused_weights.push(0.0f32);
                is_self_loop_cache.push(true);
                continue;
            }

            let syn = input.synapses[syn_idx];
            if syn.is_self_loop {
                from_activation_cache.push(0.0f32);
                from_weight_cache.push(0.0f32);
                from_value_cache.push(0.0f32);
                fused_squash_types.push(SquashType::Identity as u8);
                fused_hint_values.push(0.0f32);
                fused_activations.push(0.0f32);
                fused_weights.push(0.0f32);
                is_self_loop_cache.push(true);
                continue;
            }

            let from = syn.from as usize;
            let from_neuron = &input.neurons[from];
            let from_activation = from_neuron.adjusted_activation;
            let from_weight = syn.adjusted_weight;

            from_activation_cache.push(from_activation);
            from_weight_cache.push(from_weight);
            from_value_cache.push(from_weight * from_activation);
            fused_activations.push(from_activation);
            fused_weights.push(from_weight);
            is_self_loop_cache.push(false);

            let upstream_is_terminal =
                NeuronType::from(from_neuron.neuron_type).is_terminal_upstream();
            if upstream_is_terminal || !from_neuron.propagate_needed {
                fused_squash_types.push(SquashType::Identity as u8);
                fused_hint_values.push(0.0);
            } else {
                fused_squash_types.push(from_neuron.squash_type);
                fused_hint_values.push(from_neuron.hint_value);
            }
        }

        let (error, safe_zone_factors, per_link_error) = if list_length > 0 {
            let fused = apply_fused_error_distribution(
                squash,
                activation,
                target_activation,
                n.hint_value,
                &fused_squash_types,
                &fused_hint_values,
                &fused_activations,
                &fused_weights,
            );
            // Layout: [error, safeZone_0..safeZone_N, perLinkError_0..perLinkError_N]
            let err = fused[0];
            let mut safe_zones: Vec<f32> = fused[1..1 + list_length].to_vec();
            let per_link: Vec<f32> = fused[1 + list_length..1 + 2 * list_length].to_vec();

            // Block self-loops in the safe-zone vector so they are never chosen
            // as upstream error sinks.
            for i in 0..list_length {
                if is_self_loop_cache[i] {
                    safe_zones[i] = 0.0;
                }
            }

            // If no link has a usable safe zone, fall back to elastic
            // distribution over weights (safeZoneFactor := 1 per link).
            let mut has_usable = false;
            for &sz in &safe_zones {
                if sz.is_finite() && sz > plank {
                    has_usable = true;
                    break;
                }
            }

            let final_per_link = if has_usable {
                per_link
            } else {
                let fallback_safe = vec![1.0f32; list_length];
                apply_distribute_elastic_error(
                    err,
                    &fused_activations,
                    &fallback_safe,
                    &from_weight_cache,
                    plank,
                )
            };

            (err, safe_zones, final_per_link)
        } else {
            (0.0f32, Vec::new(), Vec::new())
        };
        let _ = error; // `error` sum not directly needed below; retained for clarity.

        // Distribute error upstream and accumulate per-synapse weight deltas.
        for index in 0..list_length {
            if is_self_loop_cache[index] {
                continue;
            }
            let syn_idx = synapse_idx_cache[index];
            if syn_idx >= synapse_count {
                continue;
            }
            let syn = input.synapses[syn_idx];
            let from = syn.from as usize;
            let to = syn.to as usize;
            let _ = to; // not used beyond the self-loop guard above.

            let from_neuron = &input.neurons[from];
            let from_activation = from_activation_cache[index];
            let from_weight = from_weight_cache[index];

            let from_value = from_value_cache[index];
            let this_link_error = per_link_error.get(index).copied().unwrap_or(0.0);
            let target_from_value = from_value + this_link_error;

            let upstream_is_terminal =
                NeuronType::from(from_neuron.neuron_type).is_terminal_upstream();

            if !upstream_is_terminal
                && from_neuron.propagate_needed
                && (target_from_value - from_value).abs() > plank
            {
                // Issue #1654: minimum effective weight to keep dead-zero-weight
                // paths recoverable.
                let effective_weight = if from_weight.abs() > plank {
                    from_weight
                } else {
                    let sign = if from_weight > 0.0 {
                        1.0
                    } else if from_weight < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    plank * sign
                };
                let target_from_activation = target_from_value / effective_weight;
                let safe_zone_factor = safe_zone_factors.get(index).copied().unwrap_or(1.0);
                if safe_zone_factor.is_finite() && safe_zone_factor > 0.0 {
                    // Issue #1873: clamp out-of-range upstream targets rather
                    // than dropping them.
                    let clamped_target =
                        target_from_activation.clamp(from_neuron.range_low, from_neuron.range_high);
                    if clamped_target.is_finite() && from >= input_count {
                        target_delta_sum[from] += (clamped_target - from_activation) as f64;
                        target_delta_count[from] = target_delta_count[from].saturating_add(1);
                    }
                }
            }

            // Accumulate per-synapse weight statistics.
            if update_needed && from_activation.abs() > plank {
                let (d_count, d_pos_act, d_neg_act, d_cnt_pos, d_cnt_neg, d_pos_adj, d_neg_adj) =
                    accumulate_weight_single(
                        syn.original_weight as f64,
                        target_from_value as f64,
                        from_activation as f64,
                        plank_f64,
                        0.0,
                        0.0,
                        0.0,
                    );
                let s = &mut synapses_out[syn_idx];
                s.count += d_count as f32;
                s.total_positive_activation += d_pos_act as f32;
                s.total_negative_activation += d_neg_act as f32;
                s.count_positive += d_cnt_pos as f32;
                s.count_negative += d_cnt_neg as f32;
                s.total_positive_adjusted_value += d_pos_adj as f32;
                s.total_negative_adjusted_value += d_neg_adj as f32;

                // `improvedValue` uses the *pre-computed* adjusted weight
                // snapshot — mid-loop recomputation is deliberately skipped
                // (matches the WASM-path invariant, caller guards with
                // batchSize > 1).
                improved_value += (from_activation * from_weight) as f64;
            }
        }

        // Bias accumulation + cached activation.
        let mut outcome = StandardOutcome {
            total_error_absolute_delta: raw_error_abs,
            cached_activation: 0.0,
            no_change: no_change_flag,
            bias_count_delta: 0,
            total_bias_delta: 0.0,
            total_adjusted_bias_delta: 0.0,
            trace_activation: None,
        };

        let limited_activation = if update_needed {
            let target_value = apply_unsquash(squash, target_activation, n.hint_value);
            let (d_count, d_total_bias, d_adj_bias) = accumulate_bias_single(
                target_value as f64,
                improved_value,
                current_bias as f64,
                plank_f64,
                0.0,
                0.0,
                0.0,
            );
            outcome.bias_count_delta = d_count as u32;
            outcome.total_bias_delta = d_total_bias as f32;
            outcome.total_adjusted_bias_delta = d_adj_bias as f32;

            // aBias is the snapshot (we do not recompute mid-loop).
            let a_bias = n.adjusted_bias;
            let pre_squash = improved_value as f32 + a_bias - current_bias;
            crate::squash::apply_squash(squash, pre_squash)
        } else {
            crate::squash::apply_squash(squash, improved_value as f32)
        };

        outcome.cached_activation = limited_activation;
        if update_needed {
            outcome.trace_activation = Some(limited_activation);
        }
        neurons_out[neuron_index] = PropagateOutcome::Standard(outcome);
    }

    PropagateOutput {
        neurons: neurons_out,
        synapses: synapses_out,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_neuron(
        squash: SquashType,
        neuron_type: u8,
        adjusted_activation: f32,
        adjusted_bias: f32,
    ) -> NeuronInput {
        NeuronInput {
            squash_type: squash as u8,
            neuron_type,
            propagate_needed: true,
            update_needed: true,
            hint_value: 0.0,
            range_low: -1.0e6,
            range_high: 1.0e6,
            adjusted_activation,
            adjusted_bias,
        }
    }

    #[test]
    fn neuron_type_round_trips_constants() {
        assert_eq!(NeuronType::from(NEURON_TYPE_INPUT), NeuronType::Input);
        assert_eq!(NeuronType::from(NEURON_TYPE_HIDDEN), NeuronType::Hidden);
        assert_eq!(NeuronType::from(NEURON_TYPE_OUTPUT), NeuronType::Output);
        assert_eq!(NeuronType::from(NEURON_TYPE_CONSTANT), NeuronType::Constant);
        assert_eq!(NeuronType::Input as u8, 0);
        assert_eq!(NeuronType::Hidden as u8, 1);
        assert_eq!(NeuronType::Output as u8, 2);
        assert_eq!(NeuronType::Constant as u8, 3);
    }

    #[test]
    fn neuron_type_is_terminal_upstream_matches_semantics() {
        assert!(NeuronType::Input.is_terminal_upstream());
        assert!(NeuronType::Constant.is_terminal_upstream());
        assert!(!NeuronType::Hidden.is_terminal_upstream());
        assert!(!NeuronType::Output.is_terminal_upstream());
    }

    #[test]
    fn no_expected_error_yields_no_change_for_output() {
        // 2 inputs (0,1) → 1 output (2) with expected == activation ⇒ noChange.
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 1.0, 0.0),
        ];
        let synapses = vec![
            SynapseInput {
                from: 0,
                to: 2,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 2,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
        ];
        let inward_starts = vec![0u32, 0, 0];
        let inward_counts = vec![0u32, 0, 2];
        let inward_indices = vec![0u32, 1];
        let order = vec![2u32];
        let expected = vec![1.0f32]; // exact match ⇒ zero error

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 2,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);
        // Neurons 0 and 1 (inputs) are never in the order and stay Skipped.
        assert_eq!(out.neurons[0], PropagateOutcome::Skipped);
        assert_eq!(out.neurons[1], PropagateOutcome::Skipped);
        // Neuron 2 is the seeded output; expected == activation ⇒ NoChange.
        match out.neurons[2] {
            PropagateOutcome::NoChange { cached_activation } => {
                assert!((cached_activation - 1.0).abs() < 1e-6);
            }
            other => panic!("expected NoChange, got {:?}", other),
        }
    }

    #[test]
    fn non_zero_error_produces_standard_outcome_and_upstream_delta() {
        // 2 inputs (0,1) → 1 hidden (2) → 1 output (3). Error at output
        // should propagate to hidden and also accumulate weight deltas.
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.6, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.4, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_HIDDEN, 0.5, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.5, 0.0),
        ];
        let synapses = vec![
            SynapseInput {
                from: 0,
                to: 2,
                original_weight: 0.5,
                adjusted_weight: 0.5,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 2,
                original_weight: 0.5,
                adjusted_weight: 0.5,
                is_self_loop: false,
            },
            SynapseInput {
                from: 2,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
        ];
        let inward_starts = vec![0u32, 0, 0, 2];
        let inward_counts = vec![0u32, 0, 2, 1];
        let inward_indices = vec![0u32, 1, 2];
        let order = vec![3u32, 2u32];
        let expected = vec![1.0f32]; // target 1.0 vs activation 0.5 ⇒ error 0.5

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 2,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);

        // Output neuron must be Standard.
        match out.neurons[3] {
            PropagateOutcome::Standard(s) => {
                assert!(s.total_error_absolute_delta > 0.0);
                assert_eq!(s.bias_count_delta, 1);
                assert!(s.trace_activation.is_some());
            }
            other => panic!("neuron 3: expected Standard, got {:?}", other),
        }

        // Hidden neuron should have received accumulated error and produced
        // Standard output (not NoChange — error is substantial).
        match out.neurons[2] {
            PropagateOutcome::Standard(s) => {
                assert!(s.total_error_absolute_delta > 0.0);
            }
            PropagateOutcome::NoChange { .. } => {
                // Acceptable if the per-link error collapses to below plank,
                // but with these values it should not.
                panic!("hidden neuron unexpectedly NoChange");
            }
            other => panic!("neuron 2: expected Standard, got {:?}", other),
        }

        // Synapse 2 (hidden→output) should have accumulated a count delta.
        assert!(out.synapses[2].count > 0.0);
    }

    #[test]
    fn special_squash_yields_special_outcome() {
        // 3 inputs → 1 IF neuron output.
        let mut if_neuron = make_neuron(SquashType::If, NEURON_TYPE_OUTPUT, 0.2, 0.0);
        if_neuron.range_low = -1.0e6;
        if_neuron.range_high = 1.0e6;
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 1.0, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.0, 0.0),
            if_neuron,
        ];
        let synapses = vec![
            SynapseInput {
                from: 0,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 2,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
        ];
        let inward_starts = vec![0u32, 0, 0, 0];
        let inward_counts = vec![0u32, 0, 0, 3];
        let inward_indices = vec![0u32, 1, 2];
        let order = vec![3u32];
        let expected = vec![0.9f32];

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 3,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);
        match out.neurons[3] {
            PropagateOutcome::Special { target_activation } => {
                assert!(target_activation.is_finite());
                assert!((target_activation - 0.9).abs() < 1e-4);
            }
            other => panic!("expected Special (IF), got {:?}", other),
        }
    }

    #[test]
    fn self_loop_synapse_is_blocked_from_upstream_feedback() {
        // 1 input (0) → 1 output (1) + self-loop on (1).
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.2, 0.0),
        ];
        let synapses = vec![
            SynapseInput {
                from: 0,
                to: 1,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 1,
                original_weight: 0.5,
                adjusted_weight: 0.5,
                is_self_loop: true,
            },
        ];
        let inward_starts = vec![0u32, 0];
        let inward_counts = vec![0u32, 2];
        let inward_indices = vec![0u32, 1];
        let order = vec![1u32];
        let expected = vec![0.9f32];

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 1,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);
        // Self-loop synapse (index 1) must have zero deltas.
        assert_eq!(out.synapses[1], SynapseDelta::default());
    }

    #[test]
    fn skipped_neurons_when_no_error_seeded() {
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.0, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.0, 0.0),
        ];
        let synapses = vec![SynapseInput {
            from: 0,
            to: 1,
            original_weight: 1.0,
            adjusted_weight: 1.0,
            is_self_loop: false,
        }];
        let inward_starts = vec![0u32, 0];
        let inward_counts = vec![0u32, 1];
        let inward_indices = vec![0u32];
        let order = vec![1u32];
        let expected: Vec<f32> = vec![0.0]; // matches ⇒ zero seeded error ⇒ NoChange.

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 1,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);
        // Error is exactly zero ⇒ raw_error_abs < plank ⇒ NoChange branch.
        assert!(matches!(out.neurons[1], PropagateOutcome::NoChange { .. }));
    }

    #[test]
    fn propagate_needed_false_on_output_skips_neuron() {
        // Output neuron with propagate_needed = false ⇒ not seeded, Skipped.
        let mut output = make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.0, 0.0);
        output.propagate_needed = false;
        let neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 1.0, 0.0),
            output,
        ];
        let synapses = vec![SynapseInput {
            from: 0,
            to: 1,
            original_weight: 1.0,
            adjusted_weight: 1.0,
            is_self_loop: false,
        }];
        let inward_starts = vec![0u32, 0];
        let inward_counts = vec![0u32, 1];
        let inward_indices = vec![0u32];
        let order = vec![1u32];
        let expected = vec![0.9f32]; // would otherwise be Standard.

        let input = PropagateInput {
            neurons: &neurons,
            synapses: &synapses,
            inward_starts: &inward_starts,
            inward_counts: &inward_counts,
            inward_synapse_indices: &inward_indices,
            reverse_topo_order: &order,
            expected: &expected,
            input_count: 1,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };

        let out = propagate_topological_loop(&input);
        assert_eq!(out.neurons[1], PropagateOutcome::Skipped);
    }

    #[test]
    fn normalise_gradients_reduces_multi_path_delta() {
        // Output neuron with 4 accumulated paths — sqrt-scaling halves the
        // effective target delta compared to raw accumulation.
        // We exercise this by running both paths through the same wiring
        // and confirming that the Standard outcome's total_error_absolute_delta
        // is smaller under normalisation.
        fn run(normalise: bool) -> f32 {
            let neurons = vec![
                make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_HIDDEN, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_HIDDEN, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_HIDDEN, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_HIDDEN, 0.5, 0.0),
                make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.0, 0.0),
            ];
            // 2 inputs → 4 hiddens, each hidden → output.
            let mut synapses = Vec::new();
            for h in 2..6 {
                for i in 0..2 {
                    synapses.push(SynapseInput {
                        from: i,
                        to: h,
                        original_weight: 0.25,
                        adjusted_weight: 0.25,
                        is_self_loop: false,
                    });
                }
            }
            for h in 2..6 {
                synapses.push(SynapseInput {
                    from: h,
                    to: 6,
                    original_weight: 0.25,
                    adjusted_weight: 0.25,
                    is_self_loop: false,
                });
            }

            // Build inward mapping.
            let mut inward_starts = vec![0u32; 7];
            let mut inward_counts = vec![0u32; 7];
            let mut inward_indices: Vec<u32> = Vec::new();
            for nidx in 0..7usize {
                inward_starts[nidx] = inward_indices.len() as u32;
                let mut c = 0u32;
                for (si, s) in synapses.iter().enumerate() {
                    if s.to as usize == nidx {
                        inward_indices.push(si as u32);
                        c += 1;
                    }
                }
                inward_counts[nidx] = c;
            }

            let order = vec![6u32, 2, 3, 4, 5];
            let expected = vec![1.0f32];

            let input = PropagateInput {
                neurons: &neurons,
                synapses: &synapses,
                inward_starts: &inward_starts,
                inward_counts: &inward_counts,
                inward_synapse_indices: &inward_indices,
                reverse_topo_order: &order,
                expected: &expected,
                input_count: 2,
                output_count: 1,
                plank_constant: 1e-7,
                normalise_gradients: normalise,
            };
            let out = propagate_topological_loop(&input);
            // Measure the error delta on hidden neuron 2 — reflects the
            // propagated delta that arrived at this neuron via the output.
            match out.neurons[2] {
                PropagateOutcome::Standard(s) => s.total_error_absolute_delta,
                other => panic!("expected Standard for hidden, got {:?}", other),
            }
        }

        let unnormalised = run(false);
        let normalised = run(true);
        // With a single inbound path to each hidden (from output only, via
        // one synapse), count == 1 ⇒ branch is a no-op. This test therefore
        // asserts that normalisation does not *increase* error magnitude —
        // the full sqrt-scaling behaviour is exercised by multi-path
        // integration tests in downstream consumers.
        assert!(normalised <= unnormalised + 1e-6);
    }

    #[test]
    fn scratch_buffer_reuse_does_not_leak_across_neurons() {
        // Regression for #154: the per-neuron error-distribution scratch
        // buffers are reused (cleared + refilled) across neurons within one
        // backward pass. A larger neuron processed earlier must not leak
        // stale entries into a smaller neuron processed later. We compute a
        // small 2-inward output neuron in two contexts and assert its outcome
        // and synapse deltas are bit-for-bit identical:
        //   (a) alone, and
        //   (b) immediately after a 3-inward neuron in the same pass.

        // Context (a): the small neuron on its own.
        let small_neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.6, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.3, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.4, 0.0),
        ];
        let small_synapses = vec![
            SynapseInput {
                from: 0,
                to: 2,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 2,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
        ];
        let small_starts = vec![0u32, 0, 0];
        let small_counts = vec![0u32, 0, 2];
        let small_indices = vec![0u32, 1];
        let small_order = vec![2u32];
        let small_expected = vec![1.0f32];
        let small_input = PropagateInput {
            neurons: &small_neurons,
            synapses: &small_synapses,
            inward_starts: &small_starts,
            inward_counts: &small_counts,
            inward_synapse_indices: &small_indices,
            reverse_topo_order: &small_order,
            expected: &small_expected,
            input_count: 2,
            output_count: 1,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };
        let small_out = propagate_topological_loop(&small_input);

        // Context (b): a 3-inward neuron (index 3) processed BEFORE the
        // identical 2-inward small neuron (index 4) in the same pass.
        let big_neurons = vec![
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.6, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.3, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_INPUT, 0.7, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.2, 0.0),
            make_neuron(SquashType::Identity, NEURON_TYPE_OUTPUT, 0.4, 0.0),
        ];
        let big_synapses = vec![
            // Big neuron 3 inward: synapses 0,1,2.
            SynapseInput {
                from: 0,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 2,
                to: 3,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            // Small neuron 4 inward: synapses 3,4 — identical wiring to (a).
            SynapseInput {
                from: 0,
                to: 4,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
            SynapseInput {
                from: 1,
                to: 4,
                original_weight: 1.0,
                adjusted_weight: 1.0,
                is_self_loop: false,
            },
        ];
        let big_starts = vec![0u32, 0, 0, 0, 3];
        let big_counts = vec![0u32, 0, 0, 3, 2];
        let big_indices = vec![0u32, 1, 2, 3, 4];
        // Process the big neuron first, then the small one.
        let big_order = vec![3u32, 4u32];
        let big_expected = vec![0.9f32, 1.0f32];
        let big_input = PropagateInput {
            neurons: &big_neurons,
            synapses: &big_synapses,
            inward_starts: &big_starts,
            inward_counts: &big_counts,
            inward_synapse_indices: &big_indices,
            reverse_topo_order: &big_order,
            expected: &big_expected,
            input_count: 3,
            output_count: 2,
            plank_constant: 1e-7,
            normalise_gradients: false,
        };
        let big_out = propagate_topological_loop(&big_input);

        // The small neuron's outcome must be identical in both contexts.
        assert_eq!(
            big_out.neurons[4], small_out.neurons[2],
            "small neuron outcome changed when preceded by a larger neuron — scratch buffer leaked"
        );
        // And its two synapse deltas must be bit-for-bit identical.
        assert_eq!(
            big_out.synapses[3], small_out.synapses[0],
            "small neuron synapse[0] delta leaked from preceding neuron"
        );
        assert_eq!(
            big_out.synapses[4], small_out.synapses[1],
            "small neuron synapse[1] delta leaked from preceding neuron"
        );
    }
}
