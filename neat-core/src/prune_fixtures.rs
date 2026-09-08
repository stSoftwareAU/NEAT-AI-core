//! Canonical pruning parity fixtures captured from NEAT-AI's TypeScript
//! removal paths (Issue #588).
//!
//! The TypeScript engine has carried the fleet's pruning semantics for years:
//! remove a hidden neuron or one synapse, then repair whatever the removal
//! broke so the creature that comes back is still valid. Issue #587 moves that
//! behaviour into this crate, and a rewrite with no recorded "before" is a
//! rewrite nobody can grade. These fixtures are that record — each one is a
//! `(before, request, after)` triple where `after` is the creature the
//! **TypeScript** produced, captured by driving the real mutation operators.
//!
//! Nothing here prunes. [`PruneCase::after`] is the acceptance oracle the
//! shared helpers are graded against, and both now exist:
//! [`crate::prune_neuron::prune_neuron`] (Issue #590) for the
//! [`PruneRequest::RemoveNeuron`] captures and
//! [`crate::prune_synapse::prune_synapse`] (Issue #591) for the
//! [`PruneRequest::RemoveSynapse`] ones.
//!
//! **How a case is graded depends on the case**, and the per-case table in
//! `docs/research/pruning-parity-matrix.md` is the record. Where this crate's
//! canonical form coincides with the capture, `prune(before, request) == after`
//! holds byte for byte. Where the documented constant-support divergence
//! applies — the capture keeps a folded value in a constant's *bias*, this
//! crate keeps it in the reading edges' weights — the two are graded on the
//! numbers they produce instead, by compiling and activating both. Claiming
//! byte equality everywhere would be claiming a parity this crate deliberately
//! does not have.
//!
//! The fixtures themselves stay pinned by `neat-core/tests/prune_parity.rs`,
//! which checks every documented rule below against the captured pair.
//!
//! The full mapping — each TypeScript behaviour and test to its fixture, how
//! the captures were taken, and what is deliberately not captured — is in
//! `docs/research/pruning-parity-matrix.md`.
//!
//! ## How the fixtures were captured
//!
//! Each `before` creature was loaded into NEAT-AI (`Creature.fromJSON`), the
//! real operator — `SubNeuron` or `SubConnection` — was run several hundred
//! times from a fresh copy, and the distinct outcomes were grouped by which
//! neuron or synapse the operator happened to pick. The outcome for **this**
//! case's [`PruneCase::request`] is the `after` recorded here. The JSON was
//! transcribed from `exportJSON()` — re-indented and given the `forwardOnly`
//! flag this crate always writes; every neuron, synapse, weight, bias, role and
//! their order are the TypeScript output unchanged. Both halves of every pair
//! were `creatureValidate`d TypeScript-side before they were written down. The
//! capture used NEAT-AI `7.0.25`; the operator picks its target at random,
//! which is why the harness enumerates outcomes rather than forcing one. The
//! harness itself is in `docs/research/pruning-parity-matrix.md`, so the
//! captures can be re-derived when the TypeScript changes.
//!
//! ## The rules the fixtures encode
//!
//! | Case | Rule | TypeScript source |
//! | --- | --- | --- |
//! | [`CASCADE_ORPHAN_FEEDERS`] | removing a neuron removes every feeder the removal orphans, to a fixed point | `SubNeuron.ts`, `OrphanedNeuronCleanup.ts::removeHiddenNeuron` |
//! | [`EDGE_TARGET_BECOMES_CONSTANT`] | a hidden target that loses its last inward edge but keeps an outward one becomes a **constant** whose bias is its own squash of its old bias | `SubConnection.ts` |
//! | [`EDGE_SOURCE_BECOMES_DEAD`] | a hidden/constant source left with no outward edge is removed | `SubConnection.ts` |
//! | [`EDGE_ROLE_IDENTITY`] | only the requested `(from, to, role)` triple goes; the other roles of that ordered pair stay | `SubConnection.ts` (NEAT-AI #3873) |
//! | [`IF_REPAIR_COALESCES_ROLES`] | an `IF` that loses a required role is downgraded to `IDENTITY`, its inward roles are stripped, and rows that were only distinct by role are summed into one | `RepairInvalidIfNeurons.ts` |
//! | [`CONSTANT_MOVES_INTO_PREFIX`] | after a hidden→constant flip the computational slice is re-ordered constants-then-hiddens, and the synapses re-sorted | `NormaliseComputationalNeuronOrder.ts` |
//! | [`MEMETIC_DROPPED_ON_REMOVAL`] | a successful removal invalidates the content-derived identity — of which `CreatureExport` represents only `memetic` | `OrphanedNeuronCleanup.ts::removeHiddenNeuron` (NEAT-AI #3843) |
//! | [`CONSTANT_BIAS_FOLD`] | discovery's compensation folds `w · meanActivation` of the removed neuron into each target's bias | `DiscoveryNeuronRemoval.ts::applyMeanBiasFold` |
//!
//! ```mermaid
//! flowchart TD
//!     R["requested removal<br/>neuron or (from, to, role)"] --> C["cascade: drop every<br/>orphaned feeder"]
//!     C --> K["target with no inward<br/>but an outward edge<br/>→ constant, bias = squash(bias)"]
//!     K --> I["IF missing a role<br/>→ IDENTITY, strip roles,<br/>sum the coalesced rows"]
//!     I --> N["canonicalise: constants,<br/>then hiddens, then outputs;<br/>re-sort synapses"]
//!     N --> M["drop the content-derived identity<br/>(memetic here; uuid TypeScript-side)"]
//!     M --> V["validate — a successful<br/>rewrite never returns<br/>an invalid creature"]
//! ```
//!
//! ## Compensation is the caller's statistic, not ours
//!
//! [`CONSTANT_BIAS_FOLD`] is the one case carrying a
//! [`PruneCase::mean_activation`]: discovery measures the removed neuron's mean
//! activation and the rewrite folds `w · mean` into each target's bias. For a
//! **constant** neuron that mean *is* its output on every record, so the fold
//! is exact and the pruned creature scores identically — which is what
//! [`PruneCase::output_preserving`] marks and what the test asserts by
//! activating both halves.

