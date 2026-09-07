//! Hidden-neuron pruning with optional statistical compensation (Issue #590).
//!
//! [`prune_neuron`] is the shared answer to "remove this hidden neuron and give
//! me back something I can score". It cuts the requested neuron out, optionally
//! compensates the targets that read it using the **caller's** statistics, runs
//! the Issue #589 [`cleanup_creature`] fixed point over the wreckage, and
//! validates the stable result before returning it. A successful call never
//! returns an invalid creature.
//!
//! ```mermaid
//! flowchart TD
//!     Q["prune_neuron(creature, uuid, stats?)"] --> C{"what is uuid?"}
//!     C -- "observation / output / constant" --> P["Err(Protected)"]
//!     C -- "not in the creature" --> U["Err(UnknownNeuron)"]
//!     C -- hidden --> S{"statistics supplied?"}
//!     S -- "yes, and not numbers" --> N["Err(NonFiniteStatistic /<br/>NegativeVariance / DegenerateProxy)"]
//!     S -- ok --> X["cut the neuron and<br/>every edge naming it"]
//!     X --> F["compensate each target:<br/>structural value, or the<br/>caller's mean and proxy"]
//!     F --> L["cleanup_creature — cascade,<br/>fold, canonicalise, validate"]
//!     L -- fails --> E["Err(Cleanup)"]
//!     L -- passes --> R["Ok(PruneResult) —<br/>Exact or Approximate"]
//! ```
//!
//! # Only a hidden neuron is a direct target
//!
//! Observation (input) neurons and output neurons carry the fleet's declared
//! widths (Issue #550), and a constant is support structure the canonical form
//! owns (Ockham #180), so none of the three may be deleted on request:
//! [`prune_neuron`] refuses with [`PruneError::Protected`] before anything is
//! rewritten. They still *disappear* as a consequence — a constant nothing
//! references any more is dead structure cleanup removes — but that is the
//! cascade's doing, not a direct request.
//!
//! # Exact, or approximate, and never quietly the second
//!
//! [`PruneResult::transform`] is the honest label on what came back:
//!
//! - [`TransformClass::Exact`] — the pruned creature computes the **same number
//!   on every record**. Two cases reach it, both provable from the creature
//!   alone: nothing read the neuron, or the neuron had no inward edge, so it
//!   activated to one value on every record and that value folds into each
//!   target's bias. No statistic can buy this label, and a supplied mean never
//!   overrides the structural value. "Same number" means to the precision the
//!   forward pass works in: the folded value *is* the `f32`
//!   [`zero_inward_activation`] the pass would have produced, but the fold
//!   re-associates the sum, so the two agree to `f32` rounding rather than bit
//!   for bit.
//! - [`TransformClass::Approximate`] — everything else. A neuron whose
//!   activation varies is gone, and the mean fold only replaces it *on
//!   average*; an `IF` that lost a role can no longer branch at all.
//!
//! # The compensation, spelled out
//!
//! Write `a` for the removed neuron's activation, `μ` and `σ²` for the mean and
//! variance the caller measured, and `W` for the total weight the removed
//! neuron carried into one target. That target used to read `W · a`, so:
//!
//! - **mean bias fold** — `bias += W · μ`. This is `DiscoveryNeuronRemoval.ts`'s
//!   `applyMeanBiasFold`, and it leaves a residual of `W · (a − μ)` with
//!   variance `W² σ²`.
//! - **the correlated-survivor remedy** — when the caller also supplies a
//!   surviving neuron `s` that already feeds the target, with mean `μₛ`,
//!   variance `σₛ² > 0` and covariance `cov(a, aₛ)`, the least-squares
//!   predictor of `a` from `aₛ` is `μ + β(aₛ − μₛ)` with `β = cov / σₛ²`. So
//!   `β · W` is added to the weight of the `s → target` edge and the bias takes
//!   what is left of the mean, `W · (μ − β μₛ)`. The residual variance drops to
//!   `W² (σ² − cov²/σₛ²)` — the `removeNeuronCompensation` remedy
//!   `docs/research/pruning-parity-matrix.md` deferred to this issue.
//!
//! Both are reported per target on [`PruneResult::bias_folds`] and
//! [`PruneResult::weight_shares`], with the residual variance the caller's own
//! statistics imply, so a caller can see exactly what it accepted.
//!
//! # Where a bias fold means nothing, it is not attempted
//!
//! A point-wise squash computes `squash(bias + Σ w·a)`, so moving `W · μ` into
//! the bias puts the mean term back where the removed one was. An **aggregate**
//! does not: a `MINIMUM` takes the smallest inward term, a `MEAN` divides by its
//! inward **count**, a `HYPOT` squares each term, and an `IF` reads its
//! condition sum to choose a branch. Folding into their bias would be a number
//! the caller could not justify, so no fold is attempted there and the target is
//! named on [`PruneResult::uncompensated`] instead — reported, never silent.
//! The same entry records a target left uncompensated because no statistics were
//! supplied at all.
//!
//! # The memetic record is pruned, not dropped
//!
//! TypeScript drops `memetic` wholesale on every removal because its content
//! hash no longer describes the creature. This crate owns the finer-grained
//! inverse of validation rule 31 (`CreatureExport::prune_memetic`,
//! NEAT-AI-Lamarck#197) and [`cleanup_creature`] applies it, so what comes back
//! keeps every entry that still names live structure and loses exactly the
//! dangling ones. That is Issue #590's call on the choice the parity matrix
//! left open: the fine-tuning history a caller measured is worth more than a
//! blunt reset, and rule 31 is satisfied either way.

