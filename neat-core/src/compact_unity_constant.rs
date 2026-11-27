//! Constant-branch removal of one hidden neuron, from `compactUnused` (Issue #778).
//!
//! Ports the constant arm of NEAT-AI `removeNeuron`
//! (`src/compact/CompactUnused.ts`), with the `createConstantOne` and
//! `removeHiddenNeuron` surgery it calls (`src/compact/OrphanedNeuronCleanup.ts`).
//! That arm runs when an outward target is an `IF`, `MINIMUM`, `MAXIMUM`,
//! `MEAN`, `HYPOT` or `HYPOTv2` neuron. Every other case is the bias fold of
//! [`remove_neuron_by_bias_fold`], which this function calls first and keeps.
//!
//! The constant arm re-sources every outward synapse `s` from one new constant
//! neuron, with weight `s.weight * activation` and the same role, and then
//! removes the neuron as the bias fold does. The rewrite is **approximate**, as
//! the bias fold is: it is exact only while `activation` equals the removed
//! neuron's output, which a trace mean does not promise.
//!
//! The caller keeps these parts:
//!
//! - choosing the neuron, and the trace statistics, shuffle and newborn-grace
//!   skip that `compactUnused` uses to choose it;
//! - writing the `compact-unity-slot` tag back. [`crate::creature::NeuronExport`]
//!   carries no tags, so [`NeuronRemoval::unity_constant`] names the constant
//!   and the caller tags it;
//! - minting the UUID, which TypeScript takes from `crypto.randomUUID`.
//!
//! Two departures from TypeScript are deliberate:
//!
//! - **Reusing a held slot 0 is refused.** When a constant already holds slot 0,
//!   tagged or with the legacy id `-1000`, TypeScript's `createConstantOne`
//!   returns the constant it has just removed, with a stale index, so the
//!   removal rewires through another neuron. That is the defect in
//!   `unity_reuse_tagged_slot` (`docs/research/compaction-unity-constant-baseline.md`).
//!   The port refuses it with [`NeuronRemovalError::UnitySlotReuseNotPorted`].
//! - **A non-finite constant weight is refused before any change.** TypeScript
//!   inserts the constant and then throws from `connect` (`unity_overflow`). The
//!   port checks every weight first, and mints no UUID.

use std::collections::HashMap;

use crate::compact_remove_neuron::{
    RemoveNeuronError, remove_neuron_by_bias_fold, remove_with_orphan_cascade,
};
use crate::creature::{
    CreatureExport, NeuronExport, SynapseExport, canonical_input_index, parse_synapse_type,
};

/// A constant neuron that a `compact-unity-slot` tag marks, with the slot it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitySlot {
    /// The constant's UUID.
    pub uuid: String,
    /// The slot it holds, `0..=2`.
    pub slot: u8,
}

/// The result of a removal.
#[derive(Debug, Clone, PartialEq)]
pub struct NeuronRemoval {
    /// The rewritten creature.
    pub creature: CreatureExport,
    /// The unity constant the constant branch added, or `None` when the bias
    /// fold ran or the cascade removed the new constant again. The caller
    /// writes the slot tag back, since `NeuronExport` carries no tags.
    pub unity_constant: Option<UnitySlot>,
}

/// Why [`remove_neuron`] refused its arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum NeuronRemovalError {
    /// A refusal from the validation [`remove_neuron_by_bias_fold`] shares with
    /// the constant branch: width, unknown neuron, not removable, dangling
    /// target, or a draw out of range.
    Remove(RemoveNeuronError),
    /// A `unity_slots` entry names no neuron in `creature.neurons`.
    UnknownUnitySlotNeuron {
        /// The UUID the entry names.
        uuid: String,
    },
    /// A `unity_slots` entry has a slot outside `0..=2`, which TypeScript asserts.
    InvalidUnitySlot {
        /// The UUID the entry names.
        uuid: String,
        /// The slot the entry gives.
        slot: u8,
    },
    /// Slot 0 is already held by a constant, tagged or with the legacy id
    /// `-1000`. The frozen reuse path is defective, so it is refused, not ported.
    UnitySlotReuseNotPorted {
        /// The constant that holds slot 0.
        constant: String,
    },
    /// The minted UUID already names a listed neuron or an input (`input-N`, N < input).
    ConstantUuidTaken {
        /// The UUID the caller minted.
        uuid: String,
    },
    /// `weight * activation` is not finite for an outward synapse into `target`.
    /// TypeScript's `connect` throws here, after it has inserted the constant.
    NonFiniteConstantWeight {
        /// The target of the first such synapse, in TypeScript's order.
        target: String,
    },
}

