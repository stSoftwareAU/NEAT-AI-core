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
//! | `neuron.index !== indx` | the export carries no `index`; position *is* the index here |
//! | `neuron.validate()` | per-neuron host method over the live `Neuron` class |
//! | `debugWrite(creature)` diagnostics dump | writes `creatureValidate.json` to the host diagnostics dir |
//!
//! The Rust failure carries [`ValidationFailure::neuron_index`] /
//! [`ValidationFailure::synapse_index`] so the host can run its own identity
//! checks and its diagnostics dump against the same neuron or synapse the
//! shared rules stopped on.

use std::fmt;

use crate::creature::CreatureExport;

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
