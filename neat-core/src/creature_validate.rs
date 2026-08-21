//! Creature validation — the shared definition of a valid creature (Issue #559).
//!
//! This module is the Rust home of NEAT-AI's
//! `src/architecture/CreatureValidate.ts`: the invariants a healthy topology
//! must hold. NEAT-AI-Forests shipped an invalid creature that
//! `Creature.validate()` would have caught, and the fix is **one** definition
//! both stacks read — Rust consumers call [`creature_validate`] natively, and
//! NEAT-AI calls the same code over the existing WASM boundary.
//!
//! **This issue lands the contract only.** The types, the error model, the
//! rule *order* and the input format are fixed here so the two porting issues
//! (NEAT-AI#3801 / #3802) can be worked in parallel against a stable
//! interface. [`creature_validate`] therefore evaluates no rule yet — see
//! [the entry point](creature_validate) for what it returns until they land.
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
//! # Rules 23–31 are ported (Issue #561)
//!
//! [`validate_synapse_and_memetic_rules`] is the synapse, forward-only and
//! memetic half of the table above. Three things about it are worth knowing
//! before reading the code:
//!
//! - **It is not wired into [`creature_validate`] yet.** The neuron half is
//!   Issue #560, and half a rule set cannot certify a creature, so the entry
//!   point stays a loud stub until both halves exist.
//! - **The forward-only leg reuses [`crate::topology_ops`]** —
//!   `validate_topology`, `validate_structural_integrity` and `detect_cycles`
//!   — rather than restating any of those invariants, and keeps the
//!   TypeScript's `WASM ... at synapse {i}` / `at neuron {i}` message shapes so
//!   NEAT-AI's `TopologyErrorMessages.ts` labels still line up.
//! - **Diagnostic labels are ported, not invented.** Messages that name a
//!   neuron use the same text as NEAT-AI's `neuronWireLabelForDiagnostics`
//!   (`input-{index}`, `output-{outputIndex}`, else the UUID), because
//!   NEAT-AI's error-message tests assert on it.
//!
//! Rule 26 only catches duplicates that are *adjacent* after sorting, which is
//! sufficient here but not for the same reason as Issue #556: a duplicate that
//! is separated by another pair necessarily creates the sort regression rule 25
//! stops on first, so the creature is still rejected — under `SORT_FAILURE`
//! rather than `INVALID_CONNECTION`. Nothing slips through either path;
//! `validate_no_duplicate_synapses` is order-independent and rejects it too.
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

use crate::creature::{CreatureExport, MemeticExport, parse_squash_name, parse_synapse_type};
use crate::topology_ops::{
    STRUCTURAL_VALID, VALID, detect_cycles, structural_error_message, topology_error_message,
    validate_structural_integrity, validate_topology,
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
/// # Contract stub (Issue #559)
///
/// The rule bodies are ported by NEAT-AI#3801 / #3802. Until they land this
/// function checks **nothing**, and it says so: it returns a
/// [`FailureClass::Validation`] / [`reason::OTHER`] failure rather than an
/// empty [`ValidationStats`], because an unconditional `Ok` would report every
/// creature — including the invalid one this work exists to catch — as valid.
/// A consumer wiring the entry point up early fails loudly instead.
///
/// Rules 23–31 are ported and callable as
/// [`validate_synapse_and_memetic_rules`] (Issue #561); rules 1–22 are Issue
/// #560. This function starts evaluating rules once **both** halves exist —
/// running one half here would certify creatures against a rule set that is
/// only half checked, which is the failure this stub exists to prevent.
///
/// # Errors
///
/// Returns the [`ValidationFailure`] for the first violated rule.
pub fn creature_validate(
    _creature: &CreatureExport,
    _options: &ValidateOptions,
) -> Result<ValidationStats, ValidationFailure> {
    Err(ValidationFailure::validation(
        reason::OTHER,
        "creature_validate rule bodies are not ported yet (Issue #559 landed the \
         contract only): no creature can be certified valid by this build",
    ))
}

// ===========================================================================
// Issue #561 — the synapse, forward-only and memetic half.
// ===========================================================================

/// One neuron in the index space the rules walk.
///
/// Indices `0..input` are the implicit input neurons and carry `id == index`;
/// index `input + i` is `creature.neurons[i]`. See the input-format section of
/// the module documentation.
#[derive(Debug, Clone, Copy)]
struct ResolvedNeuron<'a> {
    /// The declared type, or `"input"` for an implicit input neuron.
    neuron_type: &'a str,
    /// The declared UUID; empty for an implicit input neuron, whose label is
    /// derived from its index instead.
    uuid: &'a str,
    /// The neuron id, absent when the export omitted it.
    id: Option<i64>,
    /// The bias; `0.0` for an implicit input neuron.
    bias: f64,
    /// The declared squash name, if any.
    squash: Option<&'a str>,
}

