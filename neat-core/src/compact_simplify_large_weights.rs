//! Large-weight simplification for scale-homogeneous squashes (Issue #778).
//!
//! Ports NEAT-AI `simplifyLargeWeights` (`src/compact/SimplifyLargeWeights.ts`).
//! For a hidden neuron whose squash `f` is positively homogeneous
//! (`f(kx) = k·f(x)` for `k > 0`), dividing its bias and inbound weights by `c`
//! and multiplying its outbound weights by `c` leaves the network unchanged:
//! with pre-activation `z`, the new output is `v·c·f(z/c) = v·c·f(z)/c =
//! v·f(z)`. A "huge inbound, tiny outbound" pair becomes two moderate values.
//!
//! The equality is exact only in real arithmetic. Floating-point rounding means
//! outputs agree to rounding, not bit-for-bit.
//!
//! Acceptance is the magnitude-penalty heuristic of [`weight_bias_penalty`], not
//! the authoritative score: a rescale is kept when it lowers the penalty by more
//! than `1e-15`. With an [`McmcAcceptance`] a *worse* penalty can also be
//! accepted, with probability `exp(-delta / temperature)`, to escape local
//! optima; a `NaN`, zero or negative temperature is greedy, as in TypeScript.
//! Only neurons whose input and output scales differ by more than a factor of
//! 1000 are tried; that ratio and the `1e-15` margin are the TypeScript
//! constants.
//!
//! Randomness stays at the caller boundary: the caller supplies the uniform
//! `[0, 1)` draw that TypeScript takes from `Math.random`.
//!
//! When anything is rescaled the memetic record is dropped: every rescaled
//! weight and bias invalidates the values tuned for the old scale, and pruning
//! rows by key would keep stale ones.

use std::collections::HashMap;

use crate::creature::CreatureExport;
use crate::magnitude_penalty::mean_magnitude_penalty;

/// Squash names whose scale-homogeneity this pass relies on (exact strings).
const CANDIDATE_SQUASHES: [&str; 10] = [
    "ABSOLUTE",
    "IDENTITY",
    "ReLU",
    "LeakyReLU",
    "MAXIMUM",
    "MINIMUM",
    "IF",
    "HYPOT",
    "HYPOTv2",
    "MEAN",
];

/// Only neurons whose in/out scales differ by more than this are tried.
const IMBALANCE_RATIO: f64 = 1_000.0;

/// A rescale must beat the best penalty by more than this to be a clear win.
const IMPROVEMENT_MARGIN: f64 = 1e-15;

/// Metropolis-Hastings acceptance of worsening rescales.
pub struct McmcAcceptance<'a> {
    /// Temperature; only a value above zero enables probabilistic acceptance.
    pub temperature: f64,
    /// Uniform `[0, 1)` draw, called once per worsening candidate.
    pub uniform: &'a mut dyn FnMut() -> f64,
}

/// Result of [`simplify_large_weights`].
#[derive(Debug, Clone, PartialEq)]
pub struct SimplifiedLargeWeights {
    /// The creature after the pass. Equal to the input when nothing was kept.
    pub creature: CreatureExport,
    /// Number of neurons whose rescale was accepted.
    pub rescaled_neurons: usize,
}

impl SimplifiedLargeWeights {
    /// `true` when at least one neuron was rescaled.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.rescaled_neurons > 0
    }
}

/// Mean magnitude penalty over every synapse weight, then every neuron bias.
///
/// Matches TypeScript `calculateWeightBiasPenalty`. Input neurons are not
/// listed in `neurons`, so they carry no bias here.
#[must_use]
pub fn weight_bias_penalty(creature: &CreatureExport) -> f64 {
    mean_magnitude_penalty(
        creature
            .synapses
            .iter()
            .map(|s| s.weight)
            .chain(creature.neurons.iter().map(|n| n.bias)),
    )
}

