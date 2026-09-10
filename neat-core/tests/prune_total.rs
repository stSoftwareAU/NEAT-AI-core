//! Total prunability: every hidden neuron and every listed synapse of every
//! fixture prunes to `Ok` and the creature that comes back validates
//! (NEAT-AI-Ockham #195).
//!
//! The rule-by-rule tests next door — `prune_neuron.rs`, `prune_synapse.rs` —
//! each drive one removal and grade what came back. Neither answers the
//! question Ockham's screening loop actually asks: *can any candidate I might
//! pick be pruned at all?* A screen that has to guess which of its candidates
//! the shared helper will refuse is a screen with a hole in it.
//!
//! So this file sweeps instead of sampling. For every fixture, every hidden
//! neuron of it, every `(from, to, role)` triple it lists — with no statistics
//! and with a mean-only [`PruneStats`] — and for each of those requests:
//!
//! - the call returns `Ok`. `Err(PruneError::Cleanup)` is a reachable outcome
//!   of the entry points, and the README flowcharts are right to show it: a
//!   creature a caller built by hand can be beyond repair. What it must never
//!   be is the answer for a **valid** creature, and that is what this sweep
//!   proves rather than asserts;
//! - the request was actually carried out — the neuron is gone, or the edge is
//!   named on `removed_synapses` and the creature moved;
//! - the creature that comes back passes `creature_validate` and the topology
//!   gate;
//! - it carries no more than [`MAX_SUPPORT_CONSTANTS`] constants, so a sweep
//!   cannot inflate the support prefix; and
//! - the rewrite is deterministic — the same request twice gives the same
//!   creature, field for field.
//!
//! Edges sourced at an `input-N` and edges targeting an output neuron are
//! ordinary candidates here, exactly as `prune_synapse`'s contract says: the
//! declared widths are untouched by dropping one term from a sum.
//!
//! # Where the fixtures come from
//!
//! Three homes, in this order, deduplicated by creature so an overlap counts
//! once:
//!
//! 1. every `before()` of [`PRUNE_PARITY_CASES`] — the Issue #588 TypeScript
//!    captures;
//! 2. every creature carried by [`prune_golden_cases`] — the committed
//!    boundary record, which adds the static-`IF`, restored-role, proxy and
//!    refusal shapes;
//! 3. a short inline list below, for the creatures Ockham #195 names that
//!    neither home carries.
//!
//! `prune_cleanup.rs`'s own fixtures are deliberately not enumerated: they
//! address `cleanup_creature` directly rather than the two prune entry points
//! this contract is about, and the constant cap they exercise
//! (`FIVE_CONSTANTS_JSON`) has its dedicated coverage there.
//!
//! The sweep is guarded against going vacuous by
//! [`the_sweep_covers_valid_creatures`], which counts each home separately —
//! so a fixture table that silently stopped loading fails the test rather than
//! being covered for by the other two.
//!
//! Below the sweep sit the structural guards: the three-deep chain that
//! collapses in one call, the zero-inward fold, and the four `creature_validate`
//! wiring guards over rules 16-18 (`neat-core/src/creature_validate.rs`, the
//! rule table in the module header) that the whole contract rests on.

use neat_core::prune_fixtures::PRUNE_PARITY_CASES;
use neat_core::{
    CreatureExport, MAX_SUPPORT_CONSTANTS, PruneStats, SUPPORT_CONSTANT_BIAS, SynapseKey,
    SynapseType, ValidateOptions, creature_validate, parse_creature_json, parse_synapse_type,
    prune_golden_cases, prune_neuron, prune_synapse, validate_creature_topology,
};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// Slack for a weight derived here from the documented fold formula.
///
/// A **structural** activation is whatever the forward pass computes, and the
/// forward pass computes in `f32`, so the `f64` logistic derived in this file
/// agrees with it only to `f32` precision.
const STRUCTURAL_FOLD_TOL: f64 = 1e-6;

