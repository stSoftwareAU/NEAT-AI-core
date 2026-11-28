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
//! That holds for every target the function accepts, because every squash
//! whose result is not `squash(sum + bias)` is refused (see below).
//!
//! The TypeScript guards are kept:
//!
//! - every new bias is computed first and, if any is non-finite, nothing is
//!   applied and the creature is left unchanged (`Ok(None)`, TypeScript's
//!   `false`);
//! - a target left with fewer than two inbound synapses gets a zero-weight
//!   synapse from the input `floor(u * input)`, where `u` is the caller's
//!   uniform draw (TypeScript's `Math.random`), taken once per such target in
//!   target order, and inserted where TypeScript's `(from, to)` index order
//!   puts it;
//! - the memetic record is cleared on removal, as `removeHiddenNeuron` does.
//!
//! What stays with the caller, or is refused here:
//!
//! - collecting the trace statistics, the shuffle and the newborn-grace tag are
//!   caller policy. The choice of neuron is ported as
//!   [`crate::compact_unused_choice::choose_unused_neuron`]; this function only
//!   rewrites;
//! - the **constant branch**: when any outward target is an `IF`, `MINIMUM`,
//!   `MAXIMUM`, `MEAN`, `HYPOT` or `HYPOTv2` neuron (the six squashes that
//!   define `propagate` at the frozen revision), TypeScript rewires through a
//!   reusable constant neuron keyed on the
//!   `compact-unity-slot` neuron tag.
//!   [`crate::creature::NeuronExport`] carries no tags, so this function still
//!   refuses that branch with [`RemoveNeuronError::ConstantBranchNotPorted`]
//!   rather than approximating it. The branch is ported in
//!   [`crate::compact_unity_constant::remove_neuron`], which takes the caller's
//!   slot tags and calls this function for every other case.

use std::collections::{HashMap, HashSet};

use crate::creature::{CreatureExport, SynapseExport, parse_squash_name, validate_creature_width};
use crate::squash::SquashType;

/// Why [`remove_neuron_by_bias_fold`] refused its arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoveNeuronError {
    /// The creature fails [`validate_creature_width`]; with no inputs a
    /// reconnect would name an input that does not exist.
    InvalidWidth {
        /// The width rule's message.
        reason: String,
    },
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
    /// An outward target needs TypeScript's constant-neuron branch. This
    /// function refuses it, because that branch is keyed on neuron tags core
    /// does not carry; [`crate::compact_unity_constant::remove_neuron`] runs it
    /// with the caller's slot tags.
    ConstantBranchNotPorted {
        /// The target whose squash needs that branch.
        target: String,
    },
    /// An outward synapse names a target that is not in `creature.neurons`.
    DanglingTarget {
        /// The missing target UUID.
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
            RemoveNeuronError::InvalidWidth { reason } => write!(f, "{reason}"),
            RemoveNeuronError::UnknownNeuron { uuid } => {
                write!(f, "No neuron with UUID {uuid}")
            }
            RemoveNeuronError::NotRemovable { uuid, neuron_type } => write!(
                f,
                "Neuron {uuid} is of type {neuron_type}; only hidden or constant neurons are removed"
            ),
            RemoveNeuronError::ConstantBranchNotPorted { target } => write!(
                f,
                "Target {target} needs the constant-neuron branch of removeNeuron, which remove_neuron_by_bias_fold does not take; use remove_neuron"
            ),
            RemoveNeuronError::DanglingTarget { target } => {
                write!(f, "Outward synapse targets unknown neuron {target}")
            }
            RemoveNeuronError::DrawOutOfRange { draw } => {
                write!(f, "Reconnect draw {draw} is outside [0, 1)")
            }
        }
    }
}

impl std::error::Error for RemoveNeuronError {}

/// Whether `removeNeuron` sends a target with this squash down the constant
/// branch: it does for every squash whose TypeScript activation defines
/// `propagate`. Those are exactly the aggregates, so the set is read from
/// [`SquashType::is_aggregate`], the single home of that rule (Issue #446),
/// rather than restated here: a seventh aggregate is refused without an edit.
/// `neat-core/tests/golden/compact_legacy_metadata.json` records TypeScript
/// taking that branch for each of the six at the frozen revision, and
/// `neat-core/tests/compact_remove_neuron.rs` sweeps every aggregate. A name
/// core cannot parse is not an aggregate.
fn needs_constant_branch(squash: Option<&str>) -> bool {
    squash
        .and_then(|name| parse_squash_name(name).ok())
        .is_some_and(SquashType::is_aggregate)
}

