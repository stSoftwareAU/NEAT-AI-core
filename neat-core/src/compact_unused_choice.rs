//! Neuron choice for `compactUnused`, ported from NEAT-AI
//! `src/compact/CompactUnused.ts`, lines 39–117 (Issue #778).
//!
//! [`choose_unused_neuron`] is the selection loop. It tallies each neuron's
//! in-degree and outward scale from the creature's synapses, then visits the
//! caller's shuffled order from the last entry to the first. The effect of an
//! eligible neuron is `|max - min| * min(scale, 1) - plank * in_degree`, and the
//! smallest effect wins. A negative effect ends the visit at once. The result
//! carries the activation the removal folds in.
//!
//! The caller keeps these parts:
//!
//! - collecting the activation traces (TypeScript's `activateAndTrace`), which
//!   become [`UnusedNeuronCandidate::trace`];
//! - the shuffle. TypeScript's `CreatureUtil.shuffle` draws from the run's RNG,
//!   so the caller passes the resulting visit order as `shuffled`;
//! - reading the `newborn-grace` tag and converting it with
//!   [`newborn_grace_remaining`](crate::compact_unused_tags::newborn_grace_remaining),
//!   which becomes [`UnusedNeuronCandidate::newborn_grace_remaining`];
//! - writing the `unused`, `approach` and `old-*` tags and the aged grace,
//!   computing the values with
//!   [`compact_unused_pass_tags`](crate::compact_unused_tags::compact_unused_pass_tags)
//!   and [`age_newborn_grace`](crate::compact_unused_tags::age_newborn_grace), and
//!   the uuid and score comparison that decides whether the pass changed anything.
//!
//! The choice feeds the removal. Pass `choice.uuid` and `choice.activation` to
//! [`remove_neuron`](crate::compact_unity_constant::remove_neuron), which is
//! TypeScript's `removeNeuron`.
//!
//! ```
//! use neat_core::{
//!     CreatureExport, NeuronExport, NeuronTraceStats, SynapseExport, UnusedNeuronCandidate,
//!     choose_unused_neuron,
//! };
//!
//! let neuron = |kind: &str, uuid: &str| NeuronExport {
//!     id: None,
//!     neuron_type: kind.to_string(),
//!     uuid: uuid.to_string(),
//!     bias: 0.0,
//!     squash: Some("IDENTITY".to_string()),
//! };
//! let synapse = |from: &str, to: &str, weight: f64| SynapseExport {
//!     from_uuid: from.to_string(),
//!     to_uuid: to.to_string(),
//!     weight,
//!     synapse_type: None,
//! };
//! let creature = CreatureExport {
//!     input: 2,
//!     output: 1,
//!     neurons: vec![
//!         neuron("hidden", "hidden-0"),
//!         neuron("hidden", "hidden-1"),
//!         neuron("output", "output-0"),
//!     ],
//!     synapses: vec![
//!         synapse("input-0", "hidden-0", 1.0),
//!         synapse("input-1", "hidden-1", 1.0),
//!         synapse("hidden-0", "output-0", 0.5),
//!         synapse("hidden-1", "output-0", 0.25),
//!     ],
//!     semantic_version: None,
//!     forward_only: true,
//!     memetic: None,
//! };
//! let swing = NeuronTraceStats {
//!     count: 4,
//!     maximum_activation: 1.0,
//!     minimum_activation: 0.0,
//!     total_activation: Some(2.0),
//! };
//! let candidates = [
//!     UnusedNeuronCandidate { trace: Some(swing), newborn_grace_remaining: 0 },
//!     UnusedNeuronCandidate { trace: Some(swing), newborn_grace_remaining: 0 },
//!     UnusedNeuronCandidate { trace: None, newborn_grace_remaining: 0 },
//! ];
//!
//! // Plank 0.25, and each hidden neuron has in-degree 1:
//! //   hidden-0 = |1.0 - 0.0| * min(0.5, 1) - 0.25 * 1 = 0.25
//! //   hidden-1 = |1.0 - 0.0| * min(0.25, 1) - 0.25 * 1 = 0.0
//! // The visit [0, 1, 2] runs output-0, then hidden-1, then hidden-0, so hidden-1
//! // is the smallest. Its activation is total 2.0 / count 4 = 0.5.
//! let choice = choose_unused_neuron(&creature, &candidates, &[0, 1, 2], 0.25)
//!     .unwrap()
//!     .unwrap();
//! assert_eq!(choice.uuid, "hidden-1");
//! assert_eq!(choice.effect, 0.0);
//! assert_eq!(choice.activation, 0.5);
//! ```

use std::collections::HashMap;
use std::fmt;

use crate::creature::CreatureExport;

/// The trace statistics `compactUnused` reads for one neuron.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeuronTraceStats {
    /// How many times the neuron was observed (TypeScript `trace.count`).
    pub count: u64,
    /// The largest observed activation (TypeScript `maximumActivation`).
    pub maximum_activation: f64,
    /// The smallest observed activation (TypeScript `minimumActivation`).
    pub minimum_activation: f64,
    /// The sum of the observed activations, or `None` when the trace carries
    /// none (TypeScript `totalActivation`).
    pub total_activation: Option<f64>,
}

