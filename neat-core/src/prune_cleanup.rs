//! Canonical fixed-point cleanup for a creature a caller has just cut into
//! (Issue #589).
//!
//! Every prune operation in the fleet — remove a hidden neuron (Issue #590),
//! remove one typed synapse (Issue #591) — leaves the same wreckage behind:
//! feeders nothing reads any more, a hidden neuron with nothing left to sum, a
//! support constant no edge names, an `IF` short a role, and a neuron list no
//! longer in the order the forward pass reads. [`cleanup_creature`] is the one
//! deterministic entry point that repairs all of it, to a **fixed point**, and
//! validates the stable result before returning it. A successful call never
//! returns an invalid creature.
//!
//! ```mermaid
//! flowchart TD
//!     I["creature, straight after<br/>the caller's deletion"] --> R["repair: an IF short a role<br/>→ IDENTITY, roles stripped"]
//!     R --> D["remove dead structure:<br/>non-output nodes with<br/>no outward edge"]
//!     D --> C["constant support invariants:<br/>bias 1, at most three,<br/>none unreferenced"]
//!     C --> F["fold: hidden with no inward edge<br/>→ bias-1 support constant,<br/>squash(bias) into its weights"]
//!     F --> N["canonicalise: constants, hiddens,<br/>outputs; edges sorted by (from, to, role)"]
//!     N --> Q{"anything change?"}
//!     Q -- yes --> R
//!     Q -- no --> M["prune the memetic record<br/>of references the edits stranded"]
//!     M --> V["creature_validate"]
//!     V -- fails --> E["Err(CleanupError::Invalid)"]
//!     V -- passes --> O["Ok(CleanupOutcome)"]
//! ```
//!
//! # Every rewrite is exact
//!
//! Cleanup only ever removes structure nothing reads, or rewrites structure
//! into a form that computes the **same number on every record**. It never
//! approximates: statistical compensation is the caller's, supplied with the
//! caller's own statistics (Issue #590). Two rewrites carry that weight:
//!
//! - **the constant fold.** A hidden neuron with no inward edge sums nothing,
//!   so its activation is `squash(bias)` on every record — a constant. Its
//!   value is folded into the **weights** of its outward edges and the neuron
//!   becomes (or is replaced by) a bias-`1` support constant, so `w · v`
//!   reaches each target exactly as it did before.
//! - **the constant rescale.** A constant of value `b` feeding an edge of
//!   weight `w` is the same term as a bias-`1` constant feeding an edge of
//!   weight `w · b`, which is how the "every constant has bias 1" invariant is
//!   reached without changing a single output.
//!
//! Both can leave two edges from the same constant into the same target, and
//! merging those is only exact where the target's squash **sums** its inward
//! terms (every point-wise squash, and an `IF` within one role). A
//! `MINIMUM` / `MAXIMUM` target instead takes the smaller / larger of the two
//! constant terms, which is exact because a constant contributes the same term
//! on every record; a `MEAN` divides by its edge **count** and a `HYPOT`
//! squares each term, so merging there would change the value and cleanup
//! refuses to do it — it keeps the constants apart instead, and never trades
//! correctness for the constant budget.
//!
//! # Constants are support nodes
//!
//! Constants exist to carry a fixed value into the legal synapse roles, not to
//! be optimised. The invariants this module enforces (Ockham #180) are:
//! every constant has bias exactly [`SUPPORT_CONSTANT_BIAS`], a fold **reuses**
//! an existing compatible constant rather than proliferating new ones, a
//! constant no edge references is removed, and a creature carries at most
//! [`MAX_SUPPORT_CONSTANTS`] of them.
//!
//! That is a deliberate, documented divergence from the TypeScript captures in
//! [`crate::prune_fixtures`], which record constants carrying the folded value
//! in their **bias** (`LOGISTIC(0.4)` and friends). The two forms are the same
//! function of the inputs — `neat-core/tests/prune_cleanup.rs` proves it by
//! activating both — and only this one holds the support-node invariants.
//!
//! # Two boundaries worth naming
//!
//! **Dead structure is "nothing reads it", not "no output reads it".** A node
//! with no outward edge cannot reach an output, and in a `forwardOnly` creature
//! — every creature this fleet trains — those two statements are the same, so
//! the sweep is complete. A *recurrent* creature can hold an island that feeds
//! only itself; each member has an outward edge, so cleanup leaves it alone
//! rather than widening the rule the caller asked for.
//!
//! **The `IF` repair follows the structural gate, not rule 12's exemption.**
//! [`creature_validate`] skips rule 12 for a neuron at index `<= 2`, but
//! [`validate_creature_topology`]'s structural leg has no such exemption and
//! cleanup is gated on both, so an `IF` short a role is repaired wherever it
//! sits.
//!
//! # Two `IF` repair policies, and why there are two
//!
//! [`cleanup_creature`] keeps TypeScript parity ([`IfRepair::Downgrade`]): an
//! `IF` short a role becomes the `IDENTITY` sum of everything still reaching
//! it. That is the one **inexact** rewrite in this module, and it is what the
//! [`crate::prune_fixtures`] captures record.
//!
//! [`cleanup_creature_with`] lets a caller ask for [`IfRepair::Rewrite`]
//! instead (Issue #591): the `IF` is rewritten into the closest form that
//! computes the **same number on every record** — the branch a statically
//! decided condition always takes, or a zero-weight support edge giving back
//! the branch role the removal emptied. Synapse pruning uses it so typed
//! structure is rewritten rather than refused; neuron pruning (Issue #590)
//! keeps the parity default.
//!
//! # What is preserved
//!
//! Observation (input) neurons and output neurons are never removed, never
//! rewritten and never reordered: the declared `input` / `output` widths are
//! the fleet's contract (Issue #550) and the output block's order *is* the
//! output vector. Only hidden and constant neurons are cleanup's to move.

use std::collections::{HashMap, HashSet};

use crate::creature::{
    CreatureError, CreatureExport, NeuronExport, SynapseExport, parse_squash_name,
    parse_synapse_type, squash_name_from, synapse_type_name_from, validate_creature_width,
};
use crate::creature_validate::{ValidateOptions, ValidationFailure, creature_validate};
use crate::if_graft::{GraftError, sort_synapses_canonically, validate_creature_topology};
use crate::range::apply_limit_range;
use crate::squash::{SquashType, apply_squash};
use crate::synapse_type::SynapseType;
use crate::topology_invariants::{IfRoles, if_neuron_fault};

/// Bias every surviving constant carries.
///
/// A constant is a support node: it delivers `1`, and the value each consumer
/// wants lives in the **weight** of the edge that reads it, which is what
/// training adjusts. Matches [`crate::if_graft::GRAFT_CONSTANT_BIAS`], so a
/// grafted constant and a folded one are the same kind of node.
pub const SUPPORT_CONSTANT_BIAS: f64 = 1.0;

/// Name prefix of a support constant this module has to mint itself.
///
/// Only [`IfRepair::Rewrite`] ever needs one, and only for a creature that
/// carries no constant at all to hang a restored `IF` role on.
const SUPPORT_CONSTANT_PREFIX: &str = "prune-support-";

/// The most constants a canonical creature carries.
///
/// Three is the number of legal synapse roles a constant may need to support
/// at once (`condition`, `positive`, `negative`); beyond that a constant is
/// duplication, and cleanup merges the surplus into an existing one.
pub const MAX_SUPPORT_CONSTANTS: usize = 3;