use std::collections::HashMap;

use crate::creature::{CreatureExport, parse_squash_name, parse_synapse_type, squash_name_from};
use crate::prune_cleanup::{CleanupError, SynapseKey, cleanup_creature, zero_inward_activation};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// How faithful the rewrite was to the creature it started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformClass {
    /// The pruned creature computes the same number on every record, to the
    /// `f32` precision the forward pass itself works in.
    Exact,
    /// The pruned creature is a compensated approximation of the original.
    Approximate,
}

/// Which class of protected node a caller asked to delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectedKind {
    /// An observation (input) neuron — the declared input width.
    Observation,
    /// An output neuron — the declared output width, in order.
    Output,
    /// A constant — canonical support structure, removed only as dead
    /// structure once nothing references it.
    Constant,
}

impl ProtectedKind {
    /// The word this kind is called by in a message.
    const fn label(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Output => "output",
            Self::Constant => "constant",
        }
    }
}

/// Statistics for a **surviving** neuron the caller believes predicts the one
/// being removed, all measured over the same records.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyStats {
    /// Wire UUID of the surviving neuron.
    pub uuid: String,
    /// Its mean activation.
    pub mean_activation: f64,
    /// Its activation variance. Must be `> 0`: `β` divides by it.
    pub variance: f64,
    /// Covariance between the removed neuron's activation and this one's.
    pub covariance: f64,
}

/// What the caller measured about the neuron it wants removed.
///
/// Collecting these is the caller's half of the Issue #587 ownership boundary;
/// this crate only ever *uses* them, and says so in the result.
#[derive(Debug, Clone, PartialEq)]
pub struct PruneStats {
    /// Mean activation of the neuron being removed.
    pub mean_activation: f64,
    /// Its activation variance, when measured. Only used to report the
    /// residual variance the compensation could not carry.
    pub variance: Option<f64>,
    /// A correlated survivor that can carry part of what the neuron did.
    pub proxy: Option<ProxyStats>,
}

impl PruneStats {
    /// The mean-only statistic — the `applyMeanBiasFold` case.
    #[must_use]
    pub const fn mean(mean_activation: f64) -> Self {
        Self {
            mean_activation,
            variance: None,
            proxy: None,
        }
    }
}

