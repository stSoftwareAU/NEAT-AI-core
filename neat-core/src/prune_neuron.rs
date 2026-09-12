//! Hidden-neuron pruning with optional statistical compensation (Issue #590).
//!
//! [`prune_neuron`] is the shared answer to "remove this hidden neuron and give
//! me back something I can score". It cuts the requested neuron out, optionally
//! compensates the targets that read it using the **caller's** statistics, runs
//! the Issue #589 [`cleanup_creature_with`]
//! fixed point over the wreckage under
//! [`IfRepair::Rewrite`], and
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
//!     F --> L["cleanup (IfRepair::Rewrite) —<br/>exact IF rewrites, cascade,<br/>fold, canonicalise, validate"]
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
//!   on every record**. **Three** cases reach it, each provable from the
//!   creature alone: nothing read the neuron; the neuron had no inward edge, so
//!   it activated to one value on every record and that value folds into each
//!   target's bias; or every shortfall the removal left is one the `IF` rewrite
//!   proves cost nothing, which is the third case and is spelled out under
//!   "An `IF` short a role is rewritten, not downgraded" below. No statistic can
//!   buy this label, and a supplied mean never overrides the structural value.
//!   "Same number" means to the precision the forward pass works in: the folded
//!   value *is* the `f32` activation the pass would have produced for a neuron
//!   with nothing to sum, but the fold re-associates the sum, so the two agree
//!   to `f32` rounding rather than bit for bit.
//! - [`TransformClass::Approximate`] — everything else, and it is the honest
//!   answer far more often than not. A neuron whose activation varies is gone
//!   and the mean fold only replaces it *on average*; an `IF` still branching on
//!   a condition that varies lost a term the forward pass reads; an `IF` lost a
//!   condition term that moved which arm the forward pass reads; a target lost a
//!   term with no statistic to stand in for it.
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
//! that still has something to aggregate does not: a `MINIMUM` takes the
//! smallest inward term, a `MEAN` divides by its inward **count**, a `HYPOT`
//! squares each term, and an `IF` reads its condition sum to choose a branch.
//! Folding into their bias would be a number the caller could not justify, so
//! no fold is attempted there and the target is named on
//! [`PruneResult::uncompensated`] instead — reported, never silent. The same
//! entry records a target left uncompensated because no statistics were
//! supplied at all.
//!
//! # What an aggregate that keeps its edges answers with (Ockham #197)
//!
//! Reporting the target is not the whole answer, because what the removal left
//! behind decides what can still be said about it:
//!
//! | What the cut left the aggregate | What comes back |
//! |---|---|
//! | exactly **one** inward edge | the squash is rewritten to the point-wise one that computes the same number — `MINIMUM`/`MAXIMUM`/`MEAN` to `IDENTITY`, `HYPOTv2` (and `HYPOT` at bias `0`) to `ABSOLUTE` — and the rewrite is named on [`PruneResult::converted_neurons`]. Reducing one term is that term, so nothing the creature computes moves ([`mod@crate::prune_rewrite`]) |
//! | **two or more** inward edges | the squash stands: it is still reducing a range, and no point-wise form says the same thing |
//! | either way | the target is named on [`PruneResult::uncompensated`] carrying [`UncompensatedTarget::dropped_mean`] — the magnitude `W · μ` of the term that went, where a statistic or the creature's own structure proves one |
//!
//! **No magnitude refuses a prune.** `dropped_mean` is reported so the caller's
//! scorer can judge the loss; nothing in this crate compares it against a
//! threshold, because deciding whether a creature is still worth keeping is the
//! caller's half of the Issue #587 boundary. Statistics this crate cannot make
//! sense of are a different matter and still refuse outright.
//!
//! # An aggregate left with **no** inward edge takes the fold (Ockham #196)
//!
//! `fold_policy` is the single rule, asked by this module and by
//! [`mod@crate::prune_synapse`] alike. Once the cut leaves an aggregate with
//! nothing to aggregate, the forward pass stops reading a set of terms and
//! evaluates the neuron from its bias alone
//! (`prune_cleanup::zero_inward_activation`) — a point-wise reading
//! again — so the fold is the closest creature there is rather than a number
//! nobody can justify. The shape of the fold follows the empty form:
//!
//! | Squash | one inward term | no inward term | the fold |
//! |---|---|---|---|
//! | `MINIMUM` / `MAXIMUM` / `MEAN` | `W·a + bias` | `bias` | `bias += W·μ` |
//! | `HYPOT` | `\|W·a\| + bias` | `bias` | `bias += \|W·μ\|` — the term is a magnitude |
//! | `HYPOTv2` | `\|bias + W·a\|` | `0`, the bias never read | `bias += W·μ` **and the squash becomes `ABSOLUTE`** |
//!
//! `HYPOTv2` is the one squash a **zero-edge fold** rewrites — the single-edge
//! conversion table above rewrites four more, and both report what they did on
//! [`PruneResult::converted_neurons`], so a caller never has to discover a
//! squash it did not send in. With no inward edge its bias lives inside a
//! per-synapse square
//! that no longer exists, so the forward pass answers `0` and a bias fold alone
//! would change nothing; `ABSOLUTE` over the folded bias computes
//! `\|bias + W·μ\|`, which is what `HYPOTv2` computed with the term still
//! there. The two forms share an activation range — both `[0, f32::MAX]` — so
//! the replacement cannot answer a value the original would have clamped away.
//!
//! An `IF` is excluded whatever it is left with: rule 12 means an `IF` short an
//! edge is short a **role**, and repairing that is
//! [`crate::prune_cleanup::IfRepair`]'s job, not a number's.
//!
//! No correlated survivor helps an **aggregate** target with no inward edge — a
//! share can only land on an edge into that target, and there is none — so the
//! fold there is mean-only and a supplied proxy goes unused. It is still
//! *checked*: a proxy that is not a number, not a survivor, or not consistent
//! with the variances refuses the whole prune as it always did. A **point-wise**
//! target left bare is unchanged by Ockham #196 and still takes the ordinary
//! path, so a proxy with a non-zero share into it is refused with
//! [`PruneError::MissingProxyEdge`] — there is no edge left to carry it.
//! # An `IF` short a role is rewritten, not downgraded
//!
//! Removing a neuron can leave an `IF` without a `condition`, a `positive` or a
//! negative arm. This entry point asks cleanup for
//! [`IfRepair::Rewrite`] — the
//! exact repair [`crate::prune_synapse::prune_synapse`] already used (Ockham
//! #198) — so what comes back is the closest creature that computes the **same
//! number on every record**: the `IDENTITY` sum of the arm a statically decided
//! condition always takes, or a zero-weight support edge giving back the arm the
//! removal emptied. It never asks for the TypeScript-parity downgrade, so
//! [`PruneResult::downgraded_if_neurons`] is always empty here and
//! [`PruneResult::static_if_neurons`] / [`PruneResult::restored_if_roles`] carry
//! what happened instead.
//! [`crate::prune_cleanup::cleanup_creature`]'s own default
//! policy is unchanged, and [`crate::prune_fixtures`]'s captures still have a
//! caller that reproduces them.
//!
//! Rewriting exactly is not the same as costing nothing. The rewrite reads the
//! creature the **cut** left behind, so an `IF` whose condition the removal
//! emptied flattens onto the arm an empty condition takes — which is the arm the
//! original creature took only when the original decided its condition the same
//! way. `shortfall_costs_nothing` is where that is proved, and it is the only
//! route by which an aggregate shortfall still reaches
//! [`TransformClass::Exact`]: a condition term the creature itself fixed, whose
//! loss leaves the branch where it was, was never read for anything else.
//!
//! # The memetic record is pruned, not dropped
//!
//! TypeScript drops `memetic` wholesale on every removal because its content
//! hash no longer describes the creature. This crate owns the finer-grained
//! inverse of validation rule 31 (`CreatureExport::prune_memetic`,
//! NEAT-AI-Lamarck#197) and cleanup applies it, so what comes back keeps every
//! entry that still names live structure and loses exactly the dangling ones.
//! That is Issue #590's call on the choice the parity matrix left open: the
//! fine-tuning history a caller measured is worth more than a blunt reset, and
//! rule 31 is satisfied either way.

