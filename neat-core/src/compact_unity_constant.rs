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
//! neuron's output, which a trace mean does not promise. A reuse of a held slot
//! is not exact even then: setting a holder's bias to `1.0`, and merging a second
//! holder into the first, change what the holders' other targets receive. That is
//! TypeScript's behaviour, ported as it is.
//!
//! The caller keeps these parts:
//!
//! - collecting the trace statistics, the shuffle and the newborn-grace tag that
//!   `compactUnused` chooses from. The choice itself is ported as
//!   [`crate::compact_unused_choice::choose_unused_neuron`], and its
//!   `activation` is what this function takes;
//! - writing the `compact-unity-slot` tag back. [`crate::creature::NeuronExport`]
//!   carries no tags, so [`NeuronRemoval::unity_constant`] names the constant
//!   and the caller tags it;
//! - minting the UUID, which TypeScript takes from `crypto.randomUUID`.
//!
//! Departures from TypeScript are deliberate, and each is listed here:
//!
//! - **A held slot is reused only in the sound case.** When a constant already
//!   holds slot 0, tagged or with the legacy id `-1000`, TypeScript's
//!   `createConstantOne` sets that constant's bias to `1.0` and then returns a
//!   constant it has just removed, with a stale index. The index names the removed
//!   neuron only when the holder is one of the leading constants and the removed
//!   neuron sits at the insertion point. `removeNeuron` then finds that neuron
//!   already wired to its targets and asks for slot 1, and then slot 2. The port
//!   takes that case, as `compact_constant_second` and `compact_keep_order_second`
//!   capture it in `golden/compact_legacy_unused.json`. It sets the holder's bias to
//!   `1.0`, as TypeScript does, and the new constant takes slot 1. When slot 1 is
//!   held too, by a second leading constant (tagged or legacy id `-1001`), that
//!   holder is reused the same way and the new constant takes slot 2, as
//!   `unity_reuse_slot_one_held` captures in `golden/compact_legacy_unity.json`.
//!   When a second leading constant holds the same slot by tag, TypeScript's
//!   dedupe loop re-sources its outward synapses onto the first holder, removes
//!   it and stops, and the new constant takes that slot fresh. The port takes that
//!   merge for slots 0, 1 and 2, as `unity_reuse_second_slot_zero_holder`,
//!   `unity_reuse_third_slot_zero_holder`, `unity_reuse_second_slot_one_holder`
//!   and `unity_reuse_second_slot_two_holder` capture. A further holder, a
//!   holder matched by its legacy id alone (the loop
//!   matches tags only), and a tagged holder behind the insertion point are left
//!   as they are, as the loop leaves them. Every other held-slot shape is refused
//!   with [`NeuronRemovalError::UnitySlotReuseNotPorted`] before any change:
//!   - the only holder does not lead the list. TypeScript's `Creature.fromJSON`
//!     puts the constants first, so `compactUnused` never hands `removeNeuron`
//!     this shape (`unity_reuse_holder_behind`). The port reads the order it is
//!     given;
//!   - the stale index names another neuron (the defect in
//!     `unity_reuse_tagged_slot`, `docs/research/compaction-unity-constant-baseline.md`);
//!   - a merge that would duplicate one of the first holder's synapses, or would
//!     remove a holder that has an inward synapse, so that its feeder could
//!     cascade;
//!   - one constant holding slots 0 and 1, whose slot-0 tag TypeScript would
//!     overwrite, or slots 1 and 2 (for example slot 1 by legacy id -1001 and
//!     slot 2 by tag), whose slot-1 tag it would overwrite and for which it
//!     returns `false`. This is refused before any slot-2 merge;
//!   - slot 2 is held by a single constant. TypeScript treats the removed neuron as
//!     its own constant and returns `false` after setting every holder's bias to 1
//!     (`unity_reuse_slot_two_held`). With two tagged leading slot-2 holders the
//!     dedupe merge applies instead and `removeNeuron` succeeds, as captured;
//!   - the removed constant itself leads the list while a merge applies: it may be
//!     one of the holders, and no capture backs the result.
//!
//!   The caller writes the slot tag onto a holder matched by its legacy id, as
//!   TypeScript's `addTag` does.
//! - **The reuse path mints one UUID, where TypeScript may mint more.** Each
//!   `createConstantOne` call mints a temporary constant. On a stale-index reuse
//!   it is removed again, so TypeScript mints two for slot 1 and, for slot 2, one
//!   more per outward synapse, since the slot-1 call repeats once per synapse. On
//!   a merge the temporary constant survives, so it mints one for a slot-0 merge
//!   and two for a slot-1 merge. A slot-2 merge mints two, plus one per outward
//!   synapse, because the slot-1 call repeats once per synapse and each of those
//!   is stale; `unity_reuse_second_slot_two_holder` has one outward synapse, so
//!   its captured `minted` is three. The port mints only the surviving constant,
//!   so `mint_uuid` is called once.
//! - **A non-finite constant weight is refused before any change.** TypeScript
//!   inserts the constant and then throws from `connect` (`unity_overflow`). The
//!   port checks every weight first, and mints no UUID.

