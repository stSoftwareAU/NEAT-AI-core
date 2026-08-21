//! Creature validation — the shared definition of a valid creature (Issue #559).
//!
//! This module is the Rust home of NEAT-AI's
//! `src/architecture/CreatureValidate.ts`: the invariants a healthy topology
//! must hold. NEAT-AI-Forests shipped an invalid creature that
//! `Creature.validate()` would have caught, and the fix is **one** definition
//! both stacks read — Rust consumers call [`creature_validate`] natively, and
//! NEAT-AI calls the same code over the existing WASM boundary.
//!
//! **Rules 1–22 — the neuron half — are ported** (Issue #560). The synapse,
//! forward-only and memetic rules (23–31) are Issue #561, so
//! [`creature_validate`] still refuses to certify any creature: it runs every
//! neuron rule and then reports the un-ported half rather than returning an
//! `Ok` it cannot stand behind. See [the entry point](creature_validate).
//!
//! # Options
//!
//! [`ValidateOptions`] mirrors the TypeScript options bag, including two
//! asymmetries that are easy to lose in a port and are pinned by
//! `neat-core/tests/creature_validate_contract.rs`:
//!
//! - `forwardOnly === true` forces `feedbackLoop` to `false`
//!   ([`ValidateOptions::resolved_feedback_loop`]); otherwise `feedbackLoop`
//!   passes through and **only** an explicit `Some(false)` rejects `from > to`
//!   ([`ValidateOptions::rejects_recursive_synapses`]).
//! - `if (options && options.neurons)` is a *truthiness* test, so an expected
//!   neuron count of `0` is **not** checked, while
//!   `Number.isInteger(options.connections)` checks `0` like any other integer
//!   ([`ValidateOptions::expected_neurons`] /
//!   [`ValidateOptions::expected_connections`]).
//!
//! # Rule order is part of the contract
//!
//! First failure wins: [`creature_validate`] returns on the first violated
//! rule, matching the TypeScript `throw`. The order below is the order the
//! ports must evaluate in — a creature breaking two rules must name the same
//! one in both stacks.
//!
//! | # | Rule | Class / `reason` |
//! |---|------|------------------|
//! | 1 | expected neuron count (when truthy) | `Validation` / `OTHER` |
//! | 2 | `input` is an integer `>= 1` | `Validation` / `OTHER` |
//! | 3 | `output` is an integer `>= 1` | `Validation` / `OTHER` |
//! | 4 | neuron has an `id` | `Validation` / `OTHER` |
//! | 5 | `id` is an integer `<=` [`MAX_NEURON_ID`] | `Validation` / `OTHER` |
//! | 6 | `id` is unique | `Validation` / `OTHER` |
//! | 7 | input neuron's `id` equals its index | `Validation` / `OTHER` |
//! | 8 | non-input neuron has a finite bias | `Validation` / `OTHER` |
//! | 9 | no neuron follows an output neuron | `Validation` / `OTHER` |
//! | 10 | no input neuron past the declared input width | `Validation` / `OTHER` |
//! | 11 | in the computational slice, no constant after a hidden | `Validation` / `NEURON_ORDER` |
//! | 12 | an `IF` neuron has >= 3 inward edges, one of each role | `Validation` / `IF_CONDITIONS` |
//! | 13 | `input` neuron has no inward edges | `Topology` / `INVALID_CONNECTION` |
//! | 14 | `constant` neuron has no inward edges | `Topology` / `INVALID_CONNECTION` |
//! | 15 | `constant` neuron has no squash | `Topology` / `INVALID_SQUASH` |
//! | 16 | `constant` neuron has an outward edge | `Validation` / `NO_OUTWARD_CONNECTIONS` |
//! | 17 | `hidden` neuron has an inward edge | `Validation` / `NO_INWARD_CONNECTIONS` |
//! | 18 | `hidden` neuron has an outward edge | `Validation` / `NO_OUTWARD_CONNECTIONS` |
//! | 19 | `hidden` neuron has a finite bias | `Topology` / `INVALID_STATE` |
//! | 20 | declared type is one of the four known types | `Topology` / `INVALID_NEURON_TYPE` |
//! | 21 | counted inputs match the declared `input` | `Validation` / `OTHER` |
//! | 22 | counted outputs match the declared `output` | `Topology` / `INVALID_STATE` |
//! | 23 | no synapse points at an input neuron | `Topology` / `INVALID_CONNECTION` |
//! | 24 | no self connection (when `forward_only`) | `Validation` / `SELF_CONNECTION` |
//! | 25 | synapses sorted by `(from, to)` | `Topology` / `SORT_FAILURE` |
//! | 26 | no duplicate `(from, to)` pair | `Topology` / `INVALID_CONNECTION` |
//! | 27 | no `from > to` when recursion is disallowed | `Validation` / `RECURSIVE_SYNAPSE` |
//! | 28 | expected connection count (when set) | `Validation` / `OTHER` |
//! | 29 | forward-only creatures are sorted, self-loop free and acyclic | `Topology` / `INVALID_CONNECTION` |
//! | 30 | forward-only structural integrity | `Validation` / `OTHER` |
//! | 31 | memetic biases / weights resolve to real neurons and synapses | `Validation` / `MEMETIC` |
//!
//! Rule 26 is the same notion of "duplicate" as
//! [`crate::creature::validate_no_duplicate_synapses`] (Issue #556) — one
//! ordered `(from, to)` pair, at most once — reported here under the
//! TypeScript's own class and reason (`TopologyError` / `INVALID_CONNECTION`,
//! *not* `DUPLICATE_SYNAPSE`, which `creatureValidate` never raises).
//!
//! # Input format
//!
//! A creature reaches the validator as a [`CreatureExport`] — the JSON wire
//! shape this crate already parses — not as a second, validator-only struct.
//! Two fields were added for the rules above, both optional and both skipped
//! when absent, so every existing [`crate::creature::parse_creature_json`]
//! caller and every already-written creature file is unaffected:
//!
//! | Field | Why the validator needs it |
//! |-------|----------------------------|
//! | [`crate::creature::NeuronExport::id`] | rules 4–7 and the memetic keys; output neurons carry **negative** ids (NEAT-AI #1958), so it is signed |
//! | [`crate::creature::CreatureExport::memetic`] | rule 31 (`biases`, `weights`) |
//!
//! The export form is **index-free**: it lists only non-input neurons and
//! wires them by UUID, while the TypeScript validator walks an in-memory
//! creature indexed from zero. The port derives the indices the same way
//! [`crate::creature::compile_creature`] does, and that derivation is part of
//! the contract:
//!
//! - indices `0..input` are the implicit input neurons, UUID `input-N`, with
//!   `id == index` (so rules 7, 10 and 21 hold by construction, and rule 21
//!   reports `stats.input == creature.input`);
//! - index `input + i` is `creature.neurons[i]`;
//! - a synapse's `from` / `to` are those indices, resolved from `fromUUID` /
//!   `toUUID`. An endpoint naming no neuron is `Topology` /
//!   `INVALID_SYNAPSE_REFERENCE` — the one failure the in-memory TypeScript
//!   form cannot express, mapped onto its existing union rather than a new
//!   name;
//! - `neurons` may not declare `type: "input"`; input neurons are implicit, so
//!   an entry claiming to be one is `Topology` / `INVALID_NEURON_TYPE`;
//! - `creature.neurons.length` in the TypeScript rules (1, 11) means
//!   `input + neurons.len()` here.
//!
//! Two TypeScript checks are unreachable in the Rust shape because serde
//! rejects the input earlier, and that is deliberate — a malformed file fails
//! at the parse boundary instead of reaching the validator: a non-integer
//! neuron `id` (rule 5) and a non-array memetic `weights` entry (rule 31) are
//! [`crate::creature::CreatureError::Json`].
//!
//! # Runtime ids are derived, not required (Issue #560)
//!
//! `NeuronExport::id` is optional because NEAT-AI stopped writing it: the wire
//! identity is the UUID (NEAT-AI #1958). The rules still read an id, so this
//! port reproduces the assignment NEAT-AI's loader
//! (`src/creature/CreatureSerialization.ts`, `src/neuron/NeuronSerialization.ts`)
//! performs *before* `creatureValidate` ever runs — otherwise every modern
//! export would fail rule 4:
//!
//! | Neuron | Id |
//! |--------|----|
//! | implicit input at index `i` | `i` |
//! | exported `output` | `-(outputIndex + 1)`, overriding any exported id |
//! | anything else | the exported `id`, else a deterministic hash of the UUID into `[1_000_000, 2_000_000_000)` |
//!
//! A neuron with neither an id nor a UUID has no derivable identity —
//! TypeScript falls back to a process-global counter there, which nothing else
//! can reproduce — so it reaches the walk with no id and rule 4 reports it.
//!
//! Because the ids of inputs and outputs are derived rather than read, rules 7,
//! 10 and 21 cannot fail for a `CreatureExport`; they are implemented and
//! covered against the walk itself, so the port stays 1:1 with the TypeScript.
//! Rule 19 is unreachable in **both** stacks — rule 8 rejects a missing or
//! non-finite bias on every non-input neuron first — and is ported for the same
//! reason.
//!
//! # Diagnostic labels
//!
//! Where the TypeScript interpolates `neuron.ID()` the message carries the
//! integer id verbatim. Where it interpolates
//! `neuronWireLabelForDiagnostics(neuron, index)` — rules 11, 16, 17, 18 — this
//! port reproduces `src/neuron/NeuronSerialization.ts` branch for branch, since
//! NEAT-AI's error-message tests assert on that text: `input-N` for an input,
//! `output-N` for an output carrying its negative id, otherwise the wire UUID,
//! and `non-output-negative-id-{id}@index-{index}` /
//! `missing-uuid@index-{index}` when there is no UUID to use.
//!
//! # Shared invariants
//!
//! The wiring questions this module and
//! [`crate::topology_ops::validate_structural_integrity`] both ask — inward and
//! outward degree, "is this hidden neuron wired in and out", "does this `IF`
//! neuron carry all three roles" — have one implementation, in
//! [`crate::topology_invariants`]. The two report them differently (a numeric
//! code and a neuron index there, the TypeScript message here), so neither
//! calls the other; what they share is the logic, not the answer.
//!
//! # What stays host-side (NEAT-AI#3802)
//!
//! These checks depend on JavaScript object identity or on the host
//! filesystem, so they do **not** move here and stay in
//! `CreatureValidate.ts`:
//!
//! | TypeScript check | Why it stays |
//! |------------------|--------------|
//! | `neuron.creature !== creature` | object identity — a `CreatureExport` has no back-reference |
//! | `neuron.index` vs its loop position | the export carries no `index`; position *is* the index here |
//! | `neuron.validate()` | per-neuron host method over the live `Neuron` class |
//! | `debugWrite(creature)` diagnostics dump | writes `creatureValidate.json` to the host diagnostics dir |
//!
//! The Rust failure carries [`ValidationFailure::neuron_index`] /
//! [`ValidationFailure::synapse_index`] so the host can run its own identity
//! checks and its diagnostics dump against the same neuron or synapse the
//! shared rules stopped on.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::creature::{CreatureExport, parse_synapse_type};
use crate::synapse_type::SynapseType;
use crate::topology_invariants::{
    ConnectionIndex, IfFault, IfRoles, WiringFault, hidden_wiring_fault, if_neuron_fault,
};

