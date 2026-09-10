//! Rewriting an aggregate the cut left with a single inward edge (Ockham #197).
//!
//! A cut takes one term out of a target's sum. Where the target **aggregates**
//! — `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT`, `HYPOTv2` — no bias fold stands in
//! for the term it lost ([`mod@crate::prune_neuron`]), so the target is reported on
//! [`PruneResult::uncompensated`](crate::prune_neuron::PruneResult::uncompensated)
//! and left alone. But an aggregate over **one** term is no longer aggregating
//! anything: reducing a single value is that value. So the squash can be
//! rewritten to the point-wise one that computes the same number, and every
//! later pass — a second prune, a bias fold, cleanup's own merge rules — reads
//! a sum rather than a reduction.
//!
//! `convert_single_edge_aggregates` is that rewrite, and it is **exact**: each
//! rule below is the equality between two arms of
//! [`crate::network::CompiledNetwork::activate`], so nothing the creature
//! computes moves. Only the targets the request itself touched are examined —
//! cleanup owns the canonical form of everything else, and a sweep over the
//! whole creature would rewrite structure no caller asked about.
//!
//! # The rules, each read off the forward pass
//!
//! Write `w · a` for the one surviving term and `b` for the target's bias.
//!
//! | Aggregate | With one term the forward pass computes | Replacement | Bias |
//! |---|---|---|---|
//! | `MINIMUM` / `MAXIMUM` | the extreme of one term, `w·a + b` | `IDENTITY` | unchanged |
//! | `MEAN` | `w·a / 1 + b` | `IDENTITY` | unchanged |
//! | `HYPOTv2` | `sqrt((b + w·a)²)`, the magnitude of `b + w·a` | `ABSOLUTE` | unchanged |
//! | `HYPOT`, `b == 0` | `sqrt((w·a)²) + b`, the magnitude of `w·a` | `ABSOLUTE` | unchanged |
//! | `HYPOT`, `b != 0` | the magnitude of `w·a`, **plus** `b` | **kept** | — |
//! | `IF` | the branch its condition sum picks | **kept** | — |
//!
//! `HYPOT` **adds** its bias to the root where `ABSOLUTE` folds the bias inside
//! it, so `abs(w·a) + b` and `abs(b + w·a)` are the same number only at
//! `b == 0`.
//! Away from zero the `HYPOT` is kept, which costs nothing: it is already exact
//! and valid as it stands, and the one thing a rewrite must never do is change
//! what the creature answers.
//!
//! `IF` is never converted. It reads its condition sum to choose a branch
//! rather than reducing its terms, so no point-wise squash stands in for it at
//! any edge count, and what to do when a removal leaves it short of validation
//! rule 12's one-edge-per-role is
//! [`IfRepair`](crate::prune_cleanup::IfRepair)'s to decide, not this module's.
//!
//! # The output clamp is part of the equality
//!
//! The forward pass puts every activation through
//! [`apply_limit_range`](crate::range::apply_limit_range), whose bounds depend
//! on the squash. A replacement that clamped differently would compute the same
//! number and then report a different one, so each rule is checked against the
//! ranges before it is taken and a rule whose clamp moved is **skipped** rather
//! than applied. The `MINIMUM`/`MAXIMUM`/`MEAN` and `HYPOTv2` rules land on
//! identical ranges; `ABSOLUTE` floors at `0` where `HYPOT` does not, which is
//! sound for the one `HYPOT` rule because at `b == 0` its activation is the
//! magnitude of `w·a` and never reaches below that floor.
//!
//! # Where the equality stops, said plainly
//!
//! "The same number" means the same thing it means for
//! [`mod@crate::prune_neuron`]'s structural fold: the same number to the `f32`
//! precision the forward pass itself works in, for every **finite** term that
//! pass can represent without overflowing. Two boundaries sit outside that, and
//! neither is reachable from a finite creature scored on finite records:
//!
//! - a term of `NaN` or `±inf` — only reachable from a non-finite input record,
//!   since every neuron activation has already been through
//!   [`apply_limit_range`](crate::range::apply_limit_range) — leaves
//!   `MINIMUM`/`MAXIMUM` on the sentinel their empty-input arm uses and so
//!   answers the bias, where `IDENTITY` propagates the term;
//! - a term whose square leaves the `f32` normal range already makes `HYPOT` and
//!   `HYPOTv2` disagree with the magnitude they are meant to be: above about
//!   `1.8e19` the square overflows and they answer `f32::MAX` where `ABSOLUTE`
//!   answers the term, and below about `1.1e-19` the square is subnormal, so
//!   `sqrt` loses bits — a measured `1e-22` term answers `9.904085e-23` rather
//!   than `1e-22`, about 1% out. That is a difference the aggregate form has
//!   with its own arithmetic, not one this rewrite introduces: `ABSOLUTE` is the
//!   more accurate of the two. The rewrite is still a change of answer there,
//!   which is why it is named here rather than claimed away.
//!
//! Neither is papered over: the rewrite is not claimed to be bit-exact on
//! garbage, and a caller scoring non-finite records has a problem this module
//! cannot fix.
//!
//! ```mermaid
//! flowchart TD
//!     T["a target the request touched"] --> E{"exactly one inward edge?"}
//!     E -- no --> K["keep the squash"]
//!     E -- yes --> A{"an aggregate, and not IF?"}
//!     A -- no --> K
//!     A -- yes --> R{"a replacement the rule<br/>table names?"}
//!     R -- no --> K
//!     R -- yes --> C{"does it clamp the same?"}
//!     C -- no --> K
//!     C -- yes --> W["rewrite the squash,<br/>report the conversion"]
//! ```

