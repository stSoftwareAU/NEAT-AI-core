//! Exact parallel-bridge neuron merging (Issue #777).
//!
//! Ports NEAT-AI `mergeParallelBridges` (`src/compact/ParallelBridgeMerge.ts`,
//! pinned revision `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`) into this crate.
//! A *bridge* is a hidden neuron with exactly one inbound and one outbound
//! synapse. Several bridges with the same mergeable squash that feed the same
//! target through the same synapse type collapse into one `IDENTITY` neuron
//! without changing a summed forward pass.
//!
//! # The maths
//!
//! `IDENTITY` (`f(x) = x`) is already linear:
//!
//! - `w_merged_i = w_out_i * w_in_i`
//! - `bias_merged = sum(w_out_i * bias_i)`
//!
//! `COMPLEMENT` (`f(x) = 1 - x`) is affine and converts to `IDENTITY` first:
//! `1 - (w*x + b) = (-w)*x + (1 - b)`, so `w_in' = -w_in` and
//! `bias' = 1 - bias`. The standard `IDENTITY` merge then applies. Every
//! operation is `f64`, in the same order as the TypeScript, so the result is
//! bit-identical to the frozen captures.
//!
//! # Ordering (NEAT-AI Issue #3809)
//!
//! Activation order is inputs first, then `neurons` in array order. The merge
//! rewires every member's inbound synapse onto the kept neuron, so the kept
//! neuron is the member **latest** in that order, and only bridges whose own
//! edges already run forward (`source < bridge < target`) are candidates.
//! Anything else would turn a forward edge into a recurrent one.
//!
//! # Refusals
//!
//! - **Aggregate targets.** Folding two synapses into one is lossless only
//!   where the target adds its inputs. A target whose squash is in
//!   [`crate::squash::SquashType::is_aggregate`] (this crate's one aggregate
//!   set) is refused.
//! - A group with two members fed by the same source is refused, because the
//!   merge would create duplicate synapses onto the kept neuron.
//! - Bridges with different squash or different outbound synapse type never
//!   share a group (an `IF` target reads its roles as separate sums).
//!
//! # Deliberate departures from the TypeScript (stricter only)
//!
//! 1. TypeScript's aggregation list lacks `MEAN`. A `MEAN` target averages its
//!    inputs, so replacing two synapses by one is not exact; core refuses it
//!    because `is_aggregate` includes `MEAN`.
//! 2. A target whose squash name core cannot parse is refused: the pass cannot
//!    prove such a target sums its inputs. TypeScript treats a missing squash
//!    as non-aggregate, and so does this port (`None` is accepted).
//!
//! No capture exercises either departure; both only decline a merge.
//!
//! # What this pass does not own
//!
//! - **Tags.** [`crate::creature::CreatureExport`] does not carry them, so
//!   `mergeTagsByNameValue` stays on the TypeScript wire adapter.
//! - **One group per call.** Like the TypeScript, the first mergeable group is
//!   merged and the pass stops; callers loop to a fixed point.
//! - There is no `frozen` guard in the TypeScript rule, so none is added.
//! - **Memetic data.** TypeScript does not touch it. Here the result goes
//!   through [`CreatureExport::prune_memetic`], so rows naming a removed
//!   neuron or edge go and the record stays, as in
//!   [`crate::compact_zero_weight`]. Rows for the kept neuron's rewritten
//!   weights are kept as `prune_memetic` defines, not recomputed.

use std::collections::HashMap;
use std::fmt;

use crate::creature::{CreatureExport, parse_squash_name};

/// Result of [`merge_parallel_bridges`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBridgeMerge {
    /// The creature after the pass. Equal to the input when nothing merged.
    pub creature: CreatureExport,
    /// Number of bridge neurons removed by the merge.
    pub removed_neurons: usize,
}

/// A merge that would produce a non-finite number.
///
/// TypeScript asserts finiteness mid-merge; here the input is never mutated,
/// so the error leaves nothing half-written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParallelBridgeMergeError {
    /// A merged or redirected synapse weight overflowed.
    NonFiniteWeight {
        /// UUID of the bridge whose weight was not finite.
        neuron_uuid: String,
    },
    /// The kept neuron's merged bias was not finite.
    NonFiniteBias {
        /// UUID of the kept bridge.
        neuron_uuid: String,
    },
}

impl fmt::Display for ParallelBridgeMergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteWeight { neuron_uuid } => write!(
                f,
                "parallel bridge merge would give bridge {neuron_uuid} a non-finite weight"
            ),
            Self::NonFiniteBias { neuron_uuid } => write!(
                f,
                "parallel bridge merge would give bridge {neuron_uuid} a non-finite bias"
            ),
        }
    }
}

impl std::error::Error for ParallelBridgeMergeError {}