use std::collections::HashMap;

use crate::compact_remove_neuron::{
    RemoveNeuronError, remove_neuron_by_bias_fold, remove_with_orphan_cascade,
};
use crate::creature::{CreatureExport, NeuronExport, SynapseExport, parse_synapse_type};

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
    /// A held unity slot cannot be reused the way this removal needs, so the
    /// removal is refused. The only slot-0 or slot-1 holder does not lead the
    /// list, the removed neuron is not at the insertion point, merging a second
    /// holder would duplicate a synapse or remove a holder with an inward
    /// synapse, one constant holds slots 0 and 1 or slots 1 and 2, or a single
    /// constant holds slot 2 when slots 0 and 1 are held. The field names the
    /// constant in the way, and nothing is changed.
    UnitySlotReuseNotPorted {
        /// The constant that holds the slot in the way.
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
                "Constant {constant} holds a unity slot this removal cannot reuse, and that reuse is not ported"
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
/// [`NeuronRemovalError::UnitySlotReuseNotPorted`] when a held unity slot cannot be
/// reused (see the module docs for the one reuse that is taken);
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

/// The constant arm of `removeNeuron`, with the held-slot rule of the module docs.
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
    // `indexToInsertComputationalConstant`: after the leading constants, and never
    // into the trailing outputs.
    let at = insertion_point(creature);

    // `createConstantOne(creature, 0)` resolves the first constant that holds slot 0
    // and inserts a temporary constant at `at`. Its dedupe loop then meets the
    // slot-0 constants in list order. When a second tagged holder leads, it is
    // merged into the first and the temporary constant survives in slot 0.
    // Otherwise the temporary constant is removed again and returned with a stale
    // index, which names the removed neuron only when the holder leads the list
    // and that neuron sits at `at`. Only that shape is reused: `removeNeuron` then
    // finds the removed neuron already wired to its targets and asks for slot 1,
    // where the same two outcomes repeat, and then slot 2. Every holder TypeScript
    // resolves on the way has its bias set to 1.
    let mut reset_bias: Vec<&str> = Vec::new();
    let mut slot: u8 = 0;
    let mut merged = merge_second_holder(creature, uuid, unity_slots, 0, -1000)?;
    if merged.is_none()
        && let Some((held_at, held)) = slot_holder(&creature.neurons, unity_slots, 0, -1000)
    {
        let sound = held_at < at && creature.neurons.get(at).is_some_and(|n| n.uuid == uuid);
        if !sound {
            return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                constant: held.uuid.clone(),
            });
        }
        // One constant holding both slots would have its slot-0 tag overwritten
        // by `createConstantOne(creature, 1)`, a shape the port does not follow.
        if let Some((_, both)) = slot_holders(&creature.neurons, unity_slots, 1, -1001)
            .find(|(_, neuron)| neuron.uuid == held.uuid)
        {
            return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                constant: both.uuid.clone(),
            });
        }
        reset_bias.push(&held.uuid);
        slot = 1;
        merged = merge_second_holder(creature, uuid, unity_slots, 1, -1001)?;
        if merged.is_none()
            && let Some((one_at, one)) = slot_holder(&creature.neurons, unity_slots, 1, -1001)
        {
            // The stale index names the removed neuron again only when this holder
            // also leads the list.
            if one_at >= at {
                return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                    constant: one.uuid.clone(),
                });
            }
            // One constant holding slots 1 and 2 would have its slot-1 tag
            // overwritten by `createConstantOne(creature, 2)`; TypeScript returns
            // `false` for it, so it is refused before any slot-2 merge, as one
            // constant holding slots 0 and 1 is above.
            if let Some((_, both)) = slot_holders(&creature.neurons, unity_slots, 2, -1002)
                .find(|(_, neuron)| neuron.uuid == one.uuid)
            {
                return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                    constant: both.uuid.clone(),
                });
            }
            // With two tagged leading slot-2 holders, the slot-2 dedupe loop
            // merges them and `removeNeuron` succeeds, the survivor taking slot 2
            // fresh (`unity_reuse_second_slot_two_holder`). `merge_second_holder`
            // has already set the first holder's bias to 1.
            let merged_two = merge_second_holder(creature, uuid, unity_slots, 2, -1002)?;
            // A single slot-2 holder is refused. `removeNeuron` treats the
            // removed neuron as its own constant (`unity_reuse_slot_two_held`):
            // TypeScript then returns `false` with every holder's bias already
            // set to 1, or, for an untyped edge, folds the weight into the edge
            // it removes.
            if merged_two.is_none()
                && let Some((_, two)) = slot_holder(&creature.neurons, unity_slots, 2, -1002)
            {
                return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                    constant: two.uuid.clone(),
                });
            }
            reset_bias.push(&one.uuid);
            slot = 2;
            merged = merged_two;
        }
    }
    // A merge removed a holder, so the insertion point moves up by one.
    let original = creature;
    let creature = merged.as_ref().unwrap_or(original);
    let at = insertion_point(creature);

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

    let mut result = creature.clone();
    // TypeScript's `createConstantOne` sets each held constant's bias to 1.
    for neuron in &mut result.neurons {
        if reset_bias.contains(&neuron.uuid.as_str()) {
            neuron.bias = 1.0;
        }
    }
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
        uuid.strip_prefix("input-")
            .and_then(|i| i.parse::<usize>().ok())
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
            slot,
        });
    Ok(NeuronRemoval {
        creature: result,
        unity_constant,
    })
}