use crate::creature::{CreatureExport, squash_name_from};
use crate::prune_cleanup::CleanupError;
use crate::prune_neuron::{PruneError, squash_of};
use crate::range::apply_get_range;
use crate::squash::SquashType;

/// One aggregate rewritten to the point-wise squash that computes the same
/// number, reported so a caller never has to discover it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SquashConversion {
    /// Wire UUID of the neuron whose squash was rewritten.
    pub uuid: String,
    /// The aggregate squash it declared.
    pub from: &'static str,
    /// The point-wise squash it declares now.
    pub to: &'static str,
}

/// Rewrite every touched target the cut left as a single-edge aggregate, and
/// name what was rewritten.
///
/// `touched_targets` are the targets the request itself reached into — the one
/// target of a synapse prune, every target of a removed neuron. Repeats are
/// examined once. A target the cut left with no edge, or with two or more, is
/// left exactly as it is, and so is one whose squash the rule table does not
/// name.
///
/// # Errors
///
/// Returns [`PruneError::Cleanup`] when a touched target is not a neuron of the
/// creature, or declares a squash name this crate does not know. Both callers
/// resolved those targets before the cut, so neither can fire for them — they
/// are propagated rather than skipped so a future caller cannot reach a silent
/// no-op.
pub(crate) fn convert_single_edge_aggregates(
    creature: &mut CreatureExport,
    touched_targets: &[String],
) -> Result<Vec<SquashConversion>, PruneError> {
    let mut conversions = Vec::new();
    let mut examined: Vec<&str> = Vec::new();

    for uuid in touched_targets {
        if examined.contains(&uuid.as_str()) {
            continue;
        }
        examined.push(uuid.as_str());

        if inward_edge_count(creature, uuid) != 1 {
            continue;
        }
        // A target the request named must be a neuron of the creature: both
        // callers resolved its squash before the cut. Absent is a defect, not a
        // condition to skip over quietly — the same reading the unknown-squash
        // arm below takes.
        let index = creature
            .neurons
            .iter()
            .position(|n| n.uuid == *uuid)
            .ok_or_else(|| {
                PruneError::Cleanup(CleanupError::UnknownEndpoint { uuid: uuid.clone() })
            })?;
        let from = squash_of(&creature.neurons[index])?;
        let Some(to) = replacement(from, creature.neurons[index].bias) else {
            continue;
        };

        creature.neurons[index].squash = Some(squash_name_from(to).to_string());
        conversions.push(SquashConversion {
            uuid: uuid.clone(),
            from: squash_name_from(from),
            to: squash_name_from(to),
        });
    }

    Ok(conversions)
}

/// How many inward rows the creature carries for this target.
///
/// Rows, not readable keys: the forward pass walks the target's synapse range
/// one row at a time, so two rows are two terms to it however they are typed.
fn inward_edge_count(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.to_uuid == uuid)
        .count()
}