/// The full identity of one synapse: the ordered pair **and** the role.
///
/// A pair may repeat into an `IF` target, once per role (Issue #577), so the
/// role is part of what names an edge rather than decoration on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SynapseKey {
    /// Wire UUID of the source neuron.
    pub from_uuid: String,
    /// Wire UUID of the target neuron.
    pub to_uuid: String,
    /// Role the edge plays at its target.
    pub role: SynapseType,
}

/// What [`cleanup_creature`] produced, and everything it did to get there.
///
/// The record is what a prune operation reports to its own caller: which
/// structure the requested deletion cost beyond the deletion itself.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupOutcome {
    /// The canonical, validated creature.
    pub creature: CreatureExport,
    /// True when the creature that came back differs from the one supplied.
    pub changed: bool,
    /// How many fixed-point passes were needed; the last one changed nothing.
    pub passes: usize,
    /// Neurons removed as dead structure, in removal order.
    pub removed_neurons: Vec<String>,
    /// Synapses removed alongside them, in removal order.
    pub removed_synapses: Vec<SynapseKey>,
    /// Hidden neurons folded into constant support, in fold order.
    pub folded_neurons: Vec<String>,
    /// Constants whose value moved into their outward weights.
    pub rescaled_constants: Vec<String>,
    /// Surplus constants merged into a surviving support constant.
    pub merged_constants: Vec<String>,
    /// Constants still over [`MAX_SUPPORT_CONSTANTS`] because no merge of them
    /// would have been exact, named rather than left for a caller to discover.
    ///
    /// Empty in the canonical case. It fills only where a surplus constant
    /// lands on a target whose squash reads its inward **count** (`MEAN`) or
    /// squares each term (`HYPOT`), where one edge cannot carry what two did —
    /// cleanup keeps the constants apart rather than change what the creature
    /// computes, and says so here.
    pub surplus_constants: Vec<String>,
    /// `IF` neurons downgraded to `IDENTITY` because a required role was gone.
    ///
    /// Only [`IfRepair::Downgrade`] fills this — the one inexact rewrite this
    /// module has. Under [`IfRepair::Rewrite`] it is always empty, and
    /// [`Self::static_if_neurons`] / [`Self::restored_if_roles`] carry the
    /// exact rewrites that replaced it.
    pub downgraded_if_neurons: Vec<String>,
    /// `IF` neurons flattened to `IDENTITY` because their condition was
    /// decided by the creature alone (Issue #591).
    pub static_if_neurons: Vec<StaticIfRewrite>,
    /// Zero-weight support edges added to give an `IF` back a branch role it
    /// lost, without changing what it computes (Issue #591).
    pub restored_if_roles: Vec<SynapseKey>,
}

/// One `IF` whose condition the creature itself decides, and the branch that
/// therefore survives.
///
/// The other branch cannot be reached on any record, so it is removed and its
/// feeders go with it through the ordinary dead-structure cascade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticIfRewrite {
    /// Wire UUID of the `IF` neuron.
    pub uuid: String,
    /// The branch the condition always takes: [`SynapseType::Positive`] when
    /// the condition sum is `> 0`, [`SynapseType::Negative`] otherwise.
    pub branch: SynapseType,
}

/// How cleanup repairs an `IF` neuron a removal left short of a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IfRepair {
    /// TypeScript parity (`RepairInvalidIfNeurons.ts`): the `IF` becomes the
    /// `IDENTITY` sum of **everything** still reaching it and its inward roles
    /// are stripped. Cheap, total, and not what the `IF` computed — the one
    /// inexact rewrite in this module, reported on
    /// [`CleanupOutcome::downgraded_if_neurons`].
    #[default]
    Downgrade,
    /// Issue #591: rewrite the `IF` into the closest form that computes the
    /// **same number on every record**, and never blanket-downgrade.
    ///
    /// | What the removal left | Rewrite | Why it is exact |
    /// |---|---|---|
    /// | a condition the creature decides (no condition edge, or every condition source structurally constant) | `IDENTITY` over the branch that is always taken; the other branch and the condition edges go | the forward pass could never take the other branch |
    /// | a missing `positive` / `negative` branch, condition still varying | a **zero-weight** edge from a support constant into that role | an empty branch sum is `0`, and so is `0 · 1` |
    Rewrite,
}

/// What one cleanup run is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupOptions {
    /// How an `IF` short a role is repaired.
    pub if_repair: IfRepair,
}

/// Why a cleanup produced no creature.
///
/// Every variant means **nothing was returned** — cleanup fails closed rather
/// than handing back a creature it could not make valid.
#[derive(Debug)]
pub enum CleanupError {
    /// The creature broke a [`crate::creature`] rule, e.g. the observation
    /// width contract or an unknown squash name.
    Creature(CreatureError),
    /// A synapse named a neuron the creature does not carry.
    UnknownEndpoint {
        /// The unknown UUID.
        uuid: String,
    },
    /// Two neurons share a wire UUID, so no edge can name either of them.
    DuplicateUuid {
        /// The repeated UUID.
        uuid: String,
    },
    /// A neuron declared a type outside `hidden | output | constant`.
    UnknownNeuronType {
        /// UUID of the offending neuron.
        uuid: String,
        /// The type it declared.
        declared: String,
    },
    /// A synapse pointed at an observation (input) neuron, which takes none.
    SynapseTargetsInput {
        /// UUID of the input neuron.
        uuid: String,
    },
    /// A synapse pointed at a constant, which is a source and never a sink.
    ConstantHasInward {
        /// UUID of the constant.
        uuid: String,
    },
    /// A bias was `NaN` or infinite, so no fold of it could be exact.
    NonFiniteBias {
        /// UUID of the neuron carrying it.
        uuid: String,
    },
    /// A weight was `NaN` or infinite, or a fold made one so.
    NonFiniteWeight {
        /// Source neuron UUID.
        from_uuid: String,
        /// Target neuron UUID.
        to_uuid: String,
        /// The offending weight.
        weight: f64,
    },
    /// Two edges would have to merge into one at a target whose squash reads
    /// its inward **count** or squares each term, where merging would change
    /// the value the target computes.
    InexactMerge {
        /// Source neuron UUID.
        from_uuid: String,
        /// Target neuron UUID.
        to_uuid: String,
        /// The target's squash, which is what makes the merge inexact.
        squash: &'static str,
    },
    /// The passes never stopped changing the creature — a defect in this
    /// module, reported rather than looped on.
    NotStable {
        /// How many passes ran before the cap was hit.
        passes: usize,
    },
    /// The stable creature failed the shared validator, so it was not returned.
    Invalid(ValidationFailure),
    /// The stable creature failed the shared topology gate — the index-space
    /// and order-independent legs `creature_validate` does not cover — so it
    /// was not returned.
    MalformedResult(GraftError),
}