/// One synapse with both endpoints resolved to indices.
#[derive(Debug, Clone, Copy)]
struct ResolvedSynapse {
    /// Source neuron index.
    from: usize,
    /// Destination neuron index.
    to: usize,
    /// [`crate::synapse_type::SynapseType`] discriminant, defaulted the way
    /// [`parse_synapse_type`] defaults it.
    synapse_type: u8,
}

/// A [`CreatureExport`] projected onto the indexed form the TypeScript
/// validator walks.
#[derive(Debug)]
struct ResolvedCreature<'a> {
    /// Declared observation width.
    input: usize,
    /// Declared target width.
    output: usize,
    /// Implicit input neurons first, then the exported ones in order.
    neurons: Vec<ResolvedNeuron<'a>>,
    /// Synapses in declaration order, endpoints resolved to indices.
    synapses: Vec<ResolvedSynapse>,
}

impl ResolvedCreature<'_> {
    /// The diagnostic label NEAT-AI's `neuronWireLabelForDiagnostics` produces.
    ///
    /// Ported verbatim (`src/neuron/NeuronSerialization.ts`) because NEAT-AI's
    /// error-message tests assert on the text this builds into a failure
    /// message: `input-{index}` for an input neuron, `output-{outputIndex}`
    /// for an output neuron with a negative id (NEAT-AI #1958), otherwise the
    /// UUID, and two named fallbacks when there is no UUID at all.
    fn wire_label(&self, index: usize) -> String {
        let neuron = self.neurons[index];
        if neuron.neuron_type == "input" {
            return format!("input-{index}");
        }
        if neuron.neuron_type == "output"
            && let Some(id) = neuron.id
            && id < 0
        {
            return format!("output-{}", -(id + 1));
        }
        if !neuron.uuid.is_empty() {
            return neuron.uuid.to_string();
        }
        if let Some(id) = neuron.id
            && id < 0
        {
            return format!("non-output-negative-id-{id}@index-{index}");
        }
        format!("missing-uuid@index-{index}")
    }
}

/// Project a [`CreatureExport`] onto the indexed form the rules walk.
///
/// The derivation is [`crate::creature::compile_creature`]'s and is part of
/// the contract: implicit `input-N` neurons first, then the exported neurons,
/// with every synapse endpoint resolved through the same UUID map.
fn resolve(creature: &CreatureExport) -> Result<ResolvedCreature<'_>, ValidationFailure> {
    let input = creature.input;
    let mut neurons: Vec<ResolvedNeuron<'_>> = Vec::with_capacity(input + creature.neurons.len());
    let mut uuid_to_index: HashMap<String, usize> =
        HashMap::with_capacity(input + creature.neurons.len());

    for index in 0..input {
        neurons.push(ResolvedNeuron {
            neuron_type: "input",
            uuid: "",
            id: Some(index as i64),
            bias: 0.0,
            squash: None,
        });
        uuid_to_index.insert(format!("input-{index}"), index);
    }

    for (offset, neuron) in creature.neurons.iter().enumerate() {
        let index = input + offset;
        if neuron.neuron_type == "input" {
            return Err(ValidationFailure::topology(
                reason::INVALID_NEURON_TYPE,
                format!(
                    "{}) Invalid type: input — input neurons are implicit in the export form",
                    neuron.uuid
                ),
            )
            .at_neuron(index as u32));
        }
        neurons.push(ResolvedNeuron {
            neuron_type: neuron.neuron_type.as_str(),
            uuid: neuron.uuid.as_str(),
            id: neuron.id,
            bias: neuron.bias,
            squash: neuron.squash.as_deref(),
        });
        uuid_to_index.insert(neuron.uuid.clone(), index);
    }

    let mut synapses = Vec::with_capacity(creature.synapses.len());
    for (indx, synapse) in creature.synapses.iter().enumerate() {
        let from = *uuid_to_index
            .get(synapse.from_uuid.as_str())
            .ok_or_else(|| unresolved_endpoint(indx, "fromUUID", &synapse.from_uuid))?;
        let to = *uuid_to_index
            .get(synapse.to_uuid.as_str())
            .ok_or_else(|| unresolved_endpoint(indx, "toUUID", &synapse.to_uuid))?;
        synapses.push(ResolvedSynapse {
            from,
            to,
            synapse_type: parse_synapse_type(synapse.synapse_type.as_deref()) as u8,
        });
    }

    Ok(ResolvedCreature {
        input,
        output: creature.output,
        neurons,
        synapses,
    })
}