use std::collections::HashMap;

use crate::creature::{CreatureExport, parse_squash_name, parse_synapse_type, squash_name_from};
use crate::prune_cleanup::{
    CleanupError, CleanupOptions, CleanupOutcome, IfRepair, StaticIfRewrite, SynapseKey,
    cleanup_creature_with, fixed_activation, is_observation_uuid, static_condition_branch,
};
use crate::prune_rewrite::{SquashConversion, convert_single_edge_aggregates};
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
    /// True when the folded value is what the removed term was worth on
    /// **every** record, so the fold changes nothing the creature computes.
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
    /// The target's squash aggregates the inward terms it **still has**, so a
    /// bias fold does not stand in for the term the removal took away. An
    /// aggregate the cut left with no inward edge at all is folded instead
    /// (Ockham #196); an `IF` is never folded, because what it lost is a role.
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
    /// Magnitude of the term the target lost, where a number proves one
    /// (Ockham #197).
    ///
    /// `Some(weight_sum · μ)` when the caller supplied statistics, and
    /// `Some(weight_sum · a)` when the creature itself fixes the source's
    /// activation `a` — the same precedence the compensation takes, so the
    /// structural value outranks a supplied mean here too. `None` where neither
    /// exists, which is every [`UncompensatedReason::NoStatistics`] entry: no
    /// magnitude is invented to fill the gap.
    ///
    /// This is reported, never enforced. Nothing in this crate refuses a prune
    /// for the size of what it dropped — judging the loss is the scorer's half
    /// of the Issue #587 boundary.
    pub dropped_mean: Option<f64>,
}