impl std::fmt::Display for CleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CleanupError::Creature(e) => write!(f, "Creature error: {e}"),
            CleanupError::UnknownEndpoint { uuid } => {
                write!(f, "Synapse names unknown neuron {uuid}")
            }
            CleanupError::DuplicateUuid { uuid } => write!(f, "Repeated neuron UUID {uuid}"),
            CleanupError::UnknownNeuronType { uuid, declared } => {
                write!(f, "Neuron {uuid} declares unknown type '{declared}'")
            }
            CleanupError::SynapseTargetsInput { uuid } => {
                write!(f, "Synapse targets input neuron {uuid}")
            }
            CleanupError::ConstantHasInward { uuid } => {
                write!(f, "Constant {uuid} has an inward synapse")
            }
            CleanupError::NonFiniteBias { uuid } => write!(f, "Non-finite bias on neuron {uuid}"),
            CleanupError::NonFiniteWeight {
                from_uuid,
                to_uuid,
                weight,
            } => write!(
                f,
                "Non-finite weight {weight} from {from_uuid} to {to_uuid}"
            ),
            CleanupError::InexactMerge {
                from_uuid,
                to_uuid,
                squash,
            } => write!(
                f,
                "Merging the edges from {from_uuid} into {to_uuid} would change what its {squash} squash computes"
            ),
            CleanupError::NotStable { passes } => {
                write!(f, "Cleanup did not reach a fixed point in {passes} passes")
            }
            CleanupError::Invalid(failure) => write!(
                f,
                "Cleaned creature is invalid ({}): {}",
                failure.reason, failure.message
            ),
            CleanupError::MalformedResult(e) => {
                write!(f, "Cleaned creature failed the topology gate: {e}")
            }
        }
    }
}

impl std::error::Error for CleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CleanupError::Creature(e) => Some(e),
            CleanupError::Invalid(e) => Some(e),
            CleanupError::MalformedResult(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CreatureError> for CleanupError {
    fn from(e: CreatureError) -> Self {
        CleanupError::Creature(e)
    }
}

/// Repair a creature to a canonical fixed point and validate it.
///
/// The single cleanup entry point every prune operation reuses. The input is
/// the creature **as the caller left it** — one neuron or one synapse short,
/// and very possibly invalid because of it. What comes back is canonical,
/// exactly equivalent on every record to what the caller cut, and accepted by
/// [`creature_validate`].
///
/// Cleanup is deterministic (the same input gives the same output, byte for
/// byte) and idempotent: running it on its own result reports
/// [`CleanupOutcome::changed`] `== false` and returns the same creature.
///
/// # Errors
///
/// Returns a [`CleanupError`] when the creature names structure that does not
/// exist, carries a value that is not finite, would need an inexact merge to
/// canonicalise, or is invalid in a way cleanup cannot exactly repair — a
/// backward edge in a `forwardOnly` creature, say. No creature is returned in
/// any of those cases.
///
/// # Examples
///
/// ```
/// use neat_core::{cleanup_creature, parse_creature_json};
///
/// // `h-1` fed only `h-2`, and the caller has just removed that edge.
/// let creature = parse_creature_json(r#"{
///   "input":1,"output":1,"forwardOnly":true,
///   "neurons":[
///     {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
///     {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
///   ],
///   "synapses":[
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
///   ]
/// }"#).unwrap();
///
/// let outcome = cleanup_creature(&creature).unwrap();
/// assert!(outcome.changed);
/// assert_eq!(outcome.removed_neurons, vec!["h-1".to_string()]);
/// assert_eq!(outcome.creature.neurons.len(), 1);
/// ```
pub fn cleanup_creature(creature: &CreatureExport) -> Result<CleanupOutcome, CleanupError> {
    cleanup_creature_with(creature, CleanupOptions::default())
}

/// [`cleanup_creature`], with the caller's choice of `IF` repair policy.
///
/// The default policy is TypeScript parity ([`IfRepair::Downgrade`]); synapse
/// pruning (Issue #591) asks for [`IfRepair::Rewrite`] instead, so an `IF` that
/// lost a role is rewritten into a form that computes the same number rather
/// than flattened into a sum it never took.
///
/// # Errors
///
/// The same [`CleanupError`]s as [`cleanup_creature`].
///
/// # Examples
///
/// ```
/// use neat_core::{CleanupOptions, IfRepair, SynapseType, cleanup_creature_with, parse_creature_json};
///
/// // `if-1` lost its only condition source, so the condition sum is 0 on every
/// // record and the negative branch is the only one the forward pass takes.
/// let creature = parse_creature_json(r#"{
///   "input":1,"output":1,"forwardOnly":true,
///   "neurons":[
///     {"type":"hidden","uuid":"h-a","bias":0.0,"squash":"IDENTITY"},
///     {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
///     {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
///   ],
///   "synapses":[
///     {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
///     {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
///     {"weight":-3.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
///     {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
///   ]
/// }"#).unwrap();
///
/// let options = CleanupOptions { if_repair: IfRepair::Rewrite };
/// let outcome = cleanup_creature_with(&creature, options).unwrap();
///
/// assert_eq!(outcome.static_if_neurons[0].uuid, "if-1");
/// assert_eq!(outcome.static_if_neurons[0].branch, SynapseType::Negative);
/// // Only the negative arm survives, at its own weight.
/// assert_eq!(outcome.creature.synapses[1].weight, -3.0);
/// ```
pub fn cleanup_creature_with(
    creature: &CreatureExport,
    options: CleanupOptions,
) -> Result<CleanupOutcome, CleanupError> {
    validate_creature_width(creature)?;

    let mut engine = Engine::new(creature.clone(), options);
    engine.check_references()?;

    let cap = 8 + 4 * (creature.neurons.len() + creature.synapses.len());
    let mut passes = 0usize;
    loop {
        if passes >= cap {
            return Err(CleanupError::NotStable { passes });
        }
        passes += 1;

        let mut changed = engine.repair_if_neurons()?;
        changed |= engine.remove_dead_structure()?;
        // Constants are normalised before the fold so a fold has bias-1
        // support nodes to reuse rather than minting one of its own.
        changed |= engine.normalise_constants()?;
        changed |= engine.fold_zero_inward_hidden()?;
        changed |= engine.canonicalise()?;

        if !changed {
            break;
        }
    }

    let mut result = engine.creature;
    // Rule 31's inverse: a memetic entry naming structure the edits removed
    // would make the creature invalid, so the record is pruned of exactly
    // those references (NEAT-AI-Lamarck#197) — never dropped wholesale.
    result.prune_memetic();
    let changed = result != *creature;

    let options = ValidateOptions {
        neurons: None,
        connections: None,
        feedback_loop: None,
        forward_only: result.forward_only,
    };
    creature_validate(&result, &options).map_err(CleanupError::Invalid)?;
    // Both gates, as `if_graft::validated` does: `creature_validate` speaks the
    // TypeScript rule table, `validate_creature_topology` adds the index-space
    // and order-independent legs (including `validate_no_duplicate_synapses`).
    // A creature this module produced must satisfy both or not be returned.
    validate_creature_topology(&result).map_err(CleanupError::MalformedResult)?;

    let surplus_constants: Vec<String> = result
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "constant")
        .skip(MAX_SUPPORT_CONSTANTS)
        .map(|n| n.uuid.clone())
        .collect();

    Ok(CleanupOutcome {
        creature: result,
        changed,
        passes,
        removed_neurons: engine.removed_neurons,
        removed_synapses: engine.removed_synapses,
        folded_neurons: engine.folded_neurons,
        rescaled_constants: engine.rescaled_constants,
        merged_constants: engine.merged_constants,
        surplus_constants,
        downgraded_if_neurons: engine.downgraded_if_neurons,
        static_if_neurons: engine.static_if_neurons,
        restored_if_roles: engine.restored_if_roles,
    })
}

/// What a neuron is, once its declared type has been checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Constant,
    Hidden,
    Output,
}