/// What the caller knows about one neuron before the choice. There is one per
/// entry of `creature.neurons`, in the same order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnusedNeuronCandidate {
    /// The neuron's trace, or `None` when it has none.
    pub trace: Option<NeuronTraceStats>,
    /// The rounds of newborn grace left. The caller reads the `newborn-grace` tag
    /// and converts it with
    /// [`newborn_grace_remaining`](crate::compact_unused_tags::newborn_grace_remaining).
    /// A neuron with grace left is skipped.
    pub newborn_grace_remaining: u32,
}

/// The neuron that `compactUnused` would remove, with the score and the
/// activation to fold in.
#[derive(Debug, Clone, PartialEq)]
pub struct UnusedNeuronChoice {
    /// The chosen neuron's UUID.
    pub uuid: String,
    /// Its effect score. The smallest effect wins.
    pub effect: f64,
    /// The activation to fold in: the trace mean, or the total over the count.
    pub activation: f64,
}

/// Why a visit order is not a permutation of `0..neurons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitOrderFault {
    /// The order has a different length from `creature.neurons`.
    WrongLength,
    /// An entry is `>= neurons`.
    OutOfRange {
        /// The offending entry.
        index: usize,
    },
    /// An entry appears more than once.
    Duplicate {
        /// The entry that repeats.
        index: usize,
    },
}

/// Why [`choose_unused_neuron`] refused its arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum UnusedChoiceError {
    /// The candidate list does not have one entry per neuron.
    CandidateCountMismatch {
        /// The number of neurons in the creature.
        neurons: usize,
        /// The number of candidates given.
        candidates: usize,
    },
    /// The visit order is not a permutation of `0..neurons`.
    InvalidVisitOrder {
        /// The number of neurons in the creature.
        neurons: usize,
        /// The length of the visit order given.
        visits: usize,
        /// What is wrong with it.
        fault: VisitOrderFault,
    },
    /// A neuron the loop reached has no inward synapse. TypeScript asserts this.
    NoInwardSynapse {
        /// The neuron's UUID.
        uuid: String,
    },
    /// A neuron the loop reached has no outward synapse. TypeScript asserts this.
    NoOutwardSynapse {
        /// The neuron's UUID.
        uuid: String,
    },
}

impl fmt::Display for UnusedChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnusedChoiceError::CandidateCountMismatch {
                neurons,
                candidates,
            } => write!(
                f,
                "Expected one candidate per neuron, but there are {neurons} neurons and {candidates} candidates"
            ),
            UnusedChoiceError::InvalidVisitOrder {
                neurons,
                visits,
                fault,
            } => match fault {
                VisitOrderFault::WrongLength => {
                    write!(f, "Visit order has {visits} entries for {neurons} neurons")
                }
                VisitOrderFault::OutOfRange { index } => write!(
                    f,
                    "Visit order names neuron {index}, which is outside 0..{neurons}"
                ),
                VisitOrderFault::Duplicate { index } => {
                    write!(f, "Visit order names neuron {index} more than once")
                }
            },
            UnusedChoiceError::NoInwardSynapse { uuid } => {
                write!(f, "Neuron {uuid} has no inward synapse")
            }
            UnusedChoiceError::NoOutwardSynapse { uuid } => {
                write!(f, "Neuron {uuid} has no outward synapse")
            }
        }
    }
}

impl std::error::Error for UnusedChoiceError {}