/// `indexToInsertComputationalConstant`: the position after the leading
/// constants, and never into the trailing outputs.
fn insertion_point(creature: &CreatureExport) -> usize {
    let end = creature.neurons.len().saturating_sub(creature.output);
    (0..end)
        .find(|&p| creature.neurons[p].neuron_type != "constant")
        .unwrap_or(end)
}

/// TypeScript's dedupe loop in `createConstantOne(creature, slot)`, when a second
/// tagged constant holding `slot` leads the list.
///
/// The loop meets the slot's constants in list order, tagged ones only, and the
/// temporary constant sits after the leading constants. A second tagged holder
/// that leads is therefore met before it: its outward synapses are re-sourced
/// onto the first holder, it is removed, the synapses are re-sorted, and the loop
/// stops. Any later holder is left as it is. The temporary constant survives with
/// a valid index and takes `slot`, as `unity_reuse_second_slot_zero_holder`,
/// `unity_reuse_third_slot_zero_holder`, `unity_reuse_second_slot_one_holder` and
/// `unity_reuse_second_slot_two_holder` capture. The first holder's bias is set
/// to 1, as on every reuse.
///
/// Returns `Ok(None)` when no second tagged holder leads; TypeScript then removes
/// the temporary constant again. A merge the frozen loop does not make soundly is
/// refused before any change: the second holder has an inward synapse, whose
/// removal could cascade into its feeder, or a synapse that would duplicate one of
/// the first holder's, or the removed neuron `uuid` leads the list, where it may be
/// a holder itself.
fn merge_second_holder(
    creature: &CreatureExport,
    uuid: &str,
    unity_slots: &[UnitySlot],
    slot: u8,
    legacy_id: i64,
) -> Result<Option<CreatureExport>, NeuronRemovalError> {
    let at = insertion_point(creature);
    let mut holders = slot_holders(&creature.neurons, unity_slots, slot, legacy_id);
    let Some((first_at, first)) = holders.next() else {
        return Ok(None);
    };
    // `addTag` gives the first holder the tag, so only the others need one.
    let Some((second_at, second)) = holders.find(|&(position, neuron)| {
        position < at
            && unity_slots
                .iter()
                .any(|tag| tag.slot == slot && tag.uuid == neuron.uuid)
    }) else {
        return Ok(None);
    };
    // The capture-backed merge removes a hidden neuron that sits at or after the
    // insertion point. A removed constant that leads the list may itself be a holder
    // or shift the stale index, so no frozen outcome backs it.
    if creature.neurons.iter().position(|n| n.uuid == uuid) < Some(at) {
        return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: uuid.to_string(),
        });
    }
    let fed_by_first: Vec<(&str, u8)> = creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == first.uuid)
        .map(|s| (s.to_uuid.as_str(), role_rank(s)))
        .collect();
    if creature.synapses.iter().any(|s| {
        s.to_uuid == second.uuid
            || (s.from_uuid == second.uuid
                && fed_by_first.contains(&(s.to_uuid.as_str(), role_rank(s))))
    }) {
        return Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: second.uuid.clone(),
        });
    }

    let mut merged = creature.clone();
    merged.neurons[first_at].bias = 1.0;
    for synapse in &mut merged.synapses {
        if synapse.from_uuid == second.uuid {
            synapse.from_uuid = first.uuid.clone();
        }
    }
    merged.neurons.remove(second_at);
    // `compareSynapses`: runtime `from`, then `to`, then role. The sort is stable
    // and the keys are unique once the duplicate check above has passed.
    let input = merged.input;
    let runtime: HashMap<String, usize> = merged
        .neurons
        .iter()
        .enumerate()
        .map(|(position, neuron)| (neuron.uuid.clone(), input + position))
        .collect();
    let index = |uuid: &str| -> usize {
        uuid.strip_prefix("input-")
            .and_then(|i| i.parse::<usize>().ok())
            .filter(|&i| i < input)
            .or_else(|| runtime.get(uuid).copied())
            .unwrap_or(usize::MAX)
    };
    merged
        .synapses
        .sort_by_key(|s| (index(&s.from_uuid), index(&s.to_uuid), role_rank(s)));
    Ok(Some(merged))
}