/// Largest neuron id NEAT-AI accepts — `int32` max, mirroring
/// `MAX_NEURON_ID` in `src/architecture/CreatureValidate.ts`.
pub const MAX_NEURON_ID: i64 = 2_147_483_647;

/// The permitted [`ValidationFailure::reason`] values, one constant each.
///
/// **Source of truth: NEAT-AI `src/errors/ValidationError.ts`
/// (`ValidationErrorName`) and `src/errors/TopologyError.ts`
/// (`TopologyErrorReason`).** The strings are the union members verbatim so
/// NEAT-AI rehydrates the right error type from the wire without a translation
/// table; a name that drifts from those two files silently loses a failure
/// mode, so change them together.
///
/// [`VALIDATION_REASONS`] and [`TOPOLOGY_REASONS`] are the full lists, and
/// [`FailureClass::permits`] is what the failure constructors enforce.
pub mod reason {
    // src/errors/ValidationError.ts — ValidationErrorName
    /// Catch-all validation failure.
    pub const OTHER: &str = "OTHER";
    /// Neurons are not in `input, constant, hidden, output` order.
    pub const NEURON_ORDER: &str = "NEURON_ORDER";
    /// A neuron that must feed something has no outward synapse.
    pub const NO_OUTWARD_CONNECTIONS: &str = "NO_OUTWARD_CONNECTIONS";
    /// A hidden neuron has no inward synapse.
    pub const NO_INWARD_CONNECTIONS: &str = "NO_INWARD_CONNECTIONS";
    /// An `IF` neuron is missing one of its condition / positive / negative roles.
    pub const IF_CONDITIONS: &str = "IF_CONDITIONS";
    /// A backward synapse (`from > to`) where recursion is disallowed.
    pub const RECURSIVE_SYNAPSE: &str = "RECURSIVE_SYNAPSE";
    /// A self connection (`from == to`) in a forward-only creature.
    pub const SELF_CONNECTION: &str = "SELF_CONNECTION";
    /// Union member kept verbatim; `creatureValidate` reports a repeated
    /// `(from, to)` pair as [`INVALID_CONNECTION`] instead.
    pub const DUPLICATE_SYNAPSE: &str = "DUPLICATE_SYNAPSE";
    /// A memetic bias or weight does not resolve to a neuron or synapse.
    pub const MEMETIC: &str = "MEMETIC";

    // src/errors/TopologyError.ts — TopologyErrorReason
    /// A neuron declares a type outside `input | constant | hidden | output`.
    pub const INVALID_NEURON_TYPE: &str = "INVALID_NEURON_TYPE";
    /// A neuron's bias is not a finite number.
    pub const INVALID_NEURON_BIAS: &str = "INVALID_NEURON_BIAS";
    /// A neuron carries a squash it may not have.
    pub const INVALID_SQUASH: &str = "INVALID_SQUASH";
    /// A synapse weight is not a finite number.
    pub const INVALID_SYNAPSE_WEIGHT: &str = "INVALID_SYNAPSE_WEIGHT";
    /// A synapse endpoint does not resolve to a neuron.
    pub const INVALID_SYNAPSE_REFERENCE: &str = "INVALID_SYNAPSE_REFERENCE";
    /// A neuron that requires a squash has none.
    pub const MISSING_SQUASH: &str = "MISSING_SQUASH";
    /// A synapse is wired somewhere it may not be — including a duplicate pair.
    pub const INVALID_CONNECTION: &str = "INVALID_CONNECTION";
    /// The creature's own bookkeeping disagrees with its neurons.
    pub const INVALID_STATE: &str = "INVALID_STATE";
    /// Two neurons share a UUID.
    pub const DUPLICATE_UUID: &str = "DUPLICATE_UUID";
    /// A referenced neuron is absent.
    pub const MISSING_NEURON: &str = "MISSING_NEURON";
    /// A neuron has no UUID.
    pub const MISSING_NEURON_UUID: &str = "MISSING_NEURON_UUID";
    /// Synapses are not sorted by `(from, to)`.
    pub const SORT_FAILURE: &str = "SORT_FAILURE";
    /// Too many errors to keep reporting.
    pub const EXCESSIVE_ERRORS: &str = "EXCESSIVE_ERRORS";
}

/// Every `ValidationErrorName`, in `ValidationError.ts` declaration order.
pub const VALIDATION_REASONS: [&str; 9] = [
    reason::OTHER,
    reason::NEURON_ORDER,
    reason::NO_OUTWARD_CONNECTIONS,
    reason::NO_INWARD_CONNECTIONS,
    reason::IF_CONDITIONS,
    reason::RECURSIVE_SYNAPSE,
    reason::SELF_CONNECTION,
    reason::DUPLICATE_SYNAPSE,
    reason::MEMETIC,
];