/// The creature a prune produced, and everything the removal cost.
#[derive(Debug, Clone, PartialEq)]
pub struct PruneResult {
    /// The canonical, validated creature.
    pub creature: CreatureExport,
    /// Wire UUID of the neuron the caller asked to remove, or `None` when the
    /// request named a **synapse** ([`crate::prune_synapse::prune_synapse`], Issue #591).
    pub removed_neuron: Option<String>,
    /// The edges the request itself took: every edge naming the removed neuron
    /// for [`prune_neuron`], the one requested triple for
    /// [`crate::prune_synapse::prune_synapse`].
    pub removed_synapses: Vec<SynapseKey>,
    /// Neurons the cleanup cascade removed on top of the requested one.
    pub cascade_neurons: Vec<String>,
    /// Synapses the cascade removed alongside them.
    pub cascade_synapses: Vec<SynapseKey>,
    /// Hidden neurons the cascade folded into constant support.
    pub folded_neurons: Vec<String>,
    /// `IF` neurons downgraded to `IDENTITY` because a role went with the
    /// removal — the one cleanup rewrite that is not exact.
    ///
    /// **Always empty.** Both entry points ask cleanup for
    /// [`IfRepair::Rewrite`] and take
    /// the exact rewrites below instead (Issue #591 for the synapse path,
    /// Ockham #198 for the neuron path). The field stays because
    /// [`crate::prune_cleanup::cleanup_creature`]'s own
    /// default policy still downgrades for the TypeScript-parity captures, and
    /// a non-empty list here would mean a caller had gone back to it.
    pub downgraded_if_neurons: Vec<String>,
    /// `IF` neurons the removal left with a condition the creature itself
    /// decides, flattened to the branch that survives (Issue #591).
    pub static_if_neurons: Vec<StaticIfRewrite>,
    /// Zero-weight support edges added to give an `IF` back a branch role the
    /// removal emptied (Issue #591). Each one is a neuron the caller's creature
    /// did not name, so it is reported rather than left to be discovered.
    pub restored_if_roles: Vec<SynapseKey>,
    /// The mean folds applied, one per compensated target.
    pub bias_folds: Vec<BiasFold>,
    /// The correlated-survivor shares applied.
    pub weight_shares: Vec<WeightShare>,
    /// Targets that read the removed neuron and got nothing back.
    pub uncompensated: Vec<UncompensatedTarget>,
    /// Aggregates the cut left with a single inward edge, rewritten to the
    /// point-wise squash that computes the same number (Ockham #197). Every
    /// conversion is exact, so none of them moves
    /// [`transform`](Self::transform).
    pub converted_neurons: Vec<SquashConversion>,
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
    /// The creature carries no synapse with that `(from, to, role)` triple.
    ///
    /// The role is part of the identity (Issue #577): asking for a role a pair
    /// does not carry names no edge, and removing "the other one" instead
    /// would delete structure the caller never asked about.
    UnknownSynapse {
        /// Wire UUID of the source asked for.
        from_uuid: String,
        /// Wire UUID of the target asked for.
        to_uuid: String,
        /// The role asked for.
        role: SynapseType,
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
            PruneError::UnknownSynapse {
                from_uuid,
                to_uuid,
                role,
            } => write!(
                f,
                "No synapse {from_uuid} -> {to_uuid} ({role:?}) to remove"
            ),
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
    // The targets whose inward count the cut moves, named before the loop
    // consumes them: cutting the neuron removes its *inward* edges too, but
    // those only cost their sources an outward edge, so nothing else is touched.
    let touched_targets: Vec<String> = targets.iter().map(|(uuid, ..)| uuid.clone()).collect();

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
        check_proxy(&cut, Some(neuron_uuid), proxy)?;
    }