/// What one target's bias took to stand in for the removed neuron.
#[derive(Debug, Clone, PartialEq)]
pub struct BiasFold {
    /// Wire UUID of the target whose bias moved.
    pub target_uuid: String,
    /// Total weight the removed neuron carried into that target.
    pub weight_sum: f64,
    /// What was added to the target's bias.
    pub delta: f64,
    /// True when the folded value is the neuron's activation on **every**
    /// record, so the fold changes nothing the creature computes.
    pub exact: bool,
    /// Variance of what the compensation could not carry, when the caller
    /// supplied the variance it is derived from.
    pub residual_variance: Option<f64>,
}

/// Weight moved onto a correlated survivor's existing edge.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightShare {
    /// Wire UUID of the survivor carrying the correlated part.
    pub from_uuid: String,
    /// Wire UUID of the target it feeds.
    pub to_uuid: String,
    /// What was added to that edge's weight — `β · W`.
    pub delta: f64,
}

/// Why a target that read the removed neuron got no compensation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UncompensatedReason {
    /// No statistics were supplied, so there is no number to fold.
    NoStatistics,
    /// The target's squash aggregates its inward terms, so a bias fold does
    /// not stand in for the term the removal took away.
    AggregateTarget,
}

/// A target left carrying the removal without compensation.
///
/// One entry per **readable key**, not per target: an `IF` never sums its arms,
/// so a neuron feeding one on two roles is reported once per role rather than
/// as a total the target never computed. Every other squash sums whatever
/// reaches it, so there the role is always [`SynapseType::Standard`] and the
/// entry is the target.
#[derive(Debug, Clone, PartialEq)]
pub struct UncompensatedTarget {
    /// Wire UUID of the target.
    pub target_uuid: String,
    /// Role the removed edges played at that target.
    pub role: SynapseType,
    /// Total weight the removed neuron carried into that role of that target.
    pub weight_sum: f64,
    /// The target's squash, which is what makes an aggregate uncompensable.
    pub squash: &'static str,
    /// Why nothing was folded.
    pub reason: UncompensatedReason,
}

/// The creature a prune produced, and everything the removal cost.
#[derive(Debug, Clone, PartialEq)]
pub struct PruneResult {
    /// The canonical, validated creature.
    pub creature: CreatureExport,
    /// Wire UUID of the neuron the caller asked to remove.
    pub removed_neuron: String,
    /// The edges naming that neuron, in either direction, that went with it.
    pub removed_synapses: Vec<SynapseKey>,
    /// Neurons the cleanup cascade removed on top of the requested one.
    pub cascade_neurons: Vec<String>,
    /// Synapses the cascade removed alongside them.
    pub cascade_synapses: Vec<SynapseKey>,
    /// Hidden neurons the cascade folded into constant support.
    pub folded_neurons: Vec<String>,
    /// `IF` neurons downgraded to `IDENTITY` because a role went with the
    /// removal — the one cleanup rewrite that is not exact.
    pub downgraded_if_neurons: Vec<String>,
    /// The mean folds applied, one per compensated target.
    pub bias_folds: Vec<BiasFold>,
    /// The correlated-survivor shares applied.
    pub weight_shares: Vec<WeightShare>,
    /// Targets that read the removed neuron and got nothing back.
    pub uncompensated: Vec<UncompensatedTarget>,
    /// Whether the whole rewrite preserves the creature's output exactly.
    pub transform: TransformClass,
    /// How many cleanup passes the fixed point took.
    pub passes: usize,
}