/// Every `TopologyErrorReason`, in `TopologyError.ts` declaration order.
pub const TOPOLOGY_REASONS: [&str; 13] = [
    reason::INVALID_NEURON_TYPE,
    reason::INVALID_NEURON_BIAS,
    reason::INVALID_SQUASH,
    reason::INVALID_SYNAPSE_WEIGHT,
    reason::INVALID_SYNAPSE_REFERENCE,
    reason::MISSING_SQUASH,
    reason::INVALID_CONNECTION,
    reason::INVALID_STATE,
    reason::DUPLICATE_UUID,
    reason::MISSING_NEURON,
    reason::MISSING_NEURON_UUID,
    reason::SORT_FAILURE,
    reason::EXCESSIVE_ERRORS,
];

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn all_distinct(list: &[&str]) -> bool {
    let mut i = 0;
    while i < list.len() {
        let mut j = i + 1;
        while j < list.len() {
            if str_eq(list[i], list[j]) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

const fn disjoint(left: &[&str], right: &[&str]) -> bool {
    let mut i = 0;
    while i < left.len() {
        let mut j = 0;
        while j < right.len() {
            if str_eq(left[i], right[j]) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// A reason must name exactly one class, or NEAT-AI cannot rehydrate the right
// error type from it. Checked at compile time so a copy-paste into the wrong
// list never builds.
const _: () = assert!(all_distinct(&VALIDATION_REASONS));
const _: () = assert!(all_distinct(&TOPOLOGY_REASONS));
const _: () = assert!(disjoint(&VALIDATION_REASONS, &TOPOLOGY_REASONS));

/// Which TypeScript error class a failure rehydrates as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureClass {
    /// NEAT-AI `TopologyError` — reasons from [`TOPOLOGY_REASONS`].
    Topology,
    /// NEAT-AI `ValidationError` — reasons from [`VALIDATION_REASONS`].
    Validation,
}

impl FailureClass {
    /// The TypeScript class name (`Error.name`) this class maps to.
    pub const fn as_str(self) -> &'static str {
        match self {
            FailureClass::Topology => "TopologyError",
            FailureClass::Validation => "ValidationError",
        }
    }

    /// The reasons this class may carry.
    pub const fn permitted_reasons(self) -> &'static [&'static str] {
        match self {
            FailureClass::Topology => &TOPOLOGY_REASONS,
            FailureClass::Validation => &VALIDATION_REASONS,
        }
    }

    /// Whether `reason` belongs to this class's union.
    pub fn permits(self, reason: &str) -> bool {
        self.permitted_reasons().contains(&reason)
    }
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What [`creature_validate`] returns for a creature that broke no rule —
/// the `stats` object `creatureValidate` returns today.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationStats {
    /// Input neurons counted (always the declared `input`, see the input
    /// format section: input neurons are implicit in the export form).
    pub input: u32,
    /// Constant neurons counted.
    pub constant: u32,
    /// Hidden neurons counted.
    pub hidden: u32,
    /// Output neurons counted.
    pub output: u32,
    /// Synapses counted.
    pub connections: u32,
}

impl ValidationStats {
    /// Total neurons counted — the TypeScript `creature.neurons.length`.
    pub const fn neurons(&self) -> u32 {
        self.input + self.constant + self.hidden + self.output
    }
}

/// The first violated rule: which class of TypeScript error it raises, the
/// verbatim `reason`, the human-readable message and where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationFailure {
    /// Whether NEAT-AI rehydrates this as a `TopologyError` or a `ValidationError`.
    pub class: FailureClass,
    /// One of [`FailureClass::permitted_reasons`], verbatim from the
    /// TypeScript union — see [`reason`].
    pub reason: &'static str,
    /// Human-readable text reproducing the TypeScript message.
    pub message: String,
    /// Index of the neuron the rule stopped on, if it was a neuron rule.
    pub neuron_index: Option<u32>,
    /// Index of the synapse the rule stopped on, if it was a synapse rule.
    pub synapse_index: Option<u32>,
}

impl ValidationFailure {
    /// Build a failure, rejecting a `reason` outside `class`'s union.
    ///
    /// # Panics
    ///
    /// Panics when `reason` is not one of [`FailureClass::permitted_reasons`].
    /// Every caller passes a [`reason`] constant, so this can only fire on a
    /// programming error — and it fires loudly rather than sending NEAT-AI a
    /// string it cannot rehydrate into a typed error.
    pub fn new(class: FailureClass, reason: &'static str, message: impl Into<String>) -> Self {
        assert!(
            class.permits(reason),
            "{reason:?} is not a {} reason",
            class.as_str()
        );
        Self {
            class,
            reason,
            message: message.into(),
            neuron_index: None,
            synapse_index: None,
        }
    }

    /// Build a [`FailureClass::Validation`] failure. See [`Self::new`].
    pub fn validation(reason: &'static str, message: impl Into<String>) -> Self {
        Self::new(FailureClass::Validation, reason, message)
    }

    /// Build a [`FailureClass::Topology`] failure. See [`Self::new`].
    pub fn topology(reason: &'static str, message: impl Into<String>) -> Self {
        Self::new(FailureClass::Topology, reason, message)
    }

    /// Record the neuron index the rule stopped on.
    #[must_use]
    pub fn at_neuron(mut self, index: u32) -> Self {
        self.neuron_index = Some(index);
        self
    }

    /// Record the synapse index the rule stopped on.
    #[must_use]
    pub fn at_synapse(mut self, index: u32) -> Self {
        self.synapse_index = Some(index);
        self
    }
}

impl fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({}): {}", self.class, self.reason, self.message)
    }
}

impl std::error::Error for ValidationFailure {}

/// Options the caller may pin, mirroring the TypeScript options bag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidateOptions {
    /// Expected total neuron count, `input + neurons.len()`.
    ///
    /// Read through [`Self::expected_neurons`]: TypeScript tests this for
    /// *truthiness*, so `Some(0)` is not checked.
    pub neurons: Option<usize>,
    /// Expected synapse count, read through [`Self::expected_connections`].
    /// TypeScript tests this with `Number.isInteger`, so `Some(0)` **is**
    /// checked.
    pub connections: Option<usize>,
    /// `None` = allow recursive synapses (NEAT-AI's `undefined`).
    /// `Some(false)` rejects them; `Some(true)` allows them explicitly.
    pub feedback_loop: Option<bool>,
    /// Production feed-forward validation: forces [`Self::feedback_loop`] to
    /// `Some(false)` and rejects self connections.
    pub forward_only: bool,
}

impl ValidateOptions {
    /// `const feedbackLoop = forwardOnly ? false : options?.feedbackLoop;`
    pub const fn resolved_feedback_loop(&self) -> Option<bool> {
        if self.forward_only {
            Some(false)
        } else {
            self.feedback_loop
        }
    }

    /// Whether a backward synapse (`from > to`) is a failure — true only when
    /// [`Self::resolved_feedback_loop`] is an explicit `Some(false)`.
    pub const fn rejects_recursive_synapses(&self) -> bool {
        matches!(self.resolved_feedback_loop(), Some(false))
    }

    /// The neuron count to check, or `None` when the rule is skipped.
    ///
    /// `if (options && options.neurons)` is a truthiness test in TypeScript,
    /// so `Some(0)` skips the rule rather than demanding an empty creature.
    pub const fn expected_neurons(&self) -> Option<usize> {
        match self.neurons {
            Some(0) | None => None,
            some => some,
        }
    }

    /// The synapse count to check, or `None` when the rule is skipped.
    ///
    /// `Number.isInteger(options.connections)` accepts `0`, so a caller can
    /// demand a creature with no synapses at all.
    pub const fn expected_connections(&self) -> Option<usize> {
        self.connections
    }
}

/// Validate a creature against the rules in the module documentation.
///
/// First failure wins: the first violated rule is returned and no later rule
/// runs, matching the TypeScript `throw`.
///
/// # Half ported (Issue #560)
///
/// Rules 1–22, the neuron half, run and report exactly as the TypeScript does.
/// Rules 23–31 — the synapse walk, the forward-only checks and the memetic
/// block — are Issue #561, so a creature that breaks none of the neuron rules
/// still comes back as a [`FailureClass::Validation`] / [`reason::OTHER`]
/// failure saying which half is missing, rather than as an `Ok` this build
/// cannot stand behind. A consumer wiring the entry point up early fails
/// loudly instead of certifying the invalid creature this work exists to catch.
///
/// # Errors
///
/// Returns the [`ValidationFailure`] for the first violated rule.
pub fn creature_validate(
    creature: &CreatureExport,
    options: &ValidateOptions,
) -> Result<ValidationStats, ValidationFailure> {
    let _stats = validate_neuron_rules(creature, options)?;

    Err(ValidationFailure::validation(
        reason::OTHER,
        "creature_validate synapse, forward-only and memetic rule bodies are not \
         ported yet (Issue #561): no creature can be certified valid by this build",
    ))
}