/// The failure for a synapse endpoint that names no neuron — the one failure
/// the in-memory TypeScript form cannot express (see the module docs).
fn unresolved_endpoint(indx: usize, field: &str, uuid: &str) -> ValidationFailure {
    ValidationFailure::topology(
        reason::INVALID_SYNAPSE_REFERENCE,
        format!("{indx}) synapse {field} {uuid} does not name a neuron"),
    )
    .at_synapse(indx as u32)
}

/// Rules 23–27: a single pass over the synapses, tallying
/// [`ValidationStats::connections`].
fn synapse_walk(
    creature: &ResolvedCreature<'_>,
    options: &ValidateOptions,
    stats: &mut ValidationStats,
) -> Result<(), ValidationFailure> {
    let mut last_from: i64 = -1;
    let mut last_to: i64 = -1;

    for (indx, synapse) in creature.synapses.iter().enumerate() {
        stats.connections += 1;
        let synapse_index = indx as u32;

        if creature.neurons[synapse.to].neuron_type == "input" {
            return Err(ValidationFailure::topology(
                reason::INVALID_CONNECTION,
                format!("{indx}) connection points to an input node"),
            )
            .at_synapse(synapse_index));
        }

        if options.forward_only && synapse.from == synapse.to {
            let from_label = creature.wire_label(synapse.from);
            return Err(ValidationFailure::validation(
                reason::SELF_CONNECTION,
                format!("{indx}) Self connection synapse {from_label} -> {from_label}"),
            )
            .at_synapse(synapse_index));
        }

        let from = synapse.from as i64;
        let to = synapse.to as i64;

        if from < last_from {
            return Err(ValidationFailure::topology(
                reason::SORT_FAILURE,
                format!("{indx}) synapses not sorted"),
            )
            .at_synapse(synapse_index));
        } else if from > last_from {
            // Belt and braces with the `from == last_from` guard below, and
            // kept because the TypeScript keeps it: a new `from` starts its
            // `to` ordering afresh.
            last_to = -1;
        }

        if from == last_from {
            if to < last_to {
                return Err(ValidationFailure::topology(
                    reason::SORT_FAILURE,
                    format!("{indx}) synapses not sorted {from}->{to} last to: {last_to}"),
                )
                .at_synapse(synapse_index));
            } else if to == last_to {
                // Issue #556 — the same "one ordered (from, to) pair, at most
                // once" invariant as `validate_no_duplicate_synapses`, reported
                // under the TypeScript's own class and reason.
                let from_label = creature.wire_label(synapse.from);
                let to_label = creature.wire_label(synapse.to);
                return Err(ValidationFailure::topology(
                    reason::INVALID_CONNECTION,
                    format!("{indx}) duplicate synapse {from_label} -> {to_label}"),
                )
                .at_synapse(synapse_index));
            }
        }

        if from > to && options.rejects_recursive_synapses() {
            let from_label = creature.wire_label(synapse.from);
            let to_label = creature.wire_label(synapse.to);
            return Err(ValidationFailure::validation(
                reason::RECURSIVE_SYNAPSE,
                format!("{indx}) Recursive synapse {from_label} -> {to_label}"),
            )
            .at_synapse(synapse_index));
        }

        last_from = from;
        last_to = to;
    }

    Ok(())
}