use crate::creature::{CreatureExport, parse_creature_json};
use crate::synapse_type::SynapseType;

/// What the caller asked the (future) pruning helper to remove.
///
/// Callers own *which* neuron or synapse to try — see Issue #587's ownership
/// boundary — so a case records the request rather than a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneRequest {
    /// Remove one hidden or constant neuron, named by its wire UUID.
    RemoveNeuron {
        /// Wire UUID of the neuron to remove.
        uuid: &'static str,
    },
    /// Remove one synapse, named by the full `(from, to, role)` triple.
    ///
    /// The role is part of the identity (Issue #577, NEAT-AI #3873): an `IF`
    /// target may be fed once per role by the same source, and only the
    /// requested role is removed.
    RemoveSynapse {
        /// Wire UUID of the source neuron.
        from_uuid: &'static str,
        /// Wire UUID of the target neuron.
        to_uuid: &'static str,
        /// Which role of that ordered pair to remove.
        role: SynapseType,
    },
}

/// One captured TypeScript removal: the creature before, the request, and the
/// creature TypeScript produced.
#[derive(Debug, Clone, Copy)]
pub struct PruneCase {
    /// Stable name of the case, used in the parity matrix and test failures.
    pub name: &'static str,
    /// The behaviour this case pins, in one line.
    pub rule: &'static str,
    /// TypeScript source file(s) the behaviour lives in.
    pub ts_source: &'static str,
    /// TypeScript test that already covers the behaviour, or `""` when the
    /// rule is only pinned by its source.
    pub ts_test: &'static str,
    /// What was asked for.
    pub request: PruneRequest,
    /// Mean activation of the removed neuron, when the case carries the
    /// caller-supplied statistic discovery's compensation needs.
    pub mean_activation: Option<f64>,
    /// True when the rewrite provably preserves the creature's output on every
    /// record — an *exact* transform rather than an approximate one.
    pub output_preserving: bool,
    before_json: &'static str,
    after_json: &'static str,
}