/// Rules 1–22 — everything the TypeScript evaluates before it reaches
/// `creature.synapses.forEach` (Issue #560).
///
/// `stats.connections` stays `0`: the synapse walk that fills it is Issue #561.
fn validate_neuron_rules(
    creature: &CreatureExport,
    options: &ValidateOptions,
) -> Result<ValidationStats, ValidationFailure> {
    let total_neurons = creature.input + creature.neurons.len();

    // Rule 1 — `if (options && options.neurons)`, a truthiness test.
    if let Some(expected) = options.expected_neurons()
        && total_neurons != expected
    {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!("Neurons length: {total_neurons} expected: {expected}"),
        ));
    }

    // Rules 2 and 3 — a creature with no observations or no targets is not a
    // creature. `input` / `output` are `usize` here, so the TypeScript
    // `Number.isInteger` half of each test is unrepresentable.
    if creature.input < 1 {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!(
                "Must have at least one input neurons was: {}",
                creature.input
            ),
        ));
    }
    if creature.output < 1 {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!(
                "Must have at least one output neurons was: {}",
                creature.output
            ),
        ));
    }

    let views = neuron_views(creature);
    let (from, to, synapse_types) = resolve_synapse_endpoints(creature)?;
    let connections = ConnectionIndex::build(&from, &to, total_neurons);

    walk_neurons(
        &views,
        creature.input,
        creature.output,
        &connections,
        &synapse_types,
    )
}

/// What the walk needs to know about one neuron, inputs included.
///
/// The export form lists only non-input neurons and carries no indices, so the
/// walk runs over this derived view rather than over `creature.neurons`
/// directly — see the *Input format* section of the module documentation.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NeuronView<'a> {
    /// Runtime integer id, derived as NEAT-AI's loader derives it
    /// ([`derived_neuron_id`]). `None` only when no id can be derived at all.
    id: Option<i64>,
    /// The four types the rules branch on, plus [`NeuronKind::Invalid`].
    kind: NeuronKind,
    /// The `type` string as declared, reproduced verbatim in messages.
    declared_type: &'a str,
    /// The wire UUID; `None` for the implicit input neurons, which are labelled
    /// `input-N` from their index.
    uuid: Option<&'a str>,
    /// `None` is TypeScript's `undefined` bias — unreachable from JSON, where
    /// `bias` is a required number.
    bias: Option<f64>,
    /// Activation function name, absent for constants and implicit inputs.
    squash: Option<&'a str>,
}

/// A neuron type as the rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NeuronKind {
    /// One of the implicit `0..input` neurons.
    Input,
    /// A `constant` neuron.
    Constant,
    /// A `hidden` neuron.
    Hidden,
    /// An `output` neuron.
    Output,
    /// Anything else — including an entry that declares `type: "input"`, which
    /// the export form does not permit (input neurons are implicit).
    Invalid,
}

impl NeuronKind {
    /// Read the `type` of an exported (non-input) neuron.
    fn from_declared(declared_type: &str) -> Self {
        match declared_type {
            "constant" => NeuronKind::Constant,
            "hidden" => NeuronKind::Hidden,
            "output" => NeuronKind::Output,
            _ => NeuronKind::Invalid,
        }
    }
}

impl NeuronView<'_> {
    /// `neuron.ID()` — `String(this.id)`, so a missing id reads `undefined`.
    fn id_text(&self) -> String {
        match self.id {
            Some(id) => id.to_string(),
            None => "undefined".to_string(),
        }
    }

    /// The UUID when it is a usable label — TypeScript tests `neuron.uuid` for
    /// truthiness, so an empty string is no label at all.
    fn stable_uuid(&self) -> Option<&str> {
        self.uuid.filter(|uuid| !uuid.is_empty())
    }

    /// Rust's `neuronWireLabelForDiagnostics(neuron, index)`.
    ///
    /// NEAT-AI's error-message tests assert on this text, so it reproduces
    /// `src/neuron/NeuronSerialization.ts` branch for branch: `input-N` for an
    /// input, `output-N` for an output carrying its negative id, otherwise the
    /// wire UUID, and the two diagnostic fallbacks when there is no UUID.
    fn wire_label(&self, index: usize) -> String {
        if self.kind == NeuronKind::Input {
            return format!("input-{index}");
        }
        let negative_id = self.id.filter(|id| *id < 0);
        if let (NeuronKind::Output, Some(id)) = (self.kind, negative_id) {
            return format!("output-{}", -(id + 1));
        }
        if let Some(uuid) = self.stable_uuid() {
            return uuid.to_string();
        }
        match negative_id {
            Some(id) => format!("non-output-negative-id-{id}@index-{index}"),
            None => format!("missing-uuid@index-{index}"),
        }
    }
}

/// Render a number the way JavaScript's template interpolation does.
///
/// Only the non-finite cases are reachable from the rules that call it (a
/// finite bias breaks none of them), but an integral value must still print as
/// `3` rather than Rust's `3`-with-no-fraction guarantee slipping to `3.0`.
fn number_text(value: Option<f64>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(value) if value.is_nan() => "NaN".to_string(),
        Some(value) if value.is_infinite() => {
            if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
        }
        Some(value) if value.fract() == 0.0 && value.abs() < 1e21 => format!("{}", value as i128),
        Some(value) => format!("{value}"),
    }
}

/// The runtime id NEAT-AI's loader gives a neuron that exports none.
///
/// `deterministicIdFromUuid` in `src/neuron/NeuronSerialization.ts`: the
/// 32-bit string hash, folded into `[1_000_000, 2_000_000_000)` so it can
/// collide with neither an input id (`0..input`) nor an output id (negative).
/// `None` when there is no UUID to hash — TypeScript falls back to a
/// process-global counter there, which no other process can reproduce, so the
/// neuron reaches the walk with no id at all and rule 4 reports it.
fn derived_neuron_id(uuid: &str) -> Option<i64> {
    if uuid.is_empty() {
        return None;
    }
    let mut hash: i32 = 0;
    for unit in uuid.encode_utf16() {
        // `((hash << 5) - hash + chr) | 0` — int32 arithmetic throughout.
        hash = hash
            .wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(i32::from(unit));
    }
    Some(1_000_000 + i64::from((hash % 1_999_000_000).unsigned_abs()))
}

/// Derive the neuron the rules walk over, index by index.
///
/// Indices follow [`crate::creature::compile_creature`]: `0..input` are the
/// implicit input neurons, then `input + i` is `creature.neurons[i]`. Ids
/// follow NEAT-AI's loader (`src/creature/CreatureSerialization.ts`), which
/// assigns them *before* `creatureValidate` ever runs: an input is its own
/// index, an output is `-(outputIndex + 1)` whatever the file says, and
/// everything else keeps its exported id or takes [`derived_neuron_id`].
fn neuron_views(creature: &CreatureExport) -> Vec<NeuronView<'_>> {
    let mut views = Vec::with_capacity(creature.input + creature.neurons.len());

    for index in 0..creature.input {
        views.push(NeuronView {
            id: Some(index as i64),
            kind: NeuronKind::Input,
            declared_type: "input",
            uuid: None,
            bias: Some(0.0),
            squash: None,
        });
    }

    let mut output_index: i64 = 0;
    for neuron in &creature.neurons {
        let kind = NeuronKind::from_declared(&neuron.neuron_type);
        let id = if kind == NeuronKind::Output {
            let id = -(output_index + 1);
            output_index += 1;
            Some(id)
        } else {
            neuron.id.or_else(|| derived_neuron_id(&neuron.uuid))
        };

        views.push(NeuronView {
            id,
            kind,
            declared_type: &neuron.neuron_type,
            uuid: Some(neuron.uuid.as_str()),
            bias: Some(neuron.bias),
            squash: neuron.squash.as_deref(),
        });
    }

    views
}

/// Resolve every synapse's `fromUUID` / `toUUID` to the neuron indices the
/// rules use, keeping the declared synapse roles alongside.
///
/// The in-memory TypeScript creature cannot express a dangling endpoint — its
/// synapses already hold indices — so this is the one failure with no
/// TypeScript counterpart, reported as `Topology` / `INVALID_SYNAPSE_REFERENCE`
/// (module documentation, *Input format*). It runs after rules 1–3 and before
/// the walk, because the walk needs the connection index it feeds.
#[allow(clippy::type_complexity)]
fn resolve_synapse_endpoints(
    creature: &CreatureExport,
) -> Result<(Vec<u32>, Vec<u32>, Vec<SynapseType>), ValidationFailure> {
    let mut uuid_to_index: HashMap<&str, u32> = HashMap::with_capacity(creature.neurons.len());
    for (i, neuron) in creature.neurons.iter().enumerate() {
        uuid_to_index.insert(neuron.uuid.as_str(), (creature.input + i) as u32);
    }

    let resolve = |uuid: &str| -> Option<u32> {
        if let Some(index) = uuid_to_index.get(uuid) {
            return Some(*index);
        }
        // Implicit input neurons are wired as `input-N`, never listed.
        let index: usize = uuid.strip_prefix("input-")?.parse().ok()?;
        (index < creature.input).then_some(index as u32)
    };

    let mut from = Vec::with_capacity(creature.synapses.len());
    let mut to = Vec::with_capacity(creature.synapses.len());
    let mut types = Vec::with_capacity(creature.synapses.len());

    for (index, synapse) in creature.synapses.iter().enumerate() {
        let dangling = |endpoint: &str, uuid: &str| {
            ValidationFailure::topology(
                reason::INVALID_SYNAPSE_REFERENCE,
                format!("{index}) synapse {endpoint} {uuid} does not name a neuron"),
            )
            .at_synapse(index as u32)
        };

        from.push(resolve(&synapse.from_uuid).ok_or_else(|| dangling("from", &synapse.from_uuid))?);
        to.push(resolve(&synapse.to_uuid).ok_or_else(|| dangling("to", &synapse.to_uuid))?);
        types.push(parse_synapse_type(synapse.synapse_type.as_deref()));
    }

    Ok((from, to, types))
}