/// Why a prune produced no creature.
///
/// Every variant means **nothing was returned**: an unsupported or invalid
/// request fails here, before any caller can screen or score its result.
#[derive(Debug)]
pub enum PruneError {
    /// The creature carries no neuron with that UUID.
    UnknownNeuron {
        /// The UUID asked for.
        uuid: String,
    },
    /// The neuron exists but is not a caller's to delete.
    Protected {
        /// The UUID asked for.
        uuid: String,
        /// What kind of protected node it is.
        kind: ProtectedKind,
    },
    /// A neuron declared a type outside `hidden | output | constant`.
    UnknownNeuronType {
        /// UUID of the offending neuron.
        uuid: String,
        /// The type it declared.
        declared: String,
    },
    /// A supplied statistic was `NaN` or infinite.
    NonFiniteStatistic {
        /// The neuron the statistic describes.
        uuid: String,
        /// Which statistic it was.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
    /// A variance was negative, which no sample can produce.
    NegativeVariance {
        /// The neuron the variance describes.
        uuid: String,
        /// The offending value.
        variance: f64,
    },
    /// The creature carries no surviving neuron with the proxy's UUID.
    UnknownProxy {
        /// The UUID supplied as the proxy.
        uuid: String,
    },
    /// The proxy's activation never moved, so `β = cov / σₛ²` is undefined.
    DegenerateProxy {
        /// UUID of the proxy.
        uuid: String,
        /// Its supplied variance.
        variance: f64,
    },
    /// The covariance is larger than the two variances allow, so the
    /// statistics cannot have come from one sample.
    InconsistentCovariance {
        /// UUID of the proxy the covariance was measured against.
        uuid: String,
        /// The supplied covariance.
        covariance: f64,
        /// Variance of the neuron being removed.
        variance: f64,
        /// Variance of the proxy.
        proxy_variance: f64,
    },
    /// The proxy does not already feed a target the removed neuron fed, so it
    /// cannot carry what that target loses.
    MissingProxyEdge {
        /// UUID of the proxy.
        from_uuid: String,
        /// UUID of the target it does not reach.
        to_uuid: String,
    },
    /// The cut creature could not be cleaned into a valid canonical form.
    Cleanup(CleanupError),
}

impl std::fmt::Display for PruneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PruneError::UnknownNeuron { uuid } => write!(f, "No neuron {uuid} to remove"),
            PruneError::Protected { uuid, kind } => {
                write!(
                    f,
                    "Neuron {uuid} is a {} node and is protected from direct removal",
                    kind.label()
                )
            }
            PruneError::UnknownNeuronType { uuid, declared } => {
                write!(f, "Neuron {uuid} declares unknown type '{declared}'")
            }
            PruneError::NonFiniteStatistic { uuid, field, value } => {
                write!(
                    f,
                    "Statistic {field} for {uuid} is not a finite number: {value}"
                )
            }
            PruneError::NegativeVariance { uuid, variance } => {
                write!(f, "Variance {variance} for {uuid} is negative")
            }
            PruneError::UnknownProxy { uuid } => {
                write!(f, "No surviving neuron {uuid} to carry the compensation")
            }
            PruneError::DegenerateProxy { uuid, variance } => {
                write!(
                    f,
                    "Proxy {uuid} has variance {variance}, so its regression slope is undefined"
                )
            }
            PruneError::InconsistentCovariance {
                uuid,
                covariance,
                variance,
                proxy_variance,
            } => write!(
                f,
                "Covariance {covariance} with {uuid} exceeds what variances {variance} and {proxy_variance} allow"
            ),
            PruneError::MissingProxyEdge { from_uuid, to_uuid } => {
                write!(
                    f,
                    "Proxy {from_uuid} does not feed {to_uuid}, so it cannot compensate it"
                )
            }
            PruneError::Cleanup(e) => write!(f, "Cleanup after the removal failed: {e}"),
        }
    }
}

