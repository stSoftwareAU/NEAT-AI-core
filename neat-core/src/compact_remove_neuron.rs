//! Bias-fold removal of one hidden neuron, from `compactUnused` (Issue #778).
//!
//! Ports the bias-fold branch of NEAT-AI `removeNeuron`
//! (`src/compact/CompactUnused.ts`) together with the `removeHiddenNeuron`
//! surgery it ends with (`src/compact/OrphanedNeuronCleanup.ts`). The caller
//! nominates a hidden (or constant) neuron and the activation to fold in — in
//! TypeScript, the mean of the neuron's traced activations. Every outward
//! synapse `s` then moves `s.weight * activation` into its target's bias, the
//! neuron and all its synapses go, and any hidden or constant feeder left with
//! no outward synapse is removed in cascade.
//!
//! The rewrite is **approximate**: it is exact only while the removed neuron's
//! activation equals the folded constant, which a trace mean does not promise.
//!
//! The TypeScript guards are kept:
//!
//! - every new bias is computed first and, if any is non-finite, nothing is
//!   applied and the creature is left unchanged (`Ok(None)`, TypeScript's
//!   `false`);
//! - a new bias is computed from the target's bias *before* the pass, so two
//!   typed synapses into one target leave the last one's bias, as in
//!   TypeScript;
//! - a target left with fewer than two inbound synapses gets a zero-weight
//!   synapse from the input `floor(u * input)`, where `u` is the caller's
//!   uniform draw (TypeScript's `Math.random`), taken once per such target in
//!   target order;
//! - the memetic record is cleared on removal, as `removeHiddenNeuron` does.
//!
//! What stays with the caller, or is not ported:
//!
//! - choosing the neuron — `compactUnused`'s trace statistics, shuffle and
//!   newborn-grace skip are caller policy; this function only rewrites;
//! - the **constant branch**: when any outward target is an `IF`, `MINIMUM` or
//!   `MAXIMUM` neuron (the squashes with their own `propagate`), TypeScript
//!   rewires through a reusable constant neuron keyed on the
//!   `compact-unity-slot` neuron tag.
//!   [`crate::creature::NeuronExport`] carries no tags, so that branch is
//!   refused with [`RemoveNeuronError::ConstantBranchNotPorted`] rather than
//!   approximated.

use std::collections::{HashMap, HashSet};

use crate::creature::{CreatureExport, SynapseExport};

/// Why [`remove_neuron_by_bias_fold`] refused its arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoveNeuronError {
    /// No neuron in `creature.neurons` has this UUID.
    UnknownNeuron {
        /// The UUID asked for.
        uuid: String,
    },
    /// The neuron is not hidden or constant, so it cannot be removed.
    NotRemovable {
        /// The neuron's UUID.
        uuid: String,
        /// Its type.
        neuron_type: String,
    },
    /// An outward target needs TypeScript's constant-neuron branch, which is
    /// not ported because it is keyed on neuron tags core does not carry.
    ConstantBranchNotPorted {
        /// The target whose squash needs that branch.
        target: String,
    },
    /// A reconnect draw was outside `[0, 1)`, so it names no input.
    DrawOutOfRange {
        /// The draw supplied.
        draw: f64,
    },
}

impl std::fmt::Display for RemoveNeuronError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoveNeuronError::UnknownNeuron { uuid } => {
                write!(f, "No neuron with UUID {uuid}")
            }
            RemoveNeuronError::NotRemovable { uuid, neuron_type } => write!(
                f,
                "Neuron {uuid} is of type {neuron_type}; only hidden or constant neurons are removed"
            ),
            RemoveNeuronError::ConstantBranchNotPorted { target } => write!(
                f,
                "Target {target} needs the constant-neuron branch of removeNeuron, which is not ported"
            ),
            RemoveNeuronError::DrawOutOfRange { draw } => {
                write!(f, "Reconnect draw {draw} is outside [0, 1)")
            }
        }
    }
}

impl std::error::Error for RemoveNeuronError {}

/// The squashes whose TypeScript activation defines `propagate`.
fn needs_constant_branch(squash: Option<&str>) -> bool {
    matches!(squash, Some("IF" | "MINIMUM" | "MAXIMUM"))
}