/// The first constant that holds `slot`, tagged or with `legacy_id`, with its
/// position in `neurons`. It is the search `createConstantOne` makes over the
/// creature's constants, in list order.
fn slot_holders<'a>(
    neurons: &'a [NeuronExport],
    unity_slots: &'a [UnitySlot],
    slot: u8,
    legacy_id: i64,
) -> impl Iterator<Item = (usize, &'a NeuronExport)> {
    neurons.iter().enumerate().filter(move |(_, neuron)| {
        neuron.neuron_type == "constant"
            && (neuron.id == Some(legacy_id)
                || unity_slots
                    .iter()
                    .any(|tag| tag.slot == slot && tag.uuid == neuron.uuid))
    })
}

/// The first constant that holds `slot`, which is the one TypeScript resolves.
fn slot_holder<'a>(
    neurons: &'a [NeuronExport],
    unity_slots: &'a [UnitySlot],
    slot: u8,
    legacy_id: i64,
) -> Option<(usize, &'a NeuronExport)> {
    slot_holders(neurons, unity_slots, slot, legacy_id).next()
}

/// The role rank TypeScript's `getSynapseTypeCode` gives a synapse: `standard`
/// 0, `condition` 1, `negative` 2, `positive` 3.
fn role_rank(synapse: &SynapseExport) -> u8 {
    parse_synapse_type(synapse.synapse_type.as_deref()) as u8
}

/// Whether `uuid` is the wire name of one of the `input` inputs, `input-N` with
/// `N < input`.
fn names_an_input(uuid: &str, input: usize) -> bool {
    uuid.strip_prefix("input-")
        .and_then(|i| i.parse::<usize>().ok())
        .is_some_and(|i| i < input)
}