impl std::error::Error for PruneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PruneError::Cleanup(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CleanupError> for PruneError {
    fn from(e: CleanupError) -> Self {
        PruneError::Cleanup(e)
    }
}

/// Remove one hidden neuron, compensate what read it, and return a valid
/// canonical creature.
///
/// `stats` is the caller's own measurement of the neuron being removed. Without
/// it the exact structural rewrites still run — the cascade, the constant fold,
/// the canonical order — and every target that read the neuron is named on
/// [`PruneResult::uncompensated`]. With it, the mean bias fold (and the
/// correlated-survivor remedy, when a proxy is supplied) stands in for the term
/// the removal took away.
///
/// # Errors
///
/// Returns [`PruneError`] when the UUID names nothing, names a protected node,
/// when a supplied statistic is not a usable number, when a supplied proxy
/// cannot carry the compensation, or when the creature that remains cannot be
/// cleaned into a valid canonical form. No creature is returned in any of those
/// cases.
///
/// # Examples
///
/// ```
/// use neat_core::{PruneStats, TransformClass, parse_creature_json, prune_neuron};
///
/// let creature = parse_creature_json(r#"{
///   "input":1,"output":1,"forwardOnly":true,
///   "neurons":[
///     {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
///     {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
///   ],
///   "synapses":[
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
///     {"weight":0.2,"fromUUID":"h-1","toUUID":"output-0"}
///   ]
/// }"#).unwrap();
///
/// let stats = PruneStats::mean(0.5);
/// let result = prune_neuron(&creature, "h-1", Some(&stats)).unwrap();
///
/// // 0.25 + 0.2 * 0.5, the mean the caller measured folded into the target.
/// assert!((result.creature.neurons[0].bias - 0.35).abs() < 1e-12);
/// assert_eq!(result.transform, TransformClass::Approximate);
/// ```
pub fn prune_neuron(
    creature: &CreatureExport,
    neuron_uuid: &str,
    stats: Option<&PruneStats>,
) -> Result<PruneResult, PruneError> {
    classify_target(creature, neuron_uuid)?;
    if let Some(stats) = stats {
        check_stats(neuron_uuid, stats)?;
    }

    // A neuron with nothing to sum activates to one value on every record, and
    // that value is the creature's to prove — no statistic is needed for it,
    // and none may override it. The supplied statistics are still *checked*:
    // a request this crate cannot make sense of is refused whether or not the
    // compensation would have used it.
    let invariant_value = structural_activation(creature, neuron_uuid)?;
    let supplied = stats;
    let stats = if invariant_value.is_some() {
        None
    } else {
        stats
    };

    let targets = outward_keys(creature, neuron_uuid)?;

    let mut cut = creature.clone();
    cut.neurons.retain(|n| n.uuid != neuron_uuid);
    let mut removed_synapses = Vec::new();
    cut.synapses.retain(|s| {
        let names_it = s.from_uuid == neuron_uuid || s.to_uuid == neuron_uuid;
        if names_it {
            removed_synapses.push(SynapseKey {
                from_uuid: s.from_uuid.clone(),
                to_uuid: s.to_uuid.clone(),
                role: parse_synapse_type(s.synapse_type.as_deref()),
            });
        }
        !names_it
    });

    if let Some(proxy) = supplied.and_then(|s| s.proxy.as_ref()) {
        check_proxy(&cut, neuron_uuid, proxy)?;
    }

    let mut bias_folds = Vec::new();
    let mut weight_shares = Vec::new();
    let mut uncompensated = Vec::new();

    for (target_uuid, role, squash, weight_sum) in targets {
        if squash.is_aggregate() {
            uncompensated.push(UncompensatedTarget {
                target_uuid,
                role,
                weight_sum,
                squash: squash_name_from(squash),
                reason: UncompensatedReason::AggregateTarget,
            });
            continue;
        }

        let Some(compensation) = compensate(invariant_value, stats, weight_sum) else {
            uncompensated.push(UncompensatedTarget {
                target_uuid,
                role,
                weight_sum,
                squash: squash_name_from(squash),
                reason: UncompensatedReason::NoStatistics,
            });
            continue;
        };

        // A share of exactly zero moves nothing — an uncorrelated survivor
        // predicts none of what went — so the edge it would have landed on is
        // not required to exist.
        if let Some(proxy) = stats.and_then(|s| s.proxy.as_ref())
            && compensation.share != 0.0
        {
            add_to_edge(&mut cut, &proxy.uuid, &target_uuid, compensation.share)?;
            weight_shares.push(WeightShare {
                from_uuid: proxy.uuid.clone(),
                to_uuid: target_uuid.clone(),
                delta: compensation.share,
            });
        }

        for neuron in &mut cut.neurons {
            if neuron.uuid == target_uuid {
                neuron.bias += compensation.bias_delta;
            }
        }
        bias_folds.push(BiasFold {
            target_uuid,
            weight_sum,
            delta: compensation.bias_delta,
            exact: compensation.exact,
            residual_variance: compensation.residual_variance,
        });
    }

    let outcome = cleanup_creature(&cut)?;

    // Exact means every term the removal took away was replaced by something
    // that computes the same number on every record: nothing may be left
    // uncompensated, every fold must be structural, and no `IF` may have been
    // downgraded — that repair is cleanup's one inexact rewrite.
    //
    // The `IF` clause is defence in depth rather than a branch a caller can
    // reach today: every route to a downgrade runs through an `IF` that the
    // removed neuron fed, and an `IF` is an aggregate, so `uncompensated` is
    // already non-empty. It stays because the two rules are independent —
    // widening what counts as compensable must not quietly start calling a
    // downgraded creature exact.
    let exact = uncompensated.is_empty()
        && bias_folds.iter().all(|f| f.exact)
        && outcome.downgraded_if_neurons.is_empty();

    Ok(PruneResult {
        creature: outcome.creature,
        removed_neuron: neuron_uuid.to_string(),
        removed_synapses,
        cascade_neurons: outcome.removed_neurons,
        cascade_synapses: outcome.removed_synapses,
        folded_neurons: outcome.folded_neurons,
        downgraded_if_neurons: outcome.downgraded_if_neurons,
        bias_folds,
        weight_shares,
        uncompensated,
        transform: if exact {
            TransformClass::Exact
        } else {
            TransformClass::Approximate
        },
        passes: outcome.passes,
    })
}

/// What one target's compensation came to.
struct Compensation {
    /// Added to the target's bias.
    bias_delta: f64,
    /// Added to the proxy's edge into the target; `0.0` when there is none.
    share: f64,
    /// True when the folded value is the neuron's activation on every record.
    exact: bool,
    /// Variance the compensation could not carry, when it is derivable.
    residual_variance: Option<f64>,
}

/// The compensation one target is owed, or `None` when there is nothing to
/// derive it from.
fn compensate(
    invariant_value: Option<f64>,
    stats: Option<&PruneStats>,
    weight_sum: f64,
) -> Option<Compensation> {
    if let Some(value) = invariant_value {
        return Some(Compensation {
            bias_delta: weight_sum * value,
            share: 0.0,
            exact: true,
            // The activation never moved, so nothing is left over.
            residual_variance: Some(0.0),
        });
    }

    let stats = stats?;
    let Some(proxy) = stats.proxy.as_ref() else {
        return Some(Compensation {
            bias_delta: weight_sum * stats.mean_activation,
            share: 0.0,
            exact: false,
            residual_variance: stats.variance.map(|v| weight_sum * weight_sum * v),
        });
    };

    // β = cov / σₛ² — the least-squares slope of the removed neuron on the
    // survivor. The survivor's edge takes `β · W`; the bias takes the mean the
    // survivor does not already deliver.
    let beta = proxy.covariance / proxy.variance;
    Some(Compensation {
        bias_delta: weight_sum * (stats.mean_activation - beta * proxy.mean_activation),
        share: weight_sum * beta,
        exact: false,
        residual_variance: stats.variance.map(|v| {
            weight_sum * weight_sum * (v - proxy.covariance * proxy.covariance / proxy.variance)
        }),
    })
}

/// Refuse anything that is not a hidden neuron of this creature.
fn classify_target(creature: &CreatureExport, uuid: &str) -> Result<(), PruneError> {
    if is_observation_uuid(creature, uuid) {
        return Err(PruneError::Protected {
            uuid: uuid.to_string(),
            kind: ProtectedKind::Observation,
        });
    }
    let neuron = creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .ok_or_else(|| PruneError::UnknownNeuron {
            uuid: uuid.to_string(),
        })?;

    match neuron.neuron_type.as_str() {
        "hidden" => Ok(()),
        "output" => Err(PruneError::Protected {
            uuid: uuid.to_string(),
            kind: ProtectedKind::Output,
        }),
        "constant" => Err(PruneError::Protected {
            uuid: uuid.to_string(),
            kind: ProtectedKind::Constant,
        }),
        declared => Err(PruneError::UnknownNeuronType {
            uuid: uuid.to_string(),
            declared: declared.to_string(),
        }),
    }
}

/// Is this the wire UUID of one of the creature's observation neurons?
///
/// Input neurons are not listed in `neurons` — the declared width is what says
/// they exist (Issue #550) — so the name is the only thing to test.
fn is_observation_uuid(creature: &CreatureExport, uuid: &str) -> bool {
    uuid.strip_prefix("input-")
        .and_then(|index| index.parse::<usize>().ok())
        .is_some_and(|index| index < creature.input)
}

/// The value the neuron activates to on **every** record, when the creature
/// alone proves there is one.
///
/// A neuron with no inward edge sums nothing, so its activation is fixed —
/// [`zero_inward_activation`] is the shared mirror of the forward pass for that
/// case, reused rather than restated. A neuron that *is* fed varies with the
/// record, and no amount of structure says otherwise.
fn structural_activation(creature: &CreatureExport, uuid: &str) -> Result<Option<f64>, PruneError> {
    if creature.synapses.iter().any(|s| s.to_uuid == uuid) {
        return Ok(None);
    }
    let neuron = creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .ok_or_else(|| PruneError::UnknownNeuron {
            uuid: uuid.to_string(),
        })?;
    let squash = squash_of(neuron)?;
    Ok(Some(f64::from(zero_inward_activation(squash, neuron.bias))))
}

/// The squash a target activates with, by UUID.
///
/// A constant emits its bias whatever it declares, so it reads as `IDENTITY` —
/// the same reading [`cleanup_creature`] takes, and a constant can never be a
/// target anyway.
fn target_squash(creature: &CreatureExport, uuid: &str) -> Result<SquashType, PruneError> {
    let neuron = creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .ok_or_else(|| {
            PruneError::Cleanup(CleanupError::UnknownEndpoint {
                uuid: uuid.to_string(),
            })
        })?;
    squash_of(neuron)
}

/// Parse one neuron's declared squash, reporting an unknown name the way
/// cleanup would rather than guessing a default.
fn squash_of(neuron: &crate::creature::NeuronExport) -> Result<SquashType, PruneError> {
    if neuron.neuron_type == "constant" {
        return Ok(SquashType::Identity);
    }
    parse_squash_name(neuron.squash.as_deref().unwrap_or("IDENTITY"))
        .map_err(|e| PruneError::Cleanup(CleanupError::from(e)))
}

/// What the neuron carried out of itself, one entry per **readable key**, in
/// first-edge order.
///
/// The key is `(target, role)`, and the role is only kept where the target can
/// tell roles apart: an `IF` holds a sum per role, every other squash sums
/// whatever reaches it, so two roles into one of those are the same term
/// written twice and are summed here. That is the same reading
/// [`cleanup_creature`] takes of an edge's identity, so a compensation and a
/// canonicalisation can never disagree about what one term is.
fn outward_keys(
    creature: &CreatureExport,
    uuid: &str,
) -> Result<Vec<(String, SynapseType, SquashType, f64)>, PruneError> {
    let mut order: Vec<(String, SynapseType)> = Vec::new();
    let mut totals: HashMap<(String, SynapseType), (SquashType, f64)> = HashMap::new();

    for synapse in creature.synapses.iter().filter(|s| s.from_uuid == uuid) {
        let squash = target_squash(creature, &synapse.to_uuid)?;
        let role = if squash == SquashType::If {
            parse_synapse_type(synapse.synapse_type.as_deref())
        } else {
            SynapseType::Standard
        };
        let key = (synapse.to_uuid.clone(), role);
        let entry = totals.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            (squash, 0.0)
        });
        entry.1 += synapse.weight;
    }

    Ok(order
        .into_iter()
        .map(|key| {
            let (squash, weight) = totals[&key];
            (key.0, key.1, squash, weight)
        })
        .collect())
}