/// How two edges from the same source into the same target combine, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeRule {
    /// The target sums its inward terms, so one edge of the summed weight says
    /// the same thing. Every point-wise squash, and an `IF` within one role.
    Sum,
    /// `MINIMUM` takes the smallest term; two terms from the same bias-1
    /// constant are just their weights, so the smaller weight survives.
    MinWeight,
    /// `MAXIMUM`, by the same argument as [`MergeRule::MinWeight`].
    MaxWeight,
    /// The target reads its inward **count** (`MEAN`) or squares each term
    /// (`HYPOT`, `HYPOTv2`), so no single edge carries the same value.
    Never,
}

fn merge_rule(squash: SquashType) -> MergeRule {
    match squash {
        SquashType::Minimum => MergeRule::MinWeight,
        SquashType::Maximum => MergeRule::MaxWeight,
        SquashType::Mean | SquashType::Hypotenuse | SquashType::HypotenuseV2 => MergeRule::Never,
        _ => MergeRule::Sum,
    }
}

fn kind_of(neuron: &NeuronExport) -> Result<Kind, CleanupError> {
    match neuron.neuron_type.as_str() {
        "constant" => Ok(Kind::Constant),
        "hidden" => Ok(Kind::Hidden),
        "output" => Ok(Kind::Output),
        other => Err(CleanupError::UnknownNeuronType {
            uuid: neuron.uuid.clone(),
            declared: other.to_string(),
        }),
    }
}

/// The squash a neuron activates with. A constant emits its bias whatever it
/// declares, so it reads as `IDENTITY` here (and cleanup strips the field).
fn squash_of(neuron: &NeuronExport) -> Result<SquashType, CleanupError> {
    if neuron.neuron_type == "constant" {
        return Ok(SquashType::Identity);
    }
    Ok(parse_squash_name(
        neuron.squash.as_deref().unwrap_or("IDENTITY"),
    )?)
}

/// The role an edge plays where its target can tell roles apart.
///
/// Only an `IF` keeps a sum per role; every other squash sums whatever reaches
/// it, so two roles into one of those are the same edge written twice.
///
/// At an `IF`, [`SynapseType::Standard`] **is** the positive role: an untyped
/// inward edge lands in the positive accumulator in
/// [`crate::network::CompiledNetwork::activate`] and tallies as positive in
/// [`IfRoles::tally`], so the two spellings name one branch and are folded to
/// one answer here. Reading them apart is what would let a request for the
/// positive arm miss an untyped row, which is the refusal Issue #591 exists to
/// remove.
///
/// Crate-visible because [`crate::prune_synapse`] resolves the *requested*
/// role through it (Issue #591): one definition of "what names an edge", so a
/// request and a canonicalisation can never disagree.
pub(crate) fn canonical_role(target_squash: SquashType, role: SynapseType) -> SynapseType {
    if target_squash != SquashType::If {
        return SynapseType::Standard;
    }
    match role {
        SynapseType::Standard => SynapseType::Positive,
        other => other,
    }
}

fn role_of(synapse: &SynapseExport) -> SynapseType {
    parse_synapse_type(synapse.synapse_type.as_deref())
}

/// The working state of one cleanup run.
struct Engine {
    creature: CreatureExport,
    options: CleanupOptions,
    removed_neurons: Vec<String>,
    removed_synapses: Vec<SynapseKey>,
    folded_neurons: Vec<String>,
    rescaled_constants: Vec<String>,
    merged_constants: Vec<String>,
    downgraded_if_neurons: Vec<String>,
    static_if_neurons: Vec<StaticIfRewrite>,
    restored_if_roles: Vec<SynapseKey>,
}

impl Engine {
    fn new(creature: CreatureExport, options: CleanupOptions) -> Self {
        Self {
            creature,
            options,
            removed_neurons: Vec::new(),
            removed_synapses: Vec::new(),
            folded_neurons: Vec::new(),
            rescaled_constants: Vec::new(),
            merged_constants: Vec::new(),
            downgraded_if_neurons: Vec::new(),
            static_if_neurons: Vec::new(),
            restored_if_roles: Vec::new(),
        }
    }

    /// Every neuron's squash, by UUID — rebuilt per pass rather than cached,
    /// so it can never describe a creature the passes have already changed.
    fn squash_map(&self) -> Result<HashMap<String, SquashType>, CleanupError> {
        let mut map = HashMap::with_capacity(self.creature.neurons.len());
        for neuron in &self.creature.neurons {
            map.insert(neuron.uuid.clone(), squash_of(neuron)?);
        }
        Ok(map)
    }

    fn squash_of_uuid(&self, uuid: &str) -> Result<SquashType, CleanupError> {
        self.creature
            .neurons
            .iter()
            .find(|n| n.uuid == uuid)
            .ok_or_else(|| CleanupError::UnknownEndpoint {
                uuid: uuid.to_string(),
            })
            .and_then(squash_of)
    }

    /// Is `uuid` a **support** constant — a constant worth exactly 1?
    ///
    /// The `MINIMUM` / `MAXIMUM` merge is exact only because a constant term is
    /// its own weight, which holds at bias 1 and nowhere else, so the bias is
    /// part of the question rather than an invariant assumed elsewhere.
    /// `normalise_constants` makes it true before any merge runs, so this is
    /// the invariant restated where it is relied on — it does not reject
    /// anything today, and a pass reorder must not make it start to.
    fn is_support_constant(&self, uuid: &str) -> bool {
        self.creature.neurons.iter().any(|n| {
            n.uuid == uuid && n.neuron_type == "constant" && n.bias == SUPPORT_CONSTANT_BIAS
        })
    }

    /// Reject what cleanup cannot repair exactly, before any pass runs.
    ///
    /// These are defects in the creature as supplied — an edge naming a neuron
    /// that never existed, a value that is not a number — not wreckage a
    /// removal left behind, so they fail loudly instead of being papered over.
    fn check_references(&mut self) -> Result<(), CleanupError> {
        let mut known: HashSet<String> = (0..self.creature.input)
            .map(|i| format!("input-{i}"))
            .collect();
        let inputs = known.clone();
        let mut constants: HashSet<String> = HashSet::new();

        for neuron in &self.creature.neurons {
            let kind = kind_of(neuron)?;
            squash_of(neuron)?;
            if !neuron.bias.is_finite() {
                return Err(CleanupError::NonFiniteBias {
                    uuid: neuron.uuid.clone(),
                });
            }
            if !known.insert(neuron.uuid.clone()) {
                return Err(CleanupError::DuplicateUuid {
                    uuid: neuron.uuid.clone(),
                });
            }
            if kind == Kind::Constant {
                constants.insert(neuron.uuid.clone());
            }
        }

        for synapse in &self.creature.synapses {
            for uuid in [&synapse.from_uuid, &synapse.to_uuid] {
                if !known.contains(uuid) {
                    return Err(CleanupError::UnknownEndpoint { uuid: uuid.clone() });
                }
            }
            if inputs.contains(&synapse.to_uuid) {
                return Err(CleanupError::SynapseTargetsInput {
                    uuid: synapse.to_uuid.clone(),
                });
            }
            if constants.contains(&synapse.to_uuid) {
                return Err(CleanupError::ConstantHasInward {
                    uuid: synapse.to_uuid.clone(),
                });
            }
            if !synapse.weight.is_finite() {
                return Err(CleanupError::NonFiniteWeight {
                    from_uuid: synapse.from_uuid.clone(),
                    to_uuid: synapse.to_uuid.clone(),
                    weight: synapse.weight,
                });
            }
        }
        Ok(())
    }