/// Merge the first group of parallel bridge neurons, if any group qualifies.
///
/// Never mutates `creature`. When no group merges, the returned creature is a
/// clone of the input and `removed_neurons` is `0`.
///
/// # Errors
///
/// [`ParallelBridgeMergeError`] when a merged weight or bias is not finite.
///
/// # Examples
///
/// ```
/// use neat_core::{CreatureExport, NeuronExport, SynapseExport, merge_parallel_bridges};
///
/// let neuron = |kind: &str, uuid: &str, bias: f64| NeuronExport {
///     id: None,
///     neuron_type: kind.to_string(),
///     uuid: uuid.to_string(),
///     bias,
///     squash: Some("IDENTITY".to_string()),
/// };
/// let edge = |from: &str, to: &str, weight: f64| SynapseExport {
///     from_uuid: from.to_string(),
///     to_uuid: to.to_string(),
///     weight,
///     synapse_type: None,
/// };
/// let creature = CreatureExport {
///     input: 2,
///     output: 1,
///     neurons: vec![
///         neuron("hidden", "h0", 0.5),
///         neuron("hidden", "h1", 0.25),
///         neuron("output", "output-0", 0.0),
///     ],
///     synapses: vec![
///         edge("input-0", "h0", 2.0),
///         edge("h0", "output-0", 3.0),
///         edge("input-1", "h1", 4.0),
///         edge("h1", "output-0", 0.5),
///     ],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
/// let merged = merge_parallel_bridges(&creature).unwrap();
/// assert_eq!(merged.removed_neurons, 1);
/// // h1 is latest, so it is kept: bias = 0.5 * 0.25 + 3.0 * 0.5 = 1.625.
/// assert_eq!(merged.creature.neurons[0].uuid, "h1");
/// assert_eq!(merged.creature.neurons[0].bias, 1.625);
/// // input-0 is redirected onto h1 with weight 3.0 * 2.0; h1's own inbound
/// // absorbs its outbound weight, 0.5 * 4.0.
/// assert_eq!(merged.creature.synapses[0].to_uuid, "h1");
/// assert_eq!(merged.creature.synapses[0].weight, 6.0);
/// assert_eq!(merged.creature.synapses[1].weight, 2.0);
/// assert_eq!(merged.creature.synapses[2].weight, 1.0);
/// ```
pub fn merge_parallel_bridges(
    creature: &CreatureExport,
) -> Result<ParallelBridgeMerge, ParallelBridgeMergeError> {
    let unchanged = || ParallelBridgeMerge {
        creature: creature.clone(),
        removed_neurons: 0,
    };

    let mut inward: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut outward: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, synapse) in creature.synapses.iter().enumerate() {
        outward
            .entry(synapse.from_uuid.as_str())
            .or_default()
            .push(index);
        inward
            .entry(synapse.to_uuid.as_str())
            .or_default()
            .push(index);
    }

    // Activation order: inputs at 0..input, then neurons in array order.
    let input_labels: Vec<String> = (0..creature.input).map(|i| format!("input-{i}")).collect();
    let mut position: HashMap<&str, usize> = HashMap::new();
    for (i, label) in input_labels.iter().enumerate() {
        position.insert(label.as_str(), i);
    }
    let mut neuron_index: HashMap<&str, usize> = HashMap::new();
    for (k, neuron) in creature.neurons.iter().enumerate() {
        position.insert(neuron.uuid.as_str(), creature.input + k);
        neuron_index.insert(neuron.uuid.as_str(), k);
    }

    let mut candidates: Vec<usize> = Vec::new();
    for (k, neuron) in creature.neurons.iter().enumerate() {
        if neuron.neuron_type != "hidden" {
            continue;
        }
        if !matches!(neuron.squash.as_deref(), Some("IDENTITY" | "COMPLEMENT")) {
            continue;
        }
        let (Some(ins), Some(outs)) = (
            inward.get(neuron.uuid.as_str()),
            outward.get(neuron.uuid.as_str()),
        ) else {
            continue;
        };
        if ins.len() != 1 || outs.len() != 1 {
            continue;
        }
        let in_syn = &creature.synapses[ins[0]];
        let out_syn = &creature.synapses[outs[0]];
        if in_syn.from_uuid == neuron.uuid || out_syn.to_uuid == neuron.uuid {
            continue;
        }
        let (Some(&bridge_pos), Some(&source_pos), Some(&target_pos)) = (
            position.get(neuron.uuid.as_str()),
            position.get(in_syn.from_uuid.as_str()),
            position.get(out_syn.to_uuid.as_str()),
        ) else {
            continue;
        };
        if source_pos >= bridge_pos || bridge_pos >= target_pos {
            continue;
        }
        let Some(&target_k) = neuron_index.get(out_syn.to_uuid.as_str()) else {
            continue;
        };
        if target_is_not_a_plain_sum(creature.neurons[target_k].squash.as_deref()) {
            continue;
        }
        candidates.push(k);
    }

    if candidates.len() < 2 {
        return Ok(unchanged());
    }

    // Group by (target, squash, outbound synapse type), first-insertion order.
    type Key<'a> = (&'a str, &'a str, &'a str);
    let mut groups: Vec<(Key<'_>, Vec<usize>)> = Vec::new();
    for &k in &candidates {
        let neuron = &creature.neurons[k];
        let out_syn = &creature.synapses[outward[neuron.uuid.as_str()][0]];
        let key: Key<'_> = (
            out_syn.to_uuid.as_str(),
            neuron.squash.as_deref().unwrap_or(""),
            out_syn.synapse_type.as_deref().unwrap_or(""),
        );
        match groups.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, members)) => members.push(k),
            None => groups.push((key, vec![k])),
        }
    }

    for (_, members) in &groups {
        if members.len() < 2 {
            continue;
        }
        let mut sources: Vec<&str> = Vec::with_capacity(members.len());
        let mut duplicate = false;
        for &k in members {
            let from = creature.synapses[inward[creature.neurons[k].uuid.as_str()][0]]
                .from_uuid
                .as_str();
            if sources.contains(&from) {
                duplicate = true;
                break;
            }
            sources.push(from);
        }
        if duplicate {
            continue;
        }
        return merge_group(creature, members, &inward, &outward, &position);
    }

    Ok(unchanged())
}