// --- inline fixtures --------------------------------------------------------
//
// Only the creatures Ockham #195 names that no shared home already carries.
// The issue's other named creatures are covered and are **not** copied here:
// `CONSTANT_SOURCE_JSON` is the golden record's `constant_edge_folds_exactly`,
// `ORDINARY_JSON` differs from `TWO_TARGETS_JSON` by one direct input edge the
// parity captures already carry, and of the `IF` creatures the golden record
// supplies the live, static-condition and restored-role shapes while
// `EDGE_ROLE_IDENTITY` supplies the two-sources-per-branch one. What is left
// below is what would otherwise go untested.

/// `h-1` feeds two surviving targets, one of which is also fed by a survivor.
const TWO_TARGETS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.75,"fromUUID":"input-1","toUUID":"h-2"},
    {"weight":0.5,"fromUUID":"h-1","toUUID":"h-2"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
  ]
}"#;

/// `h-1` feeds a `MINIMUM` aggregate as well as the output — the target that
/// reads its smallest inward term, so no bias fold stands in for a removal.
const AGGREGATE_TARGET_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-agg","bias":0.2,"squash":"MINIMUM"},
    {"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.75,"fromUUID":"input-1","toUUID":"h-agg"},
    {"weight":0.5,"fromUUID":"h-1","toUUID":"h-agg"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"}
  ]
}"#;

/// `h-1` sums nothing, so its activation is `LOGISTIC(0.4)` on every record.
///
/// Deliberately **non-canonical**: rule 17 refuses a hidden neuron with no
/// inward edge. It is swept anyway — see [`Fixture::canonical`].
const ZERO_INWARD_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.4,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// `h-dead` has no outward edge, so nothing reads what it computes.
///
/// Deliberately non-canonical for the mirror-image reason: rule 18.
const DEAD_NEURON_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-dead","bias":0.4,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.1,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-dead"}
  ]
}"#;