    /// Repair every `IF` neuron a removal left short of a role, under the
    /// caller's [`IfRepair`] policy.
    fn repair_if_neurons(&mut self) -> Result<bool, CleanupError> {
        match self.options.if_repair {
            IfRepair::Downgrade => self.downgrade_if_neurons(),
            IfRepair::Rewrite => self.rewrite_if_neurons(),
        }
    }

    /// Every `IF` neuron in the creature, in list order.
    fn if_uuids(&self) -> Result<Vec<String>, CleanupError> {
        let mut uuids = Vec::new();
        for neuron in &self.creature.neurons {
            if squash_of(neuron)? == SquashType::If {
                uuids.push(neuron.uuid.clone());
            }
        }
        Ok(uuids)
    }

    /// The roles still reaching one `IF`, and how many edges carry them.
    fn inward_roles(&self, uuid: &str) -> (usize, IfRoles) {
        let inward: Vec<SynapseType> = self
            .creature
            .synapses
            .iter()
            .filter(|s| s.to_uuid == uuid)
            .map(role_of)
            .collect();
        (inward.len(), IfRoles::tally(inward.iter().copied()))
    }

    /// Downgrade every `IF` neuron a removal left short of a role.
    ///
    /// An `IF` needs a `condition`, a positive and a negative branch to mean
    /// anything (validation rule 12); without one it cannot branch, so it
    /// becomes the `IDENTITY` sum of whatever still reaches it and its inward
    /// roles — now unreadable — are stripped. The rows that were only distinct
    /// by role are summed into one by [`Engine::canonicalise`].
    fn downgrade_if_neurons(&mut self) -> Result<bool, CleanupError> {
        let if_uuids = self.if_uuids()?;

        let mut changed = false;
        for uuid in if_uuids {
            // `IfRoles` / `if_neuron_fault` are the single home of "does this
            // IF carry all three roles" (Issue #560) — asked here rather than
            // restated, so a repair and a validation can never disagree.
            let (inward, roles) = self.inward_roles(&uuid);
            if if_neuron_fault(inward, roles).is_none() {
                continue;
            }

            for neuron in &mut self.creature.neurons {
                if neuron.uuid == uuid {
                    neuron.squash = Some(squash_name_from(SquashType::Identity).to_string());
                }
            }
            for synapse in &mut self.creature.synapses {
                if synapse.to_uuid == uuid {
                    synapse.synapse_type = None;
                }
            }
            self.downgraded_if_neurons.push(uuid);
            changed = true;
        }
        Ok(changed)
    }

