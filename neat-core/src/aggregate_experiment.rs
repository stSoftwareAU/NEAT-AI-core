//! **Experimental** unchecked aggregate inference kernels (Issue #510).
//!
//! This module is a *prototype*, compiled only under the non-default
//! `experimental-aggregate-unchecked` feature. It exists to answer one
//! question with measurements: does removing the per-synapse bounds check from
//! the `Minimum` / `Maximum` / `If` aggregate loops buy anything on a
//! production-sized creature?
//!
//! It carries **both** halves of the experiment so a benchmark can A/B them in
//! one process:
//!
//! - `*_safe` — the checked reference/control, a literal copy of the aggregate
//!   arms in [`crate::batch_scoring::neuron_activation_scalar`] and
//!   [`crate::network::CompiledNetwork::activate_and_trace`];
//! - `*_unchecked` — the same arithmetic with the activation gather done
//!   through `get_unchecked`.
//!
//! Both return `None` for every non-`Minimum`/`Maximum`/`If` squash, so a
//! caller falls through to its existing match arms unchanged.
//!
//! # Safety contract
//!
//! The unchecked kernels rely on two invariants, neither of which is re-checked
//! per synapse:
//!
//! 1. **Span validity** — `start_synapse <= end_synapse <= synapses.len()`.
//!    This is *not* assumed: it is verified once per neuron with
//!    `synapses.get(start..end)`. A malformed span falls back to the checked
//!    reference, which panics exactly as the current code does, so bad data is
//!    still rejected loudly rather than read out of bounds.
//! 2. **Source-index validity** — every `SynapseData::from_index` is
//!    `< activations.len()`. This is the crate-wide load-time invariant
//!    established by [`crate::network::CompiledNetwork::new`], which rejects an
//!    out-of-range `from_index` with `NetworkError::InvalidSynapseIndex`. It is
//!    the same precondition the SIMD weighted-sum kernels already depend on.
//!    Invariant 2 is the caller's obligation, hence `unsafe fn`.