/// Rules 29–30: the extra leg a forward-only creature runs, delegating to
/// [`crate::topology_ops`] rather than reimplementing any of it.
///
/// The three calls and their message shapes are the TypeScript's, including
/// the `WASM ...` wording: NEAT-AI's `TopologyErrorMessages.ts` labels and its
/// error-message tests read this exact text, and the messages crossed the WASM
/// boundary long before the rules did.
fn forward_only_rules(creature: &ResolvedCreature<'_>) -> Result<(), ValidationFailure> {
    let from_indices: Vec<u32> = creature.synapses.iter().map(|s| s.from as u32).collect();
    let to_indices: Vec<u32> = creature.synapses.iter().map(|s| s.to as u32).collect();

    let topology = validate_topology(&from_indices, &to_indices);
    if topology[0] != VALID {
        return Err(ValidationFailure::topology(
            reason::INVALID_CONNECTION,
            format!(
                "WASM topology validation failed: {} at synapse {}",
                topology_error_message(topology[0]),
                topology[1]
            ),
        )
        .at_synapse(topology[1].max(0) as u32));
    }

    let is_constant: Vec<u8> = creature
        .neurons
        .iter()
        .map(|n| u8::from(n.neuron_type == "constant"))
        .collect();
    let biases: Vec<f64> = creature.neurons.iter().map(|n| n.bias).collect();
    // A squash name this crate does not know is not one of the ported rules —
    // NEAT-AI checks it host-side in `neuron.validate()` — so an unknown name
    // maps to a sentinel that is simply "not IF" rather than being reported
    // here under a rule that does not exist.
    let squash_types: Vec<u8> = creature
        .neurons
        .iter()
        .map(|n| {
            parse_squash_name(n.squash.unwrap_or("IDENTITY"))
                .map(|squash| squash as u8)
                .unwrap_or(u8::MAX)
        })
        .collect();
    let synapse_types: Vec<u8> = creature.synapses.iter().map(|s| s.synapse_type).collect();

    let structural = validate_structural_integrity(
        &from_indices,
        &to_indices,
        &is_constant,
        &squash_types,
        &biases,
        creature.input as u32,
        creature.output as u32,
        &synapse_types,
    );
    if structural[0] != STRUCTURAL_VALID {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!(
                "WASM structural validation failed: {} at neuron {}",
                structural_error_message(structural[0]),
                structural[1]
            ),
        )
        .at_neuron(structural[1].max(0) as u32));
    }

    if detect_cycles(
        &from_indices,
        &to_indices,
        creature.neurons.len() as u32,
        creature.input as u32,
    ) != 0
    {
        return Err(ValidationFailure::topology(
            reason::INVALID_CONNECTION,
            "Forward-only creature contains cycles",
        ));
    }

    Ok(())
}

/// Rule 31: every memetic bias and weight resolves to a real neuron, and every
/// weight entry names a synapse that exists.
///
/// The match is on **neuron ids**, not indices: the synapse set is built from
/// `neurons[s.from].id -> neurons[s.to].id`, exactly as the TypeScript builds
/// it, so a creature whose ids differ from its indices still resolves. A
/// neuron with no id contributes nothing to either lookup — the neuron half
/// (Issue #560) is what rejects a missing id, and this half must not report it
/// under the wrong rule.
///
/// A key that is not an integer cannot match any id and is reported as "not
/// found", which is the outcome TypeScript's `Number(key)` reaches for every
/// non-integer key too.
fn memetic_rules(
    creature: &ResolvedCreature<'_>,
    memetic: &MemeticExport,
) -> Result<(), ValidationFailure> {
    let known_ids: HashSet<i64> = creature.neurons.iter().filter_map(|n| n.id).collect();

    let mut pairs: HashSet<String> = HashSet::with_capacity(creature.synapses.len());
    for synapse in &creature.synapses {
        if let (Some(from_id), Some(to_id)) = (
            creature.neurons[synapse.from].id,
            creature.neurons[synapse.to].id,
        ) {
            pairs.insert(format!("{from_id}->{to_id}"));
        }
    }

    let known = |key: &str| -> bool { key.parse::<i64>().is_ok_and(|id| known_ids.contains(&id)) };

    for neuron_id in memetic.biases.keys() {
        if !known(neuron_id) {
            return Err(ValidationFailure::validation(
                reason::MEMETIC,
                format!("Neuron with id {neuron_id} not found in the creature."),
            ));
        }
    }

    for (synapse_id, weights) in &memetic.weights {
        if !known(synapse_id) {
            return Err(ValidationFailure::validation(
                reason::MEMETIC,
                format!("Synapse with id {synapse_id} not found in the creature."),
            ));
        }

        for (indx, entry) in weights.iter().enumerate() {
            let Some(to_id) = entry.to_id else {
                // TypeScript interpolates the absent field as `undefined`.
                return Err(ValidationFailure::validation(
                    reason::MEMETIC,
                    format!("Memetic from id {synapse_id} to id undefined is invalid."),
                ));
            };
            if entry.weight.is_none() {
                return Err(ValidationFailure::validation(
                    reason::MEMETIC,
                    format!(
                        "Memetic from id {synapse_id} to id {to_id} has invalid weight at index {indx}."
                    ),
                ));
            }
            if !known_ids.contains(&to_id) {
                return Err(ValidationFailure::validation(
                    reason::MEMETIC,
                    format!("Memetic from id {synapse_id} has no valid neuron."),
                ));
            }
            if !pairs.contains(&format!("{synapse_id}->{to_id}")) {
                return Err(ValidationFailure::validation(
                    reason::MEMETIC,
                    format!("Memetic from id {synapse_id} to id {to_id} has no matching synapses."),
                ));
            }
        }
    }

    Ok(())
}

