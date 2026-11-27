//! Aggressive, speculative low-weight synapse pruning (Issue #778).
//!
//! Ports NEAT-AI `aggressivePrune` (`src/compact/AggressivePrune.ts`) into this
//! crate. Unlike [`crate::compact_zero_weight`], this pass is **approximate**:
//! removing a small but non-zero weight changes the forward pass, and it does so
//! even into aggregate consumers (`MAXIMUM`, `MINIMUM`, `HYPOT`, `HYPOTv2`) that
//! the exact pass must never touch. The result is a *candidate*, not a
//! behaviour-preserving rewrite.
//!
//! The judgement is structural and dataset-free: a synapse is a candidate when
//! `|weight| < threshold`. No error is measured here. Whether the candidate is
//! kept, by scoring it against the safe variant, stays with the caller; this
//! pass does no scoring.
//!
//! A synapse is never a candidate when it is frozen, carries any role
//! (`type`, including an empty string, which TypeScript tests with
//! `!== undefined`), or targets an `IF` neuron. Every output keeps at least one
//! inbound synapse: when an output would otherwise have none, the first tiny
//! inbound edge in order survives. Non-finite weights are never removed.
//!
//! Neurons are never removed. Hidden neurons stranded by the pruning are left to
//! [`crate::prune_cleanup::cleanup_creature`], the only rewrite owner for that
//! cascade. Weights are never rewritten.
//!
//! What this pass does not own, or does differently:
//!
//! - **frozen** flags — [`crate::creature::SynapseExport`] has no such field, so
//!   the caller names the frozen synapses by index into `creature.synapses`;
//! - synapse **tags** — [`crate::creature::CreatureExport`] does not carry them,
//!   so tag merging stays on the TypeScript adapter;
//! - wholesale memetic deletion — a removed edge's memetic row is pruned with
//!   [`CreatureExport::prune_memetic`], and the record itself stays, matching
//!   the rest of this crate rather than TypeScript's `delete memetic`;
//! - a `NaN` threshold — TypeScript would then fail every `abs(w) >= NaN` test
//!   and prune every finite unpinned edge, so it is refused here with
//!   [`AggressivePruneError::NanThreshold`].

use std::collections::{HashMap, HashSet};

use crate::creature::{CreatureExport, SynapseExport};

/// Default structural threshold below which a weight is judged low-impact.
///
/// Matches TypeScript `AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD`.
pub const AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD: f64 = 1e-3;

/// Result of [`aggressive_prune`].
#[derive(Debug, Clone, PartialEq)]
pub struct AggressivePrune {
    /// The creature after the pass. Equal to the input when nothing was removed.
    pub creature: CreatureExport,
    /// Number of synapses removed.
    pub removed_synapses: usize,
}

impl AggressivePrune {
    /// `true` when at least one synapse was removed.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.removed_synapses > 0
    }
}

/// Why [`aggressive_prune`] refused its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggressivePruneError {
    /// The weight threshold was `NaN`, which would prune every finite unpinned
    /// synapse rather than the low-impact ones.
    NanThreshold,
    /// A frozen index named a synapse the creature does not have.
    FrozenIndexOutOfRange {
        /// The offending index.
        index: usize,
        /// How many synapses the creature has.
        synapses: usize,
    },
}

impl std::fmt::Display for AggressivePruneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AggressivePruneError::NanThreshold => {
                write!(f, "Aggressive prune weight threshold is NaN")
            }
            AggressivePruneError::FrozenIndexOutOfRange { index, synapses } => write!(
                f,
                "Frozen synapse index {index} is out of range for {synapses} synapses"
            ),
        }
    }
}

impl std::error::Error for AggressivePruneError {}

