//! Exact transitive constant folding (Issue #777).
//!
//! Ports NEAT-AI `foldConstants` (`src/compact/ConstantFold.ts`, pinned
//! revision `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`). Each step folds one
//! constant producer into its plain consumers, then repeats so a neuron that
//! has just become constant is picked up.
//!
//! A `constant` neuron's value is its bias. A hidden neuron whose every
//! inbound source is already constant, and whose squash is a scalar one, has
//! value `squash(bias + Σ weight · source)`. That squash is
//! [`apply_squash`], evaluated at `f32` the way
//! the forward pass reads it. Aggregate targets are not folded into. A typed
//! edge is left in place, and an output is never left with no inbound edge.
//! A producer with no outbound edge is left for [`cleanup_creature`](crate::cleanup_creature).
//!
//! `frozen` is not a field of [`CreatureExport`], so
//! this pass has nothing to skip on that account. The memetic record is
//! pruned of dangling rows; it is not deleted.

use std::collections::HashMap;

use crate::creature::{CreatureExport, NeuronExport, SynapseExport, parse_squash_name};
use crate::squash::apply_squash;

/// Result of folding constants to a fixpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantFold {
    /// The creature after the fold. Equal to the input when nothing folded.
    pub creature: CreatureExport,
    /// Producers removed because they had no outbound edge left.
    pub removed_neurons: usize,
    /// Synapses removed, including edges that only existed to feed a removed producer.
    pub removed_synapses: usize,
}

impl ConstantFold {
    /// Whether any producer was folded.
    pub fn changed(&self) -> bool {
        self.removed_neurons > 0 || self.removed_synapses > 0
    }
}

/// Fold constant producers into their plain consumers until nothing else can.
pub fn fold_constants(creature: &CreatureExport) -> ConstantFold {
    let mut next = creature.clone();
    let mut removed_neurons = 0;
    let mut removed_synapses = 0;

    loop {
        let values = constant_values(&next);
        let Some(step) = fold_one(&mut next, &values) else {
            break;
        };
        removed_neurons += step.0;
        removed_synapses += step.1;
        next.prune_memetic();
    }

    ConstantFold {
        creature: next,
        removed_neurons,
        removed_synapses,
    }
}

/// Scalar value of every neuron that is constant from its inputs.
fn constant_values(creature: &CreatureExport) -> HashMap<String, f64> {
    let mut values: HashMap<String, f64> = HashMap::new();
    for neuron in &creature.neurons {
        if neuron.neuron_type == "constant" {
            values.insert(neuron.uuid.clone(), neuron.bias);
        }
    }

    loop {
        let mut added = false;
        for neuron in &creature.neurons {
            if neuron.neuron_type != "hidden" || values.contains_key(&neuron.uuid) {
                continue;
            }
            let Some(squash) = scalar_squash(neuron) else {
                continue;
            };
            let mut pre = neuron.bias;
            let mut all_constant = true;
            for synapse in creature
                .synapses
                .iter()
                .filter(|synapse| synapse.to_uuid == neuron.uuid)
            {
                let Some(&source) = values.get(&synapse.from_uuid) else {
                    all_constant = false;
                    break;
                };
                pre += synapse.weight * source;
            }
            if !all_constant || !pre.is_finite() {
                continue;
            }
            let value = f64::from(apply_squash(squash, pre as f32));
            if !value.is_finite() {
                continue;
            }
            values.insert(neuron.uuid.clone(), value);
            added = true;
        }
        if !added {
            break;
        }
    }
    values
}

fn scalar_squash(neuron: &NeuronExport) -> Option<crate::squash::SquashType> {
    let name = neuron.squash.as_deref()?;
    let squash = parse_squash_name(name).ok()?;
    if squash.is_aggregate() {
        return None;
    }
    Some(squash)
}

fn is_aggregate_target(neuron: &NeuronExport) -> bool {
    neuron
        .squash
        .as_deref()
        .and_then(|name| parse_squash_name(name).ok())
        .is_some_and(|squash| squash.is_aggregate())
}

fn untyped(synapse: &SynapseExport) -> bool {
    match synapse.synapse_type.as_deref() {
        None | Some("") => true,
        Some(_) => false,
    }
}

/// Fold the first producer that has a plain consumer. `(neurons, synapses)` removed.
fn fold_one(
    creature: &mut CreatureExport,
    values: &HashMap<String, f64>,
) -> Option<(usize, usize)> {
    let producers: Vec<(String, f64)> = creature
        .neurons
        .iter()
        .filter(|neuron| neuron.neuron_type != "output")
        .filter_map(|neuron| {
            values
                .get(&neuron.uuid)
                .copied()
                .map(|value| (neuron.uuid.clone(), value))
        })
        .collect();

    for (producer_uuid, value) in producers {
        let mut fold_indexes: Vec<usize> = Vec::new();
        let outbound: Vec<usize> = creature
            .synapses
            .iter()
            .enumerate()
            .filter(|(_, synapse)| synapse.from_uuid == producer_uuid)
            .map(|(index, _)| index)
            .collect();

        let mut consumers: Vec<String> = Vec::new();
        for &index in &outbound {
            let synapse = &creature.synapses[index];
            if !untyped(synapse) {
                continue;
            }
            let Some(target) = creature
                .neurons
                .iter()
                .find(|neuron| neuron.uuid == synapse.to_uuid)
            else {
                continue;
            };
            if is_aggregate_target(target) {
                continue;
            }
            if !consumers.iter().any(|uuid| uuid == &synapse.to_uuid) {
                consumers.push(synapse.to_uuid.clone());
            }
        }

        for consumer_uuid in &consumers {
            let foldable: Vec<usize> = outbound
                .iter()
                .copied()
                .filter(|&index| {
                    let synapse = &creature.synapses[index];
                    synapse.to_uuid == *consumer_uuid && untyped(synapse)
                })
                .collect();
            let total_inbound = creature
                .synapses
                .iter()
                .filter(|synapse| synapse.to_uuid == *consumer_uuid)
                .count();
            let consumer_is_output = creature
                .neurons
                .iter()
                .any(|neuron| neuron.uuid == *consumer_uuid && neuron.neuron_type == "output");
            let mut to_fold = foldable;
            if consumer_is_output && total_inbound - to_fold.len() < 1 {
                to_fold = to_fold.into_iter().skip(1).collect();
            }
            let delta: f64 = to_fold
                .iter()
                .map(|&index| creature.synapses[index].weight * value)
                .sum();
            if let Some(consumer) = creature
                .neurons
                .iter_mut()
                .find(|neuron| neuron.uuid == *consumer_uuid)
            {
                consumer.bias += delta;
            }
            fold_indexes.extend(to_fold);
        }

        if fold_indexes.is_empty() {
            continue;
        }

        let before = creature.synapses.len();
        let mut removed = 0;
        creature.synapses = creature
            .synapses
            .iter()
            .enumerate()
            .filter(|(index, _)| !fold_indexes.contains(index))
            .map(|(_, synapse)| synapse.clone())
            .collect();
        removed += before - creature.synapses.len();

        let still_out = creature
            .synapses
            .iter()
            .any(|synapse| synapse.from_uuid == producer_uuid);
        let mut removed_neurons = 0;
        if !still_out {
            let before_incident = creature.synapses.len();
            creature.synapses.retain(|synapse| {
                synapse.from_uuid != producer_uuid && synapse.to_uuid != producer_uuid
            });
            removed += before_incident - creature.synapses.len();
            creature
                .neurons
                .retain(|neuron| neuron.uuid != producer_uuid);
            removed_neurons = 1;
        }

        return Some((removed_neurons, removed));
    }
    None
}