/// Rules 23–31 — the synapse, forward-only and memetic half of
/// [`creature_validate`] (Issue #561).
///
/// These are the rules NEAT-AI's `creatureValidate` evaluates *after* the
/// neuron walk, in the same order and first-failure-wins:
///
/// 1. the single synapse pass (rules 23–27), tallying
///    [`ValidationStats::connections`] as it goes;
/// 2. the [`ValidateOptions::expected_connections`] count (rule 28);
/// 3. for a forward-only creature, [`crate::topology_ops`]'s
///    `validate_topology`, `validate_structural_integrity` and `detect_cycles`
///    (rules 29–30);
/// 4. the memetic cross-references (rule 31).
///
/// `stats` is the same object the neuron walk fills, threaded through the way
/// the TypeScript threads its single `stats` literal: this half **adds** the
/// connection tally and leaves the neuron counters alone. Callers running this
/// half on its own pass a `ValidationStats::default()`.
///
/// [`creature_validate`] does not call this yet — the neuron half is Issue
/// #560, and running only half the rules would certify a creature this crate
/// has not actually checked. It stays a loud stub until both halves exist.
///
/// # Errors
///
/// Returns the [`ValidationFailure`] for the first violated rule.
pub fn validate_synapse_and_memetic_rules(
    creature: &CreatureExport,
    options: &ValidateOptions,
    stats: &mut ValidationStats,
) -> Result<(), ValidationFailure> {
    let resolved = resolve(creature)?;

    synapse_walk(&resolved, options, stats)?;

    if let Some(expected) = options.expected_connections()
        && creature.synapses.len() != expected
    {
        return Err(ValidationFailure::validation(
            reason::OTHER,
            format!(
                "Synapses length: {} expected: {expected}",
                creature.synapses.len()
            ),
        ));
    }

    if options.forward_only {
        forward_only_rules(&resolved)?;
    }

    if let Some(memetic) = creature.memetic.as_ref() {
        memetic_rules(&resolved, memetic)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    //! Issue #561 — the parts of the ported half the public entry point cannot
    //! reach, because an earlier rule always stops the creature first.
    //!
    //! Every rule reachable through [`validate_synapse_and_memetic_rules`] is
    //! covered by `tests/creature_validate_synapse_rules.rs` instead.

    use super::*;
    use crate::synapse_type::SynapseType;

    fn neuron<'a>(neuron_type: &'a str, uuid: &'a str, id: i64) -> ResolvedNeuron<'a> {
        ResolvedNeuron {
            neuron_type,
            uuid,
            id: Some(id),
            bias: 0.5,
            squash: None,
        }
    }

    fn synapse(from: usize, to: usize) -> ResolvedSynapse {
        ResolvedSynapse {
            from,
            to,
            synapse_type: SynapseType::Standard as u8,
        }
    }

    /// `input-0`, hidden `h`, output `o` — indices `0`, `1`, `2`.
    fn resolved(synapses: Vec<ResolvedSynapse>) -> ResolvedCreature<'static> {
        ResolvedCreature {
            input: 1,
            output: 1,
            neurons: vec![
                ResolvedNeuron {
                    neuron_type: "input",
                    uuid: "",
                    id: Some(0),
                    bias: 0.0,
                    squash: None,
                },
                neuron("hidden", "h", 1),
                neuron("output", "o", -1),
            ],
            synapses,
        }
    }

    /// The synapse walk rejects a backward synapse under `forward_only` before
    /// the forward-only leg runs, so this is the only way to see the leg
    /// report a `topology_ops` error code — and it must carry the label and
    /// synapse index NEAT-AI's `TopologyErrorMessages.ts` formats around.
    #[test]
    fn the_forward_only_leg_reports_a_topology_error_code_with_its_synapse_index() {
        let creature = resolved(vec![synapse(0, 1), synapse(1, 2), synapse(2, 1)]);

        let failure = forward_only_rules(&creature).expect_err("backward connection");

        assert_eq!(failure.class, FailureClass::Topology);
        assert_eq!(failure.reason, reason::INVALID_CONNECTION);
        assert_eq!(
            failure.message,
            "WASM topology validation failed: Backward connection at synapse 2"
        );
        assert_eq!(failure.synapse_index, Some(2));
    }

    /// The topology check runs before structural integrity, so a creature
    /// breaking both reports the topology failure — the TypeScript's order.
    #[test]
    fn the_topology_check_runs_before_structural_integrity() {
        // `h` is a dead end (structural) *and* `o -> h` runs backwards.
        let creature = resolved(vec![synapse(0, 1), synapse(2, 1)]);

        let failure = forward_only_rules(&creature).expect_err("both legs fail");
        assert!(
            failure
                .message
                .starts_with("WASM topology validation failed"),
            "topology is reported first, was: {}",
            failure.message
        );
    }

    /// The cycle check is kept for parity with the TypeScript but is defence
    /// in depth: a topology-valid edge list has `from < to` everywhere, so it
    /// is acyclic by construction, and a cyclic list is stopped by the
    /// topology check above rather than slipping through unreported.
    #[test]
    fn a_cycle_never_reaches_the_cycle_check_unreported() {
        let acyclic = resolved(vec![synapse(0, 1), synapse(1, 2)]);
        let from: Vec<u32> = acyclic.synapses.iter().map(|s| s.from as u32).collect();
        let to: Vec<u32> = acyclic.synapses.iter().map(|s| s.to as u32).collect();
        assert_eq!(validate_topology(&from, &to)[0], VALID);
        assert_eq!(
            detect_cycles(&from, &to, 3, 1),
            0,
            "sorted forward is a DAG"
        );

        let cyclic = resolved(vec![synapse(1, 2), synapse(2, 1)]);
        let from: Vec<u32> = cyclic.synapses.iter().map(|s| s.from as u32).collect();
        let to: Vec<u32> = cyclic.synapses.iter().map(|s| s.to as u32).collect();
        assert_eq!(detect_cycles(&from, &to, 3, 1), 1, "the cycle is real");
        assert!(
            forward_only_rules(&cyclic).is_err(),
            "and the leg rejects it, whichever check gets there first"
        );
    }

    /// `neuronWireLabelForDiagnostics` ported verbatim — every branch, since
    /// NEAT-AI's error-message tests assert on the text it produces.
    #[test]
    fn the_wire_label_reproduces_every_typescript_branch() {
        let creature = ResolvedCreature {
            input: 1,
            output: 1,
            neurons: vec![
                ResolvedNeuron {
                    neuron_type: "input",
                    uuid: "",
                    id: Some(0),
                    bias: 0.0,
                    squash: None,
                },
                neuron("hidden", "h", 1),
                ResolvedNeuron {
                    neuron_type: "hidden",
                    uuid: "",
                    id: Some(-7),
                    bias: 0.0,
                    squash: None,
                },
                ResolvedNeuron {
                    neuron_type: "hidden",
                    uuid: "",
                    id: None,
                    bias: 0.0,
                    squash: None,
                },
                neuron("output", "o", -1),
            ],
            synapses: vec![],
        };

        assert_eq!(creature.wire_label(0), "input-0");
        assert_eq!(creature.wire_label(1), "h");
        assert_eq!(creature.wire_label(2), "non-output-negative-id--7@index-2");
        assert_eq!(creature.wire_label(3), "missing-uuid@index-3");
        assert_eq!(creature.wire_label(4), "output-0", "-(id + 1)");
    }
}
