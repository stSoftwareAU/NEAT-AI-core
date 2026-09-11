//! Synapse pruning with typed-role and `IF`-aware rewrites (Issue #591).
//!
//! [`prune_synapse`] is the shared answer to "remove this one edge and give me
//! back something I can score". It cuts exactly the requested
//! `(from, to, role)` triple, optionally compensates the target that read it
//! with the **caller's** statistics, rewrites whatever `IF` structure the
//! removal made statically decidable, runs the Issue #589 cleanup fixed point
//! over the wreckage, and validates the stable result before returning it. A
//! successful call never returns an invalid creature.
//!
//! ```mermaid
//! flowchart TD
//!     Q["prune_synapse(creature, key, stats?)"] --> F{"does the creature carry<br/>that (from, to, role)?"}
//!     F -- no --> U["Err(UnknownSynapse)"]
//!     F -- yes --> S{"statistics supplied?"}
//!     S -- "yes, and not numbers" --> N["Err(NonFiniteStatistic /<br/>NegativeVariance / DegenerateProxy)"]
//!     S -- ok --> X["cut that one triple —<br/>never the rest of the pair"]
//!     X --> C["compensate the target:<br/>structural value, or the<br/>caller's mean and proxy;<br/>an aggregate with edges left gets neither,<br/>one with none takes the fold"]
//!     C --> V["an aggregate left with one edge —<br/>rewrite it to the point-wise squash<br/>that computes the same number"]
//!     V --> R["cleanup (IfRepair::Rewrite) —<br/>exact IF rewrites, cascade,<br/>fold, canonicalise, validate"]
//!     R -- fails --> E["Err(Cleanup)"]
//!     R -- passes --> O["Ok(PruneResult) —<br/>Exact or Approximate"]
//! ```
//!
//! # The role is part of what names an edge
//!
//! An `IF` keeps a sum per role, so one source may feed two of its branches
//! (Issue #577, NEAT-AI #3873). Removing "the `h-a → if-1` synapse" is
//! therefore not a request this crate can carry out — removing the pair would
//! delete a branch the caller never mentioned — so the request names the
//! triple and only that triple goes.
//!
//! At an `IF`, `positive` and an **untyped** row are one branch, not two: the
//! forward pass adds an untyped inward edge to the positive accumulator and
//! `IfRoles::tally` counts it as positive, so a request for either spelling
//! names the same edge. Reading the two apart is what would refuse "remove the
//! positive arm" on a creature that wrote that arm untyped.
//!
//! Everywhere else a role means nothing: every other squash sums whatever
//! reaches it, so two rows written for one pair are the same term written
//! twice, and the readable key is the pair. Asking for one role of such a pair
//! therefore takes the whole term, both rows — which is not "every same-pair
//! edge" being deleted but *one* edge being deleted in the two halves the
//! caller happened to write it in. A canonical creature never carries those
//! two rows in the first place: they are a `TypedDuplicateSynapse` the shared
//! validator rejects, so the case only arises for an input a caller built by
//! hand. One reading of what names an edge (`prune_cleanup::canonical_role`),
//! shared by the request and the canonicalisation, so the two can never
//! disagree.
//!
//! An edge sourced at an observation neuron, and an edge targeting an output
//! neuron, are ordinary candidates: nothing about the declared widths is
//! touched by removing one term from a sum.
//!
//! # `IF` structure is rewritten, never refused
//!
//! NEAT-AI's `SubConnection.ts::#wouldBreakIfNeuron` declines to remove an edge
//! that would leave an `IF` short a role, so a whole class of typed structure is
//! unreachable to the mutation operators. This crate rewrites instead, and both
//! rewrites are **exact** — they compute the same number on every record:
//!
//! | What the removal left | Rewrite |
//! |---|---|
//! | no condition edge, or every condition source structurally fixed | the branch the condition always takes, as an `IDENTITY` sum; the condition edges and the unreachable branch go, and their feeders cascade |
//! | a `positive` / `negative` branch with nothing left in it, condition still varying | a **zero-weight** edge from a support constant into that role — an empty branch sum is `0`, and so is `0 · 1` |
//!
//! A third rewrite sits beside them and is exact for the same reason — it
//! restores nothing and changes nothing, it just says what the creature already
//! computes in the form every later pass can read:
//!
//! | What the removal left | Rewrite |
//! |---|---|
//! | a non-`IF` **aggregate** with exactly one inward edge | the point-wise squash that reduces to the same number — `MINIMUM`/`MAXIMUM`/`MEAN` to `IDENTITY`, `HYPOTv2` (and `HYPOT` at bias `0`) to `ABSOLUTE` — named on [`PruneResult::converted_neurons`] ([`mod@crate::prune_rewrite`], Ockham #197) |
//!
//! Neither is a compensation: they restore what the creature *already*
//! computed once the requested edge was gone, so they never make a prune look
//! more faithful than it is. Losing the term itself is what
//! [`PruneResult::transform`] grades.
//!
//! # What the removal cost, and who pays for it
//!
//! The edge carried `w · a` into its target, where `a` is the **source's**
//! activation. So:
//!
//! - where the creature fixes `a` — the source is a constant, or has nothing
//!   to sum — `w · a` folds into the target's bias exactly, with no statistic
//!   needed and none allowed to override it;
//! - where it does not, the caller's [`PruneStats`] fold `w · μ` (and hand the
//!   correlated part to a supplied survivor), exactly as Issue #590's neuron
//!   removal does. A survivor that stands in for the source cannot *be* the
//!   source: the only edge it could carry the share on is the one just
//!   removed, so the share lands nowhere and the whole prune is refused with
//!   [`PruneError::MissingProxyEdge`] rather than half-applied;
//! - where the target **aggregates** and the cut leaves it something to
//!   aggregate — `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT`, or an `IF` reading one
//!   role's sum — no bias fold stands in for the term, so none is attempted
//!   and the target is named on [`PruneResult::uncompensated`] with the role it
//!   lost. That entry carries the magnitude of what went on
//!   [`UncompensatedTarget::dropped_mean`] — `w · μ`, or `w · a` where the
//!   creature fixes the source — so the caller's scorer can judge the loss. No
//!   magnitude refuses a prune (Ockham #197);
//! - where the cut leaves an aggregate with **no inward edge at all**, the
//!   forward pass reads it from its bias alone, so it takes the fold after all
//!   — `bias += W·μ`, or `bias += |W·μ|` for `HYPOT`, whose term is a
//!   magnitude, or `bias += W·μ` **with the squash rewritten to `ABSOLUTE`**
//!   for `HYPOTv2`, which never reads its bias with nothing to square
//!   (Ockham #196). `prune_neuron::fold_policy` is the one rule both
//!   entry points ask. An `IF` is excluded whatever it is left with: what it
//!   lost is a role, and [`IfRepair`] owns that.
//!
//! A shortfall normally ends any
//! [`TransformClass::Exact`](crate::prune_neuron::TransformClass::Exact) claim,
//! and one shape is the exception: an `IF` whose condition **the creature
//! itself decided the same way before and after the cut** never read the term
//! that went, so the flatten costs nothing. `prune_neuron::transform_class`
//! (Ockham #198) is the single home of that whole rule, and both entry points
//! ask it in the same terms, so a synapse removal and the neuron removal that
//! takes the same term away can never disagree about what it cost.