use crate::network::{NeuronData, SynapseData};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// Checked reference (control) for the forward-only `Minimum`/`Maximum`/`If`
/// aggregate kernels.
///
/// Byte-for-byte the arithmetic of the corresponding arms in
/// [`crate::batch_scoring::neuron_activation_scalar`]. Returns the activation
/// *before* the closing range clamp, or `None` when `squash` is not one of the
/// three prototyped aggregates.
#[inline]
pub fn aggregate_forward_safe(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<f32> {
    let start = neuron.start_synapse as usize;
    let end = start + neuron.num_synapses as usize;

    let activation = match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            for s in &synapses[start..end] {
                let val = activations[s.from_index as usize] * s.weight;
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
                let val = activations[s.from_index as usize] * s.weight;
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
                let val = activations[s.from_index as usize] * s.weight;
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
        _ => return None,
    };

    Some(activation)
}

/// Attribution control: the **shipped `activate` / `activate_into` form** of
/// the forward aggregate loops, which walk `start..end` and index the synapse
/// array per iteration rather than iterating a slice.
///
/// [`aggregate_forward_safe`] mirrors `neuron_activation_scalar`, which already
/// iterates a slice. The two differ only in how the synapse array is walked, so
/// benchmarking all three arms (`safe` slice / `safe_indexed` / `unchecked`)
/// attributes any delta to *slice iteration* or to *bounds-check removal*
/// rather than lumping them together.
#[inline]
pub fn aggregate_forward_safe_indexed(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<f32> {
    let start = neuron.start_synapse as usize;
    let end = start + neuron.num_synapses as usize;

    let activation = match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            for synapse_idx in start..end {
                let s = &synapses[synapse_idx];
                let val = activations[s.from_index as usize] * s.weight;
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
            for synapse_idx in start..end {
                let s = &synapses[synapse_idx];
                let val = activations[s.from_index as usize] * s.weight;
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
            for synapse_idx in start..end {
                let s = &synapses[synapse_idx];
                let val = activations[s.from_index as usize] * s.weight;
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
        _ => return None,
    };

    Some(activation)
}

/// Prototype forward-only aggregate kernel with the activation gather done
/// unchecked.
///
/// Same arithmetic, same branch semantics, same empty-span behaviour and the
/// same floating-point ordering as [`aggregate_forward_safe`]; the only
/// difference is that the per-synapse `activations[from_index]` bounds check is
/// gone.
///
/// # Safety
///
/// Every `from_index` in `synapses[neuron.start_synapse..][..num_synapses]`
/// must be `< activations.len()`. Networks produced by
/// [`crate::network::CompiledNetwork::new`] satisfy this by construction — that
/// constructor rejects an out-of-range `from_index` — so calling this on a
/// loaded network is sound. Hand-built `CompiledNetwork` literals that skip the
/// constructor must uphold it themselves.
///
/// The synapse span is *not* a precondition: it is validated here once per
/// neuron and a malformed span falls back to the checked reference.
#[inline]
pub unsafe fn aggregate_forward_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<f32> {
    if !matches!(
        squash,
        SquashType::Minimum | SquashType::Maximum | SquashType::If
    ) {
        return None;
    }

    let start = neuron.start_synapse as usize;
    let end = start.saturating_add(neuron.num_synapses as usize);
    // Proof obligation 1, discharged once per neuron rather than per synapse.
    // A malformed span never reaches the unchecked loop: it takes the checked
    // reference, which panics on the same out-of-range slice as today.
    let Some(span) = synapses.get(start..end) else {
        return aggregate_forward_safe(synapses, activations, neuron, squash);
    };

    let activation = match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            for s in span {
                // SAFETY: proof obligation 2 — `from_index < activations.len()`
                // for every synapse of a network loaded by `CompiledNetwork::new`.
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
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
            for s in span {
                // SAFETY: as above — load-time `from_index` validation.
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
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
        _ => {
            let mut condition_sum = 0.0f32;
            let mut positive_sum = 0.0f32;
            let mut negative_sum = 0.0f32;
            for s in span {
                // SAFETY: as above — load-time `from_index` validation.
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
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
    };

    Some(activation)
}

/// Checked reference (control) for the traced `Minimum`/`Maximum`/`If`
/// kernels.
///
/// Returns `(activation, trace_info)` where `trace_info` is the winning local
/// synapse index for `Minimum`/`Maximum` and the branch flag (`1.0` positive,
/// `0.0` negative) for `If` — exactly what
/// [`crate::network::CompiledNetwork::activate_and_trace`] pushes into its
/// trace buffer. `None` for any other squash.
#[inline]
pub fn aggregate_traced_safe(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<(f32, f32)> {
    let start = neuron.start_synapse as usize;
    let num_synapse = neuron.num_synapses as usize;
    let end = start + num_synapse;

    match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            let mut min_local_idx: usize = 0;
            for local_idx in 0..num_synapse {
                let synapse = &synapses[start + local_idx];
                let val = activations[synapse.from_index as usize] * synapse.weight;
                if val < min_val {
                    min_val = val;
                    min_local_idx = local_idx;
                }
            }
            let result = if min_val == f32::INFINITY {
                neuron.bias
            } else {
                min_val + neuron.bias
            };
            Some((result, min_local_idx as f32))
        }
        SquashType::Maximum => {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_local_idx: usize = 0;
            for local_idx in 0..num_synapse {
                let synapse = &synapses[start + local_idx];
                let val = activations[synapse.from_index as usize] * synapse.weight;
                if val > max_val {
                    max_val = val;
                    max_local_idx = local_idx;
                }
            }
            let result = if max_val == f32::NEG_INFINITY {
                neuron.bias
            } else {
                max_val + neuron.bias
            };
            Some((result, max_local_idx as f32))
        }
        SquashType::If => {
            let mut condition_sum = 0.0f32;
            let mut positive_sum = 0.0f32;
            let mut negative_sum = 0.0f32;
            for synapse in &synapses[start..end] {
                let val = activations[synapse.from_index as usize] * synapse.weight;
                match SynapseType::from(synapse.synapse_type) {
                    SynapseType::Condition => condition_sum += val,
                    SynapseType::Negative => negative_sum += val,
                    SynapseType::Positive | SynapseType::Standard => positive_sum += val,
                }
            }
            let branch_taken = if condition_sum > 0.0 { 1.0f32 } else { 0.0f32 };
            let result = if condition_sum > 0.0 {
                positive_sum + neuron.bias
            } else {
                negative_sum + neuron.bias
            };
            Some((result, branch_taken))
        }
        _ => None,
    }
}

/// Attribution control for the traced kernels: identical to
/// [`aggregate_traced_safe`] but walking the synapse span as a **slice** with
/// `enumerate` instead of indexing `synapses[start + local_idx]`.
///
/// Fully safe — no `unsafe`, no unchecked gather. Benchmarking this against
/// both [`aggregate_traced_safe`] (the shipped form) and
/// [`aggregate_traced_unchecked`] says whether any measured win comes from the
/// iteration style or from removing the activation bounds check.
#[inline]
pub fn aggregate_traced_safe_span(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<(f32, f32)> {
    if !matches!(
        squash,
        SquashType::Minimum | SquashType::Maximum | SquashType::If
    ) {
        return None;
    }

    let start = neuron.start_synapse as usize;
    let end = start + neuron.num_synapses as usize;
    let span = &synapses[start..end];

    match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            let mut min_local_idx: usize = 0;
            for (local_idx, s) in span.iter().enumerate() {
                let val = activations[s.from_index as usize] * s.weight;
                if val < min_val {
                    min_val = val;
                    min_local_idx = local_idx;
                }
            }
            let result = if min_val == f32::INFINITY {
                neuron.bias
            } else {
                min_val + neuron.bias
            };
            Some((result, min_local_idx as f32))
        }
        SquashType::Maximum => {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_local_idx: usize = 0;
            for (local_idx, s) in span.iter().enumerate() {
                let val = activations[s.from_index as usize] * s.weight;
                if val > max_val {
                    max_val = val;
                    max_local_idx = local_idx;
                }
            }
            let result = if max_val == f32::NEG_INFINITY {
                neuron.bias
            } else {
                max_val + neuron.bias
            };
            Some((result, max_local_idx as f32))
        }
        _ => {
            let mut condition_sum = 0.0f32;
            let mut positive_sum = 0.0f32;
            let mut negative_sum = 0.0f32;
            for s in span {
                let val = activations[s.from_index as usize] * s.weight;
                match SynapseType::from(s.synapse_type) {
                    SynapseType::Condition => condition_sum += val,
                    SynapseType::Negative => negative_sum += val,
                    SynapseType::Positive | SynapseType::Standard => positive_sum += val,
                }
            }
            let branch_taken = if condition_sum > 0.0 { 1.0f32 } else { 0.0f32 };
            let result = if condition_sum > 0.0 {
                positive_sum + neuron.bias
            } else {
                negative_sum + neuron.bias
            };
            Some((result, branch_taken))
        }
    }
}

/// Prototype traced aggregate kernel with the activation gather done unchecked.
///
/// Preserves the winning-local-index tie-breaking (**strictly** less-than /
/// greater-than, so the first extreme wins) and the branch flag of
/// [`aggregate_traced_safe`].
///
/// # Safety
///
/// Same contract as [`aggregate_forward_unchecked`]: every `from_index` in the
/// neuron's synapse span must be `< activations.len()`, which
/// [`crate::network::CompiledNetwork::new`] guarantees for a loaded network.
#[inline]
pub unsafe fn aggregate_traced_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    neuron: &NeuronData,
    squash: SquashType,
) -> Option<(f32, f32)> {
    if !matches!(
        squash,
        SquashType::Minimum | SquashType::Maximum | SquashType::If
    ) {
        return None;
    }

    let start = neuron.start_synapse as usize;
    let end = start.saturating_add(neuron.num_synapses as usize);
    // Proof obligation 1 — see `aggregate_forward_unchecked`.
    let Some(span) = synapses.get(start..end) else {
        return aggregate_traced_safe(synapses, activations, neuron, squash);
    };

    match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            let mut min_local_idx: usize = 0;
            for (local_idx, s) in span.iter().enumerate() {
                // SAFETY: load-time `from_index` validation (obligation 2).
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
                if val < min_val {
                    min_val = val;
                    min_local_idx = local_idx;
                }
            }
            let result = if min_val == f32::INFINITY {
                neuron.bias
            } else {
                min_val + neuron.bias
            };
            Some((result, min_local_idx as f32))
        }
        SquashType::Maximum => {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_local_idx: usize = 0;
            for (local_idx, s) in span.iter().enumerate() {
                // SAFETY: load-time `from_index` validation (obligation 2).
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
                if val > max_val {
                    max_val = val;
                    max_local_idx = local_idx;
                }
            }
            let result = if max_val == f32::NEG_INFINITY {
                neuron.bias
            } else {
                max_val + neuron.bias
            };
            Some((result, max_local_idx as f32))
        }
        _ => {
            let mut condition_sum = 0.0f32;
            let mut positive_sum = 0.0f32;
            let mut negative_sum = 0.0f32;
            for s in span {
                // SAFETY: load-time `from_index` validation (obligation 2).
                let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
                match SynapseType::from(s.synapse_type) {
                    SynapseType::Condition => condition_sum += val,
                    SynapseType::Negative => negative_sum += val,
                    SynapseType::Positive | SynapseType::Standard => positive_sum += val,
                }
            }
            let branch_taken = if condition_sum > 0.0 { 1.0f32 } else { 0.0f32 };
            let result = if condition_sum > 0.0 {
                positive_sum + neuron.bias
            } else {
                negative_sum + neuron.bias
            };
            Some((result, branch_taken))
        }
    }
}