/// An `IDENTITY` neuron that sums nothing is worth `0.5` on every record — the
/// hidden-neuron twin of the captured constant fold.
const DISCOVERY_FOLD_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.5,"squash":"IDENTITY"},
    {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":0.2,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// An `IF` whose only condition source is a **hidden** neuron fed by one edge,
/// so cutting that edge fixes the condition. The golden record's static-`IF`
/// case reaches the same state from a *constant* condition source.
const IF_STATIC_AFTER_CUT_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-c","bias":0.5,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-p"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-n"},
    {"weight":1.0,"fromUUID":"h-c","toUUID":"if-1","type":"condition"},
    {"weight":2.0,"fromUUID":"h-p","toUUID":"if-1","type":"positive"},
    {"weight":-1.0,"fromUUID":"h-n","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// The same shape with the positive arm written **untyped**, which the forward
/// pass and `IfRoles::tally` both read as positive.
const IF_STATIC_UNTYPED_ARM_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-c","bias":0.5,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-p"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-n"},
    {"weight":1.0,"fromUUID":"h-c","toUUID":"if-1","type":"condition"},
    {"weight":2.0,"fromUUID":"h-p","toUUID":"if-1"},
    {"weight":-1.0,"fromUUID":"h-n","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// An `IF` with a varying condition, in a creature that **already** carries a
/// support constant: restoring an emptied branch role has to hang off that
/// constant rather than mint a second one (Ockham #180).
const IF_WITH_SUPPORT_CONSTANT_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"constant","uuid":"c-1","bias":1.0},
    {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"if-1","type":"condition"},
    {"weight":0.5,"fromUUID":"c-1","toUUID":"output-0"},
    {"weight":-3.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
    {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// Four bias-1 support constants — one more than [`MAX_SUPPORT_CONSTANTS`].
///
/// Written for this file rather than copied: the point is that the cap
/// assertion in the two sweeps is not vacuous. Every fixture above comes back
/// with at most one constant, so `constants <= MAX_SUPPORT_CONSTANTS` would
/// hold on any of them whatever the cap did. Here the cleanup has to merge the
/// surplus away on every one of the requests the sweep makes.
const SURPLUS_CONSTANTS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"constant","uuid":"k-a","bias":1.0},
    {"type":"constant","uuid":"k-b","bias":1.0},
    {"type":"constant","uuid":"k-c","bias":1.0},
    {"type":"constant","uuid":"k-d","bias":1.0},
    {"type":"hidden","uuid":"h-1","bias":0.15,"squash":"TANH"},
    {"type":"output","uuid":"output-0","bias":0.05,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.4,"fromUUID":"k-a","toUUID":"h-1"},
    {"weight":-0.6,"fromUUID":"k-b","toUUID":"h-1"},
    {"weight":1.5,"fromUUID":"k-c","toUUID":"output-0"},
    {"weight":-0.3,"fromUUID":"k-d","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// The inline creatures, paired with the name a failure reports.
const INLINE_FIXTURES: &[(&str, &str)] = &[
    ("two_targets", TWO_TARGETS_JSON),
    ("aggregate_target", AGGREGATE_TARGET_JSON),
    ("zero_inward", ZERO_INWARD_JSON),
    ("dead_neuron", DEAD_NEURON_JSON),
    ("discovery_fold", DISCOVERY_FOLD_JSON),
    ("if_static_after_cut", IF_STATIC_AFTER_CUT_JSON),
    ("if_static_untyped_arm", IF_STATIC_UNTYPED_ARM_JSON),
    ("if_with_support_constant", IF_WITH_SUPPORT_CONSTANT_JSON),
    ("surplus_constants", SURPLUS_CONSTANTS_JSON),
];

// --- the fixture table ------------------------------------------------------

/// Which home a fixture came from, so the anti-vacuity guard can count each
/// separately rather than let one cover for another going empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Parity,
    Golden,
    Inline,
}

/// One creature to sweep, and the name a failure reports it by.
struct Fixture {
    name: String,
    source: Source,
    creature: CreatureExport,
    /// Whether the fixture *itself* passes `creature_validate`.
    ///
    /// Most do. `inline/zero_inward` and `inline/dead_neuron` deliberately do
    /// not — one breaks rule 17, the other rule 18 — because the behaviour
    /// they pin next door is what a prune does with a neuron in that state.
    /// The sweep still runs them: a caller can hand core a creature straight
    /// out of a population, and the promise is about the creature that comes
    /// **back**. What the flag buys is the guard in
    /// [`the_sweep_covers_valid_creatures`]: the sweep must be reaching real
    /// canonical creatures, not a table that has quietly filled with
    /// degenerate ones.
    canonical: bool,
}

/// Every fixture the sweep covers, deduplicated by creature.
///
/// `neat-core/tests/fixtures/creature_validate/happy-paths.json` is **not**
/// here: its creatures are written in the index-addressed runtime shape
/// (`{"type":"input","id":0}` neurons, `{"from":0,"to":1}` synapses), which
/// `parse_creature_json` cannot deserialise into a `CreatureExport` at all —
/// `NeuronExport::uuid` and `SynapseExport::fromUUID` are both required. The
/// assumption on Ockham #195 admits them "only if they load through
/// `parse_creature_json`", and they do not.
fn fixtures() -> Vec<Fixture> {
    let mut out: Vec<Fixture> = Vec::new();

    for case in PRUNE_PARITY_CASES {
        push_unique(
            &mut out,
            format!("parity/{}", case.name),
            Source::Parity,
            case.before(),
        );
    }

    for case in prune_golden_cases() {
        // The boundary record carries deliberately malformed payloads too —
        // a missing creature, a creature with an unreadable field. Those are
        // requests, not creatures, so they are skipped rather than swept.
        let Some(value) = case.request.get("creature") else {
            continue;
        };
        let Ok(creature) = serde_json::from_value::<CreatureExport>(value.clone()) else {
            continue;
        };
        push_unique(
            &mut out,
            format!("golden/{}", case.name),
            Source::Golden,
            creature,
        );
    }

    for (name, json) in INLINE_FIXTURES {
        let creature = parse_creature_json(json)
            .unwrap_or_else(|e| panic!("inline fixture {name} does not parse: {e}"));
        push_unique(&mut out, format!("inline/{name}"), Source::Inline, creature);
    }

    out
}

/// Add a fixture unless the same creature is already in the table.
fn push_unique(out: &mut Vec<Fixture>, name: String, source: Source, creature: CreatureExport) {
    if out.iter().any(|f| f.creature == creature) {
        return;
    }
    let canonical = creature_validate(&creature, &OPTIONS).is_ok()
        && validate_creature_topology(&creature).is_ok();
    out.push(Fixture {
        name,
        source,
        creature,
        canonical,
    });
}

// --- helpers ----------------------------------------------------------------

fn creature(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("test fixture parses")
}

fn assert_valid(name: &str, creature: &CreatureExport) {
    creature_validate(creature, &OPTIONS)
        .unwrap_or_else(|f| panic!("{name}: creature is invalid: {f}"));
    validate_creature_topology(creature)
        .unwrap_or_else(|e| panic!("{name}: creature failed the topology gate: {e}"));
}

/// The mean-only statistic a caller supplies when it measured no more.
fn mean_only(mean: f64) -> PruneStats {
    PruneStats {
        mean_activation: mean,
        variance: None,
        proxy: None,
    }
}

fn hidden_uuids(creature: &CreatureExport) -> Vec<String> {
    creature
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "hidden")
        .map(|n| n.uuid.clone())
        .collect()
}

/// Every `(from, to, role)` triple the creature lists, in listed order.
///
/// Edges out of an `input-N` and into an output neuron are included: the
/// contract makes them ordinary candidates, so leaving them out would be
/// testing a narrower promise than the one callers rely on.
fn synapse_keys(creature: &CreatureExport) -> Vec<SynapseKey> {
    creature
        .synapses
        .iter()
        .map(|s| SynapseKey {
            from_uuid: s.from_uuid.clone(),
            to_uuid: s.to_uuid.clone(),
            role: parse_synapse_type(s.synapse_type.as_deref()),
        })
        .collect()
}

fn constant_count(creature: &CreatureExport) -> usize {
    creature
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "constant")
        .count()
}

/// Everything the contract promises about one successful prune, and the number
/// of constants it left so the sweep can prove the cap was exercised.
fn assert_prune_contract(name: &str, pruned: &CreatureExport) -> usize {
    assert_valid(name, pruned);
    let constants = constant_count(pruned);
    assert!(
        constants <= MAX_SUPPORT_CONSTANTS,
        "{name}: {constants} constants survived, above the {MAX_SUPPORT_CONSTANTS} cap"
    );
    constants
}

fn stats_label(stats: Option<&PruneStats>) -> &'static str {
    if stats.is_some() {
        "mean-only stats"
    } else {
        "no stats"
    }
}