    let mut bias_folds = Vec::new();
    let mut weight_shares = Vec::new();
    let mut uncompensated = Vec::new();
    // Both rewrites a request can perform land here: the zero-edge fold's
    // `HYPOTv2 → ABSOLUTE` below, and the single-edge conversion after the
    // loop. One list, so a caller reads every squash that moved in one place.
    let mut converted_neurons: Vec<SquashConversion> = Vec::new();

    for (target_uuid, role, squash, weight_sum) in targets {
        if !fold_policy(squash, inward_edge_count(&cut, &target_uuid)) {
            uncompensated.push(UncompensatedTarget {
                target_uuid,
                role,
                weight_sum,
                squash: squash_name_from(squash),
                reason: UncompensatedReason::AggregateTarget,
                dropped_mean: dropped_mean(invariant_value, stats, weight_sum),
            });
            continue;
        }

        // An aggregate the cut left with nothing to aggregate is foldable, but
        // in the shape its empty forward-pass form implies rather than the
        // point-wise one below.
        if squash.is_aggregate() {
            match fold_bare_aggregate(
                &mut cut,
                &target_uuid,
                role,
                squash,
                weight_sum,
                invariant_value,
                stats,
            )? {
                TargetOutcome::Folded(fold, conversion) => {
                    bias_folds.push(fold);
                    converted_neurons.extend(conversion);
                }
                TargetOutcome::Uncompensated(entry) => uncompensated.push(entry),
            }
            continue;
        }

        let Some(compensation) = compensate(invariant_value, stats, weight_sum) else {
            uncompensated.push(UncompensatedTarget {
                target_uuid,
                role,
                weight_sum,
                squash: squash_name_from(squash),
                reason: UncompensatedReason::NoStatistics,
                // No statistic and no structural value is exactly the case
                // `dropped_mean` answers `None` for.
                dropped_mean: dropped_mean(invariant_value, stats, weight_sum),
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

        add_to_bias(&mut cut, &target_uuid, compensation.bias_delta)?;
        bias_folds.push(BiasFold {
            target_uuid,
            weight_sum,
            delta: compensation.bias_delta,
            exact: compensation.exact,
            residual_variance: compensation.residual_variance,
        });
    }

    // An aggregate the cut left with one term is no longer aggregating, so it is
    // rewritten to the point-wise squash that computes the same number before
    // cleanup sees it — every later pass then reads a sum rather than a
    // reduction (Ockham #197). The rewrite is exact, so it cannot spoil the
    // label below.
    converted_neurons.extend(convert_single_edge_aggregates(&mut cut, &touched_targets)?);

    // `IfRepair::Rewrite`, the same policy `prune_synapse` asks for: an `IF`
    // the removal left short of a role is rewritten into the closest form that
    // computes the same number on every record, never blanket-downgraded to
    // `IDENTITY`. `cleanup_creature`'s own default is untouched, so the
    // TypeScript-parity captures still have a caller that reproduces them.
    let outcome = cleanup_creature_with(
        &cut,
        CleanupOptions {
            if_repair: IfRepair::Rewrite,
        },
    )?;

    let transform = transform_class(creature, &outcome, &bias_folds, &uncompensated)?;

    Ok(PruneResult {
        creature: outcome.creature,
        removed_neuron: Some(neuron_uuid.to_string()),
        removed_synapses,
        cascade_neurons: outcome.removed_neurons,
        cascade_synapses: outcome.removed_synapses,
        folded_neurons: outcome.folded_neurons,
        downgraded_if_neurons: outcome.downgraded_if_neurons,
        static_if_neurons: outcome.static_if_neurons,
        restored_if_roles: outcome.restored_if_roles,
        bias_folds,
        weight_shares,
        uncompensated,
        converted_neurons,
        transform,
        passes: outcome.passes,
    })
}

/// How faithful the whole rewrite was, from what it left behind.
///
/// `Exact` means every term the removal took away was replaced by something
/// that computes the same number on every record: every fold must be
/// structural, every shortfall must be one the `IF` rewrite proves cost
/// nothing, and no `IF` may have been downgraded.
///
/// The downgrade clause is defence in depth rather than a branch a caller can
/// reach: both entry points ask for [`IfRepair::Rewrite`], which never fills
/// [`crate::prune_cleanup::CleanupOutcome::downgraded_if_neurons`] at all. It
/// stays because the rules are independent — a future policy change must not
/// quietly start calling a downgraded creature exact.
///
/// This is the **one** home of that conjunction. `prune_neuron` and
/// [`crate::prune_synapse::prune_synapse`] both ask it rather than restating
/// it, so a rule added here can never reach one entry point and not the other.
///
/// # Errors
///
/// Returns [`CleanupError::Creature`] when a condition source of a shortfall's
/// target declares a squash name this crate does not know.
pub(crate) fn transform_class(
    before: &CreatureExport,
    outcome: &CleanupOutcome,
    bias_folds: &[BiasFold],
    uncompensated: &[UncompensatedTarget],
) -> Result<TransformClass, CleanupError> {
    let mut exact = bias_folds.iter().all(|f| f.exact) && outcome.downgraded_if_neurons.is_empty();
    for target in uncompensated {
        exact = exact && shortfall_costs_nothing(before, &outcome.static_if_neurons, target)?;
    }
    Ok(if exact {
        TransformClass::Exact
    } else {
        TransformClass::Approximate
    })
}

/// Did this shortfall cost the creature nothing at all?
///
/// A target named on [`PruneResult::uncompensated`] got no fold, which is
/// normally the end of any exactness claim. One shape is the exception, and it
/// is provable from the creature alone: an `IF` whose condition **the creature
/// itself decided both before and after the cut, the same way**.
///
/// The condition's only job is to pick an arm. When [`static_condition_branch`]
/// answers the same arm for the creature the caller handed in and for the one
/// cleanup flattened, the pick never moved, so a condition term the removal
/// took away was never read for anything else — and a term it took out of the
/// arm the pick *discards* was never read at all. Both cost nothing, and the
/// flattened `IDENTITY` computes what the `IF` computed on every record.
///
/// A term the removal took out of the **surviving** arm is a real loss, and so
/// is every other shortfall: a point-wise target left without statistics, a
/// non-`IF` aggregate, an `IF` the rewrite could not flatten because its
/// condition still varies. All of those answer `false`.
///
/// # Errors
///
/// Returns [`CleanupError::Creature`] when a condition source of the target
/// declares a squash name this crate does not know.
pub(crate) fn shortfall_costs_nothing(
    before: &CreatureExport,
    static_if_neurons: &[StaticIfRewrite],
    target: &UncompensatedTarget,
) -> Result<bool, CleanupError> {
    if target.squash != squash_name_from(SquashType::If) {
        return Ok(false);
    }
    // Not flattened means the `IF` still branches on a condition that varies,
    // so the term it lost is a term the forward pass still reads.
    let Some(rewrite) = static_if_neurons
        .iter()
        .find(|r| r.uuid == target.target_uuid)
    else {
        return Ok(false);
    };
    // Flattened, but was the arm it settled on the arm the caller's creature
    // always took? Only the caller's creature can answer that, and only when
    // it decided the condition itself.
    if static_condition_branch(before, &target.target_uuid)? != Some(rewrite.branch) {
        return Ok(false);
    }
    // The arm that survives is the one the flatten kept — an untyped edge
    // belongs to the positive arm, the same reading the flatten takes.
    let survives = match rewrite.branch {
        SynapseType::Negative => target.role == SynapseType::Negative,
        _ => matches!(target.role, SynapseType::Positive | SynapseType::Standard),
    };
    Ok(!survives)
}

/// What one target's compensation came to.
pub(crate) struct Compensation {
    /// Added to the target's bias.
    pub(crate) bias_delta: f64,
    /// Added to the proxy's edge into the target; `0.0` when there is none.
    pub(crate) share: f64,
    /// True when the folded value is what the removed term was worth on every
    /// record.
    pub(crate) exact: bool,
    /// Variance the compensation could not carry, when it is derivable.
    pub(crate) residual_variance: Option<f64>,
}

/// The compensation one target is owed, or `None` when there is nothing to
/// derive it from.
pub(crate) fn compensate(
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

/// The magnitude of the term a target lost, where a number proves one.
///
/// The same precedence [`compensate`] takes, and deliberately so: the value the
/// creature fixes outranks a supplied mean, and without either there is no
/// magnitude to report rather than a zero that would read as "nothing was
/// lost". Reported on [`UncompensatedTarget::dropped_mean`]; nothing is refused
/// for its size.
pub(crate) fn dropped_mean(
    invariant_value: Option<f64>,
    stats: Option<&PruneStats>,
    weight_sum: f64,
) -> Option<f64> {
    if let Some(value) = invariant_value {
        return Some(weight_sum * value);
    }
    Some(weight_sum * stats?.mean_activation)
}

/// May a target carrying `target_squash` take a bias fold for the term the
/// removal took away?
///
/// One rule, two callers — [`prune_neuron`] and
/// [`crate::prune_synapse::prune_synapse`] — so a neuron removal and a synapse
/// removal can never disagree about what a target is owed (Ockham #196).
///
/// - A **point-wise** squash computes `squash(bias + Σ w·a)`, so the bias is
///   exactly where the removed term sat. Always foldable.
/// - An **aggregate** reads its inward terms as a set — the smallest, the
///   largest, the mean, the root of the sum of squares — and no number in its
///   bias stands in for one of them while the set is non-empty. With
///   `remaining_inward_edges == 0` the forward pass evaluates it from its bias
///   alone (`prune_cleanup::zero_inward_activation`), which is a
///   point-wise reading again, so the fold is the closest creature there is.
/// - `IF` is never foldable, whatever it is left with: rule 12 means an `IF`
///   short an edge is short a **role**, and that is
///   [`crate::prune_cleanup::IfRepair`]'s repair to make, not a number's.
pub(crate) fn fold_policy(target_squash: SquashType, remaining_inward_edges: usize) -> bool {
    if target_squash == SquashType::If {
        return false;
    }
    if target_squash.is_aggregate() {
        return remaining_inward_edges == 0;
    }
    true
}

/// How many synapses still point at `uuid`.
///
/// Counted on the **cut** creature, because `fold_policy` asks what the
/// target is left with, not what it started from.
pub(crate) fn inward_edge_count(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.to_uuid == uuid)
        .count()
}

/// What one target's compensation came to, or why it got none.
///
/// Both arms are reported: a fold lands on [`PruneResult::bias_folds`] and a
/// refusal on [`PruneResult::uncompensated`], so nothing a target lost is ever
/// silent.
pub(crate) enum TargetOutcome {
    /// The fold that was applied, and the squash rewrite it needed to be
    /// readable at all — `Some` only for the `HYPOTv2` target of
    /// [`zero_edge_rewrite`].
    ///
    /// The rewrite is reported for the same reason
    /// [`crate::prune_rewrite::convert_single_edge_aggregates`] reports its
    /// own: a caller must never have to *discover* that the squash it sent in
    /// is not the squash it got back.
    Folded(BiasFold, Option<SquashConversion>),
    /// The target that got nothing, and why.
    Uncompensated(UncompensatedTarget),
}

/// The squash a target with **no** inward edge must be rewritten to for its
/// bias to be read at all.
///
/// `HYPOTv2` computes `sqrt(Σ(bias + w·a)²)`, so its bias lives inside a
/// per-synapse square: with no synapse left the forward pass answers `0` and
/// never reads the bias, and a fold into it would change nothing. `ABSOLUTE`
/// over the folded bias computes `|bias + W·x|`, which is what `HYPOTv2`
/// computed with the term still there. Every other form reads its bias with
/// nothing inward, so none of them is rewritten.
fn zero_edge_rewrite(squash: SquashType) -> Option<SquashType> {
    match squash {
        SquashType::HypotenuseV2 => Some(SquashType::Absolute),
        _ => None,
    }
}

/// Compensate a target the cut left with **no inward edge**, or say why it
/// could not be — the one home of the Ockham #196 rule's *action*, as
/// [`fold_policy`] is the one home of its decision.
///
/// [`prune_neuron`] and [`crate::prune_synapse::prune_synapse`] both call it,
/// so the fold a neuron removal applies and the fold a synapse removal applies
/// cannot drift apart.
///
/// `W` is the total weight the removed source carried into the target and `x`
/// is what that source was worth — the value the creature fixes, which no
/// statistic may override, or the mean the caller measured. The term the
/// target lost is `W · x`, read the way that squash reads one term (see the
/// table in the module documentation), and a supplied proxy is not used **for
/// this target**: a share can only land on an edge into it, and the cut left
/// none. The proxy is still *checked* — a request this crate cannot make sense
/// of is refused whether or not the compensation would have used it.
///
/// `W` is a **sum** over the readable key, which is what `MINIMUM`, `MAXIMUM`
/// and `MEAN` read: each takes `w·a` per term, so one term written as two rows
/// is the same term twice. `HYPOT` squares each row separately, so `|W·μ|`
/// answers for it only where the key holds one row — which is every canonical
/// creature, a second row for a pair being a `TypedDuplicateSynapse` the
/// shared validator rejects.
///
/// # Errors
///
/// Returns [`PruneError::Cleanup`] when `target_uuid` names no neuron of
/// `cut` — a fold that could not land is refused rather than reported as
/// applied.
pub(crate) fn fold_bare_aggregate(
    cut: &mut CreatureExport,
    target_uuid: &str,
    role: SynapseType,
    squash: SquashType,
    weight_sum: f64,
    invariant_value: Option<f64>,
    stats: Option<&PruneStats>,
) -> Result<TargetOutcome, PruneError> {
    // A share has nowhere to land here, so the proxy is dropped before the
    // arithmetic rather than after it.
    let mean_only = stats.map(|s| PruneStats {
        mean_activation: s.mean_activation,
        variance: s.variance,
        proxy: None,
    });
    let Some(compensation) = compensate(invariant_value, mean_only.as_ref(), weight_sum) else {
        return Ok(TargetOutcome::Uncompensated(UncompensatedTarget {
            target_uuid: target_uuid.to_string(),
            role,
            weight_sum,
            squash: squash_name_from(squash),
            reason: UncompensatedReason::NoStatistics,
            // No statistic and no structural value is exactly the case
            // `dropped_mean` answers `None` for (Ockham #197).
            dropped_mean: dropped_mean(invariant_value, stats, weight_sum),
        }));
    };

    // `HYPOT` reads one term as `|w·a|`, so a **magnitude** is what folds: the
    // residual is `Var(|W·a|)`, which the caller's `σ²` does not describe, and
    // none is claimed rather than one that cannot be justified. `HYPOTv2` is
    // not that case — its fold is linear in the sum, so the residual keeps the
    // `W² σ²` shape every other fold reports, and the magnitude it is read
    // through afterwards is the squash's, exactly as `LOGISTIC` is on the
    // point-wise path.
    let folds_a_magnitude = squash == SquashType::Hypotenuse;
    let bias_delta = if folds_a_magnitude {
        compensation.bias_delta.abs()
    } else {
        compensation.bias_delta
    };
    let residual_variance = if folds_a_magnitude && !compensation.exact {
        None
    } else {
        compensation.residual_variance
    };

    add_to_bias(cut, target_uuid, bias_delta)?;
    let conversion = match zero_edge_rewrite(squash) {
        Some(rewrite) => {
            set_squash(cut, target_uuid, rewrite)?;
            Some(SquashConversion {
                uuid: target_uuid.to_string(),
                from: squash_name_from(squash),
                to: squash_name_from(rewrite),
            })
        }
        None => None,
    };
    Ok(TargetOutcome::Folded(
        BiasFold {
            target_uuid: target_uuid.to_string(),
            weight_sum,
            delta: bias_delta,
            exact: compensation.exact,
            residual_variance,
        },
        conversion,
    ))
}

/// Add `delta` to `uuid`'s bias, or refuse.
///
/// The single home of that edit, so a fold that names a neuron the creature
/// does not carry fails loudly instead of quietly moving nothing while the
/// result claims it moved.
///
/// # Errors
///
/// Returns [`PruneError::Cleanup`] when `uuid` names no neuron.
pub(crate) fn add_to_bias(
    creature: &mut CreatureExport,
    uuid: &str,
    delta: f64,
) -> Result<(), PruneError> {
    neuron_mut(creature, uuid)?.bias += delta;
    Ok(())
}

/// Rewrite `uuid`'s declared squash, or refuse.
///
/// # Errors
///
/// Returns [`PruneError::Cleanup`] when `uuid` names no neuron.
fn set_squash(
    creature: &mut CreatureExport,
    uuid: &str,
    squash: SquashType,
) -> Result<(), PruneError> {
    neuron_mut(creature, uuid)?.squash = Some(squash_name_from(squash).to_string());
    Ok(())
}

/// The neuron `uuid` names, or the endpoint error cleanup would raise for it.
fn neuron_mut<'a>(
    creature: &'a mut CreatureExport,
    uuid: &str,
) -> Result<&'a mut crate::creature::NeuronExport, PruneError> {
    creature
        .neurons
        .iter_mut()
        .find(|n| n.uuid == uuid)
        .ok_or_else(|| {
            PruneError::Cleanup(CleanupError::UnknownEndpoint {
                uuid: uuid.to_string(),
            })
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

/// The value the neuron activates to on **every** record, when the creature
/// alone proves there is one.
///
/// [`fixed_activation`] is the single home of that question (Issue #591), asked
/// here rather than restated so a fold and an `IF` rewrite can never disagree
/// about what "fixed" means. A neuron that *is* fed varies with the record, and
/// no amount of structure says otherwise.
fn structural_activation(creature: &CreatureExport, uuid: &str) -> Result<Option<f64>, PruneError> {
    Ok(fixed_activation(creature, uuid)?.map(f64::from))
}

/// The squash a target activates with, by UUID.
///
/// A constant emits its bias whatever it declares, so it reads as `IDENTITY` —
/// the same reading cleanup takes, and a constant can never be a
/// target anyway.
pub(crate) fn target_squash(
    creature: &CreatureExport,
    uuid: &str,
) -> Result<SquashType, PruneError> {
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
pub(crate) fn squash_of(neuron: &crate::creature::NeuronExport) -> Result<SquashType, PruneError> {
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
/// cleanup takes of an edge's identity, so a compensation and a
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
pub(crate) fn check_stats(uuid: &str, stats: &PruneStats) -> Result<(), PruneError> {
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
/// `removed` names the neuron the request deletes, when it deletes one: a
/// neuron prune (Issue #590) passes it, because a survivor cannot be the node
/// that just went, and a synapse prune (Issue #591) passes `None`, because it
/// deletes no neuron at all.
///
/// Whether the proxy also *reaches* each target is settled per target by
/// [`add_to_edge`], because only there is the share it would carry known — a
/// survivor that predicts nothing carries nothing and needs no edge. A proxy
/// that must carry a share into a target it does not feed is a request this
/// crate cannot carry out as described, so the whole prune is refused rather
/// than half-applied: no partially compensated creature is ever returned. Call
/// without the proxy — or add the missing edge first — to prune anyway.
pub(crate) fn check_proxy(
    cut: &CreatureExport,
    removed: Option<&str>,
    proxy: &ProxyStats,
) -> Result<(), PruneError> {
    if removed == Some(proxy.uuid.as_str())
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
pub(crate) fn add_to_edge(
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
