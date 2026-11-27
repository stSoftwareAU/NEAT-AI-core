//! Exact zero-weight synapse pruning (Issue #777).
//!
//! Ports NEAT-AI `pruneZeroWeightSynapses`
//! (`src/compact/SynapsePruning.ts`) into this crate. A synapse whose weight
//! is exactly `0` (including `-0`) does not change a summed forward pass, so
//! dropping it is an algebraic no-op. The guards below are the existing
//! TypeScript ones; this pass does not invent a threshold or a new removal.
//!
//! Dead neurons that removal strands are **not** deleted here.
//! [`crate::prune_cleanup::cleanup_creature`] already removes every non-output
//! neuron with no outward edge, to a fixed point, and is the only rewrite
//! owner for that cascade. Callers that want the compact pipeline's next step
//! hand this function's creature to cleanup. A second dead-subgraph walk would
//! duplicate that algorithm.
//!
//! What this pass does not own:
//!
//! - synapse **tags** — [`crate::creature::CreatureExport`] does not carry
//!   them, so tag merging stays on the TypeScript adapter;
//! - wholesale memetic deletion — a removed edge's memetic row is pruned with
//!   [`CreatureExport::prune_memetic`], and the record itself stays, matching
//!   the rest of this crate rather than TypeScript's `delete memetic`.

use std::collections::{HashMap, HashSet};

use crate::creature::CreatureExport;

/// Result of [`prune_zero_weight_synapses`].
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroWeightPrune {
    /// The creature after the pass. Equal to the input when nothing was removed.
    pub creature: CreatureExport,
    /// Untyped exact-zero synapses removed.
    ///
    /// Does not count a zero edge that a guard kept, and does not count
    /// non-finite weights.
    pub removed_zero_synapses: usize,
    /// Non-finite weights dropped.
    ///
    /// This is the repair branch already inside TypeScript
    /// `pruneZeroWeightSynapses` (`Number.isFinite` fails, the row goes),
    /// including typed rows and the last inbound edge of an output. It is not
    /// an algebraic rewrite: a non-finite weight was never a defined forward
    /// pass.
    pub repaired_non_finite: usize,
}

impl ZeroWeightPrune {
    /// `true` when either the exact-zero rule or the non-finite repair removed
    /// a synapse.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.removed_zero_synapses > 0 || self.repaired_non_finite > 0
    }
}

/// Drop exact-zero synapses, keeping the TypeScript guards.
///
/// A synapse is removed only when its weight is finite and exactly zero, it
/// has no role (`type`), and its target is not an `IF` neuron. Two further
/// guards keep a row that would otherwise qualify:
///
/// - any role (`condition`, `positive`, `negative`, or any other non-empty
///   `type`) stays, because dropping it changes an `IF` or strips a role the
///   load-side repair would reinvent;
/// - the first exact-zero untyped inbound of an output stays when that output
///   would otherwise have no inbound left. Outputs are part of the declared
///   width and must keep a structural inbound, matching `Creature.fix()`.
///
/// Non-finite weights are removed unconditionally, before those guards. An
/// empty `type` is untyped, matching JavaScript's falsy check.
///
/// The pass is idempotent. It does not reorder surviving synapses, rewrite
/// weights, or remove neurons. On a no-op the returned creature compares equal
/// to the input, memetic record included.
///
/// # Examples
///
/// ```
/// use neat_core::{CreatureExport, NeuronExport, SynapseExport, prune_zero_weight_synapses};
///
/// let creature = CreatureExport {
///     input: 1,
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
///             from_uuid: "input-0".to_string(),
///             to_uuid: "output-0".to_string(),
///             weight: 0.0,
///             synapse_type: None,
///         },
///     ],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
/// let pruned = prune_zero_weight_synapses(&creature);
/// assert_eq!(pruned.removed_zero_synapses, 1);
/// assert_eq!(pruned.creature.synapses.len(), 1);
/// assert_eq!(pruned.creature.synapses[0].weight, 1.0);
/// ```
#[must_use]
pub fn prune_zero_weight_synapses(creature: &CreatureExport) -> ZeroWeightPrune {
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

    let mut kept_inbound: HashMap<&str, usize> = HashMap::new();
    for synapse in &creature.synapses {
        if !synapse.weight.is_finite() {
            continue;
        }
        if always_keep(synapse, &if_targets) {
            *kept_inbound.entry(synapse.to_uuid.as_str()).or_insert(0) += 1;
        }
    }

    let mut preserved_output_zero: HashSet<&str> = HashSet::new();
    let mut kept = Vec::with_capacity(creature.synapses.len());
    let mut removed_zero_synapses = 0usize;
    let mut repaired_non_finite = 0usize;

    for synapse in &creature.synapses {
        if !synapse.weight.is_finite() {
            repaired_non_finite += 1;
            continue;
        }
        if always_keep(synapse, &if_targets) {
            kept.push(synapse.clone());
            continue;
        }
        // Finite, exactly zero, untyped, and not aimed at an IF neuron.
        if outputs.contains(synapse.to_uuid.as_str()) {
            let inbound = kept_inbound
                .get(synapse.to_uuid.as_str())
                .copied()
                .unwrap_or(0);
            if inbound == 0 && preserved_output_zero.insert(synapse.to_uuid.as_str()) {
                kept.push(synapse.clone());
                continue;
            }
        }
        removed_zero_synapses += 1;
    }

    if removed_zero_synapses == 0 && repaired_non_finite == 0 {
        return ZeroWeightPrune {
            creature: creature.clone(),
            removed_zero_synapses,
            repaired_non_finite,
        };
    }

    let mut pruned = creature.clone();
    pruned.synapses = kept;
    pruned.prune_memetic();
    ZeroWeightPrune {
        creature: pruned,
        removed_zero_synapses,
        repaired_non_finite,
    }
}

/// TypeScript `shouldAlwaysKeep`: non-zero, typed, or targeting an `IF`.
///
/// The caller has already excluded non-finite weights. `-0` compares equal to
/// `0`, so it is not kept here.
fn always_keep(synapse: &crate::creature::SynapseExport, if_targets: &HashSet<&str>) -> bool {
    if synapse.weight != 0.0 {
        return true;
    }
    if is_typed(synapse) {
        return true;
    }
    if_targets.contains(synapse.to_uuid.as_str())
}

/// JavaScript treats `""` as untyped (`if (s.type)` is false). Any other
/// `type` string is a role and must stay.
fn is_typed(synapse: &crate::creature::SynapseExport) -> bool {
    matches!(synapse.synapse_type.as_deref(), Some(role) if !role.is_empty())
}