// --- the sweep --------------------------------------------------------------

/// The sweep is reaching real canonical creatures, and enough of them, from
/// every home it claims to read.
///
/// This is the guard the whole file rests on: a fixture home that stopped
/// loading, or filled up with degenerate creatures, would let the two sweeps
/// below pass with nothing to prove. Each home is counted **separately**, so
/// one going empty cannot hide behind the others.
#[test]
fn the_sweep_covers_valid_creatures() {
    let fixtures = fixtures();

    let parity = fixtures
        .iter()
        .filter(|f| f.source == Source::Parity)
        .count();
    assert_eq!(
        parity,
        PRUNE_PARITY_CASES.len(),
        "the parity captures did not all reach the table"
    );
    assert!(
        parity >= 8,
        "only {parity} captured parity cases — the Issue #588 record has shrunk"
    );

    let golden = fixtures
        .iter()
        .filter(|f| f.source == Source::Golden)
        .count();
    assert!(
        golden >= 1,
        "no creature reached the table from the golden boundary record"
    );

    let inline = fixtures
        .iter()
        .filter(|f| f.source == Source::Inline)
        .count();
    assert_eq!(
        inline,
        INLINE_FIXTURES.len(),
        "an inline fixture was swallowed as a duplicate — it is no longer earning its place"
    );

    let canonical = fixtures.iter().filter(|f| f.canonical).count();
    assert!(
        canonical >= 8,
        "only {canonical} of {} fixtures pass creature_validate unaided — the sweep is no longer testing the total-prunability contract on valid creatures",
        fixtures.len()
    );

    let with_hidden = fixtures
        .iter()
        .filter(|f| !hidden_uuids(&f.creature).is_empty())
        .count();
    assert!(
        with_hidden >= 8,
        "only {with_hidden} fixtures carry a hidden neuron"
    );

    for fixture in &fixtures {
        assert!(
            !synapse_keys(&fixture.creature).is_empty(),
            "{}: no synapse to sweep",
            fixture.name
        );
        // A fixture with no hidden neuron must be one built round a constant —
        // `prune_neuron` protects constants, so there is genuinely nothing for
        // the neuron sweep to ask of it. Anything else is a fixture that has
        // lost its computational slice.
        if hidden_uuids(&fixture.creature).is_empty() {
            assert!(
                fixture
                    .creature
                    .neurons
                    .iter()
                    .any(|n| n.neuron_type == "constant"),
                "{}: neither a hidden neuron nor a constant — nothing to prune",
                fixture.name
            );
        }
    }
}