impl PruneCase {
    /// The creature as it stood before the removal.
    ///
    /// # Panics
    ///
    /// Panics when the fixture JSON does not parse — a fixture this crate
    /// cannot read is a defect in the fixture, not a runtime condition, and
    /// must fail loudly rather than be skipped.
    pub fn before(&self) -> CreatureExport {
        parse_creature_json(self.before_json).unwrap_or_else(|e| {
            panic!(
                "prune fixture {} has an unparsable `before`: {e}",
                self.name
            )
        })
    }

    /// The creature NEAT-AI's TypeScript produced for [`Self::request`].
    ///
    /// # Panics
    ///
    /// Panics when the fixture JSON does not parse, for the reason given on
    /// [`Self::before`].
    pub fn after(&self) -> CreatureExport {
        parse_creature_json(self.after_json).unwrap_or_else(|e| {
            panic!("prune fixture {} has an unparsable `after`: {e}", self.name)
        })
    }
}

/// Removing a neuron cascades through every feeder the removal orphans.
///
/// `input-0 → h-c → h-a → h-x`, `input-1 → h-d → h-b → h-x`, `h-x → output-0`,
/// plus a direct `input-0 → output-0` that keeps the output fed. Removing
/// `h-x` orphans `h-a` and `h-b`, whose removal orphans `h-c` and `h-d`, so
/// the whole computational slice goes and only the direct edge survives.
pub const CASCADE_ORPHAN_FEEDERS: PruneCase = PruneCase {
    name: "cascade_orphan_feeders",
    rule: "removing a neuron removes every feeder the removal orphans, to a fixed point",
    ts_source: "src/mutate/SubNeuron.ts, src/compact/OrphanedNeuronCleanup.ts",
    ts_test: "test/mutate/SubNeuronCascadeOrphan.ts",
    request: PruneRequest::RemoveNeuron { uuid: "h-x" },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-c","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-d","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-a","bias":0.3,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-b","bias":0.4,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-x","bias":0.5,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.6,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
        {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-d"},
        {"weight":1.0,"fromUUID":"h-c","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"h-d","toUUID":"h-b"},
        {"weight":1.0,"fromUUID":"h-a","toUUID":"h-x"},
        {"weight":1.0,"fromUUID":"h-b","toUUID":"h-x"},
        {"weight":1.0,"fromUUID":"h-x","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"output","uuid":"output-0","bias":0.6,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
};

/// A hidden target that loses its last inward edge becomes a constant.
///
/// `h-1` keeps its outward edge into the output, so it is not removed: its
/// bias becomes `LOGISTIC(0.4) = 0.598687660112452` — its own squash applied
/// to its old bias — the squash is dropped, and the neuron is now a constant.
pub const EDGE_TARGET_BECOMES_CONSTANT: PruneCase = PruneCase {
    name: "edge_target_becomes_constant",
    rule: "a hidden target with no inward edge left but an outward one becomes a constant, bias = squash(bias)",
    ts_source: "src/mutate/SubConnection.ts",
    ts_test: "test/mutate/SubConnection.ts",
    request: PruneRequest::RemoveSynapse {
        from_uuid: "input-0",
        to_uuid: "h-1",
        role: SynapseType::Standard,
    },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-1","bias":0.4,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.5,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":0.5,"fromUUID":"input-1","toUUID":"output-0"},
        {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"constant","uuid":"h-1","bias":0.598687660112452},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":0.5,"fromUUID":"input-1","toUUID":"output-0"},
        {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
};

/// A source left with nothing to feed is removed.
///
/// `h-1`'s only outward edge was the one removed, so it goes; `h-2` keeps its
/// other inward edge from `input-1` and stays a hidden neuron.
pub const EDGE_SOURCE_BECOMES_DEAD: PruneCase = PruneCase {
    name: "edge_source_becomes_dead",
    rule: "a hidden or constant source left with no outward edge is removed",
    ts_source: "src/mutate/SubConnection.ts",
    ts_test: "test/mutate/SubConnectionStaleFromIndex.ts",
    request: PruneRequest::RemoveSynapse {
        from_uuid: "h-1",
        to_uuid: "h-2",
        role: SynapseType::Standard,
    },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":0.25,"fromUUID":"input-1","toUUID":"h-2"},
        {"weight":0.75,"fromUUID":"h-1","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":0.25,"fromUUID":"input-1","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
      ]
    }"#,
};

/// Removing one role leaves the other roles of the same ordered pair alone.
///
/// `h-a` feeds `if-1` twice — `positive` and `negative`. Removing the
/// `negative` role keeps the `positive` row at its own weight, and every other
/// pair into `if-1` is untouched. The `IF` keeps a condition, a positive and a
/// negative, so no repair is triggered.
pub const EDGE_ROLE_IDENTITY: PruneCase = PruneCase {
    name: "edge_role_identity",
    rule: "only the requested (from, to, role) triple is removed, not every edge of the pair",
    ts_source: "src/mutate/SubConnection.ts, src/architecture/SynapseKey.ts",
    ts_test: "test/creature/TypedSynapseKey.ts",
    request: PruneRequest::RemoveSynapse {
        from_uuid: "h-a",
        to_uuid: "if-1",
        role: SynapseType::Negative,
    },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-cond","bias":1.0},
        {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-b","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"if-1","type":"condition"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-b"},
        {"weight":-0.5,"fromUUID":"c-cond","toUUID":"if-1","type":"condition"},
        {"weight":-1.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
        {"weight":-2.0,"fromUUID":"h-b","toUUID":"if-1","type":"negative"},
        {"weight":3.0,"fromUUID":"h-b","toUUID":"if-1","type":"positive"},
        {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-cond","bias":1.0},
        {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-b","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"if-1","type":"condition"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-b"},
        {"weight":-0.5,"fromUUID":"c-cond","toUUID":"if-1","type":"condition"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
        {"weight":-2.0,"fromUUID":"h-b","toUUID":"if-1","type":"negative"},
        {"weight":3.0,"fromUUID":"h-b","toUUID":"if-1","type":"positive"},
        {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
      ]
    }"#,
};

/// An `IF` that loses a required role is downgraded, and its rows coalesce.
///
/// `h-cond` is the only source of `if-1`'s `condition` role. Removing it
/// leaves the `IF` structurally invalid, so it is downgraded to `IDENTITY` and
/// its inward roles are stripped — and because `IDENTITY` sums every inward
/// row regardless of role, `h-a`'s `positive` (`2.0`) and `negative` (`-3.0`)
/// rows become **one** untyped row of `-1.0` rather than a duplicate pair.
pub const IF_REPAIR_COALESCES_ROLES: PruneCase = PruneCase {
    name: "if_repair_coalesces_roles",
    rule: "an IF missing a required role is downgraded to IDENTITY, roles stripped, coalesced rows summed",
    ts_source: "src/architecture/RepairInvalidIfNeurons.ts, src/architecture/CoalesceInwardSynapses.ts",
    ts_test: "test/NEAT/MutatorForwardOnlyIFConditionsRepair.ts, test/fix/IfRoleAssignmentCoalesce.ts",
    request: PruneRequest::RemoveNeuron { uuid: "h-cond" },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-cond","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-a","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-cond"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"h-cond","toUUID":"if-1","type":"condition"},
        {"weight":-3.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
        {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IDENTITY"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-a"},
        {"weight":-1.0,"fromUUID":"h-a","toUUID":"if-1"},
        {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
      ]
    }"#,
};

/// A converted constant is moved into the constant prefix.
///
/// `h-2` loses its only inward edge and flips to a constant with bias
/// `LOGISTIC(0.3) = 0.574442516811659`. The computational slice was
/// `[c-1, h-1, h-2]`; afterwards it is `[c-1, h-2, h-1]` — constants first,
/// hiddens after — and the synapses are re-sorted into the new index order.
pub const CONSTANT_MOVES_INTO_PREFIX: PruneCase = PruneCase {
    name: "constant_moves_into_prefix",
    rule: "after a hidden→constant flip the slice is re-ordered constants-then-hiddens and synapses re-sorted",
    ts_source: "src/architecture/NormaliseComputationalNeuronOrder.ts",
    ts_test: "test/architecture/ForwardOnlyTopologyAfterBulkRemap.ts, test/validate/CreatureValidate.ts",
    request: PruneRequest::RemoveSynapse {
        from_uuid: "input-0",
        to_uuid: "h-2",
        role: SynapseType::Standard,
    },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":0.5},
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-2","bias":0.3,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-1"},
        {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":0.5},
        {"type":"constant","uuid":"h-2","bias":0.574442516811659},
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-1"},
        {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
};

/// A successful removal sheds the content-derived identity.
///
/// Deliberately the **same** topology and request as
/// [`CASCADE_ORPHAN_FEEDERS`], so the memetic record is the only thing that
/// differs between the two captures and the rule is isolated — a test pins that
/// equality, so the pair cannot drift apart. The record's bias and weight both
/// name structure the removal deletes.
///
/// TypeScript drops `memetic` **and** `uuid` wholesale on every removal rather
/// than pruning them: both are derived from the neurons and synapses, so
/// neither describes the creature any more. Only `memetic` is representable
/// here — a `CreatureExport` carries no creature-level `uuid` — so that is the
/// half this fixture captures.
pub const MEMETIC_DROPPED_ON_REMOVAL: PruneCase = PruneCase {
    name: "memetic_dropped_on_removal",
    rule: "a successful removal drops the content-derived identity — memetic, the half a CreatureExport carries",
    ts_source: "src/compact/OrphanedNeuronCleanup.ts, src/mutate/SubNeuron.ts",
    ts_test: "test/architecture/StaleUuidAfterStructuralChange.ts",
    request: PruneRequest::RemoveNeuron { uuid: "h-x" },
    mean_activation: None,
    output_preserving: false,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-c","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-d","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-a","bias":0.3,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-b","bias":0.4,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-x","bias":0.5,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.6,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
        {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-d"},
        {"weight":1.0,"fromUUID":"h-c","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"h-d","toUUID":"h-b"},
        {"weight":1.0,"fromUUID":"h-a","toUUID":"h-x"},
        {"weight":1.0,"fromUUID":"h-b","toUUID":"h-x"},
        {"weight":1.0,"fromUUID":"h-x","toUUID":"output-0"}
      ],
      "memetic":{
        "generation":3,"score":0.25,
        "biases":{"h-x":0.05},
        "weights":[{"fromUUID":"h-x","toUUID":"output-0","weight":1.0}]
      }
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"output","uuid":"output-0","bias":0.6,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
};

/// Discovery's compensation folds the removed neuron's mean into its targets.
///
/// `c-1` is a constant of `0.5` feeding `output-0` with weight `0.2`, so its
/// mean activation is exactly `0.5` and the fold is
/// `output-0.bias += 0.2 · 0.5 = 0.1`, taking the bias from `0.25` to `0.35`.
/// A constant carries no per-sample variance, so the fold is complete on its
/// own and the pruned creature scores identically on every record — the one
/// [`PruneCase::output_preserving`] case here.
pub const CONSTANT_BIAS_FOLD: PruneCase = PruneCase {
    name: "constant_bias_fold",
    rule: "compensation folds w · meanActivation of the removed neuron into each target's bias",
    ts_source: "src/architecture/ErrorGuidedStructuralEvolution/DiscoveryNeuronRemoval.ts",
    ts_test: "test/ErrorGuidedStructuralEvolution/DiscoveryOperationIntegrity.ts",
    request: PruneRequest::RemoveNeuron { uuid: "c-1" },
    mean_activation: Some(0.5),
    output_preserving: true,
    before_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":0.5},
        {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"}
      ]
    }"#,
    after_json: r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"output","uuid":"output-0","bias":0.35,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
};

/// Every captured case, in parity-matrix order.
///
/// `neat-core/tests/prune_parity.rs` walks this slice, so a case added here is
/// checked against every shared rule without touching the test.
pub const PRUNE_PARITY_CASES: &[PruneCase] = &[
    CASCADE_ORPHAN_FEEDERS,
    EDGE_TARGET_BECOMES_CONSTANT,
    EDGE_SOURCE_BECOMES_DEAD,
    EDGE_ROLE_IDENTITY,
    IF_REPAIR_COALESCES_ROLES,
    CONSTANT_MOVES_INTO_PREFIX,
    MEMETIC_DROPPED_ON_REMOVAL,
    CONSTANT_BIAS_FOLD,
];