/// Remove `uuid`, folding `activation` into the biases of its targets.
///
/// `draw` supplies one uniform value in `[0, 1)` for each target that would be
/// left with fewer than two inbound synapses; it is not called otherwise.
///
/// Returns `Ok(Some(creature))` after a removal and `Ok(None)` when a folded
/// bias would be non-finite, in which case nothing changed. The input is never
/// modified.
///
/// # Errors
///
/// [`RemoveNeuronError::UnknownNeuron`], [`RemoveNeuronError::NotRemovable`]
/// for an input or output, [`RemoveNeuronError::ConstantBranchNotPorted`] for
/// an `IF`/`MINIMUM`/`MAXIMUM` target, and
/// [`RemoveNeuronError::DrawOutOfRange`] for a draw outside `[0, 1)`.
///
/// # Examples
///
/// ```
/// use neat_core::{CreatureExport, NeuronExport, SynapseExport, remove_neuron_by_bias_fold};
///
/// let neuron = |kind: &str, uuid: &str, bias: f64| NeuronExport {
///     id: None,
///     neuron_type: kind.to_string(),
///     uuid: uuid.to_string(),
///     bias,
///     squash: Some("IDENTITY".to_string()),
/// };
/// let synapse = |from: &str, to: &str, weight: f64| SynapseExport {
///     from_uuid: from.to_string(),
///     to_uuid: to.to_string(),
///     weight,
///     synapse_type: None,
/// };
/// let creature = CreatureExport {
///     input: 2,
///     output: 1,
///     neurons: vec![neuron("hidden", "hidden-0", 0.1), neuron("output", "output-0", 0.5)],
///     synapses: vec![
///         synapse("input-0", "hidden-0", 1.0),
///         synapse("hidden-0", "output-0", 0.3),
///     ],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
///
/// let removed = remove_neuron_by_bias_fold(&creature, "hidden-0", 0.5, || 0.75)
///     .unwrap()
///     .unwrap();
/// // 0.5 + 0.3 * 0.5, and output-0 is reconnected from input floor(0.75 * 2) = 1.
/// assert_eq!(removed.neurons.len(), 1);
/// assert_eq!(removed.neurons[0].bias, 0.5 + 0.3 * 0.5);
/// assert_eq!(removed.synapses, vec![synapse("input-1", "output-0", 0.0)]);
/// ```
pub fn remove_neuron_by_bias_fold(
    creature: &CreatureExport,
    uuid: &str,
    activation: f64,
    mut draw: impl FnMut() -> f64,
) -> Result<Option<CreatureExport>, RemoveNeuronError> {
    let neuron = creature
        .neurons
        .iter()
        .find(|neuron| neuron.uuid == uuid)
        .ok_or_else(|| RemoveNeuronError::UnknownNeuron {
            uuid: uuid.to_string(),
        })?;
    if neuron.neuron_type != "hidden" && neuron.neuron_type != "constant" {
        return Err(RemoveNeuronError::NotRemovable {
            uuid: uuid.to_string(),
            neuron_type: neuron.neuron_type.clone(),
        });
    }

    let position: HashMap<&str, usize> = creature
        .neurons
        .iter()
        .enumerate()
        .map(|(position, neuron)| (neuron.uuid.as_str(), position))
        .collect();
    let squash_of = |target: &str| {
        position
            .get(target)
            .and_then(|&position| creature.neurons[position].squash.as_deref())
    };

    // TypeScript walks `outwardConnections`, which the runtime keeps sorted by
    // target index; inputs never receive synapses, so neuron order is enough.
    let mut outward: Vec<&SynapseExport> = creature
        .synapses
        .iter()
        .filter(|synapse| synapse.from_uuid == uuid)
        .collect();
    outward.sort_by_key(|synapse| position.get(synapse.to_uuid.as_str()).copied());

    if let Some(synapse) = outward
        .iter()
        .find(|synapse| needs_constant_branch(squash_of(&synapse.to_uuid)))
    {
        return Err(RemoveNeuronError::ConstantBranchNotPorted {
            target: synapse.to_uuid.clone(),
        });
    }

    let mut adjustments = Vec::with_capacity(outward.len());
    for synapse in &outward {
        let Some(&target) = position.get(synapse.to_uuid.as_str()) else {
            continue;
        };
        let new_bias = creature.neurons[target].bias + synapse.weight * activation;
        if !new_bias.is_finite() {
            return Ok(None);
        }
        adjustments.push((target, new_bias));
    }

    let mut result = creature.clone();
    for (target, new_bias) in adjustments {
        result.neurons[target].bias = new_bias;
    }

    for synapse in &outward {
        let inbound = result
            .synapses
            .iter()
            .filter(|existing| existing.to_uuid == synapse.to_uuid)
            .count();
        if inbound < 2 {
            let u = draw();
            if !(0.0..1.0).contains(&u) {
                return Err(RemoveNeuronError::DrawOutOfRange { draw: u });
            }
            // `u < 1` and `input >= 1` keep the floor inside `0..input`.
            let input = (u * creature.input as f64).floor() as usize;
            result.synapses.push(SynapseExport {
                from_uuid: format!("input-{input}"),
                to_uuid: synapse.to_uuid.clone(),
                weight: 0.0,
                synapse_type: None,
            });
        }
    }

    remove_with_orphan_cascade(&mut result, uuid);
    result.memetic = None;
    Ok(Some(result))
}

/// `removeHiddenNeuron`: drop the neuron and every synapse touching it, then
/// remove, recursively, each hidden or constant feeder left with no outward
/// synapse.
fn remove_with_orphan_cascade(creature: &mut CreatureExport, uuid: &str) {
    let removable: HashSet<String> = creature
        .neurons
        .iter()
        .filter(|neuron| neuron.neuron_type == "hidden" || neuron.neuron_type == "constant")
        .map(|neuron| neuron.uuid.clone())
        .collect();
    let mut feeders: Vec<String> = Vec::new();
    for synapse in &creature.synapses {
        if synapse.to_uuid == uuid
            && synapse.from_uuid != uuid
            && removable.contains(&synapse.from_uuid)
            && !feeders.contains(&synapse.from_uuid)
        {
            feeders.push(synapse.from_uuid.clone());
        }
    }

    creature.neurons.retain(|neuron| neuron.uuid != uuid);
    creature
        .synapses
        .retain(|synapse| synapse.from_uuid != uuid && synapse.to_uuid != uuid);

    for feeder in feeders {
        let still_present = creature.neurons.iter().any(|neuron| neuron.uuid == feeder);
        let has_outward = creature
            .synapses
            .iter()
            .any(|synapse| synapse.from_uuid == feeder);
        if still_present && !has_outward {
            remove_with_orphan_cascade(creature, &feeder);
        }
    }
}