/// Rules 4–22: the per-neuron walk and the two counts checked after it.
///
/// Evaluation order is the contract (module documentation), so this reads as
/// one pass in the same order as the TypeScript `forEach` body — first failure
/// wins.
fn walk_neurons(
    views: &[NeuronView<'_>],
    input: usize,
    output: usize,
    connections: &ConnectionIndex,
    synapse_types: &[SynapseType],
) -> Result<ValidationStats, ValidationFailure> {
    let mut stats = ValidationStats::default();
    let mut neuron_ids: HashSet<i64> = HashSet::with_capacity(views.len());
    let mut outputs_seen = 0usize;
    let mut computational_seen_hidden = false;

    for (index, neuron) in views.iter().enumerate() {
        let at = |failure: ValidationFailure| failure.at_neuron(index as u32);

        // Rule 4 — every neuron has an id.
        let Some(id) = neuron.id else {
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{}) no id", neuron.id_text()),
            )));
        };

        // Rule 5 — ids fit int32. Negative ids are legal: an output neuron's
        // id is `-(outputIndex + 1)` (NEAT-AI #1958).
        if id > MAX_NEURON_ID {
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{id}) invalid neuron id: {id}"),
            )));
        }

        // Rule 6 — ids are unique.
        if !neuron_ids.insert(id) {
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{id}) duplicate neuron id: {id}"),
            )));
        }

        if neuron.kind == NeuronKind::Input {
            // Rule 7 — an input neuron is identified by its own index.
            if id != index as i64 {
                return Err(at(ValidationFailure::validation(
                    reason::OTHER,
                    format!("{id}) invalid input neuron id: {id}"),
                )));
            }
        } else if !neuron.bias.is_some_and(f64::is_finite) {
            // Rule 8 — every other neuron carries a finite bias.
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{id}) invalid bias: {}", number_text(neuron.bias)),
            )));
        }

        // Rule 9 — outputs come last.
        if neuron.kind == NeuronKind::Output {
            outputs_seen += 1;
        } else if outputs_seen > 0 {
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{id}) type {} after output neuron", neuron.declared_type),
            )));
        }

        // Rule 10 — inputs come first.
        if neuron.kind == NeuronKind::Input && index > input {
            return Err(at(ValidationFailure::validation(
                reason::OTHER,
                format!("{id}) input neuron after the maximum input neurons"),
            )));
        }

        // Rule 11 — inside the computational slice, constants precede hiddens.
        if index >= input && index < views.len() - output {
            if neuron.kind == NeuronKind::Constant && computational_seen_hidden {
                return Err(at(ValidationFailure::validation(
                    reason::NEURON_ORDER,
                    format!(
                        "{}) type constant after hidden neuron; required order is input, constant, hidden, output",
                        neuron.wire_label(index)
                    ),
                )));
            }
            if neuron.kind == NeuronKind::Hidden {
                computational_seen_hidden = true;
            }
        }

        // Rule 12 — an `IF` neuron decides between two branches, so it needs a
        // condition, a positive and a negative input.
        if neuron.squash == Some("IF") && index > 2 {
            let inward = connections.inward_synapses(index);
            let roles = IfRoles::tally(
                inward
                    .iter()
                    .filter_map(|synapse| synapse_types.get(*synapse as usize).copied()),
            );
            if let Some(fault) = if_neuron_fault(inward.len(), roles) {
                let message = match fault {
                    IfFault::TooFewInward { found } => {
                        format!("{id}) 'IF' should have at least 3 inward connections was: {found}")
                    }
                    IfFault::MissingCondition => format!("{id}) 'IF' should have a condition(s)"),
                    IfFault::MissingPositive => {
                        format!("{id}) 'IF' should have a positive connection(s)")
                    }
                    IfFault::MissingNegative => {
                        format!("{id}) 'IF' should have a negative connection(s)")
                    }
                };
                return Err(at(ValidationFailure::validation(
                    reason::IF_CONDITIONS,
                    message,
                )));
            }
        }

        let inward = connections.inward_count(index);
        let outward = connections.outward_count(index);

        match neuron.kind {
            NeuronKind::Input => {
                stats.input += 1;
                // Rule 13 — nothing feeds an observation.
                if inward > 0 {
                    return Err(at(ValidationFailure::topology(
                        reason::INVALID_CONNECTION,
                        format!("'input' neuron {id} has inward connections: {inward}"),
                    )));
                }
            }
            NeuronKind::Constant => {
                stats.constant += 1;
                // Rule 14 — a constant is a source, never a sink.
                if inward > 0 {
                    return Err(at(ValidationFailure::topology(
                        reason::INVALID_CONNECTION,
                        format!(
                            "'{}' neuron {id} has inward connections: {inward}",
                            neuron.declared_type
                        ),
                    )));
                }
                // Rule 15 — a constant emits its bias; a squash would change it.
                if let Some(squash) = neuron.squash {
                    return Err(at(ValidationFailure::topology(
                        reason::INVALID_SQUASH,
                        format!("Node {id} '{}' has squash: {squash}", neuron.declared_type),
                    )));
                }
                // Rule 16 — a constant nothing reads is dead weight.
                if outward == 0 {
                    return Err(at(ValidationFailure::validation(
                        reason::NO_OUTWARD_CONNECTIONS,
                        format!(
                            "constants neuron {} has no outward connections",
                            neuron.wire_label(index)
                        ),
                    )));
                }
            }
            NeuronKind::Hidden => {
                stats.hidden += 1;
                // Rules 17 and 18 — a hidden neuron is wired in and out.
                if let Some(fault) = hidden_wiring_fault(inward, outward) {
                    let direction = match fault {
                        WiringFault::NoInward => "inward",
                        WiringFault::NoOutward => "outward",
                    };
                    let wiring_reason = match fault {
                        WiringFault::NoInward => reason::NO_INWARD_CONNECTIONS,
                        WiringFault::NoOutward => reason::NO_OUTWARD_CONNECTIONS,
                    };
                    return Err(at(ValidationFailure::validation(
                        wiring_reason,
                        format!(
                            "hidden neuron {} has no {direction} connections",
                            neuron.wire_label(index)
                        ),
                    )));
                }
                // Rule 19 — kept for the port, unreachable behind rule 8.
                if let Some(failure) = hidden_bias_failure(id, neuron.bias) {
                    return Err(at(failure));
                }
            }
            NeuronKind::Output => {
                stats.output += 1;
            }
            // Rule 20 — the type is one of the four, and `input` is not one of
            // them here: input neurons are implicit, so an entry claiming to be
            // one is as invalid as a typo.
            NeuronKind::Invalid => {
                return Err(at(ValidationFailure::topology(
                    reason::INVALID_NEURON_TYPE,
                    format!("{id}) Invalid type: {}", neuron.declared_type),
                )));
            }
        }
    }

    // Rule 21 — the counted inputs are the declared width.
    if stats.input as usize != input {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!("Expected {input} input neurons found: {}", stats.input),
        ));
    }

    // Rule 22 — and so are the counted outputs.
    if stats.output as usize != output {
        return Err(ValidationFailure::topology(
            reason::INVALID_STATE,
            format!("Expected {output} output neurons found: {}", stats.output),
        ));
    }

    Ok(stats)
}