/// Remove `uuid`, folding `activation` into the biases of its targets.
///
/// `draw` supplies one uniform value in `[0, 1)` for each target that would be
/// left with fewer than two inbound synapses; it is not called otherwise.
///
/// The synapses are expected in TypeScript's `(from, to)` runtime index order,
/// as every TypeScript export lists them: the reconnect edge is inserted at the
/// slot that order gives it, and the rest keep their order. TypeScript
/// re-sorts the whole list; for an already sorted list the two agree.
///
/// Returns `Ok(Some(creature))` after a removal and `Ok(None)` when a folded
/// bias would be non-finite, in which case nothing changed. The input is never
/// modified.
///
/// # Errors
///
/// [`RemoveNeuronError::InvalidWidth`] when [`validate_creature_width`]
/// refuses the creature, [`RemoveNeuronError::UnknownNeuron`],
/// [`RemoveNeuronError::NotRemovable`]
/// for an input or output, [`RemoveNeuronError::DanglingTarget`] for an
/// outward synapse into no listed neuron,
/// [`RemoveNeuronError::ConstantBranchNotPorted`] for an
/// `IF`/`MINIMUM`/`MAXIMUM`/`MEAN`/`HYPOT`/`HYPOTv2` target, and
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
    validate_creature_width(creature).map_err(|error| RemoveNeuronError::InvalidWidth {
        reason: error.to_string(),
    })?;
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
    // Runtime index: inputs first, then the listed neurons in order.
    let index_of = |uuid: &str| {
        uuid.strip_prefix("input-")
            .and_then(|i| i.parse::<usize>().ok())
            .filter(|&i| i < creature.input)
            .or_else(|| position.get(uuid).map(|&p| creature.input + p))
    };

    // TypeScript walks `outwardConnections`, which the runtime keeps sorted by
    // target index; inputs never receive synapses, so neuron order is enough.
    let mut outward: Vec<(usize, &SynapseExport)> = Vec::new();
    for synapse in creature.synapses.iter().filter(|s| s.from_uuid == uuid) {
        let Some(&target) = position.get(synapse.to_uuid.as_str()) else {
            return Err(RemoveNeuronError::DanglingTarget {
                target: synapse.to_uuid.clone(),
            });
        };
        outward.push((target, synapse));
    }
    outward.sort_by_key(|&(target, _)| target);

    if let Some(&(_, synapse)) = outward
        .iter()
        .find(|&&(target, _)| needs_constant_branch(creature.neurons[target].squash.as_deref()))
    {
        return Err(RemoveNeuronError::ConstantBranchNotPorted {
            target: synapse.to_uuid.clone(),
        });
    }

    let mut adjustments = Vec::with_capacity(outward.len());
    for &(target, synapse) in &outward {
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

    for &(target, synapse) in &outward {
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
            let key = (input, creature.input + target);
            let at = result
                .synapses
                .iter()
                .position(|existing| {
                    match (index_of(&existing.from_uuid), index_of(&existing.to_uuid)) {
                        (Some(from), Some(to)) => (from, to) > key,
                        _ => false,
                    }
                })
                .unwrap_or(result.synapses.len());
            result.synapses.insert(
                at,
                SynapseExport {
                    from_uuid: format!("input-{input}"),
                    to_uuid: synapse.to_uuid.clone(),
                    weight: 0.0,
                    synapse_type: None,
                },
            );
        }
    }

    remove_with_orphan_cascade(&mut result, uuid);
    result.memetic = None;
    Ok(Some(result))
}

/// `removeHiddenNeuron`: drop the neuron and every synapse touching it, then
/// remove, recursively, each hidden or constant feeder left with no outward
/// synapse.
pub(crate) fn remove_with_orphan_cascade(creature: &mut CreatureExport, uuid: &str) {
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