/// Speculatively drop low-impact synapses, keeping the TypeScript guards.
///
/// `frozen_synapses` holds indices into `creature.synapses` of synapses the
/// caller knows are frozen; duplicates are fine. An index at or past the
/// synapse count is [`AggressivePruneError::FrozenIndexOutOfRange`].
///
/// A synapse is pinned (never removed) when it is frozen, has any role, or
/// targets an `IF` neuron. Of the rest, a finite weight with
/// `|weight| < weight_threshold` is removed, except that the first such edge
/// into an output stays when the output would otherwise keep no inbound.
/// Thresholds other than `NaN` (negative, zero, infinite) follow the plain
/// comparisons.
///
/// The surviving synapses keep their order. On a no-op the returned creature
/// compares equal to the input, memetic record included. The input is never
/// modified.
///
/// # Errors
///
/// [`AggressivePruneError::NanThreshold`] for a `NaN` threshold and
/// [`AggressivePruneError::FrozenIndexOutOfRange`] for a bad frozen index.
///
/// # Examples
///
/// ```
/// use neat_core::{
///     AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, CreatureExport, NeuronExport, SynapseExport,
///     aggressive_prune,
/// };
///
/// let creature = CreatureExport {
///     input: 2,
///     output: 1,
///     neurons: vec![NeuronExport {
///         id: None,
///         neuron_type: "output".to_string(),
///         uuid: "output-0".to_string(),
///         bias: 0.0,
///         squash: Some("IDENTITY".to_string()),
///     }],
///     synapses: vec![
///         SynapseExport {
///             from_uuid: "input-0".to_string(),
///             to_uuid: "output-0".to_string(),
///             weight: 1.0,
///             synapse_type: None,
///         },
///         SynapseExport {
///             from_uuid: "input-1".to_string(),
///             to_uuid: "output-0".to_string(),
///             weight: 1e-6,
///             synapse_type: None,
///         },
///     ],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
/// let pruned = aggressive_prune(&creature, AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, &[]).unwrap();
/// assert_eq!(pruned.removed_synapses, 1);
/// assert_eq!(pruned.creature.synapses.len(), 1);
/// assert_eq!(pruned.creature.synapses[0].weight, 1.0);
/// ```
pub fn aggressive_prune(
    creature: &CreatureExport,
    weight_threshold: f64,
    frozen_synapses: &[usize],
) -> Result<AggressivePrune, AggressivePruneError> {
    if weight_threshold.is_nan() {
        return Err(AggressivePruneError::NanThreshold);
    }
    let synapse_count = creature.synapses.len();
    if let Some(&index) = frozen_synapses.iter().find(|&&i| i >= synapse_count) {
        return Err(AggressivePruneError::FrozenIndexOutOfRange {
            index,
            synapses: synapse_count,
        });
    }
    let frozen: HashSet<usize> = frozen_synapses.iter().copied().collect();

    let if_targets: HashSet<&str> = creature
        .neurons
        .iter()
        .filter(|neuron| neuron.squash.as_deref() == Some("IF"))
        .map(|neuron| neuron.uuid.as_str())
        .collect();
    let outputs: HashSet<&str> = creature
        .neurons
        .iter()
        .filter(|neuron| neuron.neuron_type == "output")
        .map(|neuron| neuron.uuid.as_str())
        .collect();

    let pinned = |index: usize, synapse: &SynapseExport| {
        frozen.contains(&index)
            || synapse.synapse_type.is_some()
            || if_targets.contains(synapse.to_uuid.as_str())
    };

    let mut kept_inbound: HashMap<&str, usize> = HashMap::new();
    for (index, synapse) in creature.synapses.iter().enumerate() {
        if !outputs.contains(synapse.to_uuid.as_str()) {
            continue;
        }
        if pinned(index, synapse) || synapse.weight.abs() >= weight_threshold {
            *kept_inbound.entry(synapse.to_uuid.as_str()).or_insert(0) += 1;
        }
    }

    let mut kept = Vec::with_capacity(synapse_count);
    let mut removed_synapses = 0usize;
    for (index, synapse) in creature.synapses.iter().enumerate() {
        let keep = pinned(index, synapse)
            || !synapse.weight.is_finite()
            || synapse.weight.abs() >= weight_threshold
            || (outputs.contains(synapse.to_uuid.as_str()) && {
                let count = kept_inbound.entry(synapse.to_uuid.as_str()).or_insert(0);
                if *count == 0 {
                    *count = 1;
                    true
                } else {
                    false
                }
            });
        if keep {
            kept.push(synapse.clone());
        } else {
            removed_synapses += 1;
        }
    }

    if removed_synapses == 0 {
        return Ok(AggressivePrune {
            creature: creature.clone(),
            removed_synapses,
        });
    }

    let mut pruned = creature.clone();
    pruned.synapses = kept;
    pruned.prune_memetic();
    Ok(AggressivePrune {
        creature: pruned,
        removed_synapses,
    })
}