/// Rule 19 — a hidden neuron's bias must be present and finite.
///
/// Both branches are **unreachable** in NEAT-AI and here: rule 8 rejects a
/// missing or non-finite bias on every non-input neuron before the type switch
/// is reached. The rule is ported anyway so the two stacks stay line-for-line
/// comparable, and it is a function so the messages stay under test rather than
/// being dead text nothing can exercise.
fn hidden_bias_failure(id: i64, bias: Option<f64>) -> Option<ValidationFailure> {
    if bias.is_none() {
        return Some(ValidationFailure::topology(
            reason::INVALID_STATE,
            format!("hidden neuron {id} should have a bias was: undefined"),
        ));
    }
    if !bias.is_some_and(f64::is_finite) {
        return Some(ValidationFailure::topology(
            reason::INVALID_STATE,
            format!(
                "{id}) hidden neuron should have a finite bias was: {}",
                number_text(bias)
            ),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{NeuronExport, SynapseExport};
    use crate::decision_tree::{depth2_tree_creature, stump_creature};

    // -----------------------------------------------------------------------
    // Fixtures — every id that reaches a message is pinned, so the assertions
    // are on the TypeScript text rather than on a derived hash.
    // -----------------------------------------------------------------------

    fn neuron(neuron_type: &str, uuid: &str, id: Option<i64>, bias: f64) -> NeuronExport {
        NeuronExport {
            id,
            neuron_type: neuron_type.to_string(),
            uuid: uuid.to_string(),
            bias,
            squash: None,
        }
    }

    fn squashed(neuron_type: &str, uuid: &str, id: Option<i64>, squash: &str) -> NeuronExport {
        NeuronExport {
            squash: Some(squash.to_string()),
            ..neuron(neuron_type, uuid, id, 0.0)
        }
    }

    fn edge(from: &str, to: &str) -> SynapseExport {
        SynapseExport {
            from_uuid: from.to_string(),
            to_uuid: to.to_string(),
            weight: 1.0,
            synapse_type: None,
        }
    }

    fn typed_edge(from: &str, to: &str, synapse_type: &str) -> SynapseExport {
        SynapseExport {
            synapse_type: Some(synapse_type.to_string()),
            ..edge(from, to)
        }
    }

    fn creature(
        input: usize,
        output: usize,
        neurons: Vec<NeuronExport>,
        synapses: Vec<SynapseExport>,
    ) -> CreatureExport {
        CreatureExport {
            input,
            output,
            neurons,
            synapses,
            semantic_version: None,
            forward_only: false,
            memetic: None,
        }
    }

    fn rejects(creature: &CreatureExport) -> ValidationFailure {
        validate_neuron_rules(creature, &ValidateOptions::default())
            .expect_err("a neuron rule should have rejected this creature")
    }

    fn accepts(creature: &CreatureExport) -> ValidationStats {
        validate_neuron_rules(creature, &ValidateOptions::default())
            .expect("no neuron rule is broken by this creature")
    }

    #[track_caller]
    fn assert_failure(
        failure: &ValidationFailure,
        class: FailureClass,
        reason: &str,
        message: &str,
    ) {
        assert_eq!(
            (failure.class, failure.reason, failure.message.as_str()),
            (class, reason, message)
        );
    }

    // -----------------------------------------------------------------------
    // The canonical valid creatures.
    // -----------------------------------------------------------------------

    /// The decision-tree fixtures break no neuron rule, and the walk counts
    /// every type it saw. `connections` is left to Issue #561.
    #[test]
    fn the_decision_tree_fixtures_pass_every_neuron_rule() {
        assert_eq!(
            accepts(&stump_creature()),
            ValidationStats {
                input: 1,
                constant: 3,
                hidden: 0,
                output: 1,
                connections: 0,
            }
        );
        assert_eq!(
            accepts(&depth2_tree_creature()),
            ValidationStats {
                input: 2,
                constant: 3,
                hidden: 2,
                output: 1,
                connections: 0,
            }
        );
    }

    /// A creature that breaks a neuron rule is reported by the entry point
    /// itself, not swallowed by the not-yet-ported synapse half.
    #[test]
    fn the_entry_point_reports_a_neuron_rule_before_the_unported_half() {
        let mut broken = stump_creature();
        broken.neurons[0].squash = Some("TANH".to_string());

        let failure = creature_validate(&broken, &ValidateOptions::default())
            .expect_err("a constant may not carry a squash");

        assert_eq!(failure.reason, reason::INVALID_SQUASH);
        assert!(
            !failure.message.contains("not ported"),
            "the neuron rules ran: {}",
            failure.message
        );
    }

    // -----------------------------------------------------------------------
    // Rules 1-3 — pre-walk.
    // -----------------------------------------------------------------------

    /// Rule 1: the expected count is the *total*, inputs included.
    #[test]
    fn an_expected_neuron_count_that_misses_the_total_is_reported_with_both_counts() {
        let stump = stump_creature();
        let options = |neurons: usize| ValidateOptions {
            neurons: Some(neurons),
            ..ValidateOptions::default()
        };

        let failure = validate_neuron_rules(&stump, &options(9)).expect_err("5 neurons, not 9");
        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "Neurons length: 5 expected: 9",
        );

        assert!(
            validate_neuron_rules(&stump, &options(5)).is_ok(),
            "1 input + 4 exported neurons"
        );
    }

    /// Rule 2: a creature with no observations is not a creature.
    #[test]
    fn a_creature_with_no_input_neurons_is_rejected() {
        let widthless = creature(0, 1, vec![neuron("output", "out-0", None, 0.0)], vec![]);

        assert_failure(
            &rejects(&widthless),
            FailureClass::Validation,
            reason::OTHER,
            "Must have at least one input neurons was: 0",
        );
    }

    /// Rule 3: nor is one with no targets.
    #[test]
    fn a_creature_with_no_output_neurons_is_rejected() {
        let widthless = creature(1, 0, vec![neuron("output", "out-0", None, 0.0)], vec![]);

        assert_failure(
            &rejects(&widthless),
            FailureClass::Validation,
            reason::OTHER,
            "Must have at least one output neurons was: 0",
        );
    }

    // -----------------------------------------------------------------------
    // Rules 4-6 — neuron ids.
    // -----------------------------------------------------------------------

    /// Rule 4: a neuron with neither an exported id nor a UUID to derive one
    /// from has no identity at all. `neuron.ID()` renders that as `undefined`.
    #[test]
    fn a_neuron_with_no_id_and_no_uuid_is_rejected() {
        let anonymous = creature(
            1,
            1,
            vec![
                neuron("constant", "", None, 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("input-0", "out-0")],
        );

        let failure = rejects(&anonymous);
        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "undefined) no id",
        );
        assert_eq!(failure.neuron_index, Some(1));
    }

    /// A creature that exports no ids at all still validates: NEAT-AI's loader
    /// derives them from the UUIDs before `creatureValidate` runs, and so does
    /// this port.
    #[test]
    fn ids_are_derived_from_the_uuid_when_the_export_carries_none() {
        assert!(stump_creature().neurons.iter().all(|n| n.id.is_none()));

        let derived = derived_neuron_id("uuid-a").expect("a UUID always derives an id");
        assert!(
            (1_000_000..2_000_000_000).contains(&derived),
            "derived ids avoid the input (0..) and output (negative) ranges, was {derived}"
        );
        assert_eq!(
            derived,
            derived_neuron_id("uuid-a").unwrap(),
            "the same UUID always derives the same id"
        );
        assert_ne!(derived, derived_neuron_id("uuid-b").unwrap());
        assert_eq!(derived_neuron_id(""), None, "nothing to hash");
    }

    /// Rule 5: ids fit int32.
    #[test]
    fn an_id_above_int32_max_is_rejected_and_the_boundary_is_accepted() {
        let over = creature(
            1,
            1,
            vec![
                neuron("constant", "c-0", Some(MAX_NEURON_ID + 1), 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("c-0", "out-0")],
        );

        assert_failure(
            &rejects(&over),
            FailureClass::Validation,
            reason::OTHER,
            "2147483648) invalid neuron id: 2147483648",
        );

        let mut at_boundary = over.clone();
        at_boundary.neurons[0].id = Some(MAX_NEURON_ID);
        assert_eq!(accepts(&at_boundary).constant, 1);
    }

    /// Rule 6: two neurons may not share an id.
    #[test]
    fn a_duplicate_neuron_id_is_rejected() {
        let duplicated = creature(
            1,
            1,
            vec![
                neuron("constant", "c-0", Some(7), 1.0),
                neuron("constant", "c-1", Some(7), 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("c-0", "out-0"), edge("c-1", "out-0")],
        );

        let failure = rejects(&duplicated);
        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "7) duplicate neuron id: 7",
        );
        assert_eq!(failure.neuron_index, Some(2), "the second one to claim it");
    }

    /// Rule 6 (negative): distinct ids, including the negative output id, pass.
    #[test]
    fn distinct_ids_including_the_negative_output_id_are_accepted() {
        let distinct = creature(
            1,
            1,
            vec![
                neuron("constant", "c-0", Some(7), 1.0),
                neuron("constant", "c-1", Some(8), 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("c-0", "out-0"), edge("c-1", "out-0")],
        );

        assert_eq!(accepts(&distinct).constant, 2);
        assert_eq!(
            neuron_views(&distinct)[3].id,
            Some(-1),
            "output ids are -(outputIndex + 1) whatever the file says"
        );
    }

    // -----------------------------------------------------------------------
    // Rules 7 and 10 — input neuron placement. Both hold by construction from
    // a `CreatureExport` (the walk derives the input neurons itself), so they
    // are exercised against the walk directly rather than through a creature
    // that cannot be written.
    // -----------------------------------------------------------------------

    fn walk(
        views: &[NeuronView<'_>],
        input: usize,
        output: usize,
    ) -> Result<ValidationStats, ValidationFailure> {
        walk_neurons(
            views,
            input,
            output,
            &ConnectionIndex::build(&[], &[], views.len()),
            &[],
        )
    }

    fn input_view(id: i64) -> NeuronView<'static> {
        NeuronView {
            id: Some(id),
            kind: NeuronKind::Input,
            declared_type: "input",
            uuid: None,
            bias: Some(0.0),
            squash: None,
        }
    }

    fn output_view(id: i64) -> NeuronView<'static> {
        NeuronView {
            id: Some(id),
            kind: NeuronKind::Output,
            declared_type: "output",
            uuid: Some("out"),
            bias: Some(0.0),
            squash: None,
        }
    }

    /// Rule 7: an input neuron is identified by its own index.
    #[test]
    fn an_input_neuron_whose_id_is_not_its_index_is_rejected() {
        let failure = walk(&[input_view(5)], 1, 0).expect_err("id 5 at index 0");

        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "5) invalid input neuron id: 5",
        );
        assert!(walk(&[input_view(0)], 1, 0).is_ok(), "id 0 at index 0");
    }

    /// Rule 10: an input neuron past the declared width is rejected — after
    /// the index-matching id check, which a stray input at index 1 survives.
    #[test]
    fn an_input_neuron_past_the_declared_width_is_rejected() {
        let stray = [input_view(0), input_view(1), input_view(2)];

        let failure = walk(&stray, 1, 0).expect_err("one input was declared, three were found");
        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "2) input neuron after the maximum input neurons",
        );
        assert!(
            walk(&stray[..2], 2, 0).is_ok(),
            "two inputs declared, two walked"
        );
    }

    // -----------------------------------------------------------------------
    // Rules 8, 9, 11 — bias and ordering.
    // -----------------------------------------------------------------------

    /// Rule 8: every non-input neuron carries a finite bias, and the message
    /// renders the value the way JavaScript does.
    #[test]
    fn a_non_finite_bias_on_a_non_input_neuron_is_rejected() {
        let with_bias = |bias: f64| {
            creature(
                1,
                1,
                vec![
                    neuron("constant", "c-0", Some(7), bias),
                    neuron("output", "out-0", None, 0.0),
                ],
                vec![edge("c-0", "out-0")],
            )
        };

        for (bias, text) in [
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_failure(
                &rejects(&with_bias(bias)),
                FailureClass::Validation,
                reason::OTHER,
                &format!("7) invalid bias: {text}"),
            );
        }

        assert_eq!(accepts(&with_bias(-1.5)).constant, 1);
    }

    /// Rule 9: nothing but an output may follow an output.
    #[test]
    fn a_neuron_after_an_output_neuron_is_rejected() {
        let trailing = creature(
            1,
            1,
            vec![
                squashed("output", "out-0", None, "IDENTITY"),
                neuron("hidden", "h-0", Some(7), 0.0),
            ],
            vec![edge("input-0", "out-0"), edge("input-0", "h-0")],
        );

        assert_failure(
            &rejects(&trailing),
            FailureClass::Validation,
            reason::OTHER,
            "7) type hidden after output neuron",
        );
    }

    /// Rule 11: inside the computational slice the order is constant, then
    /// hidden — the message labels the neuron the way the wire does.
    #[test]
    fn a_constant_after_a_hidden_neuron_breaks_the_required_order() {
        let out_of_order = creature(
            1,
            1,
            vec![
                neuron("hidden", "h-0", Some(7), 0.0),
                neuron("constant", "c-late", Some(8), 1.0),
                squashed("output", "out-0", None, "IDENTITY"),
            ],
            vec![
                edge("input-0", "h-0"),
                edge("h-0", "out-0"),
                edge("c-late", "out-0"),
            ],
        );

        let failure = rejects(&out_of_order);
        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::NEURON_ORDER,
            "c-late) type constant after hidden neuron; required order is input, constant, hidden, output",
        );
        assert_eq!(failure.neuron_index, Some(2));

        let mut ordered = out_of_order.clone();
        ordered.neurons.swap(0, 1);
        assert_eq!(accepts(&ordered).hidden, 1, "constant first is fine");
    }

    // -----------------------------------------------------------------------
    // Rule 12 — the `IF` squash.
    // -----------------------------------------------------------------------

    /// Three inputs feeding one `IF` output at index 3, with the roles named.
    fn if_creature(roles: [&str; 3]) -> CreatureExport {
        creature(
            3,
            1,
            vec![squashed("output", "out-0", None, "IF")],
            roles
                .iter()
                .enumerate()
                .map(|(i, role)| typed_edge(&format!("input-{i}"), "out-0", role))
                .collect(),
        )
    }

    #[test]
    fn an_if_neuron_with_fewer_than_three_inward_connections_is_rejected() {
        let mut starved = if_creature(["condition", "positive", "negative"]);
        starved.synapses.pop();

        assert_failure(
            &rejects(&starved),
            FailureClass::Validation,
            reason::IF_CONDITIONS,
            "-1) 'IF' should have at least 3 inward connections was: 2",
        );
        assert_eq!(
            accepts(&if_creature(["condition", "positive", "negative"])).output,
            1
        );
    }

    #[test]
    fn an_if_neuron_missing_a_role_names_the_role_it_is_missing() {
        for (roles, message) in [
            (
                ["positive", "positive", "negative"],
                "-1) 'IF' should have a condition(s)",
            ),
            (
                ["condition", "condition", "negative"],
                "-1) 'IF' should have a positive connection(s)",
            ),
            (
                ["condition", "positive", "positive"],
                "-1) 'IF' should have a negative connection(s)",
            ),
        ] {
            assert_failure(
                &rejects(&if_creature(roles)),
                FailureClass::Validation,
                reason::IF_CONDITIONS,
                message,
            );
        }
    }

    /// An untyped synapse is a positive input, so these three roles are
    /// complete even though only two are declared.
    #[test]
    fn an_untyped_inward_synapse_fills_the_positive_role() {
        let mut untyped = if_creature(["condition", "positive", "negative"]);
        untyped.synapses[1].synapse_type = None;

        assert_eq!(accepts(&untyped).output, 1);
    }

    /// The rule only arms past index 2 — an `IF` neuron that early cannot have
    /// three inward connections in the first place.
    #[test]
    fn the_if_rule_is_skipped_for_the_first_three_neurons() {
        let early = creature(
            1,
            1,
            vec![squashed("output", "out-0", None, "IF")],
            vec![edge("input-0", "out-0")],
        );

        assert_eq!(accepts(&early).output, 1, "index 1 is not > 2");
    }

    // -----------------------------------------------------------------------
    // Rules 13-20 — the per-type switch.
    // -----------------------------------------------------------------------

    /// Rule 13: nothing feeds an observation.
    #[test]
    fn a_synapse_into_an_input_neuron_is_rejected() {
        let mut fed_input = stump_creature();
        fed_input.synapses.push(edge("const-positive", "input-0"));

        let failure = rejects(&fed_input);
        assert_failure(
            &failure,
            FailureClass::Topology,
            reason::INVALID_CONNECTION,
            "'input' neuron 0 has inward connections: 1",
        );
        assert_eq!(failure.neuron_index, Some(0));
    }

    /// Rule 14: a constant is a source, never a sink.
    #[test]
    fn a_constant_with_an_inward_connection_is_rejected() {
        let fed_constant = creature(
            1,
            1,
            vec![
                neuron("constant", "c-0", Some(7), 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("input-0", "c-0"), edge("c-0", "out-0")],
        );

        assert_failure(
            &rejects(&fed_constant),
            FailureClass::Topology,
            reason::INVALID_CONNECTION,
            "'constant' neuron 7 has inward connections: 1",
        );
    }

    /// Rule 15: a constant emits its bias, so a squash has nothing to do.
    #[test]
    fn a_constant_carrying_a_squash_is_rejected() {
        let squashed_constant = creature(
            1,
            1,
            vec![
                squashed("constant", "c-0", Some(7), "TANH"),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("c-0", "out-0")],
        );

        assert_failure(
            &rejects(&squashed_constant),
            FailureClass::Topology,
            reason::INVALID_SQUASH,
            "Node 7 'constant' has squash: TANH",
        );
    }

    /// Rule 16: a constant nothing reads is dead weight.
    #[test]
    fn a_constant_with_no_outward_connection_is_rejected() {
        let orphaned = creature(
            1,
            1,
            vec![
                neuron("constant", "c-lonely", Some(7), 1.0),
                neuron("output", "out-0", None, 0.0),
            ],
            vec![edge("input-0", "out-0")],
        );

        assert_failure(
            &rejects(&orphaned),
            FailureClass::Validation,
            reason::NO_OUTWARD_CONNECTIONS,
            "constants neuron c-lonely has no outward connections",
        );

        let mut wired = orphaned.clone();
        wired.synapses.push(edge("c-lonely", "out-0"));
        assert_eq!(accepts(&wired).constant, 1);
    }

    /// Rules 17 and 18: a hidden neuron is wired in *and* out, inward first.
    #[test]
    fn a_hidden_neuron_must_be_wired_in_and_out() {
        let hidden = |synapses: Vec<SynapseExport>| {
            creature(
                1,
                1,
                vec![
                    neuron("hidden", "h-0", Some(7), 0.0),
                    neuron("output", "out-0", None, 0.0),
                ],
                synapses,
            )
        };

        assert_failure(
            &rejects(&hidden(vec![edge("h-0", "out-0")])),
            FailureClass::Validation,
            reason::NO_INWARD_CONNECTIONS,
            "hidden neuron h-0 has no inward connections",
        );
        assert_failure(
            &rejects(&hidden(vec![
                edge("input-0", "h-0"),
                edge("input-0", "out-0"),
            ])),
            FailureClass::Validation,
            reason::NO_OUTWARD_CONNECTIONS,
            "hidden neuron h-0 has no outward connections",
        );
        assert_eq!(
            accepts(&hidden(vec![edge("input-0", "h-0"), edge("h-0", "out-0")])).hidden,
            1
        );
    }

    /// Rule 19: ported for completeness, and unreachable — rule 8 rejects both
    /// biases first, so the messages are exercised against the rule itself.
    #[test]
    fn a_hidden_neuron_needs_a_present_and_finite_bias() {
        assert_eq!(
            hidden_bias_failure(7, None).map(|f| f.message),
            Some("hidden neuron 7 should have a bias was: undefined".to_string())
        );
        let non_finite = hidden_bias_failure(7, Some(f64::INFINITY)).expect("not finite");
        assert_failure(
            &non_finite,
            FailureClass::Topology,
            reason::INVALID_STATE,
            "7) hidden neuron should have a finite bias was: Infinity",
        );
        assert!(hidden_bias_failure(7, Some(0.5)).is_none());
    }

    /// Rule 20: the type is one of the four known ones — and `input` is not one
    /// of them in the export form, where input neurons are implicit.
    #[test]
    fn an_unknown_neuron_type_is_rejected_and_so_is_a_declared_input() {
        let typed = |neuron_type: &str| {
            creature(
                1,
                1,
                vec![
                    neuron(neuron_type, "n-0", Some(7), 0.0),
                    neuron("output", "out-0", None, 0.0),
                ],
                vec![edge("input-0", "out-0")],
            )
        };

        assert_failure(
            &rejects(&typed("mystery")),
            FailureClass::Topology,
            reason::INVALID_NEURON_TYPE,
            "7) Invalid type: mystery",
        );
        assert_failure(
            &rejects(&typed("input")),
            FailureClass::Topology,
            reason::INVALID_NEURON_TYPE,
            "7) Invalid type: input",
        );
    }

    // -----------------------------------------------------------------------
    // Rules 21 and 22 — post-walk counts.
    // -----------------------------------------------------------------------

    /// Rule 21: the counted inputs are the declared width. Unreachable from a
    /// `CreatureExport`, whose input neurons are derived from that same width.
    #[test]
    fn fewer_input_neurons_than_declared_is_rejected() {
        let failure = walk(&[output_view(-1), output_view(-2)], 1, 2)
            .expect_err("one input was declared, none were walked");

        assert_failure(
            &failure,
            FailureClass::Validation,
            reason::OTHER,
            "Expected 1 input neurons found: 0",
        );
        assert!(walk(&[input_view(0), output_view(-1)], 1, 1).is_ok());
    }

    /// Rule 22: and so are the counted outputs.
    #[test]
    fn fewer_output_neurons_than_declared_is_rejected() {
        let short = creature(
            1,
            2,
            vec![neuron("output", "out-0", None, 0.0)],
            vec![edge("input-0", "out-0")],
        );

        assert_failure(
            &rejects(&short),
            FailureClass::Topology,
            reason::INVALID_STATE,
            "Expected 2 output neurons found: 1",
        );
    }

    // -----------------------------------------------------------------------
    // Input format — the one failure the TypeScript shape cannot express.
    // -----------------------------------------------------------------------

    /// A synapse endpoint naming no neuron is `INVALID_SYNAPSE_REFERENCE`,
    /// carrying the synapse index rather than a neuron index.
    #[test]
    fn a_synapse_endpoint_that_names_no_neuron_is_rejected() {
        let mut dangling = stump_creature();
        dangling.synapses.push(edge("ghost", "output-0"));

        let failure = rejects(&dangling);
        assert_eq!(failure.class, FailureClass::Topology);
        assert_eq!(failure.reason, reason::INVALID_SYNAPSE_REFERENCE);
        assert_eq!(failure.synapse_index, Some(4));
        assert_eq!(failure.neuron_index, None);
        assert!(
            failure.message.contains("ghost"),
            "names the endpoint: {}",
            failure.message
        );

        let mut unknown_target = stump_creature();
        unknown_target.synapses.push(edge("input-0", "input-9"));
        assert_eq!(
            rejects(&unknown_target).reason,
            reason::INVALID_SYNAPSE_REFERENCE,
            "an input index past the declared width names no neuron either"
        );
    }

    /// The wire label reproduces `neuronWireLabelForDiagnostics` branch for
    /// branch, including both no-UUID fallbacks.
    #[test]
    fn the_wire_label_reproduces_the_typescript_diagnostic_label() {
        let hidden = NeuronView {
            kind: NeuronKind::Hidden,
            declared_type: "hidden",
            ..output_view(-2)
        };

        assert_eq!(input_view(0).wire_label(0), "input-0");
        assert_eq!(
            output_view(-2).wire_label(4),
            "output-1",
            "an output is labelled from its id, -(id + 1), not its UUID"
        );
        assert_eq!(
            NeuronView {
                id: Some(7),
                ..output_view(-2)
            }
            .wire_label(4),
            "out",
            "an output without an output id falls back to the UUID"
        );
        assert_eq!(hidden.wire_label(4), "out", "a hidden neuron uses its UUID");
        assert_eq!(
            NeuronView {
                uuid: Some(""),
                ..hidden
            }
            .wire_label(4),
            "non-output-negative-id--2@index-4",
            "a stray negative id on a non-output neuron is called out"
        );
        assert_eq!(
            NeuronView {
                id: Some(7),
                uuid: None,
                ..hidden
            }
            .wire_label(4),
            "missing-uuid@index-4"
        );
    }

    /// JavaScript renders an integral double without a fraction, and names its
    /// three non-finite values.
    #[test]
    fn numbers_render_the_way_javascript_interpolates_them() {
        assert_eq!(number_text(Some(3.0)), "3");
        assert_eq!(number_text(Some(-0.25)), "-0.25");
        assert_eq!(number_text(Some(f64::NAN)), "NaN");
        assert_eq!(number_text(Some(f64::INFINITY)), "Infinity");
        assert_eq!(number_text(Some(f64::NEG_INFINITY)), "-Infinity");
        assert_eq!(number_text(None), "undefined");
    }
}