/// Every hidden neuron of every fixture prunes to `Ok` and validates.
#[test]
fn every_hidden_neuron_of_every_fixture_prunes() {
    let fixtures = fixtures();
    let stats = mean_only(0.5);
    let mut requests = 0usize;
    let mut swept_fixtures = 0usize;
    let mut max_constants = 0usize;

    for fixture in &fixtures {
        let hidden = hidden_uuids(&fixture.creature);
        if !hidden.is_empty() {
            swept_fixtures += 1;
        }
        for uuid in &hidden {
            for supplied in [None, Some(&stats)] {
                let label = format!("{} neuron {uuid} ({})", fixture.name, stats_label(supplied));
                let result = prune_neuron(&fixture.creature, uuid, supplied).unwrap_or_else(|e| {
                    panic!("{label}: refused, but every hidden neuron must prune: {e}")
                });

                max_constants = max_constants.max(assert_prune_contract(&label, &result.creature));
                assert_eq!(
                    result.removed_neuron.as_deref(),
                    Some(uuid.as_str()),
                    "{label}: a different neuron was reported removed"
                );
                assert!(
                    !result.creature.neurons.iter().any(|n| &n.uuid == uuid),
                    "{label}: the neuron was reported removed but is still in the creature"
                );
                assert!(
                    !result
                        .creature
                        .synapses
                        .iter()
                        .any(|s| &s.from_uuid == uuid || &s.to_uuid == uuid),
                    "{label}: an edge still names the removed neuron"
                );

                let again = prune_neuron(&fixture.creature, uuid, supplied)
                    .unwrap_or_else(|e| panic!("{label}: the second call refused: {e}"));
                assert_eq!(
                    result.creature, again.creature,
                    "{label}: the rewrite is not deterministic"
                );
                requests += 1;
            }
        }
    }

    assert!(
        swept_fixtures >= 8,
        "only {swept_fixtures} fixtures carried a hidden neuron — the sweep has gone vacuous"
    );
    assert_eq!(
        requests % 2,
        0,
        "{requests} neuron requests — each should run with and without statistics"
    );
    assert!(
        requests >= 2 * swept_fixtures,
        "{requests} neuron requests over {swept_fixtures} fixtures"
    );
    assert!(
        max_constants >= 2,
        "the most constants any pruned creature carried was {max_constants} — `inline/surplus_constants` should drive the {MAX_SUPPORT_CONSTANTS} cap, so the bound is not being exercised"
    );
}