/// JavaScript `Math.max`: `NaN` propagates (Rust `f64::max` ignores it).
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// Rescale hidden neurons with a homogeneous squash when that lowers the
/// magnitude penalty. The input is never modified.
///
/// Without `mcmc` the pass is greedy. A neuron is skipped when it has no
/// inbound or no outbound synapse, when either side's largest magnitude is
/// zero (or `NaN`-poisoned into a non-finite scale factor), when the scales are
/// within a factor of 1000, or when the rescale would produce a non-finite
/// value.
///
/// # Examples
///
/// ```
/// use neat_core::{CreatureExport, NeuronExport, SynapseExport, simplify_large_weights};
///
/// let neuron = |kind: &str, uuid: &str, bias: f64, squash: &str| NeuronExport {
///     id: None,
///     neuron_type: kind.to_string(),
///     uuid: uuid.to_string(),
///     bias,
///     squash: Some(squash.to_string()),
/// };
/// let edge = |from: &str, to: &str, weight: f64| SynapseExport {
///     from_uuid: from.to_string(),
///     to_uuid: to.to_string(),
///     weight,
///     synapse_type: None,
/// };
/// let creature = CreatureExport {
///     input: 1,
///     output: 1,
///     neurons: vec![
///         neuron("hidden", "hidden-0", 1e6, "ABSOLUTE"),
///         neuron("output", "output-0", 0.0, "IDENTITY"),
///     ],
///     synapses: vec![edge("input-0", "hidden-0", 1e6), edge("hidden-0", "output-0", 1e-6)],
///     semantic_version: None,
///     forward_only: true,
///     memetic: None,
/// };
/// let result = simplify_large_weights(&creature, None);
/// assert!(result.changed());
/// assert_eq!(result.creature.neurons[0].bias, 1.0);
/// assert_eq!(result.creature.synapses[0].weight, 1.0);
/// assert_eq!(result.creature.synapses[1].weight, 1.0);
/// ```
pub fn simplify_large_weights(
    creature: &CreatureExport,
    mut mcmc: Option<McmcAcceptance<'_>>,
) -> SimplifiedLargeWeights {
    let mut work = creature.clone();

    // Built once from the original synapses; a self-loop is in both lists.
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

    let mut rescaled_neurons = 0usize;
    let mut best_penalty = weight_bias_penalty(&work);

    for (neuron_index, neuron) in creature.neurons.iter().enumerate() {
        if neuron.neuron_type != "hidden" {
            continue;
        }
        let Some(squash) = neuron.squash.as_deref() else {
            continue;
        };
        if !CANDIDATE_SQUASHES.contains(&squash) {
            continue;
        }

        let in_conns = inward
            .get(neuron.uuid.as_str())
            .map_or(&[][..], Vec::as_slice);
        let out_conns = outward
            .get(neuron.uuid.as_str())
            .map_or(&[][..], Vec::as_slice);
        if in_conns.is_empty() || out_conns.is_empty() {
            continue;
        }

        let mut max_in = work.neurons[neuron_index].bias.abs();
        for &i in in_conns {
            max_in = js_max(max_in, work.synapses[i].weight.abs());
        }
        let mut max_out = 0.0;
        for &i in out_conns {
            max_out = js_max(max_out, work.synapses[i].weight.abs());
        }
        if max_in == 0.0 || max_out == 0.0 {
            continue;
        }

        let ratio = max_in / max_out;
        if !(ratio < 1.0 / IMBALANCE_RATIO || ratio > IMBALANCE_RATIO) {
            continue;
        }
        // Equalise maxIn/c and maxOut*c, minimising their maximum.
        let c = (max_in / max_out).sqrt();
        if !c.is_finite() || c == 0.0 || c == 1.0 {
            continue;
        }

        let old_bias = work.neurons[neuron_index].bias;
        let old_in: Vec<f64> = in_conns.iter().map(|&i| work.synapses[i].weight).collect();
        let old_out: Vec<f64> = out_conns.iter().map(|&i| work.synapses[i].weight).collect();

        work.neurons[neuron_index].bias = old_bias / c;
        for &i in in_conns {
            work.synapses[i].weight /= c;
        }
        for &i in out_conns {
            work.synapses[i].weight *= c;
        }

        let revert = |work: &mut CreatureExport| {
            work.neurons[neuron_index].bias = old_bias;
            for (&i, &w) in in_conns.iter().zip(&old_in) {
                work.synapses[i].weight = w;
            }
            for (&i, &w) in out_conns.iter().zip(&old_out) {
                work.synapses[i].weight = w;
            }
        };

        let all_finite = work.neurons[neuron_index].bias.is_finite()
            && in_conns
                .iter()
                .all(|&i| work.synapses[i].weight.is_finite())
            && out_conns
                .iter()
                .all(|&i| work.synapses[i].weight.is_finite());
        if !all_finite {
            revert(&mut work);
            continue;
        }

        // SIMPLE-ON-PURPOSE: recomputes the whole-creature penalty per candidate, as TypeScript does — upgrade when a profiled caller shows this pass dominating
        let next_penalty = weight_bias_penalty(&work);
        if next_penalty + IMPROVEMENT_MARGIN < best_penalty {
            best_penalty = next_penalty;
            rescaled_neurons += 1;
        } else if let Some(m) = mcmc.as_mut().filter(|m| m.temperature > 0.0) {
            let accept_probability = (-(next_penalty - best_penalty) / m.temperature).exp();
            if (m.uniform)() < accept_probability {
                best_penalty = next_penalty;
                rescaled_neurons += 1;
            } else {
                revert(&mut work);
            }
        } else {
            revert(&mut work);
        }
    }

    if rescaled_neurons == 0 {
        return SimplifiedLargeWeights {
            creature: creature.clone(),
            rescaled_neurons,
        };
    }
    // Memetic values were tuned for the previous scale; they are stale.
    work.memetic = None;
    SimplifiedLargeWeights {
        creature: work,
        rescaled_neurons,
    }
}
