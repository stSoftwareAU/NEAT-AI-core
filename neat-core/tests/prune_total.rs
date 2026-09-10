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
//! So this file sweeps instead of sampling. Every fixture the prune tests
//! carry, every hidden neuron of it, every `(from, to, role)` triple it
//! lists — with no statistics and with a mean-only [`PruneStats`] — and for
//! each of those requests:
//!
//! - the call returns `Ok`; an `Err(PruneError::Cleanup)` on a **valid**
//!   fixture is a core defect, not a refusal a caller should code around;
//! - the creature that comes back passes `creature_validate` and the topology
//!   gate;
//! - it carries no more than [`MAX_SUPPORT_CONSTANTS`] constants, so a sweep
//!   cannot inflate the support prefix; and
//! - the rewrite is deterministic — the same request twice gives the same
//!   creature, byte for byte.
//!
//! Edges sourced at an `input-N` and edges targeting an output neuron are
//! ordinary candidates here, exactly as `prune_synapse`'s contract says: the
//! declared widths are untouched by dropping one term from a sum.
//!
//! The sweep is guarded against going vacuous. It asserts how many fixtures it
//! enumerated, that every fixture yielded at least one synapse request, and
//! that at least eight of them yielded at least one hidden-neuron request —
//! so a fixture table that silently stopped loading fails the test rather
//! than passing it with nothing to do.
//!
//! Below the sweep sit the structural guards: the three-deep chain that
//! collapses in one call, the zero-inward fold, and the four
//! `creature_validate` wiring rules (16, 17, 18) the whole contract rests on.

use neat_core::prune_fixtures::PRUNE_PARITY_CASES;
use neat_core::{
    CreatureExport, MAX_SUPPORT_CONSTANTS, PruneStats, SUPPORT_CONSTANT_BIAS, SynapseKey,
    SynapseType, ValidateOptions, creature_validate, parse_creature_json, parse_synapse_type,
    prune_neuron, prune_synapse, validate_creature_topology,
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

/// Floors on how much the two sweeps actually do, measured on the table as it
/// stands (102 neuron and 222 synapse requests) and rounded down.
///
/// A relative "twice the fixture count" check would still pass on a table that
/// had shrunk to one neuron and one edge per creature. These are absolute, so
/// coverage cannot quietly erode.
const MIN_NEURON_REQUESTS: usize = 100;
/// Companion floor for the synapse sweep — see [`MIN_NEURON_REQUESTS`].
const MIN_SYNAPSE_REQUESTS: usize = 200;

// --- fixtures ---------------------------------------------------------------
//
// The inline creatures are the ones `prune_neuron.rs` and `prune_synapse.rs`
// carry, transcribed unchanged. Only the **valid** ones are swept: those files
// also carry deliberately malformed creatures (a dangling target, an unknown
// squash, a backward edge) whose refusal is the behaviour under test there, and
// a refusal on an invalid creature says nothing about total prunability.

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

/// `h-1` feeds a `MINIMUM` aggregate as well as the output.
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

/// An `IDENTITY` neuron that sums nothing is worth `0.5` on every record.
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

/// `c-1` is a constant, so what it carried into the output is known exactly.
/// The one fixture here with no hidden neuron at all.
const CONSTANT_SOURCE_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"constant","uuid":"c-1","bias":0.5},
    {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"}
  ]
}"#;

/// `h-1` feeds two surviving targets; `h-2` also feeds the output.
const ORDINARY_JSON: &str = r#"{
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
    {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"},
    {"weight":0.25,"fromUUID":"input-0","toUUID":"output-0"}
  ]
}"#;

/// A live `IF`: `h-cond` is its only condition source and `h-a` feeds both
/// branches.
const IF_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-cond","bias":0.1,"squash":"TANH"},
    {"type":"hidden","uuid":"h-a","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.05,"squash":"IF"},
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
}"#;