    /// Rewrite every `IF` neuron into the closest form that computes the same
    /// number on every record (Issue #591).
    ///
    /// Two rewrites, both exact, and neither of them a refusal:
    ///
    /// - **the condition the creature decides.** With no condition edge the
    ///   condition sum is `0`, and `0 > 0` is false, so the forward pass takes
    ///   the negative branch on every record; with every condition source
    ///   structurally fixed the sum is a number this module can compute. Either
    ///   way one branch is unreachable, so the neuron becomes the `IDENTITY`
    ///   sum of the branch that survives and the condition edges and the dead
    ///   branch go — [`Engine::remove_dead_structure`] then takes whatever fed
    ///   only them.
    /// - **the branch that lost its last edge.** An empty branch sum is `0`,
    ///   which is exactly what a zero-weight edge from a support constant
    ///   contributes, so that edge restores validation rule 12 without moving a
    ///   single output.
    fn rewrite_if_neurons(&mut self) -> Result<bool, CleanupError> {
        let if_uuids = self.if_uuids()?;

        let mut changed = false;
        for uuid in if_uuids {
            if let Some(branch) = static_condition_branch(&self.creature, &uuid)? {
                self.flatten_static_if(&uuid, branch);
                self.static_if_neurons
                    .push(StaticIfRewrite { uuid, branch });
                changed = true;
                continue;
            }

            // The condition varies, so the `IF` still has to branch: give back
            // whichever arm the removal emptied rather than refuse the removal.
            //
            // `if_neuron_fault` (Issue #560) is the single home of "is this IF
            // wired", and `downgrade_if_neurons` asks it; it reports only the
            // *first* fault, so it cannot say which arms to restore when both
            // are empty. The tally it is built from answers that, and restoring
            // every empty arm satisfies the fault helper by construction.
            let (_, roles) = self.inward_roles(&uuid);
            for role in [SynapseType::Positive, SynapseType::Negative] {
                let present = match role {
                    SynapseType::Negative => roles.negative,
                    _ => roles.positive,
                };
                if present {
                    continue;
                }
                self.restore_if_role(&uuid, role);
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Turn a statically-decided `IF` into the `IDENTITY` sum of the branch it
    /// always takes, dropping the condition edges and the branch it never does.
    fn flatten_static_if(&mut self, uuid: &str, branch: SynapseType) {
        for neuron in &mut self.creature.neurons {
            if neuron.uuid == uuid {
                neuron.squash = Some(squash_name_from(SquashType::Identity).to_string());
            }
        }

        let mut kept = Vec::with_capacity(self.creature.synapses.len());
        for mut synapse in std::mem::take(&mut self.creature.synapses) {
            if synapse.to_uuid != uuid {
                kept.push(synapse);
                continue;
            }
            let role = role_of(&synapse);
            let survives = match branch {
                SynapseType::Negative => role == SynapseType::Negative,
                _ => matches!(role, SynapseType::Positive | SynapseType::Standard),
            };
            if survives {
                synapse.synapse_type = None;
                kept.push(synapse);
            } else {
                self.removed_synapses.push(SynapseKey {
                    from_uuid: synapse.from_uuid,
                    to_uuid: synapse.to_uuid,
                    role,
                });
            }
        }
        self.creature.synapses = kept;
    }

    /// Give an `IF` back a branch role, on a zero-weight edge from a support
    /// constant so nothing it computes moves.
    fn restore_if_role(&mut self, uuid: &str, role: SynapseType) {
        let support = match self
            .creature
            .neurons
            .iter()
            .find(|n| n.neuron_type == "constant")
        {
            Some(constant) => constant.uuid.clone(),
            None => self.mint_support_constant(),
        };

        self.creature.synapses.push(SynapseExport {
            from_uuid: support.clone(),
            to_uuid: uuid.to_string(),
            weight: 0.0,
            synapse_type: synapse_type_name_from(role).map(str::to_string),
        });
        self.restored_if_roles.push(SynapseKey {
            from_uuid: support,
            to_uuid: uuid.to_string(),
            role,
        });
    }

    /// Add a bias-1 support constant under a name no neuron already carries.
    fn mint_support_constant(&mut self) -> String {
        let mut index = 0usize;
        let uuid = loop {
            let candidate = format!("{SUPPORT_CONSTANT_PREFIX}{index}");
            if !self.creature.neurons.iter().any(|n| n.uuid == candidate) {
                break candidate;
            }
            index += 1;
        };
        self.creature.neurons.push(NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: uuid.clone(),
            bias: SUPPORT_CONSTANT_BIAS,
            squash: None,
        });
        uuid
    }

    /// Remove, recursively, every non-output neuron nothing reads.
    ///
    /// A hidden or constant neuron with no outward edge cannot influence an
    /// output, so it and its inward edges go — which can strand its feeders,
    /// so the sweep repeats until nothing more is dead. Output neurons stay
    /// whatever happens to their edges: the declared output width is the
    /// caller's contract, not cleanup's to change.
    fn remove_dead_structure(&mut self) -> Result<bool, CleanupError> {
        let mut changed = false;
        loop {
            let has_outward: HashSet<&str> = self
                .creature
                .synapses
                .iter()
                .map(|s| s.from_uuid.as_str())
                .collect();

            let mut dead: HashSet<String> = HashSet::new();
            for neuron in &self.creature.neurons {
                if kind_of(neuron)? != Kind::Output && !has_outward.contains(neuron.uuid.as_str()) {
                    dead.insert(neuron.uuid.clone());
                }
            }
            if dead.is_empty() {
                return Ok(changed);
            }

            for synapse in &self.creature.synapses {
                if dead.contains(&synapse.to_uuid) {
                    self.removed_synapses.push(SynapseKey {
                        from_uuid: synapse.from_uuid.clone(),
                        to_uuid: synapse.to_uuid.clone(),
                        role: role_of(synapse),
                    });
                }
            }
            self.creature
                .synapses
                .retain(|s| !dead.contains(&s.to_uuid));

            for neuron in &self.creature.neurons {
                if dead.contains(&neuron.uuid) {
                    self.removed_neurons.push(neuron.uuid.clone());
                }
            }
            self.creature.neurons.retain(|n| !dead.contains(&n.uuid));
            changed = true;
        }
    }

    /// Fold the first hidden neuron that has nothing left to sum.
    ///
    /// With no inward edge the neuron's activation is `squash(bias)` on every
    /// record, so it is a constant wearing a hidden neuron's clothes. The value
    /// moves into the weights of its outward edges — exactly, because each
    /// target reads `activation · weight` — and the neuron itself becomes, or
    /// is replaced by, a bias-1 support constant.
    ///
    /// One fold per call: the choice of which support constant to reuse depends
    /// on the constants present, so folds are applied one at a time and the
    /// fixed-point loop comes back for the next.
    fn fold_zero_inward_hidden(&mut self) -> Result<bool, CleanupError> {
        let fed: HashSet<&str> = self
            .creature
            .synapses
            .iter()
            .map(|s| s.to_uuid.as_str())
            .collect();

        let mut folding = None;
        for neuron in &self.creature.neurons {
            if kind_of(neuron)? == Kind::Hidden && !fed.contains(neuron.uuid.as_str()) {
                folding = Some((neuron.uuid.clone(), squash_of(neuron)?, neuron.bias));
                break;
            }
        }
        let Some((uuid, squash, bias)) = folding else {
            return Ok(false);
        };

        // The value the forward pass computes for this neuron — see
        // [`zero_inward_activation`], which mirrors `CompiledNetwork::activate`
        // for a neuron with no synapses rather than assuming `apply_squash`
        // covers the aggregates.
        let value = f64::from(zero_inward_activation(squash, bias));
        if !value.is_finite() {
            return Err(CleanupError::NonFiniteBias { uuid });
        }

        match self.reusable_support_constant(&uuid)? {
            Some(support) => {
                self.repoint_outward(&uuid, &support, value)?;
                self.creature.neurons.retain(|n| n.uuid != uuid);
            }
            None => {
                self.scale_outward(&uuid, value)?;
                for neuron in &mut self.creature.neurons {
                    if neuron.uuid == uuid {
                        neuron.neuron_type = "constant".to_string();
                        neuron.squash = None;
                        neuron.bias = SUPPORT_CONSTANT_BIAS;
                    }
                }
            }
        }
        self.folded_neurons.push(uuid);
        Ok(true)
    }

    /// The support constant `uuid`'s outward edges can move onto, if any.
    ///
    /// Reuse is the invariant (Ockham #180) — a fold must not proliferate
    /// constants — but only where every edge that would land on an existing
    /// edge can merge **exactly**. Where it cannot, the neuron keeps its own
    /// support node: correctness outranks the constant budget.
    fn reusable_support_constant(&self, uuid: &str) -> Result<Option<String>, CleanupError> {
        let squashes = self.squash_map()?;
        // `normalise_constants` runs before the fold in every pass, so the bias
        // test is a re-check of that invariant rather than a filter that fires
        // — kept so a future pass reorder cannot quietly fold a value onto a
        // constant that is not worth 1.
        let candidates: Vec<String> = self
            .creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "constant" && n.bias == SUPPORT_CONSTANT_BIAS)
            .map(|n| n.uuid.clone())
            .collect();

        for candidate in candidates {
            if self.can_repoint_onto(uuid, &candidate, &squashes) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Would moving every outward edge of `from` onto `onto` stay exact?
    fn can_repoint_onto(
        &self,
        from: &str,
        onto: &str,
        squashes: &HashMap<String, SquashType>,
    ) -> bool {
        let mut landing: HashSet<(String, SynapseType)> = self
            .creature
            .synapses
            .iter()
            .filter(|s| s.from_uuid == onto)
            .filter_map(|s| {
                squashes
                    .get(&s.to_uuid)
                    .map(|sq| (s.to_uuid.clone(), canonical_role(*sq, role_of(s))))
            })
            .collect();

        for synapse in self
            .creature
            .synapses
            .iter()
            .filter(|s| s.from_uuid == from)
        {
            let Some(target_squash) = squashes.get(&synapse.to_uuid) else {
                return false;
            };
            let key = (
                synapse.to_uuid.clone(),
                canonical_role(*target_squash, role_of(synapse)),
            );
            let collides = !landing.insert(key);
            if collides && merge_rule(*target_squash) == MergeRule::Never {
                return false;
            }
        }
        true
    }

    /// Multiply every outward weight of `uuid` by `scale`.
    fn scale_outward(&mut self, uuid: &str, scale: f64) -> Result<(), CleanupError> {
        for synapse in &mut self.creature.synapses {
            if synapse.from_uuid != uuid {
                continue;
            }
            synapse.weight *= scale;
            if !synapse.weight.is_finite() {
                return Err(CleanupError::NonFiniteWeight {
                    from_uuid: synapse.from_uuid.clone(),
                    to_uuid: synapse.to_uuid.clone(),
                    weight: synapse.weight,
                });
            }
        }
        Ok(())
    }

    /// Move every outward edge of `from` onto `onto`, scaling as it goes.
    fn repoint_outward(&mut self, from: &str, onto: &str, scale: f64) -> Result<(), CleanupError> {
        let mut moving: Vec<SynapseExport> = Vec::new();
        let mut kept: Vec<SynapseExport> = Vec::new();
        for synapse in self.creature.synapses.drain(..) {
            if synapse.from_uuid == from {
                moving.push(synapse);
            } else {
                kept.push(synapse);
            }
        }
        self.creature.synapses = kept;

        for mut synapse in moving {
            synapse.from_uuid = onto.to_string();
            synapse.weight *= scale;
            if !synapse.weight.is_finite() {
                return Err(CleanupError::NonFiniteWeight {
                    from_uuid: synapse.from_uuid.clone(),
                    to_uuid: synapse.to_uuid.clone(),
                    weight: synapse.weight,
                });
            }
            self.insert_edge(synapse)?;
        }
        Ok(())
    }

    /// Add one edge, merging it into the edge already occupying its key.
    fn insert_edge(&mut self, synapse: SynapseExport) -> Result<(), CleanupError> {
        let target_squash = self.squash_of_uuid(&synapse.to_uuid)?;
        let role = canonical_role(target_squash, role_of(&synapse));

        let existing = self.creature.synapses.iter().position(|s| {
            s.from_uuid == synapse.from_uuid
                && s.to_uuid == synapse.to_uuid
                && canonical_role(target_squash, role_of(s)) == role
        });

        let Some(index) = existing else {
            self.creature.synapses.push(synapse);
            return Ok(());
        };

        let source_is_constant = self.is_support_constant(&synapse.from_uuid);
        let merged = merge_weights(
            merge_rule(target_squash),
            self.creature.synapses[index].weight,
            synapse.weight,
            source_is_constant,
            &synapse,
            target_squash,
        )?;
        self.creature.synapses[index].weight = merged;
        self.removed_synapses.push(SynapseKey {
            from_uuid: synapse.from_uuid,
            to_uuid: synapse.to_uuid,
            role,
        });
        Ok(())
    }

    /// Hold the constant support invariants: no squash, bias exactly 1, and no
    /// more than [`MAX_SUPPORT_CONSTANTS`] of them.
    ///
    /// A constant of value `b` is a bias-1 constant whose readers each scale by
    /// `b`, so the rescale is exact; so is folding one constant's edges onto
    /// another, since both deliver the same `1`. Unreferenced constants are not
    /// this pass's business — [`Engine::remove_dead_structure`] has already
    /// taken them, by the same rule it takes any dead node.
    fn normalise_constants(&mut self) -> Result<bool, CleanupError> {
        let mut changed = false;

        for neuron in &mut self.creature.neurons {
            if neuron.neuron_type == "constant" && neuron.squash.is_some() {
                neuron.squash = None;
                changed = true;
            }
        }

        let rescale: Vec<(String, f64)> = self
            .creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "constant" && n.bias != SUPPORT_CONSTANT_BIAS)
            .map(|n| (n.uuid.clone(), n.bias))
            .collect();
        for (uuid, bias) in rescale {
            self.scale_outward(&uuid, bias)?;
            for neuron in &mut self.creature.neurons {
                if neuron.uuid == uuid {
                    neuron.bias = SUPPORT_CONSTANT_BIAS;
                }
            }
            self.rescaled_constants.push(uuid);
            changed = true;
        }

        changed |= self.enforce_constant_cap()?;
        Ok(changed)
    }

    /// Merge surplus constants into a surviving one until at most
    /// [`MAX_SUPPORT_CONSTANTS`] remain.
    ///
    /// A merge that could not be made exactly — an edge landing on a `MEAN` or
    /// `HYPOT` target that already reads the host constant — is skipped and its
    /// constant survives. The budget is an invariant of the canonical form, not
    /// a licence to change what the creature computes.
    fn enforce_constant_cap(&mut self) -> Result<bool, CleanupError> {
        let mut changed = false;
        loop {
            let constants: Vec<String> = self
                .creature
                .neurons
                .iter()
                .filter(|n| n.neuron_type == "constant")
                .map(|n| n.uuid.clone())
                .collect();
            if constants.len() <= MAX_SUPPORT_CONSTANTS {
                return Ok(changed);
            }

            let squashes = self.squash_map()?;
            let mut merged = false;
            'surplus: for surplus in constants.iter().skip(MAX_SUPPORT_CONSTANTS) {
                for host in constants.iter().take(MAX_SUPPORT_CONSTANTS) {
                    if !self.can_repoint_onto(surplus, host, &squashes) {
                        continue;
                    }
                    self.repoint_outward(surplus, host, 1.0)?;
                    self.creature.neurons.retain(|n| &n.uuid != surplus);
                    self.merged_constants.push(surplus.clone());
                    merged = true;
                    changed = true;
                    break 'surplus;
                }
            }
            if !merged {
                return Ok(changed);
            }
        }
    }

    /// Put the creature in canonical form: one edge per readable key, the
    /// computational slice as constants then hiddens, and the synapses sorted
    /// by `(from, to, role)`.
    fn canonicalise(&mut self) -> Result<bool, CleanupError> {
        let squashes = self.squash_map()?;
        let mut changed = false;

        // A role only means something to an `IF`. Anywhere else it is noise
        // that would make two readings of the same edge look distinct.
        for synapse in &mut self.creature.synapses {
            let Some(target_squash) = squashes.get(&synapse.to_uuid) else {
                return Err(CleanupError::UnknownEndpoint {
                    uuid: synapse.to_uuid.clone(),
                });
            };
            if *target_squash != SquashType::If && synapse.synapse_type.is_some() {
                synapse.synapse_type = None;
                changed = true;
            }
        }

        changed |= self.coalesce_synapses(&squashes)?;
        changed |= self.order_neurons()?;
        changed |= self.sort_synapses()?;
        Ok(changed)
    }

    /// Sum (or, at a `MINIMUM` / `MAXIMUM` target, select) the edges that share
    /// one readable key, keeping the first occurrence's position.
    fn coalesce_synapses(
        &mut self,
        squashes: &HashMap<String, SquashType>,
    ) -> Result<bool, CleanupError> {
        let mut seen: HashMap<(String, String, SynapseType), usize> = HashMap::new();
        let mut kept: Vec<SynapseExport> = Vec::with_capacity(self.creature.synapses.len());
        let mut changed = false;

        for synapse in std::mem::take(&mut self.creature.synapses) {
            let Some(target_squash) = squashes.get(&synapse.to_uuid).copied() else {
                return Err(CleanupError::UnknownEndpoint {
                    uuid: synapse.to_uuid.clone(),
                });
            };
            let key = (
                synapse.from_uuid.clone(),
                synapse.to_uuid.clone(),
                canonical_role(target_squash, role_of(&synapse)),
            );
            match seen.get(&key) {
                None => {
                    seen.insert(key, kept.len());
                    kept.push(synapse);
                }
                Some(&index) => {
                    let source_is_constant = self.is_support_constant(&synapse.from_uuid);
                    kept[index].weight = merge_weights(
                        merge_rule(target_squash),
                        kept[index].weight,
                        synapse.weight,
                        source_is_constant,
                        &synapse,
                        target_squash,
                    )?;
                    self.removed_synapses.push(SynapseKey {
                        from_uuid: key.0,
                        to_uuid: key.1,
                        role: key.2,
                    });
                    changed = true;
                }
            }
        }
        self.creature.synapses = kept;
        Ok(changed)
    }

    /// List the computational slice as constants, then hiddens, then the
    /// outputs — validation rule 11's order, and the one the forward pass
    /// reads. Relative order within each group is preserved, which is what
    /// keeps a `forwardOnly` creature's edges pointing forwards.
    fn order_neurons(&mut self) -> Result<bool, CleanupError> {
        let before = self.creature.neurons.clone();
        let mut ordered: Vec<NeuronExport> = Vec::with_capacity(before.len());
        for wanted in [Kind::Constant, Kind::Hidden, Kind::Output] {
            for neuron in &before {
                if kind_of(neuron)? == wanted {
                    ordered.push(neuron.clone());
                }
            }
        }
        let changed = ordered != before;
        self.creature.neurons = ordered;
        Ok(changed)
    }

    /// Sort the synapses into `(from, to, role)` index order — validation
    /// rule 25, and what makes two equivalent creatures compare equal.
    ///
    /// Reuses [`sort_synapses_canonically`], the single home of that order
    /// (Issue #577), rather than restating its key; the comparison is only here
    /// to tell the fixed-point loop whether the order actually moved.
    fn sort_synapses(&mut self) -> Result<bool, CleanupError> {
        let before = self.creature.synapses.clone();
        sort_synapses_canonically(&mut self.creature);
        Ok(self.creature.synapses != before)
    }
}

/// What the forward pass computes for a neuron with **no** inward synapses.
///
/// This has to mirror [`crate::network::CompiledNetwork::activate`] exactly,
/// and [`apply_squash`] alone does not: the aggregate squashes are handled
/// outside the sum-then-squash path there, and `apply_squash` documents itself
/// as a single-value *fallback* for them (`HYPOT(x) = |x|`), which is not what
/// an empty aggregate produces. Getting this wrong is silent — the creature
/// keeps scoring, just differently — so each aggregate is spelled out:
///
/// | Squash | Empty-input activation | Why |
/// |---|---|---|
/// | `MINIMUM` / `MAXIMUM` | `bias` | no terms, so the extreme falls back to the bias |
/// | `MEAN` | `bias` | `n == 0`, so the division is skipped |
/// | `IF` | `bias` | the condition sum is `0`, so the empty negative branch is taken |
/// | `HYPOT` | `bias` | `sqrt(0) + bias` |
/// | `HYPOTv2` | `0` | the bias lives *inside* the per-synapse square, so an empty sum never sees it |
/// | anything else | `squash(bias)` | the ordinary weighted-sum path with a sum of zero |
///
/// The result goes through [`apply_limit_range`] because the forward pass does.
/// That clamp is a no-op for every bias [`Engine::check_references`] admits —
/// `apply_squash` already bounds its own outputs, and a finite bias is in range
/// for each aggregate arm above — so it is here to keep the mirror complete
/// rather than because a test can tell it apart.
///
/// Crate-visible because [`crate::prune_neuron`] needs the same value to fold a
/// structurally constant neuron into its targets (Issue #590): one mirror of
/// the forward pass, not two.
pub(crate) fn zero_inward_activation(squash: SquashType, bias: f64) -> f32 {
    let bias = bias as f32;
    let raw = match squash {
        SquashType::Minimum
        | SquashType::Maximum
        | SquashType::Mean
        | SquashType::Hypotenuse
        | SquashType::If => bias,
        SquashType::HypotenuseV2 => 0.0,
        _ => apply_squash(squash, bias),
    };
    apply_limit_range(squash, raw)
}

/// The activation `uuid` produces on **every** record, when the creature alone
/// proves there is one.
///
/// A constant emits its bias, and a neuron with no inward edge sums nothing —
/// [`zero_inward_activation`] is the shared mirror of the forward pass for that
/// case, and a constant reaches it through the same door. An observation
/// neuron, and any neuron something still feeds, varies with the record and
/// answers `None`.
///
/// This is the **one** home of that question: the `IF` static-condition rewrite
/// here, `prune_neuron`'s structural fold (Issue #590) and `prune_synapse`'s
/// source fold (Issue #591) all ask it here rather than restating it, so they
/// cannot drift apart on what "fixed" means.
///
/// # Errors
///
/// Returns [`CleanupError::Creature`] when the neuron declares a squash name
/// this crate does not know.
pub(crate) fn fixed_activation(
    creature: &CreatureExport,
    uuid: &str,
) -> Result<Option<f32>, CleanupError> {
    // An observation neuron is not listed in `neurons`, and it varies with the
    // record; a name that is neither listed nor an observation is a dangling
    // endpoint `check_references` fails on before any pass runs.
    if is_observation_uuid(creature, uuid) || creature.synapses.iter().any(|s| s.to_uuid == uuid) {
        return Ok(None);
    }
    let Some(neuron) = creature.neurons.iter().find(|n| n.uuid == uuid) else {
        return Ok(None);
    };
    Ok(Some(zero_inward_activation(
        squash_of(neuron)?,
        neuron.bias,
    )))
}

/// The branch an `IF` always takes, when the creature alone decides it.
///
/// `None` means at least one condition source varies with the record, so the
/// branch is genuinely dynamic and no rewrite may assume one.
///
/// `> 0` chooses the positive arm, and an untyped edge belongs to it: both
/// readings are [`crate::network::CompiledNetwork::activate`]'s, restated here
/// because that helper reasons over a compiled synapse range and this pass has
/// only the exported creature.
///
/// This is the **one** home of that question. The `IF` rewrite here asks it of
/// the creature it is repairing, and [`crate::prune_neuron`] asks it of the
/// creature the caller handed in — the two readings are what prove a flatten
/// left the branch where it was — so neither may restate the rule.
///
/// The sum is accumulated in `f32`, walking the creature's synapse list in
/// storage order, because that is the closest available mirror of what
/// [`crate::network::CompiledNetwork::activate`] computes for **this**
/// creature: `compile_creature` groups the inward edges of a target in the
/// order it meets them, and a condition edge only ever lands in the condition
/// accumulator. The creature in hand is the one whose behaviour the rewrite
/// must preserve, so mirroring its own storage order — not the canonical order
/// a later pass will impose — is what keeps the strict `> 0` branch decision
/// the same on both sides.
///
/// The mirror is a mirror, not the pass itself: `compile_creature` regroups the
/// edges per target, so the two additions can associate differently and a sum
/// within `f32` rounding of `0` is a boundary neither side can claim to settle
/// for the other. Away from that boundary — which is every condition any
/// training run produces — the branch the two pick is the same, and the flatten
/// is exact.
///
/// # Errors
///
/// Returns [`CleanupError::Creature`] when a condition source declares a squash
/// name this crate does not know.
pub(crate) fn static_condition_branch(
    creature: &CreatureExport,
    uuid: &str,
) -> Result<Option<SynapseType>, CleanupError> {
    let mut sum = 0.0f32;
    for synapse in creature
        .synapses
        .iter()
        .filter(|s| s.to_uuid == uuid && role_of(s) == SynapseType::Condition)
    {
        let Some(activation) = fixed_activation(creature, &synapse.from_uuid)? else {
            return Ok(None);
        };
        sum += activation * synapse.weight as f32;
    }
    Ok(Some(if sum > 0.0 {
        SynapseType::Positive
    } else {
        SynapseType::Negative
    }))
}

/// Is this the wire UUID of one of the creature's observation neurons?
///
/// Input neurons are not listed in `neurons` — the declared width is what says
/// they exist (Issue #550) — so the name is the only thing to test.
pub(crate) fn is_observation_uuid(creature: &CreatureExport, uuid: &str) -> bool {
    uuid.strip_prefix("input-")
        .and_then(|index| index.parse::<usize>().ok())
        .is_some_and(|index| index < creature.input)
}

/// Combine two edges that share one readable key, or refuse to.
fn merge_weights(
    rule: MergeRule,
    existing: f64,
    added: f64,
    source_is_constant: bool,
    synapse: &SynapseExport,
    target_squash: SquashType,
) -> Result<f64, CleanupError> {
    let inexact = || CleanupError::InexactMerge {
        from_uuid: synapse.from_uuid.clone(),
        to_uuid: synapse.to_uuid.clone(),
        squash: squash_name_from(target_squash),
    };
    match rule {
        MergeRule::Sum => Ok(existing + added),
        // Exact only for a constant source: both terms are then the weight
        // itself, on every record, so the extreme of the two is the extreme.
        MergeRule::MinWeight if source_is_constant => Ok(existing.min(added)),
        MergeRule::MaxWeight if source_is_constant => Ok(existing.max(added)),
        _ => Err(inexact()),
    }
}