impl std::fmt::Display for NeuronRemovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NeuronRemovalError::Remove(error) => write!(f, "{error}"),
            NeuronRemovalError::UnknownUnitySlotNeuron { uuid } => write!(
                f,
                "Unity slot names neuron {uuid}, which is not in the creature"
            ),
            NeuronRemovalError::InvalidUnitySlot { uuid, slot } => {
                write!(f, "Unity slot {slot} for neuron {uuid} is outside 0..=2")
            }
            NeuronRemovalError::UnitySlotReuseNotPorted { constant } => write!(
                f,
                "Unity slot 0 is already held by constant {constant}, and reusing a held slot is not ported"
            ),
            NeuronRemovalError::ConstantUuidTaken { uuid } => write!(
                f,
                "Minted constant UUID {uuid} is already used by a neuron or an input"
            ),
            NeuronRemovalError::NonFiniteConstantWeight { target } => {
                write!(f, "The constant's weight into {target} is not finite")
            }
        }
    }
}

impl std::error::Error for NeuronRemovalError {}

/// Remove `uuid`, folding `activation` into its outward edges, and use a unity
/// constant when a target is an aggregate.
///
/// The bias fold of [`remove_neuron_by_bias_fold`] runs first. Its result is
/// returned with `unity_constant: None` whenever the bias fold accepts the
/// neuron. When its only refusal is
/// [`RemoveNeuronError::ConstantBranchNotPorted`], the constant branch runs
/// instead: a new constant `1.0` is inserted after the leading constants, every
/// outward synapse is re-sourced from it at `weight * activation`, and the
/// neuron is removed with its orphan cascade.
///
/// `unity_slots` are the caller's `compact-unity-slot` tags. Each entry must
/// name a listed neuron and hold a slot in `0..=2`. An entry on a non-constant
/// neuron is accepted and ignored, as TypeScript ignores tags on non-constants.
/// `mint_uuid` is called once, and only when the constant branch runs and
/// passes its checks. `draw` is used only by the bias fold.
///
/// The synapses are expected in TypeScript's `(from, to, role)` order, as
/// [`remove_neuron_by_bias_fold`] expects. The input is never modified.
///
/// # Errors
///
/// [`NeuronRemovalError::UnknownUnitySlotNeuron`] and
/// [`NeuronRemovalError::InvalidUnitySlot`] for a bad `unity_slots` entry;
/// [`NeuronRemovalError::Remove`] for any refusal of the shared validation,
/// including a draw outside `[0, 1)`;
/// [`NeuronRemovalError::UnitySlotReuseNotPorted`] when slot 0 is already held;
/// [`NeuronRemovalError::ConstantUuidTaken`] when `mint_uuid` returns a listed
/// neuron's UUID or an input's; and
/// [`NeuronRemovalError::NonFiniteConstantWeight`] when an outward weight times
/// `activation` is not finite. Each of these leaves the creature unchanged.
///
/// # Examples
///
/// ```
/// use neat_core::{CreatureExport, NeuronExport, SynapseExport, remove_neuron};
///
/// let neuron = |kind: &str, uuid: &str, squash: &str, bias: f64| NeuronExport {
///     id: None,
///     neuron_type: kind.to_string(),
///     uuid: uuid.to_string(),
///     bias,
///     squash: Some(squash.to_string()),
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
///     neurons: vec![
///         neuron("hidden", "hidden-0", "IDENTITY", 0.0),
///         neuron("output", "output-0", "IF", 0.25),
///     ],
///     // In TypeScript's (from, to) order: input-0 -> hidden-0, input-1 -> output-0,
///     // hidden-0 -> output-0.
///     synapses: vec![
///         synapse("input-0", "hidden-0", 1.0),
///         synapse("input-1", "output-0", 1.0),
///         synapse("hidden-0", "output-0", 2.0),
///     ],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
///
/// // `output-0` is an IF, so the constant branch runs. The new constant's weight
/// // into it is 2.0 * 1.5 = 3.0, and the constant is placed after the inputs.
/// let removal = remove_neuron(
///     &creature,
///     "hidden-0",
///     1.5,
///     &[],
///     || "unity-constant-0".to_string(),
///     || 0.0,
/// )
/// .unwrap()
/// .unwrap();
/// let constant = removal.unity_constant.unwrap();
/// assert_eq!(constant.uuid, "unity-constant-0");
/// assert_eq!(constant.slot, 0);
/// assert_eq!(removal.creature.neurons[0].uuid, "unity-constant-0");
/// assert_eq!(removal.creature.synapses[1].weight, 3.0);
/// ```
pub fn remove_neuron(
    creature: &CreatureExport,
    uuid: &str,
    activation: f64,
    unity_slots: &[UnitySlot],
    mint_uuid: impl FnOnce() -> String,
    draw: impl FnMut() -> f64,
) -> Result<Option<NeuronRemoval>, NeuronRemovalError> {
    for slot in unity_slots {
        if !creature
            .neurons
            .iter()
            .any(|neuron| neuron.uuid == slot.uuid)
        {
            return Err(NeuronRemovalError::UnknownUnitySlotNeuron {
                uuid: slot.uuid.clone(),
            });
        }
        if slot.slot > 2 {
            return Err(NeuronRemovalError::InvalidUnitySlot {
                uuid: slot.uuid.clone(),
                slot: slot.slot,
            });
        }
    }

    match remove_neuron_by_bias_fold(creature, uuid, activation, draw) {
        Ok(Some(folded)) => Ok(Some(NeuronRemoval {
            creature: folded,
            unity_constant: None,
        })),
        Ok(None) => Ok(None),
        Err(RemoveNeuronError::ConstantBranchNotPorted { .. }) => {
            constant_branch(creature, uuid, activation, unity_slots, mint_uuid).map(Some)
        }
        Err(error) => Err(NeuronRemovalError::Remove(error)),
    }
}