/// An `IF` whose branches are fed by two sources each.
const IF_SHARED_BRANCHES_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-b","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-b"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"if-1","type":"condition"},
    {"weight":-1.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
    {"weight":2.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
    {"weight":-2.0,"fromUUID":"h-b","toUUID":"if-1","type":"negative"},
    {"weight":3.0,"fromUUID":"h-b","toUUID":"if-1","type":"positive"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// `h-c` is the `IF`'s only condition source, fed by exactly one edge.
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

/// The static-condition shape with the negative branch behind a two-step chain.
const IF_STATIC_MULTILEVEL_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-c","bias":0.5,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n2","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n","bias":0.4,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-p"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-n2"},
    {"weight":1.0,"fromUUID":"h-n2","toUUID":"h-n"},
    {"weight":1.0,"fromUUID":"h-c","toUUID":"if-1","type":"condition"},
    {"weight":2.0,"fromUUID":"h-p","toUUID":"if-1","type":"positive"},
    {"weight":-1.0,"fromUUID":"h-n","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// The static-condition shape with the positive arm written **untyped**.
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

/// An `IF` with a varying condition, in a creature already carrying support.
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

/// The inline creatures, paired with the name a failure reports.
const INLINE_FIXTURES: &[(&str, &str)] = &[
    ("two_targets", TWO_TARGETS_JSON),
    ("aggregate_target", AGGREGATE_TARGET_JSON),
    ("zero_inward", ZERO_INWARD_JSON),
    ("dead_neuron", DEAD_NEURON_JSON),
    ("discovery_fold", DISCOVERY_FOLD_JSON),
    ("constant_source", CONSTANT_SOURCE_JSON),
    ("ordinary", ORDINARY_JSON),
    ("if_live", IF_JSON),
    ("if_shared_branches", IF_SHARED_BRANCHES_JSON),
    ("if_static_after_cut", IF_STATIC_AFTER_CUT_JSON),
    ("if_static_multilevel", IF_STATIC_MULTILEVEL_JSON),
    ("if_static_untyped_arm", IF_STATIC_UNTYPED_ARM_JSON),
    ("if_with_support_constant", IF_WITH_SUPPORT_CONSTANT_JSON),
];

/// One creature to sweep, and the name a failure reports it by.
struct Fixture {
    name: String,
    creature: CreatureExport,
    /// Whether the fixture *itself* passes `creature_validate`.
    ///
    /// Most do. `inline/zero_inward` deliberately does not — its `h-1` has no
    /// inward edge, which is rule 17 — because the behaviour it pins next door
    /// is what a prune does with a neuron in that state. The sweep still runs
    /// it: a caller can hand core a creature straight out of a population, and
    /// the promise is about the creature that comes **back**. What the flag
    /// buys is the anti-vacuity guard in
    /// [`the_sweep_covers_valid_creatures`]: the sweep must be reaching real
    /// canonical creatures, not a table that has quietly filled with
    /// degenerate ones.
    canonical: bool,
}

/// Every fixture the sweep covers: the `before` of each captured parity case,
/// then the inline creatures above.
///
/// `neat-core/tests/fixtures/creature_validate/happy-paths.json` is **not**
/// here: its creatures are written in the index-addressed runtime shape
/// (`{"type":"input","id":0}` neurons, `{"from":0,"to":1}` synapses), which
/// `parse_creature_json` cannot deserialise into a `CreatureExport` at all —
/// `NeuronExport::uuid` and `SynapseExport::fromUUID` are both required. The
/// assumption on Ockham #195 admits them "only if they load through
/// `parse_creature_json`", and they do not.
fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    for case in PRUNE_PARITY_CASES {
        out.push(fixture(format!("parity/{}", case.name), case.before()));
    }
    for (name, json) in INLINE_FIXTURES {
        let creature = parse_creature_json(json)
            .unwrap_or_else(|e| panic!("inline fixture {name} does not parse: {e}"));
        out.push(fixture(format!("inline/{name}"), creature));
    }
    out
}

fn fixture(name: String, creature: CreatureExport) -> Fixture {
    let canonical = creature_validate(&creature, &OPTIONS).is_ok()
        && validate_creature_topology(&creature).is_ok();
    Fixture {
        name,
        creature,
        canonical,
    }
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

/// Everything the contract promises about one successful prune.
fn assert_prune_contract(name: &str, pruned: &CreatureExport) {
    assert_valid(name, pruned);
    let constants = constant_count(pruned);
    assert!(
        constants <= MAX_SUPPORT_CONSTANTS,
        "{name}: {constants} constants survived, above the {MAX_SUPPORT_CONSTANTS} cap"
    );
}

/// What the caller asked for, spelled for a failure message.
fn neuron_label(fixture: &str, uuid: &str, stats: Option<&PruneStats>) -> String {
    format!("{fixture} neuron {uuid} ({})", stats_label(stats))
}

fn synapse_label(fixture: &str, key: &SynapseKey, stats: Option<&PruneStats>) -> String {
    format!(
        "{fixture} synapse {} -> {} ({:?}, {})",
        key.from_uuid,
        key.to_uuid,
        key.role,
        stats_label(stats)
    )
}

fn stats_label(stats: Option<&PruneStats>) -> &'static str {
    if stats.is_some() {
        "mean-only stats"
    } else {
        "no stats"
    }
}

// --- the sweep --------------------------------------------------------------

/// The sweep is reaching real canonical creatures, and enough of them.
///
/// This is the anti-vacuity guard the whole file rests on: a fixture table that
/// stopped loading, or filled up with degenerate creatures, would let the two
/// sweeps below pass with nothing to prove. It pins the count of fixtures, the
/// count that pass `creature_validate` unaided, and — one fixture at a time —
/// that every fixture yields at least one synapse request and all but the
/// constant-only ones yield at least one hidden neuron.
#[test]
fn the_sweep_covers_valid_creatures() {
    let fixtures = fixtures();
    assert!(
        fixtures.len() >= 8,
        "only {} fixtures enumerated",
        fixtures.len()
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
    let mut fixtures_with_hidden = 0usize;

    for fixture in &fixtures {
        let hidden = hidden_uuids(&fixture.creature);
        if !hidden.is_empty() {
            fixtures_with_hidden += 1;
        }
        for uuid in &hidden {
            for supplied in [None, Some(&stats)] {
                let label = neuron_label(&fixture.name, uuid, supplied);
                let result = prune_neuron(&fixture.creature, uuid, supplied)
                    .unwrap_or_else(|e| panic!("{label}: refused, but every hidden neuron of a valid creature must prune: {e}"));
                assert_prune_contract(&label, &result.creature);
                assert_eq!(
                    result.removed_neuron.as_deref(),
                    Some(uuid.as_str()),
                    "{label}: a different neuron was reported removed"
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
        fixtures_with_hidden >= 8,
        "only {fixtures_with_hidden} fixtures carried a hidden neuron — the sweep has gone vacuous"
    );
    assert!(
        requests >= 2 * fixtures_with_hidden && requests >= MIN_NEURON_REQUESTS,
        "only {requests} neuron requests over {fixtures_with_hidden} fixtures — at least {MIN_NEURON_REQUESTS} are expected, each hidden neuron run with and without statistics"
    );
}

/// Every listed synapse of every fixture prunes to `Ok` and validates.
#[test]
fn every_listed_synapse_of_every_fixture_prunes() {
    let fixtures = fixtures();
    let stats = mean_only(0.5);
    let mut requests = 0usize;

    for fixture in &fixtures {
        let keys = synapse_keys(&fixture.creature);
        assert!(
            !keys.is_empty(),
            "{}: no synapse to sweep — the fixture has gone empty",
            fixture.name
        );
        for key in &keys {
            for supplied in [None, Some(&stats)] {
                let label = synapse_label(&fixture.name, key, supplied);
                let result = prune_synapse(&fixture.creature, key, supplied)
                    .unwrap_or_else(|e| panic!("{label}: refused, but every listed synapse of a valid creature must prune: {e}"));
                assert_prune_contract(&label, &result.creature);
                assert!(
                    result.removed_neuron.is_none(),
                    "{label}: a synapse request reported a removed neuron"
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
        requests >= 2 * fixtures.len() && requests >= MIN_SYNAPSE_REQUESTS,
        "only {requests} synapse requests over {} fixtures — at least {MIN_SYNAPSE_REQUESTS} are expected, each listed triple run with and without statistics",
        fixtures.len()
    );
}

// --- structural guards ------------------------------------------------------

/// A three-deep hidden chain feeding one output collapses in **one** call.
///
/// `input-0 → h-1 → h-2 → h-3 → output-0`, with a direct `input-0 → output-0`
/// so the output stays fed. Cutting the last edge of the chain leaves `h-3`
/// with nothing to feed; removing it strands `h-2`, and removing that strands
/// `h-1` — so the whole chain goes in the single `prune_synapse` call, not one
/// neuron per call the caller has to iterate (corner case 12).
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