/// Every supplied statistic must be a number a compensation can be derived
/// from, checked before a single edit is made.
fn check_stats(uuid: &str, stats: &PruneStats) -> Result<(), PruneError> {
    finite(uuid, "mean_activation", stats.mean_activation)?;
    if let Some(variance) = stats.variance {
        finite(uuid, "variance", variance)?;
        if variance < 0.0 {
            return Err(PruneError::NegativeVariance {
                uuid: uuid.to_string(),
                variance,
            });
        }
    }
    let Some(proxy) = stats.proxy.as_ref() else {
        return Ok(());
    };
    finite(&proxy.uuid, "mean_activation", proxy.mean_activation)?;
    finite(&proxy.uuid, "variance", proxy.variance)?;
    finite(&proxy.uuid, "covariance", proxy.covariance)?;
    if proxy.variance < 0.0 {
        return Err(PruneError::NegativeVariance {
            uuid: proxy.uuid.clone(),
            variance: proxy.variance,
        });
    }
    if proxy.variance == 0.0 {
        return Err(PruneError::DegenerateProxy {
            uuid: proxy.uuid.clone(),
            variance: proxy.variance,
        });
    }
    // Cauchy-Schwarz: `cov² <= σ² σₛ²` for any two series measured over the
    // same records. Beyond it the remedy would subtract more variance than the
    // neuron carried and report a negative residual, so the statistics are
    // refused rather than turned into a number no caller could act on. The
    // slack absorbs the rounding of a genuine `|ρ| = 1` sample.
    if let Some(variance) = stats.variance
        && proxy.covariance * proxy.covariance
            > variance * proxy.variance * (1.0 + COVARIANCE_SLACK)
    {
        return Err(PruneError::InconsistentCovariance {
            uuid: proxy.uuid.clone(),
            covariance: proxy.covariance,
            variance,
            proxy_variance: proxy.variance,
        });
    }
    Ok(())
}