/// The constant arm of `removeNeuron`, without the slot-reuse path.
///
/// Called only after [`remove_neuron_by_bias_fold`] has validated `uuid` and
/// every outward target, so those lookups cannot fail for a listed neuron. The
/// one that could is still reported, not assumed.
fn constant_branch(
    creature: &CreatureExport,
    uuid: &str,
    activation: f64,
    unity_slots: &[UnitySlot],
    mint_uuid: impl FnOnce() -> String,
) -> Result<NeuronRemoval, NeuronRemovalError> {
    // `createConstantOne(creature, 0)` resolves the first constant that holds slot 0.
    let holds_slot_zero = |neuron: &NeuronExport| {
        neuron.neuron_type == "constant"
            && (neuron.id == Some(-1000)
                || unity_slots
                    .iter()
                    .any(|slot| slot.slot == 0 && slot.uuid == neuron.uuid))
    };
    if let Some(held) = creature
        .neurons
        .iter()
        .find(|&neuron| holds_slot_zero(neuron))
    {
        return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: held.uuid.clone(),
        });
    }

    let position: HashMap<&str, usize> = creature
        .neurons
        .iter()
        .enumerate()
        .map(|(position, neuron)| (neuron.uuid.as_str(), position))
        .collect();

    // TypeScript's `outwardConnections`: ordered by target runtime index, then role.
    // A stable sort keeps the original relative order for equal keys.
    let mut outward: Vec<(usize, u8, &SynapseExport)> = Vec::new();
    for synapse in creature.synapses.iter().filter(|s| s.from_uuid == uuid) {
        let Some(&target) = position.get(synapse.to_uuid.as_str()) else {
            return Err(NeuronRemovalError::Remove(
                RemoveNeuronError::DanglingTarget {
                    target: synapse.to_uuid.clone(),
                },
            ));
        };
        outward.push((target, role_rank(synapse), synapse));
    }
    outward.sort_by_key(|&(target, role, _)| (target, role));

    // Every weight is computed before anything is minted or changed, so a
    // refusal here takes no UUID and leaves the creature as it was.
    let mut weights = Vec::with_capacity(outward.len());
    for &(_, _, synapse) in &outward {
        let weight = synapse.weight * activation;
        if !weight.is_finite() {
            return Err(NeuronRemovalError::NonFiniteConstantWeight {
                target: synapse.to_uuid.clone(),
            });
        }
        weights.push(weight);
    }

    let new_uuid = mint_uuid();
    if creature
        .neurons
        .iter()
        .any(|neuron| neuron.uuid == new_uuid)
        || names_an_input(&new_uuid, creature.input)
    {
        return Err(NeuronRemovalError::ConstantUuidTaken { uuid: new_uuid });
    }

    // `indexToInsertComputationalConstant`: after the leading constants, and never
    // into the trailing outputs.
    let end = creature.neurons.len().saturating_sub(creature.output);
    let at = (0..end)
        .find(|&p| creature.neurons[p].neuron_type != "constant")
        .unwrap_or(end);

    let mut result = creature.clone();
    result.neurons.insert(
        at,
        NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: new_uuid.clone(),
            bias: 1.0,
            squash: None,
        },
    );

    // Runtime indices in the cloned creature, after the insertion. Only the
    // neuron list is borrowed here, so the synapse list can still grow.
    let input = creature.input;
    let positions: HashMap<&str, usize> = result
        .neurons
        .iter()
        .enumerate()
        .map(|(position, neuron)| (neuron.uuid.as_str(), position))
        .collect();
    let runtime = |uuid: &str| -> Option<usize> {
        canonical_input_index(uuid)
            .filter(|&i| i < input)
            .or_else(|| positions.get(uuid).map(|&p| input + p))
    };

    let constant_index = input + at;
    for (&(target, role, synapse), weight) in outward.iter().zip(weights) {
        // Listed positions at or after `at` moved one place up with the insert.
        let to_index = input + target + usize::from(target >= at);
        let key = (constant_index, to_index, role);
        // TypeScript's `connect` inserts before the first synapse whose key is
        // greater. Endpoints that do not resolve compare as not-greater.
        let slot = result
            .synapses
            .iter()
            .position(
                |existing| match (runtime(&existing.from_uuid), runtime(&existing.to_uuid)) {
                    (Some(from), Some(to)) => (from, to, role_rank(existing)) > key,
                    _ => false,
                },
            )
            .unwrap_or(result.synapses.len());
        result.synapses.insert(
            slot,
            SynapseExport {
                from_uuid: new_uuid.clone(),
                to_uuid: synapse.to_uuid.clone(),
                weight,
                synapse_type: synapse.synapse_type.clone(),
            },
        );
    }

    remove_with_orphan_cascade(&mut result, uuid);
    result.memetic = None;

    // The cascade can take the new constant again: it does when the constant fed
    // nothing but the removed neuron.
    let unity_constant = result
        .neurons
        .iter()
        .any(|neuron| neuron.uuid == new_uuid)
        .then_some(UnitySlot {
            uuid: new_uuid,
            slot: 0,
        });
    Ok(NeuronRemoval {
        creature: result,
        unity_constant,
    })
}

/// The role rank TypeScript's `getSynapseTypeCode` gives a synapse: `standard`
/// 0, `condition` 1, `negative` 2, `positive` 3.
fn role_rank(synapse: &SynapseExport) -> u8 {
    parse_synapse_type(synapse.synapse_type.as_deref()) as u8
}

/// Whether `uuid` is the wire name of one of the `input` inputs, `input-N` with
/// `N < input`.
fn names_an_input(uuid: &str, input: usize) -> bool {
    canonical_input_index(uuid).is_some_and(|i| i < input)
}