/// Every listed synapse of every fixture prunes to `Ok` and validates.
#[test]
fn every_listed_synapse_of_every_fixture_prunes() {
    let fixtures = fixtures();
    let stats = mean_only(0.5);
    let mut requests = 0usize;
    let mut max_constants = 0usize;

    for fixture in &fixtures {
        let keys = synapse_keys(&fixture.creature);
        assert!(
            !keys.is_empty(),
            "{}: no synapse to sweep — the fixture has gone empty",
            fixture.name
        );
        for key in &keys {
            for supplied in [None, Some(&stats)] {
                let label = format!(
                    "{} synapse {} -> {} ({:?}, {})",
                    fixture.name,
                    key.from_uuid,
                    key.to_uuid,
                    key.role,
                    stats_label(supplied)
                );
                let result = prune_synapse(&fixture.creature, key, supplied).unwrap_or_else(|e| {
                    panic!("{label}: refused, but every listed synapse must prune: {e}")
                });

                max_constants = max_constants.max(assert_prune_contract(&label, &result.creature));
                assert!(
                    result.removed_neuron.is_none(),
                    "{label}: a synapse request reported a removed neuron"
                );
                // The request was carried out, not merely reported. A role the
                // rewrites may legitimately restore — a zero-weight support
                // edge into an emptied `IF` branch — makes "that triple is
                // absent" the wrong assertion, so the check is that the edge
                // was taken and the creature moved.
                assert!(
                    result
                        .removed_synapses
                        .iter()
                        .any(|s| s.from_uuid == key.from_uuid && s.to_uuid == key.to_uuid),
                    "{label}: the requested edge is not among the removed ones: {:?}",
                    result.removed_synapses
                );
                assert_ne!(
                    result.creature, fixture.creature,
                    "{label}: the creature came back unchanged"
                );

                let again = prune_synapse(&fixture.creature, key, supplied)
                    .unwrap_or_else(|e| panic!("{label}: the second call refused: {e}"));
                assert_eq!(
                    result.creature, again.creature,
                    "{label}: the rewrite is not deterministic"
                );
                requests += 1;
            }
        }
    }

    assert!(
        fixtures.len() >= 8,
        "only {} fixtures enumerated — the sweep has gone vacuous",
        fixtures.len()
    );
    assert!(
        requests >= 2 * fixtures.len(),
        "{requests} synapse requests over {} fixtures — each listed triple should run with and without statistics",
        fixtures.len()
    );
    assert!(
        max_constants >= 2,
        "the most constants any pruned creature carried was {max_constants} — `inline/surplus_constants` should drive the {MAX_SUPPORT_CONSTANTS} cap, so the bound is not being exercised"
    );
}

// --- structural guards ------------------------------------------------------

/// A three-deep hidden chain feeding one output collapses in **one** call.
///
/// `input-0 → h-1 → h-2 → h-3 → output-0`, with a direct `input-0 → output-0`
/// so the output stays fed. Cutting the last edge of the chain leaves `h-3`
/// with nothing to feed; removing it strands `h-2`, and removing that strands
/// `h-1` — so the whole chain goes in the single `prune_synapse` call, not one
/// neuron per call the caller has to iterate.
#[test]
fn three_deep_chain_collapses_in_one_synapse_prune() {
    let chain = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-3","bias":0.3,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-1","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"h-3"},
        {"weight":1.0,"fromUUID":"h-3","toUUID":"output-0"}
      ]
    }"#,
    );
    assert_valid("three-deep chain", &chain);

    let key = SynapseKey {
        from_uuid: "h-3".to_string(),
        to_uuid: "output-0".to_string(),
        role: SynapseType::Standard,
    };
    let result = prune_synapse(&chain, &key, None).expect("the last chain edge prunes");

    assert_valid("collapsed chain", &result.creature);
    assert_eq!(
        result.creature.neurons.len(),
        1,
        "the whole chain should have gone, leaving only the output: {:?}",
        result.creature.neurons
    );
    assert_eq!(result.creature.neurons[0].uuid, "output-0");
    assert_eq!(
        result.creature.synapses.len(),
        1,
        "only the direct input edge should survive: {:?}",
        result.creature.synapses
    );
    for uuid in ["h-1", "h-2", "h-3"] {
        assert!(
            result.cascade_neurons.iter().any(|u| u == uuid),
            "{uuid} was not reported on the cascade: {:?}",
            result.cascade_neurons
        );
    }
}