/// Choose the hidden neuron that `compactUnused` would remove (Issue #778).
///
/// `candidates` gives one [`UnusedNeuronCandidate`] per entry of
/// `creature.neurons`, in the same order. `shuffled` is the caller's visit order,
/// a permutation of `0..creature.neurons.len()`. The loop walks it from the last
/// entry to the first, as TypeScript's `for (let i = indices.length; i--;)` does.
///
/// A neuron is eligible when it is `hidden`, has a non-empty UUID, has no
/// newborn grace left, and has a trace with `count >= 1`. The effect is
/// `|max - min| * min(scale, 1) - plank_constant * in_degree`. `in_degree` counts
/// the synapses that end at the neuron, and `scale` is the largest `|weight|` on
/// its outward synapses, with a NaN weight never raising it. The strict `<` keeps
/// the neuron visited first on a tie. A negative effect ends the visit.
///
/// The removal activation is the midpoint `(max + min) / 2`. When `count > 1` and
/// the total is present and finite, it is `total / count` instead.
///
/// `plank_constant` is not validated, as in TypeScript. A NaN value makes every
/// effect NaN, so nothing is chosen. A `+inf` value makes each effect `-inf`, which
/// is chosen and ends the visit. A `-inf` value makes each effect `+inf`, so
/// nothing is chosen.
///
/// # Errors
///
/// [`UnusedChoiceError::CandidateCountMismatch`] when `candidates` does not have
/// one entry per neuron, and [`UnusedChoiceError::InvalidVisitOrder`] when
/// `shuffled` is not a permutation of `0..neurons`. Both are checked before the
/// loop. [`UnusedChoiceError::NoInwardSynapse`] and
/// [`UnusedChoiceError::NoOutwardSynapse`] are raised only for a neuron the loop
/// actually reaches, so an early break ends the loop before a later neuron is
/// checked.
///
/// # Examples
///
/// See the module documentation.
pub fn choose_unused_neuron(
    creature: &CreatureExport,
    candidates: &[UnusedNeuronCandidate],
    shuffled: &[usize],
    plank_constant: f64,
) -> Result<Option<UnusedNeuronChoice>, UnusedChoiceError> {
    let neurons = creature.neurons.len();
    if candidates.len() != neurons {
        return Err(UnusedChoiceError::CandidateCountMismatch {
            neurons,
            candidates: candidates.len(),
        });
    }
    check_visit_order(neurons, shuffled)?;

    let (in_degree, outward_scale) = tally(creature);

    let mut smallest = f64::MAX;
    let mut chosen = None;
    for &index in shuffled.iter().rev() {
        let neuron = &creature.neurons[index];
        let candidate = candidates[index];
        if neuron.neuron_type != "hidden"
            || neuron.uuid.is_empty()
            || candidate.newborn_grace_remaining > 0
        {
            continue;
        }
        let Some(stats) = candidate.trace else {
            continue;
        };
        if stats.count < 1 {
            continue;
        }

        let Some(&inward) = in_degree.get(neuron.uuid.as_str()) else {
            return Err(UnusedChoiceError::NoInwardSynapse {
                uuid: neuron.uuid.clone(),
            });
        };
        let Some(&scale) = outward_scale.get(neuron.uuid.as_str()) else {
            return Err(UnusedChoiceError::NoOutwardSynapse {
                uuid: neuron.uuid.clone(),
            });
        };

        let effect = (stats.maximum_activation - stats.minimum_activation).abs()
            * capped_scale(scale)
            - plank_constant * inward as f64;
        if effect < smallest {
            smallest = effect;
            chosen = Some(UnusedNeuronChoice {
                uuid: neuron.uuid.clone(),
                effect,
                activation: removal_activation(stats),
            });
            if effect < 0.0 {
                break;
            }
        }
    }
    Ok(chosen)
}

/// Refuse a visit order that is not a permutation of `0..neurons`.
fn check_visit_order(neurons: usize, shuffled: &[usize]) -> Result<(), UnusedChoiceError> {
    let fault = |fault| UnusedChoiceError::InvalidVisitOrder {
        neurons,
        visits: shuffled.len(),
        fault,
    };
    if shuffled.len() != neurons {
        return Err(fault(VisitOrderFault::WrongLength));
    }
    let mut seen = vec![false; neurons];
    for &index in shuffled {
        if index >= neurons {
            return Err(fault(VisitOrderFault::OutOfRange { index }));
        }
        if std::mem::replace(&mut seen[index], true) {
            return Err(fault(VisitOrderFault::Duplicate { index }));
        }
    }
    Ok(())
}

/// The in-degree and outward scale of every neuron, from the creature's synapses.
///
/// A synapse with an empty `from` or `to` UUID is skipped, as in TypeScript.
/// `scale` starts at 0 and is raised only by a magnitude greater than it. A NaN
/// magnitude fails that comparison, so it never raises the scale.
fn tally(creature: &CreatureExport) -> (HashMap<&str, u64>, HashMap<&str, f64>) {
    let mut in_degree: HashMap<&str, u64> = HashMap::new();
    let mut outward_scale: HashMap<&str, f64> = HashMap::new();
    for synapse in &creature.synapses {
        if synapse.from_uuid.is_empty() || synapse.to_uuid.is_empty() {
            continue;
        }
        *in_degree.entry(synapse.to_uuid.as_str()).or_insert(0) += 1;
        let scale = outward_scale
            .entry(synapse.from_uuid.as_str())
            .or_insert(0.0);
        let magnitude = synapse.weight.abs();
        if magnitude > *scale {
            *scale = magnitude;
        }
    }
    (in_degree, outward_scale)
}

/// JavaScript `Math.min(scale, 1)`. A NaN scale stays NaN, as it does in JavaScript,
/// because the comparison is false for NaN and the scale is returned unchanged.
/// [`tally`] cannot produce a NaN scale, so this never sees one.
fn capped_scale(scale: f64) -> f64 {
    if scale > 1.0 { 1.0 } else { scale }
}

/// The activation folded in when `stats` is removed: the total over the count when
/// there are several observations and a finite total, otherwise the midpoint.
fn removal_activation(stats: NeuronTraceStats) -> f64 {
    match stats.total_activation {
        Some(total) if stats.count > 1 && total.is_finite() => total / stats.count as f64,
        _ => (stats.maximum_activation + stats.minimum_activation) / 2.0,
    }
}
