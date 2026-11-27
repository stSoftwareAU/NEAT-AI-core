//! Exact COMPLEMENT bypass (Issue #777).
//!
//! Ports the first structural step of NEAT-AI `buildSafeCompact`
//! (`src/compact/CompactCreature.ts`, pinned revision
//! `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`). One call removes at most one
//! hidden `COMPLEMENT` neuron and folds `f(x) = 1 - x` into its targets.
//!
//! For each outbound weight `w_out` into a non-aggregate target:
//!
//! - `bias_target += w_out * (1 - bias_complement)`
//! - each inbound weight `w_in` becomes `-w_out * w_in` on that target,
//!   added to an existing edge with the same source, target and role, or
//!   pushed as a new untyped edge.
//!
//! A candidate is left in place when it has no inbound or outbound edge, a
//! missing or aggregate target, or a self-loop. When the creature is
//! forward-only and feedback is off, an outbound edge that would point
//! backwards is not folded, and the neuron is still removed — that is the
//! TypeScript loop. Tags are not on `CreatureExport`; a caller that carries
//! them keeps them itself.

use crate::creature::{CreatureExport, SynapseExport, parse_squash_name};

/// Result of one COMPLEMENT bypass attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct ComplementBypass {
    /// The creature after the attempt. Unchanged when no neuron was removed.
    pub creature: CreatureExport,
    /// UUID of the hidden COMPLEMENT that was removed, when one was.
    pub removed_neuron: Option<String>,
}

impl ComplementBypass {
    /// Whether this call removed a neuron.
    pub fn changed(&self) -> bool {
        self.removed_neuron.is_some()
    }
}

/// Remove one hidden COMPLEMENT neuron, folding its edges into its targets.
///
/// `feedback_loop` is the caller's training flag. It only matters together
/// with [`CreatureExport::forward_only`]: a forward-only creature that is not
/// in a feedback loop refuses to *fold* an edge whose source sits later in
/// the neuron list than its target. Input sources are not in that list, so
/// they never trip the check.
pub fn bypass_complement(creature: &CreatureExport, feedback_loop: bool) -> ComplementBypass {
    let Some(candidate) = first_candidate(creature) else {
        return ComplementBypass {
            creature: creature.clone(),
            removed_neuron: None,
        };
    };

    let mut next = creature.clone();
    let neurons = &creature.neurons;
    let complement = &neurons[candidate];
    let inbound: Vec<SynapseExport> = creature
        .synapses
        .iter()
        .filter(|synapse| synapse.to_uuid == complement.uuid)
        .cloned()
        .collect();
    let outbound: Vec<SynapseExport> = creature
        .synapses
        .iter()
        .filter(|synapse| synapse.from_uuid == complement.uuid)
        .cloned()
        .collect();

    for out_conn in &outbound {
        if would_point_backwards(creature, feedback_loop, &inbound, out_conn) {
            continue;
        }
        if let Some(target) = next
            .neurons
            .iter_mut()
            .find(|neuron| neuron.uuid == out_conn.to_uuid)
        {
            target.bias += out_conn.weight * (1.0 - complement.bias);
        }
        for in_conn in &inbound {
            let folded = -out_conn.weight * in_conn.weight;
            if let Some(existing) = next.synapses.iter_mut().find(|synapse| {
                synapse.from_uuid == in_conn.from_uuid
                    && synapse.to_uuid == out_conn.to_uuid
                    && role(synapse) == role(in_conn)
            }) {
                existing.weight += folded;
            } else {
                next.synapses.push(SynapseExport {
                    from_uuid: in_conn.from_uuid.clone(),
                    to_uuid: out_conn.to_uuid.clone(),
                    weight: folded,
                    synapse_type: None,
                });
            }
        }
    }

    let removed = complement.uuid.clone();
    next.neurons.retain(|neuron| neuron.uuid != removed);
    next.synapses
        .retain(|synapse| synapse.from_uuid != removed && synapse.to_uuid != removed);
    next.prune_memetic();

    ComplementBypass {
        creature: next,
        removed_neuron: Some(removed),
    }
}

fn first_candidate(creature: &CreatureExport) -> Option<usize> {
    creature.neurons.iter().position(|neuron| {
        if neuron.neuron_type != "hidden" || neuron.squash.as_deref() != Some("COMPLEMENT") {
            return false;
        }
        let inbound: Vec<&SynapseExport> = creature
            .synapses
            .iter()
            .filter(|synapse| synapse.to_uuid == neuron.uuid)
            .collect();
        let outbound: Vec<&SynapseExport> = creature
            .synapses
            .iter()
            .filter(|synapse| synapse.from_uuid == neuron.uuid)
            .collect();
        if inbound.is_empty() || outbound.is_empty() {
            return false;
        }
        if outbound
            .iter()
            .any(|synapse| !target_accepts(creature, synapse))
        {
            return false;
        }
        let loops = inbound
            .iter()
            .any(|synapse| synapse.from_uuid == neuron.uuid)
            || outbound
                .iter()
                .any(|synapse| synapse.to_uuid == neuron.uuid);
        !loops
    })
}

fn target_accepts(creature: &CreatureExport, synapse: &SynapseExport) -> bool {
    let Some(target) = creature
        .neurons
        .iter()
        .find(|neuron| neuron.uuid == synapse.to_uuid)
    else {
        return false;
    };
    !is_aggregate_squash(target.squash.as_deref())
}

fn would_point_backwards(
    creature: &CreatureExport,
    feedback_loop: bool,
    inbound: &[SynapseExport],
    outbound: &SynapseExport,
) -> bool {
    if feedback_loop || !creature.forward_only {
        return false;
    }
    let by_uuid = index_by_uuid(creature);
    let Some(&to_index) = by_uuid.get(outbound.to_uuid.as_str()) else {
        return false;
    };
    inbound.iter().any(|synapse| {
        by_uuid
            .get(synapse.from_uuid.as_str())
            .is_some_and(|&from_index| from_index > to_index)
    })
}

fn index_by_uuid(creature: &CreatureExport) -> std::collections::HashMap<&str, usize> {
    creature
        .neurons
        .iter()
        .enumerate()
        .map(|(index, neuron)| (neuron.uuid.as_str(), index))
        .collect()
}

fn role(synapse: &SynapseExport) -> &str {
    synapse.synapse_type.as_deref().unwrap_or_default()
}

fn is_aggregate_squash(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    parse_squash_name(name).is_ok_and(|squash| squash.is_aggregate())
}