/// A hidden neuron left with no inward edge folds to a bias-1 support constant.
///
/// `h-1` keeps its outward edge, so it is not dead — but with nothing to sum
/// it is worth `LOGISTIC(0.4)` on every record. The cleanup makes that explicit:
/// the neuron becomes a **bias-1** support constant and the value it was worth
/// moves into the reading edge's weight, which is exact because the target
/// reads `activation · weight`. This is the fold `fold_zero_inward_hidden`
/// applies, pinned from the outside.
#[test]
fn zero_inward_hidden_folds_to_bias_one_support_constant() {
    let before = creature(
        r#"{
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
    );
    assert_valid("pre-fold", &before);

    let key = SynapseKey {
        from_uuid: "input-0".to_string(),
        to_uuid: "h-1".to_string(),
        role: SynapseType::Standard,
    };
    let result = prune_synapse(&before, &key, None).expect("the only inward edge prunes");
    assert_valid("folded", &result.creature);

    let folded = result
        .creature
        .neurons
        .iter()
        .find(|n| n.uuid == "h-1")
        .expect("the folded neuron survives as support");
    assert_eq!(
        folded.neuron_type, "constant",
        "a zero-inward hidden neuron should have become a constant"
    );
    assert_eq!(
        folded.bias, SUPPORT_CONSTANT_BIAS,
        "support constants carry bias {SUPPORT_CONSTANT_BIAS}"
    );
    assert!(
        folded.squash.is_none(),
        "a constant carries no squash: {:?}",
        folded.squash
    );
    assert!(
        result.folded_neurons.iter().any(|u| u == "h-1"),
        "the fold was not reported: {:?}",
        result.folded_neurons
    );

    // LOGISTIC(0.4) · 2.0 — the value the neuron was worth, times the weight
    // the target read it on, derived here from the formula rather than the code.
    let expected = (1.0 / (1.0 + (-0.4f64).exp())) * 2.0;
    let weight = result
        .creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == "h-1" && s.to_uuid == "output-0")
        .expect("the reading edge survives")
        .weight;
    assert!(
        (weight - expected).abs() <= STRUCTURAL_FOLD_TOL,
        "the folded value did not land in the reading weight: expected {expected}, got {weight}"
    );
}

/// Rule 17: a hidden neuron with no inward edge at all is invalid.
#[test]
fn isolated_hidden_neuron_fails_with_no_inward_connections() {
    let isolated = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-lonely","bias":0.1,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
    );
    let failure = creature_validate(&isolated, &OPTIONS)
        .expect_err("a hidden neuron wired to nothing is invalid");
    assert_eq!(failure.reason, "NO_INWARD_CONNECTIONS");
}

/// Rule 18: a hidden neuron with an inward edge but no outward edge is invalid.
#[test]
fn inward_only_hidden_neuron_fails_with_no_outward_connections() {
    let inward_only = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-dead","bias":0.1,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-dead"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
    );
    let failure = creature_validate(&inward_only, &OPTIONS)
        .expect_err("a hidden neuron nothing reads is invalid");
    assert_eq!(failure.reason, "NO_OUTWARD_CONNECTIONS");
}

/// Rule 16: a constant nothing reads is dead weight and invalid.
#[test]
fn outward_free_constant_fails_with_no_outward_connections() {
    let unread = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-unread","bias":1.0},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
    );
    let failure =
        creature_validate(&unread, &OPTIONS).expect_err("a constant nothing reads is invalid");
    assert_eq!(failure.reason, "NO_OUTWARD_CONNECTIONS");
}

/// An output neuron with no inward edge is **valid**.
///
/// The wiring rules bind hidden neurons and constants; an output is declared by
/// the creature's width and stays whatever the removals left it, so a prune that
/// strands one has not produced an invalid creature. Ockham's screen relies on
/// that: cutting the last edge into an output is an ordinary candidate.
#[test]
fn inward_free_output_is_valid() {
    let stranded = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":2,
      "neurons":[
        {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"},
        {"type":"output","uuid":"output-1","bias":0.5,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}
      ]
    }"#,
    );
    assert_valid("stranded output", &stranded);
}