/// `true` when folding two synapses into one would not be exact for a target
/// with this squash: an aggregate (including `MEAN`) or a name core cannot
/// parse. A missing squash is accepted, as in TypeScript.
fn target_is_not_a_plain_sum(squash: Option<&str>) -> bool {
    match squash {
        None => false,
        Some(name) => match parse_squash_name(name) {
            Ok(parsed) => parsed.is_aggregate(),
            Err(_) => true,
        },
    }
}

fn merge_group(
    creature: &CreatureExport,
    members: &[usize],
    inward: &HashMap<&str, Vec<usize>>,
    outward: &HashMap<&str, Vec<usize>>,
    position: &HashMap<&str, usize>,
) -> Result<ParallelBridgeMerge, ParallelBridgeMergeError> {
    let mut merged = creature.clone();
    let in_of = |k: usize| inward[creature.neurons[k].uuid.as_str()][0];
    let out_of = |k: usize| outward[creature.neurons[k].uuid.as_str()][0];

    // convertToIdentity: 1 - (w*x + b) = (-w)*x + (1 - b).
    for &k in members {
        if merged.neurons[k].squash.as_deref() == Some("COMPLEMENT") {
            let syn = &mut merged.synapses[in_of(k)];
            syn.weight = -syn.weight;
            merged.neurons[k].bias = 1.0 - merged.neurons[k].bias;
            merged.neurons[k].squash = Some("IDENTITY".to_string());
        }
    }

    // Keep the member latest in activation order (strictly greater wins).
    let pos_of = |k: usize| position[creature.neurons[k].uuid.as_str()];
    let mut kept = members[0];
    for &k in members {
        if pos_of(k) > pos_of(kept) {
            kept = k;
        }
    }
    let removed: Vec<usize> = members.iter().copied().filter(|&k| k != kept).collect();
    let kept_uuid = creature.neurons[kept].uuid.clone();

    let kept_out_weight = merged.synapses[out_of(kept)].weight;
    let mut merged_bias = kept_out_weight * merged.neurons[kept].bias;
    for &k in &removed {
        merged_bias += merged.synapses[out_of(k)].weight * merged.neurons[k].bias;
    }

    let new_kept_in = kept_out_weight * merged.synapses[in_of(kept)].weight;
    if !new_kept_in.is_finite() {
        return Err(ParallelBridgeMergeError::NonFiniteWeight {
            neuron_uuid: kept_uuid,
        });
    }
    merged.synapses[in_of(kept)].weight = new_kept_in;
    merged.synapses[out_of(kept)].weight = 1.0;
    merged.neurons[kept].bias = merged_bias;
    if !merged_bias.is_finite() {
        return Err(ParallelBridgeMergeError::NonFiniteBias {
            neuron_uuid: kept_uuid,
        });
    }

    for &k in &removed {
        let weight = merged.synapses[out_of(k)].weight * merged.synapses[in_of(k)].weight;
        if !weight.is_finite() {
            return Err(ParallelBridgeMergeError::NonFiniteWeight {
                neuron_uuid: creature.neurons[k].uuid.clone(),
            });
        }
        // Holds by construction: every candidate's source precedes its own
        // bridge, and the kept neuron is at or after every member.
        debug_assert!(
            position[creature.synapses[in_of(k)].from_uuid.as_str()] < pos_of(kept),
            "redirected synapse must stay forward"
        );
        let syn = &mut merged.synapses[in_of(k)];
        syn.to_uuid.clone_from(&kept_uuid);
        syn.weight = weight;
    }

    let removed_uuids: Vec<&str> = removed
        .iter()
        .map(|&k| creature.neurons[k].uuid.as_str())
        .collect();
    merged
        .synapses
        .retain(|s| !removed_uuids.contains(&s.from_uuid.as_str()));
    merged
        .neurons
        .retain(|n| !removed_uuids.contains(&n.uuid.as_str()));
    merged.prune_memetic();

    Ok(ParallelBridgeMerge {
        creature: merged,
        removed_neurons: removed.len(),
    })
}