use crate::creature::{CreatureExport, parse_synapse_type, squash_name_from};
use crate::prune_cleanup::{
    CleanupOptions, IfRepair, SynapseKey, canonical_role, cleanup_creature_with, fixed_activation,
};
use crate::prune_neuron::{
    BiasFold, PruneError, PruneResult, PruneStats, TargetOutcome, UncompensatedReason,
    UncompensatedTarget, WeightShare, add_to_bias, add_to_edge, check_proxy, check_stats,
    compensate, dropped_mean, fold_bare_aggregate, fold_policy, inward_edge_count, target_squash,
    transform_class,
};
use crate::prune_rewrite::{SquashConversion, convert_single_edge_aggregates};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// Remove one typed synapse, compensate the target that read it, and return a
/// valid canonical creature.
///
/// `key` names the full `(from, to, role)` triple. The role is only read where
/// the target can tell roles apart — an `IF` — because every other squash sums
/// whatever reaches it.
///
/// `stats` is the caller's own measurement of the **source** neuron, whose
/// activation the removed edge carried. Without it the exact structural
/// rewrites still run and the target is named on
/// [`PruneResult::uncompensated`].
///
/// # Errors
///
/// Returns [`PruneError`] when the triple names no edge of this creature, when
/// a supplied statistic is not a usable number, when a supplied proxy cannot
/// carry the compensation, or when the creature that remains cannot be cleaned
/// into a valid canonical form. No creature is returned in any of those cases.
///
/// # Examples
///
/// ```
/// use neat_core::{PruneStats, SynapseKey, SynapseType, TransformClass, parse_creature_json, prune_synapse};
///
/// let creature = parse_creature_json(r#"{
///   "input":1,"output":1,"forwardOnly":true,
///   "neurons":[
///     {"type":"constant","uuid":"c-1","bias":0.5},
///     {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
///   ],
///   "synapses":[
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
///     {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"}
///   ]
/// }"#).unwrap();
///
/// let key = SynapseKey {
///     from_uuid: "c-1".to_string(),
///     to_uuid: "output-0".to_string(),
///     role: SynapseType::Standard,
/// };
/// let result = prune_synapse(&creature, &key, None).unwrap();
///
/// // The constant was worth 0.5 on every record and carried 0.2 of it, so
/// // 0.25 + 0.2 * 0.5 lands in the bias and nothing the creature computes moves.
/// assert!((result.creature.neurons[0].bias - 0.35).abs() < 1e-6);
/// assert_eq!(result.transform, TransformClass::Exact);
/// ```
pub fn prune_synapse(
    creature: &CreatureExport,
    key: &SynapseKey,
    stats: Option<&PruneStats>,
) -> Result<PruneResult, PruneError> {
    let squash = target_squash(creature, &key.to_uuid)?;
    let wanted = canonical_role(squash, key.role);

    let weight_sum = matched_weight(creature, key, squash, wanted)?;
    if let Some(stats) = stats {
        check_stats(&key.from_uuid, stats)?;
    }

    // A source the creature fixes is worth the same number on every record, so
    // the fold is provable from the structure alone — no statistic is needed
    // for it, and none may override it. Supplied statistics are still checked
    // above: a request this crate cannot make sense of is refused either way.
    let invariant_value = source_activation(creature, &key.from_uuid)?;
    let effective_stats = if invariant_value.is_some() {
        None
    } else {
        stats
    };

    let mut cut = creature.clone();
    let mut removed_synapses = Vec::new();
    cut.synapses.retain(|s| {
        let matches = s.from_uuid == key.from_uuid
            && s.to_uuid == key.to_uuid
            && canonical_role(squash, parse_synapse_type(s.synapse_type.as_deref())) == wanted;
        if matches {
            removed_synapses.push(SynapseKey {
                from_uuid: s.from_uuid.clone(),
                to_uuid: s.to_uuid.clone(),
                role: parse_synapse_type(s.synapse_type.as_deref()),
            });
        }
        !matches
    });

    if let Some(proxy) = stats.and_then(|s| s.proxy.as_ref()) {
        check_proxy(&cut, None, proxy)?;
    }

    let mut bias_folds = Vec::new();
    let mut weight_shares = Vec::new();
    let mut uncompensated = Vec::new();
    // Both rewrites a request can perform land here: the zero-edge fold's
    // `HYPOTv2 → ABSOLUTE` and the single-edge conversion below.
    let mut converted_neurons: Vec<SquashConversion> = Vec::new();

    if !fold_policy(squash, inward_edge_count(&cut, &key.to_uuid)) {
        uncompensated.push(UncompensatedTarget {
            target_uuid: key.to_uuid.clone(),
            role: wanted,
            weight_sum,
            squash: squash_name_from(squash),
            reason: UncompensatedReason::AggregateTarget,
            dropped_mean: dropped_mean(invariant_value, effective_stats, weight_sum),
        });
    } else if squash.is_aggregate() {
        // The cut left the aggregate with nothing to aggregate, so it takes
        // the fold its empty forward-pass form implies (Ockham #196).
        match fold_bare_aggregate(
            &mut cut,
            &key.to_uuid,
            wanted,
            squash,
            weight_sum,
            invariant_value,
            effective_stats,
        )? {
            TargetOutcome::Folded(fold, conversion) => {
                bias_folds.push(fold);
                converted_neurons.extend(conversion);
            }
            TargetOutcome::Uncompensated(entry) => uncompensated.push(entry),
        }
    } else if let Some(compensation) = compensate(invariant_value, effective_stats, weight_sum) {
        // A share of exactly zero moves nothing — an uncorrelated survivor
        // predicts none of what went — so the edge it would land on need not
        // exist.
        if let Some(proxy) = effective_stats.and_then(|s| s.proxy.as_ref())
            && compensation.share != 0.0
        {
            add_to_edge(&mut cut, &proxy.uuid, &key.to_uuid, compensation.share)?;
            weight_shares.push(WeightShare {
                from_uuid: proxy.uuid.clone(),
                to_uuid: key.to_uuid.clone(),
                delta: compensation.share,
            });
        }

        add_to_bias(&mut cut, &key.to_uuid, compensation.bias_delta)?;
        bias_folds.push(BiasFold {
            target_uuid: key.to_uuid.clone(),
            weight_sum,
            delta: compensation.bias_delta,
            exact: compensation.exact,
            residual_variance: compensation.residual_variance,
        });
    } else {
        uncompensated.push(UncompensatedTarget {
            target_uuid: key.to_uuid.clone(),
            role: wanted,
            weight_sum,
            squash: squash_name_from(squash),
            reason: UncompensatedReason::NoStatistics,
            // No statistic and no structural value is exactly the case
            // `dropped_mean` answers `None` for.
            dropped_mean: dropped_mean(invariant_value, effective_stats, weight_sum),
        });
    }

    // An aggregate the cut left with one term is no longer aggregating, so it is
    // rewritten to the point-wise squash that computes the same number before
    // cleanup sees it (Ockham #197). The cut moves the inward count of the
    // requested target and nothing else, so that is the one target examined.
    converted_neurons.extend(convert_single_edge_aggregates(
        &mut cut,
        std::slice::from_ref(&key.to_uuid),
    )?);

    let outcome = cleanup_creature_with(
        &cut,
        CleanupOptions {
            if_repair: IfRepair::Rewrite,
        },
    )?;

    // The `IF` rewrites are exact by construction, so they cannot spoil the
    // label. What can is a term the removal took away and nothing replaced,
    // and `transform_class` (Ockham #198) is the single home of that rule —
    // asked here in the same terms `prune_neuron` asks it, so a synapse
    // removal and the neuron removal that takes the same term away can never
    // disagree about what it cost.
    let transform = transform_class(creature, &outcome, &bias_folds, &uncompensated)?;

    Ok(PruneResult {
        creature: outcome.creature,
        removed_neuron: None,
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

/// Total weight the requested readable key carries, or a refusal.
///
/// Canonically one edge occupies a key; a creature that has not been through
/// cleanup may carry two rows the target sums into one term, and both go
/// together because they *are* one term.
fn matched_weight(
    creature: &CreatureExport,
    key: &SynapseKey,
    squash: SquashType,
    wanted: SynapseType,
) -> Result<f64, PruneError> {
    let mut total = 0.0;
    let mut found = false;
    for synapse in creature.synapses.iter().filter(|s| {
        s.from_uuid == key.from_uuid
            && s.to_uuid == key.to_uuid
            && canonical_role(squash, parse_synapse_type(s.synapse_type.as_deref())) == wanted
    }) {
        total += synapse.weight;
        found = true;
    }
    if found {
        Ok(total)
    } else {
        Err(PruneError::UnknownSynapse {
            from_uuid: key.from_uuid.clone(),
            to_uuid: key.to_uuid.clone(),
            role: key.role,
        })
    }
}

/// The value the **source** activates to on every record, when the creature
/// alone proves there is one.
///
/// [`fixed_activation`] is the single home of that question, asked here rather
/// than restated so the source fold, `prune_neuron`'s structural fold and the
/// `IF` static-condition rewrite cannot drift apart on what "fixed" means.
fn source_activation(creature: &CreatureExport, uuid: &str) -> Result<Option<f64>, PruneError> {
    Ok(fixed_activation(creature, uuid)?.map(f64::from))
}