/// The point-wise squash a **single-term** aggregate is the same function as,
/// or `None` where there is not one.
///
/// The rule table is in the module documentation, read off the aggregate arms of
/// [`crate::network::CompiledNetwork::activate`]. Every answer is checked
/// against [`clamps_identically`] before it is returned, so a range change
/// elsewhere in the crate turns a rule off rather than making it wrong.
fn replacement(squash: SquashType, bias: f64) -> Option<SquashType> {
    let (to, non_negative) = match squash {
        // Reducing one term is that term, and all three then add the bias.
        SquashType::Minimum | SquashType::Maximum | SquashType::Mean => {
            (SquashType::Identity, false)
        }
        // `sqrt((bias + w·a)²)` over one term is `|bias + w·a|`.
        SquashType::HypotenuseV2 => (SquashType::Absolute, true),
        // `sqrt((w·a)²) + bias` is `|w·a| + bias`, which `ABSOLUTE` computes
        // only where the bias it would fold inside the root is zero.
        SquashType::Hypotenuse if bias == 0.0 => (SquashType::Absolute, true),
        _ => return None,
    };
    clamps_identically(squash, to, non_negative).then_some(to)
}

/// Does the forward pass's output clamp treat `to` as it treated `from`, for
/// every value the converted arm can produce?
///
/// The upper bound must match outright. A **tighter lower** bound is only
/// allowed where the converted arm cannot reach below it — `non_negative` says
/// the replacement computes a magnitude, which is how `HYPOT → ABSOLUTE` clears
/// `ABSOLUTE`'s `0` floor. Anything else is refused, so the rule is skipped and
/// the aggregate kept.
fn clamps_identically(from: SquashType, to: SquashType, non_negative: bool) -> bool {
    let (from_low, from_high) = apply_get_range(from);
    let (to_low, to_high) = apply_get_range(to);
    if from_high != to_high {
        return false;
    }
    from_low == to_low || (non_negative && to_low <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer the table gives for each aggregate, by name.
    const TABLE: [(SquashType, f64, Option<SquashType>); 7] = [
        (SquashType::Minimum, 0.25, Some(SquashType::Identity)),
        (SquashType::Maximum, 0.25, Some(SquashType::Identity)),
        (SquashType::Mean, 0.25, Some(SquashType::Identity)),
        (SquashType::HypotenuseV2, 0.25, Some(SquashType::Absolute)),
        (SquashType::Hypotenuse, 0.0, Some(SquashType::Absolute)),
        (SquashType::Hypotenuse, 0.25, None),
        (SquashType::If, 0.0, None),
    ];

    #[test]
    fn the_rule_table_answers_what_it_claims_to() {
        for (squash, bias, expected) in TABLE {
            assert_eq!(replacement(squash, bias), expected, "{squash:?} at {bias}");
        }
    }

    /// A newly added aggregate must not slip through unconsidered. Rust cannot
    /// enumerate an enum, so the sweep walks every discriminant
    /// [`SquashType::from`] accepts and requires each aggregate it finds to be
    /// named in [`TABLE`] — a seventh aggregate added to `squash.rs` fails here
    /// rather than being silently kept by the `_` arm of `replacement`.
    #[test]
    fn every_aggregate_the_crate_carries_is_named_in_the_table() {
        let mut swept = 0;
        for code in 0u8..=u8::MAX {
            let squash = SquashType::from(code);
            if !squash.is_aggregate() {
                continue;
            }
            // `From<u8>` maps every unknown code to `Identity`, which is not an
            // aggregate, so the sweep cannot be padded by unknown codes.
            if TABLE.iter().any(|(named, ..)| *named == squash) {
                swept += 1;
                continue;
            }
            panic!("aggregate {squash:?} (code {code}) has no entry in the rule table");
        }
        // The six aggregates `SquashType::is_aggregate` names, each reached once
        // by its own discriminant.
        assert_eq!(swept, 6, "the sweep did not reach six aggregates");
    }

    /// A point-wise squash is not this module's business at any edge count.
    #[test]
    fn a_point_wise_squash_is_never_replaced() {
        for squash in [
            SquashType::Identity,
            SquashType::Logistic,
            SquashType::Absolute,
            SquashType::Relu,
        ] {
            assert_eq!(replacement(squash, 0.0), None, "{squash:?}");
            assert!(!squash.is_aggregate(), "{squash:?} is not an aggregate");
        }
    }

    /// The clamp guard is what makes the table safe to extend, so it has to
    /// actually refuse a mismatch.
    #[test]
    fn a_replacement_whose_clamp_moved_is_refused() {
        // `LOGISTIC` bounds at `(0, 1)` where `MEAN` is unbounded, so the guard
        // refuses it however the rule table were written.
        assert!(!clamps_identically(
            SquashType::Mean,
            SquashType::Logistic,
            false
        ));
        // A tighter floor passes only on the non-negative promise.
        assert!(clamps_identically(
            SquashType::Hypotenuse,
            SquashType::Absolute,
            true
        ));
        assert!(!clamps_identically(
            SquashType::Hypotenuse,
            SquashType::Absolute,
            false
        ));
    }
}