/// Relative slack on the Cauchy-Schwarz bound, so a perfectly correlated sample
/// that rounds a hair over `σ² σₛ²` is not refused for it.
const COVARIANCE_SLACK: f64 = 1e-9;

fn finite(uuid: &str, field: &'static str, value: f64) -> Result<(), PruneError> {
    if value.is_finite() {
        return Ok(());
    }
    Err(PruneError::NonFiniteStatistic {
        uuid: uuid.to_string(),
        field,
        value,
    })
}

/// The proxy must be a neuron that survives the removal.
///
/// Whether it also *reaches* each target is settled per target by
/// [`add_to_edge`], because only there is the share it would carry known — a
/// survivor that predicts nothing carries nothing and needs no edge. A proxy
/// that must carry a share into a target it does not feed is a request this
/// crate cannot carry out as described, so the whole prune is refused rather
/// than half-applied: no partially compensated creature is ever returned. Call
/// without the proxy — or add the missing edge first — to prune anyway.
fn check_proxy(
    cut: &CreatureExport,
    removed_uuid: &str,
    proxy: &ProxyStats,
) -> Result<(), PruneError> {
    if proxy.uuid == removed_uuid
        || !(is_observation_uuid(cut, &proxy.uuid)
            || cut.neurons.iter().any(|n| n.uuid == proxy.uuid))
    {
        return Err(PruneError::UnknownProxy {
            uuid: proxy.uuid.clone(),
        });
    }
    Ok(())
}

/// Add `delta` to the existing `from -> to` edge, or refuse.
fn add_to_edge(
    creature: &mut CreatureExport,
    from_uuid: &str,
    to_uuid: &str,
    delta: f64,
) -> Result<(), PruneError> {
    let edge = creature
        .synapses
        .iter_mut()
        .find(|s| s.from_uuid == from_uuid && s.to_uuid == to_uuid)
        .ok_or_else(|| PruneError::MissingProxyEdge {
            from_uuid: from_uuid.to_string(),
            to_uuid: to_uuid.to_string(),
        })?;
    edge.weight += delta;
    Ok(())
}
