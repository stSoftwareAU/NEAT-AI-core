//! Hidden-neuron pruning, rule by rule (Issue #590).
//!
//! [`neat_core::prune_neuron`] is the shared entry point for "remove this
//! hidden neuron": it cuts the neuron out, optionally compensates the targets
//! that read it with the **caller's** statistics, runs the Issue #589 cleanup
//! to a fixed point, and validates the result before returning it.
//!
//! Three oracles do the work here, and none of them is a second copy of the
//! implementation:
//!
//! - **the function itself.** Where a rewrite claims to be
//!   [`TransformClass::Exact`] the pre-prune creature and the pruned one are
//!   compiled and activated, and must agree on every probe. Nothing but an
//!   exact rewrite can pass that.
//! - **arithmetic derived in the test.** Every fold, weight share and residual
//!   variance is computed here from the documented formula and the fixture's
//!   own numbers — never read back out of the code under test.
//! - **the TypeScript captures** in [`neat_core::PRUNE_PARITY_CASES`], which
//!   are NEAT-AI's own output for the same removals (Issue #588).

#[path = "common/prune_if.rs"]
mod prune_if;

use neat_core::prune_fixtures::{
    CASCADE_ORPHAN_FEEDERS, CONSTANT_BIAS_FOLD, IF_REPAIR_COALESCES_ROLES,
    MEMETIC_DROPPED_ON_REMOVAL,
};
use neat_core::{
    CreatureExport, ProtectedKind, ProxyStats, PruneError, PruneResult, PruneStats,
    SquashConversion, StaticIfRewrite, SynapseType, TransformClass, UncompensatedReason,
    ValidateOptions, compile_creature, creature_validate, parse_creature_json, prune_neuron,
    validate_creature_topology,
};
use prune_if::{IF_STATIC_CONDITION_JSON, OUTPUT_IF_JSON};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// `f32` activation slack: the creatures carry `f64` weights and the compiled
/// network computes in `f32`, so an exact comparison would fail on rounding.
const ACTIVATION_TOL: f32 = 1e-5;

/// `f64` slack for a fold derived in the test: `0.25 + 2.0 * 0.6` is the same
/// number by two different orders of operations only to within an ulp or two.
const FOLD_TOL: f64 = 1e-12;

/// Slack for an `IF` rewrite that claims to compute the same function. Tighter
/// than [`ACTIVATION_TOL`] because nothing is re-associated: the rewrite keeps
/// the surviving arm's own weights, so the only difference between the two
/// creatures is which arm the forward pass reads.
const REWRITE_TOL: f32 = 1e-6;

/// Relative slack for a rewrite that must compute the **same** number: the two
/// forward-pass arms it is measured across agree to `f32` rounding, and nothing
/// looser is being claimed (Ockham #197).
const CONVERSION_TOL: f32 = 1e-6;

/// Slack for a fold of a **structural** activation. That value is whatever the
/// forward pass computes, and the forward pass computes in `f32`, so the
/// `f64` logistic derived here agrees only to `f32` precision.
const STRUCTURAL_FOLD_TOL: f64 = 1e-6;

// --- fixtures ---------------------------------------------------------------

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

/// The hidden-neuron twin of [`CONSTANT_BIAS_FOLD`]: an `IDENTITY` neuron that
/// sums nothing is worth `0.5` on every record, exactly as that fixture's
/// constant is, and it feeds the same target on the same weight.
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

/// `h-s` survives `h-1`'s removal and already feeds the same target, so it can
/// carry the correlated part of what `h-1` contributed.
const PROXY_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-s","bias":0.2,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-s"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-s","toUUID":"output-0"}
  ]
}"#;

/// `h-1` feeds one `IDENTITY` target twice, on roles the target cannot read.
const DUPLICATE_ROWS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0","type":"positive"},
    {"weight":-0.5,"fromUUID":"h-1","toUUID":"output-0","type":"negative"}
  ]
}"#;

/// A memetic record with one entry the removal strands and one it does not.
const MIXED_MEMETIC_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.75,"fromUUID":"input-1","toUUID":"h-2"},
    {"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
  ],
  "memetic":{
    "generation":7,"score":0.5,
    "biases":{"h-1":0.05,"h-2":0.02},
    "weights":[
      {"fromUUID":"h-1","toUUID":"output-0","weight":2.0},
      {"fromUUID":"h-2","toUUID":"output-0","weight":1.0}
    ]
  }
}"#;

/// A neuron declaring a type outside `hidden | output | constant`.
const UNKNOWN_TYPE_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"spooky","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// A target declaring a squash this crate does not know.
const UNKNOWN_SQUASH_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"WOBBLE"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// A synapse naming a target the creature does not carry.
const DANGLING_TARGET_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"h-ghost"}
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

/// A `forwardOnly` creature whose `h-b -> h-a` edge points backwards: no
/// cleanup can repair it, because reordering the hidden slice is not cleanup's
/// to do.
const BACKWARD_EDGE_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-b","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-x","bias":0.3,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-b"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-x"},
    {"weight":1.0,"fromUUID":"h-b","toUUID":"h-a"},
    {"weight":1.0,"fromUUID":"h-a","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-x","toUUID":"output-0"}
  ]
}"#;

// --- helpers ----------------------------------------------------------------

fn creature(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("test fixture parses")
}

fn pruned(creature: &CreatureExport, uuid: &str, stats: Option<&PruneStats>) -> PruneResult {
    prune_neuron(creature, uuid, stats).unwrap_or_else(|e| panic!("pruning {uuid} failed: {e}"))
}

fn neuron<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a neat_core::NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"))
}

fn has_neuron(creature: &CreatureExport, uuid: &str) -> bool {
    creature.neurons.iter().any(|n| n.uuid == uuid)
}

fn weight(creature: &CreatureExport, from: &str, to: &str) -> f64 {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from && s.to_uuid == to)
        .unwrap_or_else(|| panic!("no synapse {from} -> {to}"))
        .weight
}

fn assert_close(name: &str, actual: f64, expected: f64) {
    assert_within(name, actual, expected, FOLD_TOL);
}

fn assert_within(name: &str, actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "{name}: expected {expected}, got {actual}"
    );
}

/// The records every same-function claim is graded on — five, well above the
/// three the issue asks for, so a claim that two creatures agree is never one
/// lucky reading.
const PROBE_SEEDS: [f32; 5] = [-1.5, -0.25, 0.0, 0.75, 2.0];

fn probe_inputs(width: usize) -> Vec<Vec<f32>> {
    PROBE_SEEDS
        .iter()
        .map(|s| (0..width).map(|i| s + i as f32 * 0.125).collect())
        .collect()
}

fn outputs(creature: &CreatureExport, inputs: &[f32]) -> Vec<f32> {
    let mut net = compile_creature(creature).expect("creature compiles");
    net.activate(inputs, creature.output)
}

/// Assert two creatures are the same function of the inputs — the oracle for
/// every [`TransformClass::Exact`] claim.
fn assert_same_function(name: &str, left: &CreatureExport, right: &CreatureExport) {
    assert_same_function_within(name, ACTIVATION_TOL, left, right);
}

/// [`assert_same_function`] at a caller's own relative tolerance.
fn assert_same_function_within(
    name: &str,
    tol: f32,
    left: &CreatureExport,
    right: &CreatureExport,
) {
    assert_eq!(left.input, right.input, "{name}: observation width moved");
    assert_eq!(left.output, right.output, "{name}: target width moved");
    for probe in probe_inputs(left.input) {
        let a = outputs(left, &probe);
        let b = outputs(right, &probe);
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert!(
                (x - y).abs() <= tol * (1.0 + x.abs()),
                "{name}: output {i} moved on {probe:?}: {x} vs {y}"
            );
        }
    }
}

/// Assert two creatures are **not** the same function — the guard against a
/// vacuous "unchanged" pass.
fn assert_different_function(name: &str, left: &CreatureExport, right: &CreatureExport) {
    let moved = probe_inputs(left.input).into_iter().any(|probe| {
        let a = outputs(left, &probe);
        let b = outputs(right, &probe);
        a.iter()
            .zip(b.iter())
            .any(|(x, y)| (x - y).abs() > ACTIVATION_TOL * (1.0 + x.abs()))
    });
    assert!(moved, "{name}: the two creatures agree on every probe");
}

/// The creature with every edge **out of** `uuid` zeroed.
///
/// Without statistics `prune_neuron` folds nothing into an aggregate target, so
/// removing a neuron takes its terms away entirely — which is exactly what a
/// zero weight on each of its outward edges does. The twin is a valid creature
/// the compiler accepts, so it can be activated where the half-cut creature
/// the rewrite actually repairs cannot.
fn with_source_zeroed(creature: &CreatureExport, uuid: &str) -> CreatureExport {
    let mut twin = creature.clone();
    let mut zeroed = 0usize;
    for synapse in &mut twin.synapses {
        if synapse.from_uuid == uuid {
            synapse.weight = 0.0;
            zeroed += 1;
        }
    }
    assert!(
        zeroed > 0,
        "{uuid} feeds nothing, so the twin proves nothing"
    );
    twin
}

fn assert_valid(name: &str, creature: &CreatureExport) {
    creature_validate(creature, &OPTIONS)
        .unwrap_or_else(|f| panic!("{name}: pruned creature is invalid: {f}"));
    validate_creature_topology(creature)
        .unwrap_or_else(|e| panic!("{name}: pruned creature failed the topology gate: {e}"));
}

/// The mean-only statistic a caller supplies when it measured no more.
fn mean_only(mean: f64) -> PruneStats {
    PruneStats {
        mean_activation: mean,
        variance: None,
        proxy: None,
    }
}

// --- protected targets ------------------------------------------------------

#[test]
fn an_observation_neuron_cannot_be_removed() {
    let before = creature(TWO_TARGETS_JSON);
    match prune_neuron(&before, "input-0", None) {
        Err(PruneError::Protected { uuid, kind }) => {
            assert_eq!(uuid, "input-0");
            assert_eq!(kind, ProtectedKind::Observation);
        }
        other => panic!("an observation neuron was not protected: {other:?}"),
    }
}

#[test]
fn an_output_neuron_cannot_be_removed() {
    let before = creature(TWO_TARGETS_JSON);
    match prune_neuron(&before, "output-0", None) {
        Err(PruneError::Protected { uuid, kind }) => {
            assert_eq!(uuid, "output-0");
            assert_eq!(kind, ProtectedKind::Output);
        }
        other => panic!("an output neuron was not protected: {other:?}"),
    }
}

#[test]
fn a_constant_cannot_be_removed_directly() {
    let before = CONSTANT_BIAS_FOLD.before();
    match prune_neuron(&before, "c-1", Some(&mean_only(0.5))) {
        Err(PruneError::Protected { uuid, kind }) => {
            assert_eq!(uuid, "c-1");
            assert_eq!(kind, ProtectedKind::Constant);
        }
        other => panic!("a constant was not protected from direct removal: {other:?}"),
    }
}

#[test]
fn a_uuid_the_creature_does_not_carry_is_refused() {
    let before = creature(TWO_TARGETS_JSON);
    match prune_neuron(&before, "h-nope", None) {
        Err(PruneError::UnknownNeuron { uuid }) => assert_eq!(uuid, "h-nope"),
        other => panic!("an unknown neuron was not refused: {other:?}"),
    }
}

// --- the requested removal and its cascade ----------------------------------

#[test]
fn the_result_names_the_requested_removal_and_every_edge_it_took() {
    let before = creature(TWO_TARGETS_JSON);
    let result = pruned(&before, "h-1", Some(&mean_only(0.6)));

    assert_eq!(result.removed_neuron.as_deref(), Some("h-1"));
    assert!(!has_neuron(&result.creature, "h-1"), "h-1 survived");

    let mut removed: Vec<String> = result
        .removed_synapses
        .iter()
        .map(|k| format!("{}->{}", k.from_uuid, k.to_uuid))
        .collect();
    removed.sort();
    assert_eq!(
        removed,
        vec![
            "h-1->h-2".to_string(),
            "h-1->output-0".to_string(),
            "input-0->h-1".to_string(),
        ],
        "the direct removal must name every edge that touched h-1"
    );
    assert!(
        result.cascade_neurons.is_empty(),
        "nothing else was orphaned: {:?}",
        result.cascade_neurons
    );
}

#[test]
fn removing_a_hidden_neuron_cascades_through_every_orphaned_feeder() {
    let case = CASCADE_ORPHAN_FEEDERS;
    let result = pruned(&case.before(), "h-x", None);

    assert_eq!(
        result.creature,
        case.after(),
        "the prune did not reproduce the TypeScript cascade"
    );
    let mut cascade = result.cascade_neurons.clone();
    cascade.sort();
    assert_eq!(cascade, vec!["h-a", "h-b", "h-c", "h-d"]);
    assert_eq!(
        result.transform,
        TransformClass::Approximate,
        "a neuron whose activation varies cannot be removed exactly"
    );
}

// --- IF repair: the exact rewrite, not the downgrade -------------------------

#[test]
fn an_if_left_short_of_a_role_is_rewritten_by_the_prune() {
    // Was `an_if_left_short_of_a_role_is_downgraded_by_the_prune`: the neuron
    // path asked cleanup for the TypeScript-parity downgrade, so this creature
    // came back as the capture's `IDENTITY` sum of both arms. It now asks for
    // `IfRepair::Rewrite`, the exact repair the synapse path already used, so
    // what it asserts is the rewrite. The capture itself is still pinned —
    // `prune_cleanup.rs::an_if_that_loses_a_required_role_is_downgraded_and_its_rows_are_summed`
    // drives `cleanup_creature`'s untouched default policy over the same cut.
    let case = IF_REPAIR_COALESCES_ROLES;
    let before = case.before();
    let result = pruned(&before, "h-cond", None);

    assert_eq!(
        result.static_if_neurons,
        vec![StaticIfRewrite {
            uuid: "if-1".to_string(),
            branch: SynapseType::Negative,
        }],
        "the condition the removal emptied is decided at 0, which is not > 0"
    );
    assert!(
        result.downgraded_if_neurons.is_empty(),
        "the neuron path must never downgrade an IF again: {:?}",
        result.downgraded_if_neurons
    );

    // The rewrite is exact against the cut, not against the creature that
    // still had a condition: `h-cond` decided the branch on every record and
    // it is what the caller asked to remove.
    let twin = with_source_zeroed(&before, "h-cond");
    assert_same_function_within("if_short_a_role", REWRITE_TOL, &twin, &result.creature);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IDENTITY"),
        "a statically-decided IF is the IDENTITY sum of the arm it always takes"
    );
    assert_close(
        "the negative arm survives at its own weight",
        weight(&result.creature, "h-a", "if-1"),
        -3.0,
    );
    assert_ne!(
        result.creature,
        case.after(),
        "the exact rewrite is a different creature from the parity downgrade — \
         if they have converged this test has stopped proving anything"
    );
    assert_eq!(
        result.transform,
        TransformClass::Approximate,
        "h-cond varies with the record, so the branch it decided is genuinely lost"
    );
    assert_valid("if_short_a_role", &result.creature);
}

#[test]
fn a_static_condition_feeder_that_leaves_the_branch_where_it_was_prunes_exactly() {
    // `h-c2` sums nothing, so the creature itself fixes its `-0.5` term. The
    // condition is `+0.5` with it and `+1.0` without it: positive either way,
    // so nothing the forward pass reads moves.
    let before = creature(IF_STATIC_CONDITION_JSON);
    let result = pruned(&before, "h-c2", None);

    assert_same_function_within(
        "static_condition_kept",
        REWRITE_TOL,
        &before,
        &result.creature,
    );
    assert_eq!(
        result.transform,
        TransformClass::Exact,
        "a structurally constant condition feeder that never moved the branch costs nothing"
    );
    assert_eq!(
        result.static_if_neurons,
        vec![StaticIfRewrite {
            uuid: "if-1".to_string(),
            branch: SynapseType::Positive,
        }]
    );
    assert!(result.downgraded_if_neurons.is_empty());
    assert!(
        !has_neuron(&result.creature, "h-n"),
        "the negative arm is unreachable, so its only source is dead structure"
    );
    assert_valid("static_condition_kept", &result.creature);
}

#[test]
fn a_static_condition_feeder_that_flips_the_branch_is_only_approximate() {
    // Same creature, the other condition feeder: `+0.5` becomes `-0.5`, so the
    // forward pass reads the negative arm where it used to read the positive
    // one. The rewrite is still the closest creature there is, and the label
    // has to say the output moved.
    let before = creature(IF_STATIC_CONDITION_JSON);
    let result = pruned(&before, "h-c1", None);

    assert_different_function("static_condition_flipped", &before, &result.creature);
    assert_eq!(
        result.transform,
        TransformClass::Approximate,
        "a removal that flips the branch is not exact, however constant the feeder was"
    );
    assert_eq!(
        result.static_if_neurons,
        vec![StaticIfRewrite {
            uuid: "if-1".to_string(),
            branch: SynapseType::Negative,
        }]
    );
    assert!(result.downgraded_if_neurons.is_empty());
    assert_valid("static_condition_flipped", &result.creature);
}

#[test]
fn emptying_a_branch_the_condition_never_reaches_prunes_exactly() {
    // `h-n` feeds only the negative arm, and the condition is `+0.5` on every
    // record, so nothing it contributed was ever read. The `IF` loses a role
    // and is flattened onto the arm it always took.
    let before = creature(IF_STATIC_CONDITION_JSON);
    let result = pruned(&before, "h-n", None);

    assert_same_function_within("unreachable_branch", REWRITE_TOL, &before, &result.creature);
    assert_eq!(
        result.transform,
        TransformClass::Exact,
        "an arm the condition never reaches costs nothing to remove"
    );
    assert!(result.downgraded_if_neurons.is_empty());
    assert_valid("unreachable_branch", &result.creature);
}

#[test]
fn an_output_carrying_the_if_squash_is_rewritten_in_place_by_a_neuron_prune() {
    // Corner case (6), the neuron half. The declared target width is the
    // fleet's contract, so an output can never be removed or reordered — an
    // `IF` output short a role has to be repaired where it stands. The synapse
    // half is `prune_synapse.rs::an_output_carrying_the_if_squash_is_rewritten_in_place`,
    // over the same fixture and the same two removals.
    let before = creature(OUTPUT_IF_JSON);

    // The condition source goes: the condition is empty, so it settles at 0,
    // which is not > 0, and the output flattens onto its negative arm.
    let flattened = pruned(&before, "h-cond", None);
    assert_eq!(
        flattened.static_if_neurons,
        vec![StaticIfRewrite {
            uuid: "output-0".to_string(),
            branch: SynapseType::Negative,
        }]
    );
    assert!(flattened.downgraded_if_neurons.is_empty());
    assert_eq!(
        flattened.creature.output, before.output,
        "the declared target width moved"
    );
    assert_eq!(
        flattened
            .creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "output")
            .count(),
        1,
        "the output block lost or gained a neuron"
    );
    let condition_twin = with_source_zeroed(&before, "h-cond");
    assert_same_function_within(
        "output_if_flattened",
        REWRITE_TOL,
        &condition_twin,
        &flattened.creature,
    );
    assert_valid("output_if_flattened", &flattened.creature);

    // A branch source goes instead: the condition still varies, so the output
    // still has to branch and the emptied arm is given back on a zero-weight
    // support edge.
    let restored = pruned(&before, "h-p", None);
    assert!(restored.static_if_neurons.is_empty());
    assert!(restored.downgraded_if_neurons.is_empty());
    assert_eq!(
        restored.restored_if_roles.len(),
        1,
        "{:?}",
        restored.restored_if_roles
    );
    assert_eq!(restored.restored_if_roles[0].to_uuid, "output-0");
    assert_eq!(restored.restored_if_roles[0].role, SynapseType::Positive);
    assert_eq!(
        neuron(&restored.creature, "output-0").squash.as_deref(),
        Some("IF"),
        "a varying condition still branches, so the output keeps its squash"
    );
    assert_eq!(
        restored.creature.output, before.output,
        "the declared target width moved"
    );
    let branch_twin = with_source_zeroed(&before, "h-p");
    assert_same_function_within(
        "output_if_restored",
        REWRITE_TOL,
        &branch_twin,
        &restored.creature,
    );
    assert_valid("output_if_restored", &restored.creature);
}

#[test]
fn the_memetic_record_is_pruned_of_the_structure_the_prune_removed() {
    let case = MEMETIC_DROPPED_ON_REMOVAL;
    let result = pruned(&case.before(), "h-x", None);

    let capture = case.after();
    assert_eq!(
        result.creature.neurons, capture.neurons,
        "the surviving neurons must match the capture"
    );
    assert_eq!(
        result.creature.synapses, capture.synapses,
        "the surviving synapses must match the capture"
    );

    // The deliberate divergence (Issue #590's call): core prunes the record
    // back to live structure — rule 31's inverse — where TypeScript drops it
    // wholesale. Nothing that names removed structure may survive.
    let memetic = result
        .creature
        .memetic
        .as_ref()
        .expect("the pruned record survives, emptied of dangling references");
    assert!(
        !memetic.biases.contains_key("h-x"),
        "the record still names the removed neuron"
    );
    assert_eq!(
        memetic.weights.rows().map(<[_]>::len),
        Some(0),
        "the record still names a removed synapse: {:?}",
        memetic.weights
    );
}

#[test]
fn a_memetic_entry_the_removal_does_not_strand_survives_the_prune() {
    // The other half of the pruned-not-dropped decision: an implementation
    // that emptied the record wholesale would pass the test above, so the
    // record here names both a neuron the removal deletes and one it does not.
    let before = creature(MIXED_MEMETIC_JSON);
    let result = pruned(&before, "h-1", Some(&mean_only(0.6)));

    let memetic = result
        .creature
        .memetic
        .as_ref()
        .expect("the record survives the prune");
    assert!(
        !memetic.biases.contains_key("h-1"),
        "the record still names the removed neuron"
    );
    assert_eq!(
        memetic.biases.get("h-2"),
        Some(&0.02),
        "the surviving neuron's fine-tuning history was thrown away"
    );

    let rows = memetic
        .weights
        .rows()
        .expect("the fixture carries the row form");
    assert_eq!(rows.len(), 1, "exactly the dangling row goes: {rows:?}");
    assert_eq!(rows[0].from_uuid.as_deref(), Some("h-2"));
    assert_eq!(rows[0].to_uuid.as_deref(), Some("output-0"));

    // And the history the record exists to carry is untouched.
    assert_eq!(
        memetic.extra.get("generation").and_then(|v| v.as_i64()),
        Some(7)
    );
}

// --- compensation: the mean bias fold ---------------------------------------

#[test]
fn the_supplied_mean_is_folded_into_every_target_that_read_the_neuron() {
    let before = creature(TWO_TARGETS_JSON);
    let mean = 0.6;
    let result = pruned(&before, "h-1", Some(&mean_only(mean)));

    // The documented fold: bias += Σ w · meanActivation, per target.
    assert_close(
        "h-2 bias",
        neuron(&result.creature, "h-2").bias,
        0.2 + 0.5 * mean,
    );
    assert_close(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * mean,
    );

    let mut folded: Vec<(String, f64)> = result
        .bias_folds
        .iter()
        .map(|f| (f.target_uuid.clone(), f.delta))
        .collect();
    folded.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(folded.len(), 2, "both targets take a fold: {folded:?}");
    assert_eq!(folded[0].0, "h-2");
    assert_close("h-2 delta", folded[0].1, 0.5 * mean);
    assert_eq!(folded[1].0, "output-0");
    assert_close("output-0 delta", folded[1].1, 2.0 * mean);

    assert_eq!(result.transform, TransformClass::Approximate);
    assert!(result.uncompensated.is_empty());
    assert_valid("mean fold", &result.creature);
}

#[test]
fn a_target_shared_with_a_survivor_keeps_the_survivors_own_edge() {
    let before = creature(TWO_TARGETS_JSON);
    let result = pruned(&before, "h-1", Some(&mean_only(0.6)));

    assert_close(
        "h-2 -> output-0",
        weight(&result.creature, "h-2", "output-0"),
        1.0,
    );
    assert_close(
        "input-1 -> h-2",
        weight(&result.creature, "input-1", "h-2"),
        0.75,
    );
}

#[test]
fn an_aggregate_target_is_reported_uncompensated_rather_than_folded() {
    let before = creature(AGGREGATE_TARGET_JSON);
    let result = pruned(&before, "h-1", Some(&mean_only(0.6)));

    // A `MINIMUM` takes the smallest of its inward terms; adding `w · mean` to
    // its bias does not replace the term the removal took away, so no fold is
    // attempted and the shortfall is named.
    assert_close(
        "h-agg bias",
        neuron(&result.creature, "h-agg").bias,
        0.2,
        // unchanged
    );
    assert_eq!(result.uncompensated.len(), 1);
    let shortfall = &result.uncompensated[0];
    assert_eq!(shortfall.target_uuid, "h-agg");
    assert_eq!(shortfall.reason, UncompensatedReason::AggregateTarget);
    assert_close("h-agg weight sum", shortfall.weight_sum, 0.5);

    // The point-wise target is still compensated.
    assert_close(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * 0.6,
    );
    assert_eq!(result.transform, TransformClass::Approximate);
    assert_valid("aggregate target", &result.creature);
}

#[test]
fn two_roles_into_one_if_target_are_reported_role_by_role() {
    // `h-a` feeds `if-1` twice — `positive` at 2.0 and `negative` at -3.0. An
    // `IF` never sums its arms, so the two are reported as the two terms the
    // target lost, not as a total it never computed.
    let before = IF_REPAIR_COALESCES_ROLES.before();
    let result = pruned(&before, "h-a", Some(&mean_only(0.5)));

    let mut lost: Vec<(SynapseType, f64)> = result
        .uncompensated
        .iter()
        .map(|u| {
            assert_eq!(u.target_uuid, "if-1");
            assert_eq!(u.reason, UncompensatedReason::AggregateTarget);
            assert_eq!(u.squash, "IF");
            (u.role, u.weight_sum)
        })
        .collect();
    lost.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(lost.len(), 2, "one entry per role: {lost:?}");
    assert_eq!(lost[0].0, SynapseType::Negative);
    assert_close("the negative arm", lost[0].1, -3.0);
    assert_eq!(lost[1].0, SynapseType::Positive);
    assert_close("the positive arm", lost[1].1, 2.0);

    assert_eq!(result.transform, TransformClass::Approximate);
    assert_valid("two roles", &result.creature);
}

#[test]
fn two_edges_into_one_summing_target_are_reported_as_one_term() {
    // Every squash but `IF` sums whatever reaches it, so two rows into one of
    // those *are* one term — reported once, at their summed weight. The roles
    // here are unreadable at an `IDENTITY` target, which is what makes the two
    // rows the same key.
    let before = creature(DUPLICATE_ROWS_JSON);
    let result = pruned(&before, "h-1", None);

    assert_eq!(result.uncompensated.len(), 1);
    let shortfall = &result.uncompensated[0];
    assert_eq!(shortfall.target_uuid, "output-0");
    assert_eq!(shortfall.role, SynapseType::Standard);
    assert_close("the summed term", shortfall.weight_sum, 2.0 - 0.5);
}

#[test]
fn without_statistics_no_bias_moves_and_the_shortfall_is_named() {
    let before = creature(TWO_TARGETS_JSON);
    let result = pruned(&before, "h-1", None);

    assert_close("h-2 bias", neuron(&result.creature, "h-2").bias, 0.2);
    assert_close(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.3,
    );
    assert!(result.bias_folds.is_empty());

    let mut named: Vec<String> = result
        .uncompensated
        .iter()
        .map(|u| {
            assert_eq!(u.reason, UncompensatedReason::NoStatistics);
            u.target_uuid.clone()
        })
        .collect();
    named.sort();
    assert_eq!(named, vec!["h-2".to_string(), "output-0".to_string()]);

    assert_eq!(result.transform, TransformClass::Approximate);
    // The exact structural half still ran: the result is canonical and valid.
    assert_valid("no statistics", &result.creature);
}

// --- compensation: the exact case -------------------------------------------

#[test]
fn a_neuron_that_sums_nothing_is_removed_exactly_without_statistics() {
    let before = creature(ZERO_INWARD_JSON);
    let result = pruned(&before, "h-1", None);

    // `LOGISTIC(0.4)`, computed here from the documented formula rather than
    // read out of the crate's own squash kernel.
    let logistic = 1.0 / (1.0 + (-0.4f64).exp());
    assert_within(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.0 + 2.0 * logistic,
        STRUCTURAL_FOLD_TOL,
    );
    assert_eq!(
        result.transform,
        TransformClass::Exact,
        "a neuron with nothing to sum activates to the same value on every record"
    );
    assert_eq!(result.bias_folds.len(), 1);
    assert!(result.bias_folds[0].exact);
    assert_eq!(result.bias_folds[0].residual_variance, Some(0.0));
    assert!(result.uncompensated.is_empty());

    assert_same_function("zero inward", &before, &result.creature);
    assert_valid("zero inward", &result.creature);
}

#[test]
fn a_structurally_constant_activation_outranks_a_supplied_mean() {
    let before = creature(ZERO_INWARD_JSON);
    let honest = pruned(&before, "h-1", None);
    // A caller's sample cannot beat a value the structure proves, so a wrong
    // mean must not move the result.
    let with_bogus_mean = pruned(&before, "h-1", Some(&mean_only(99.0)));

    assert_eq!(with_bogus_mean.creature, honest.creature);
    assert_eq!(with_bogus_mean.transform, TransformClass::Exact);
}

#[test]
fn a_neuron_nothing_reads_is_removed_exactly_and_compensates_nothing() {
    let before = creature(DEAD_NEURON_JSON);
    let result = pruned(&before, "h-dead", None);

    assert!(result.bias_folds.is_empty());
    assert!(result.uncompensated.is_empty());
    assert_eq!(
        result.transform,
        TransformClass::Exact,
        "removing structure nothing reads cannot change what the creature computes"
    );
    assert_same_function("dead neuron", &before, &result.creature);
    assert_valid("dead neuron", &result.creature);
}

#[test]
fn the_discovery_bias_fold_reproduces_the_typescript_capture() {
    // `CONSTANT_BIAS_FOLD` removes a *constant*, which `prune_neuron` protects,
    // so the captured rewrite is graded on its hidden-neuron twin: an
    // `IDENTITY` neuron that sums nothing is worth `0.5` on every record just
    // as that constant is, on the same weight into the same target.
    let before = creature(DISCOVERY_FOLD_JSON);
    let result = pruned(&before, "h-1", None);

    let capture = CONSTANT_BIAS_FOLD.after();
    assert_eq!(
        result.creature.synapses, capture.synapses,
        "the surviving wiring must match the capture"
    );
    assert_eq!(result.creature.neurons.len(), capture.neurons.len());
    assert_within(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        neuron(&capture, "output-0").bias,
        STRUCTURAL_FOLD_TOL,
    );
    assert_within(
        "the documented fold",
        neuron(&result.creature, "output-0").bias,
        0.25 + 0.2
            * CONSTANT_BIAS_FOLD
                .mean_activation
                .expect("the case carries the statistic"),
        STRUCTURAL_FOLD_TOL,
    );
    assert_eq!(result.transform, TransformClass::Exact);
    assert_same_function("discovery fold", &before, &result.creature);
}

// --- compensation: the covariance remedy ------------------------------------

#[test]
fn a_correlated_survivor_carries_the_part_of_the_neuron_it_predicts() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-s".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.3,
        }),
    };
    let result = pruned(&before, "h-1", Some(&stats));

    // β = cov / var(proxy) = 0.3 / 0.5 = 0.6 — the least-squares slope of the
    // removed neuron on the survivor. The target read `h-1` on weight 2.0, so
    // the survivor's edge takes `2.0 · β` and the bias takes what is left of
    // the mean: `2.0 · (0.5 − 0.6 · 0.4)`.
    let beta = 0.3 / 0.5;
    assert_close(
        "h-s -> output-0",
        weight(&result.creature, "h-s", "output-0"),
        1.0 + 2.0 * beta,
    );
    assert_close(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.0 + 2.0 * (0.5 - beta * 0.4),
    );

    assert_eq!(result.weight_shares.len(), 1);
    assert_eq!(result.weight_shares[0].from_uuid, "h-s");
    assert_eq!(result.weight_shares[0].to_uuid, "output-0");
    assert_close("the share", result.weight_shares[0].delta, 2.0 * beta);

    // Residual variance the compensation could not carry:
    // W² · (σ² − cov²/σ_s²) = 4 · (0.25 − 0.09/0.5) = 0.28.
    let residual = result.bias_folds[0]
        .residual_variance
        .expect("the caller supplied the variance the residual is derived from");
    assert_close("residual variance", residual, 4.0 * (0.25 - 0.09 / 0.5));
    assert_eq!(result.transform, TransformClass::Approximate);
    assert_valid("covariance remedy", &result.creature);
}

#[test]
fn the_mean_only_residual_is_the_whole_variance_the_neuron_carried() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: None,
    };
    let result = pruned(&before, "h-1", Some(&stats));

    assert!(result.weight_shares.is_empty());
    // With no proxy the fold removes the mean and leaves W² · σ² behind.
    assert_close(
        "residual variance",
        result.bias_folds[0]
            .residual_variance
            .expect("the caller supplied the variance"),
        4.0 * 0.25,
    );
}

#[test]
fn an_uncorrelated_proxy_moves_no_weight_and_needs_no_edge() {
    let before = creature(TWO_TARGETS_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: Some(0.1),
        proxy: Some(ProxyStats {
            // `h-2` does not feed itself, so an edge would be missing — but a
            // survivor that predicts nothing (`cov = 0`, so `β = 0`) carries
            // nothing, and needs no edge to carry it on.
            uuid: "h-2".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.0,
        }),
    };
    let result = pruned(&before, "h-1", Some(&stats));

    assert!(result.weight_shares.is_empty());
    // With β = 0 the fold is the plain mean fold.
    assert_close(
        "output-0 bias",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * 0.6,
    );
    assert_close(
        "h-2 bias",
        neuron(&result.creature, "h-2").bias,
        0.2 + 0.5 * 0.6,
    );
    assert_valid("uncorrelated proxy", &result.creature);
}

#[test]
fn a_proxy_that_does_not_already_feed_a_target_is_refused() {
    let before = creature(TWO_TARGETS_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        // cov² = 0.09 <= σ² σₛ² = 0.25, so these statistics are consistent and
        // the refusal below is about the missing edge, nothing else.
        variance: Some(0.5),
        proxy: Some(ProxyStats {
            // `h-2` feeds `output-0` but not itself, so it cannot carry what
            // `h-1` contributed to `h-2`.
            uuid: "h-2".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.3,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::MissingProxyEdge {
            from_uuid, to_uuid, ..
        }) => {
            assert_eq!(from_uuid, "h-2");
            assert_eq!(to_uuid, "h-2");
        }
        other => panic!("a proxy that cannot reach a target was not refused: {other:?}"),
    }
}

#[test]
fn a_proxy_that_never_varied_is_refused_rather_than_divided_by_zero() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-s".to_string(),
            mean_activation: 0.4,
            variance: 0.0,
            covariance: 0.3,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::DegenerateProxy { uuid, variance }) => {
            assert_eq!(uuid, "h-s");
            assert_eq!(variance, 0.0);
        }
        other => panic!("a zero-variance proxy was not refused: {other:?}"),
    }
}

#[test]
fn a_proxy_the_creature_does_not_carry_is_refused() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-nope".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.3,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::UnknownProxy { uuid }) => assert_eq!(uuid, "h-nope"),
        other => panic!("an unknown proxy was not refused: {other:?}"),
    }
}

#[test]
fn a_bad_proxy_is_refused_even_where_the_compensation_would_not_use_it() {
    // `h-1` sums nothing, so the fold is structural and the statistics are not
    // consulted — but a request naming a survivor the creature does not carry
    // is still a request this crate cannot make sense of, and must not quietly
    // succeed where the same input fails on every other neuron.
    let before = creature(ZERO_INWARD_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-nope".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.3,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::UnknownProxy { uuid }) => assert_eq!(uuid, "h-nope"),
        other => panic!("an unknown proxy slipped past the structural fold: {other:?}"),
    }
}

#[test]
fn a_proxy_that_is_the_neuron_being_removed_is_refused() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-1".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.3,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::UnknownProxy { uuid }) => assert_eq!(uuid, "h-1"),
        other => panic!("the removed neuron was accepted as its own proxy: {other:?}"),
    }
}

// --- statistics that are not numbers ----------------------------------------

#[test]
fn a_non_finite_mean_is_refused_before_anything_is_rewritten() {
    let before = creature(TWO_TARGETS_JSON);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        match prune_neuron(&before, "h-1", Some(&mean_only(bad))) {
            Err(PruneError::NonFiniteStatistic { field, .. }) => {
                assert_eq!(field, "mean_activation");
            }
            other => panic!("a non-finite mean was accepted: {other:?}"),
        }
    }
}

#[test]
fn a_negative_variance_is_refused() {
    let before = creature(TWO_TARGETS_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: Some(-0.5),
        proxy: None,
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::NegativeVariance { uuid, variance }) => {
            assert_eq!(uuid, "h-1");
            assert_eq!(variance, -0.5);
        }
        other => panic!("a negative variance was accepted: {other:?}"),
    }
}

#[test]
fn a_covariance_larger_than_the_variances_allow_is_refused() {
    let before = creature(PROXY_JSON);
    // |cov| <= sqrt(σ² σₛ²) = sqrt(0.25 · 0.5) ≈ 0.3536 for any two series
    // measured over the same records, so 0.9 cannot have been.
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-s".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: 0.9,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::InconsistentCovariance {
            uuid, covariance, ..
        }) => {
            assert_eq!(uuid, "h-s");
            assert_eq!(covariance, 0.9);
        }
        other => panic!("an impossible covariance was accepted: {other:?}"),
    }
}

#[test]
fn a_perfectly_correlated_survivor_is_accepted_and_carries_it_all() {
    let before = creature(PROXY_JSON);
    // cov = sqrt(σ² σₛ²) exactly — |ρ| = 1, the boundary of what a sample can
    // produce — so the remedy is accepted and the residual is zero.
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-s".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: (0.25f64 * 0.5).sqrt(),
        }),
    };
    let result = pruned(&before, "h-1", Some(&stats));
    assert_close(
        "residual variance",
        result.bias_folds[0]
            .residual_variance
            .expect("the caller supplied the variance"),
        0.0,
    );
}

#[test]
fn a_non_finite_proxy_statistic_is_refused() {
    let before = creature(PROXY_JSON);
    let stats = PruneStats {
        mean_activation: 0.5,
        variance: Some(0.25),
        proxy: Some(ProxyStats {
            uuid: "h-s".to_string(),
            mean_activation: 0.4,
            variance: 0.5,
            covariance: f64::NAN,
        }),
    };
    match prune_neuron(&before, "h-1", Some(&stats)) {
        Err(PruneError::NonFiniteStatistic { field, .. }) => assert_eq!(field, "covariance"),
        other => panic!("a non-finite covariance was accepted: {other:?}"),
    }
}

// --- fail closed ------------------------------------------------------------

#[test]
fn a_prune_that_cannot_be_made_valid_is_reported_not_returned() {
    let before = creature(BACKWARD_EDGE_JSON);
    // `h-b -> h-a` points backwards in a `forwardOnly` creature, and removing
    // `h-x` does not repair it: no creature may come back.
    match prune_neuron(&before, "h-x", None) {
        Err(PruneError::Cleanup(_)) => {}
        other => panic!("an unrepairable creature produced a result: {other:?}"),
    }
}

#[test]
fn a_creature_carrying_a_value_that_is_not_a_number_is_refused() {
    let mut before = creature(TWO_TARGETS_JSON);
    // An edge the removal does **not** take with it, so the value has to reach
    // the cleanup gate to be caught.
    before
        .synapses
        .iter_mut()
        .find(|s| s.from_uuid == "h-2" && s.to_uuid == "output-0")
        .expect("the fixture carries the surviving edge")
        .weight = f64::INFINITY;
    match prune_neuron(&before, "h-1", None) {
        Err(PruneError::Cleanup(_)) => {}
        other => panic!("a non-finite weight produced a result: {other:?}"),
    }
}

#[test]
fn a_neuron_declaring_an_unknown_type_is_refused() {
    let before = creature(UNKNOWN_TYPE_JSON);
    match prune_neuron(&before, "h-1", None) {
        Err(PruneError::UnknownNeuronType { uuid, declared }) => {
            assert_eq!(uuid, "h-1");
            assert_eq!(declared, "spooky");
        }
        other => panic!("an unknown neuron type was accepted: {other:?}"),
    }
}

#[test]
fn a_target_declaring_an_unknown_squash_is_refused() {
    let before = creature(UNKNOWN_SQUASH_JSON);
    // The compensation would have to know what the target computes; it does
    // not, so nothing is rewritten.
    match prune_neuron(&before, "h-1", Some(&mean_only(0.5))) {
        Err(PruneError::Cleanup(_)) => {}
        other => panic!("an unknown squash was accepted: {other:?}"),
    }
}

#[test]
fn a_synapse_naming_a_target_that_does_not_exist_is_refused() {
    let before = creature(DANGLING_TARGET_JSON);
    match prune_neuron(&before, "h-1", Some(&mean_only(0.5))) {
        Err(PruneError::Cleanup(_)) => {}
        other => panic!("a dangling target was accepted: {other:?}"),
    }
}

// --- shared invariants ------------------------------------------------------

/// Every prune this suite can run, as `(name, creature, uuid, stats)`.
fn scenarios() -> Vec<(
    &'static str,
    CreatureExport,
    &'static str,
    Option<PruneStats>,
)> {
    vec![
        (
            "two_targets",
            creature(TWO_TARGETS_JSON),
            "h-1",
            Some(mean_only(0.6)),
        ),
        ("two_targets_bare", creature(TWO_TARGETS_JSON), "h-1", None),
        (
            "aggregate_target",
            creature(AGGREGATE_TARGET_JSON),
            "h-1",
            Some(mean_only(0.6)),
        ),
        ("zero_inward", creature(ZERO_INWARD_JSON), "h-1", None),
        ("dead_neuron", creature(DEAD_NEURON_JSON), "h-dead", None),
        ("cascade", CASCADE_ORPHAN_FEEDERS.before(), "h-x", None),
        (
            "if_repair",
            IF_REPAIR_COALESCES_ROLES.before(),
            "h-cond",
            None,
        ),
        ("discovery_fold", creature(DISCOVERY_FOLD_JSON), "h-1", None),
    ]
}

#[test]
fn every_successful_prune_returns_a_creature_both_shared_gates_accept() {
    for (name, before, uuid, stats) in scenarios() {
        let result = pruned(&before, uuid, stats.as_ref());
        assert_valid(name, &result.creature);
        assert!(
            !has_neuron(&result.creature, uuid),
            "{name}: the requested neuron survived"
        );
    }
}

#[test]
fn pruning_the_same_creature_twice_gives_the_same_creature() {
    for (name, before, uuid, stats) in scenarios() {
        let first = pruned(&before, uuid, stats.as_ref());
        let second = pruned(&before, uuid, stats.as_ref());
        assert_eq!(
            first.creature, second.creature,
            "{name}: prune is not deterministic"
        );
        assert_eq!(
            first.transform, second.transform,
            "{name}: class is not deterministic"
        );
    }
}

#[test]
fn an_exact_prune_is_always_the_same_function_of_the_inputs() {
    let mut exact_seen = 0;
    for (name, before, uuid, stats) in scenarios() {
        let result = pruned(&before, uuid, stats.as_ref());
        if result.transform == TransformClass::Exact {
            exact_seen += 1;
            assert_same_function(name, &before, &result.creature);
        }
    }
    assert!(
        exact_seen >= 3,
        "the sweep must actually arm the exact branch, saw {exact_seen}"
    );
}

// --- aggregates the removal leaves with inward edges (Ockham #197) -----------

/// `h-1` feeds `h-agg` and the output; `h-agg` also reads `input-1`, so
/// removing `h-1` leaves the aggregate with exactly **one** inward edge.
///
/// Every source is an `IDENTITY`, so what the creature computes is plain
/// arithmetic a test can derive.
fn single_edge_aggregate_json(squash: &str, bias: f64) -> String {
    format!(
        r#"{{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {{"type":"hidden","uuid":"h-1","bias":0.1,"squash":"IDENTITY"}},
        {{"type":"hidden","uuid":"h-agg","bias":{bias},"squash":"{squash}"}},
        {{"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}}
      ],
      "synapses":[
        {{"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"}},
        {{"weight":0.75,"fromUUID":"input-1","toUUID":"h-agg"}},
        {{"weight":0.5,"fromUUID":"h-1","toUUID":"h-agg"}},
        {{"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}},
        {{"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"}}
      ]
    }}"#
    )
}

/// The creature the removal leaves, built **by hand** and still declaring the
/// aggregate squash — the oracle a conversion is graded against.
///
/// `h-1` and every edge naming it go; `h-agg` keeps `input-1 → h-agg`. Reached
/// by a plain `retain`, never by the code under test, and it runs down a
/// *different* forward-pass arm than the converted creature does.
fn removal_by_hand(json: &str, uuid: &str) -> CreatureExport {
    let mut twin = creature(json);
    twin.neurons.retain(|n| n.uuid != uuid);
    twin.synapses
        .retain(|s| s.from_uuid != uuid && s.to_uuid != uuid);
    twin
}

fn squash_of(creature: &CreatureExport, uuid: &str) -> String {
    neuron(creature, uuid)
        .squash
        .clone()
        .unwrap_or_else(|| "IDENTITY".to_string())
}

#[test]
fn an_aggregate_the_removal_leaves_with_one_edge_becomes_point_wise() {
    // The same table `prune_synapse` is graded on, reached through the neuron
    // entry point: both halves of the request surface run the conversion.
    for (squash, bias, expected) in [
        ("MINIMUM", 0.2, "IDENTITY"),
        ("MAXIMUM", 0.2, "IDENTITY"),
        ("MEAN", 0.2, "IDENTITY"),
        ("HYPOT", 0.0, "ABSOLUTE"),
        ("HYPOTv2", -0.35, "ABSOLUTE"),
    ] {
        let json = single_edge_aggregate_json(squash, bias);
        let result = pruned(&creature(&json), "h-1", None);

        assert_valid(squash, &result.creature);
        assert_eq!(
            squash_of(&result.creature, "h-agg"),
            expected,
            "{squash} left with one edge was not rewritten"
        );
        assert_eq!(
            result.converted_neurons,
            vec![SquashConversion {
                uuid: "h-agg".to_string(),
                from: squash,
                to: expected,
            }],
            "{squash}: the conversion was not reported"
        );
        assert_close(
            &format!("{squash}: the bias is carried over unchanged"),
            neuron(&result.creature, "h-agg").bias,
            bias,
        );
        assert_same_function_within(
            squash,
            CONVERSION_TOL,
            &result.creature,
            &removal_by_hand(&json, "h-1"),
        );
    }
}

#[test]
fn a_hypot_adding_a_non_zero_bias_survives_a_neuron_removal_unrewritten() {
    let json = single_edge_aggregate_json("HYPOT", 0.2);
    let result = pruned(&creature(&json), "h-1", None);

    assert_eq!(
        squash_of(&result.creature, "h-agg"),
        "HYPOT",
        "a HYPOT that adds a non-zero bias was rewritten anyway"
    );
    assert!(
        result.converted_neurons.is_empty(),
        "{:?}",
        result.converted_neurons
    );
    assert_same_function_within(
        "hypot at a non-zero bias",
        CONVERSION_TOL,
        &result.creature,
        &removal_by_hand(&json, "h-1"),
    );
}

#[test]
fn the_dropped_term_magnitude_crosses_with_the_uncompensated_target() {
    // `h-agg` reads `input-1` and `h-1`, so removing `h-1` costs it `0.5 · a`
    // and the caller's mean is what says how much that was worth.
    let json = single_edge_aggregate_json("MEAN", 0.2);
    let supplied = pruned(&creature(&json), "h-1", Some(&mean_only(0.6)));

    let entry = supplied
        .uncompensated
        .iter()
        .find(|u| u.target_uuid == "h-agg")
        .expect("the aggregate is named");
    assert_eq!(entry.reason, UncompensatedReason::AggregateTarget);
    assert_close(
        "the dropped term's magnitude",
        entry
            .dropped_mean
            .expect("a supplied mean names a magnitude"),
        0.5 * 0.6,
    );

    // Without a statistic there is no number, and none is invented.
    let bare = pruned(&creature(&json), "h-1", None);
    let entry = bare
        .uncompensated
        .iter()
        .find(|u| u.target_uuid == "h-agg")
        .expect("the aggregate is named");
    assert_eq!(entry.dropped_mean, None);

    // A neuron the creature itself fixes names the magnitude with no statistic:
    // `ZERO_INWARD_JSON`'s `h-1` sums nothing, so `w · LOGISTIC(0.4)` is the
    // creature's own to prove.
    let logistic = 1.0 / (1.0 + (-0.4f64).exp());
    let fixed = pruned(&creature(ZERO_INWARD_AGGREGATE_JSON), "h-1", None);
    let entry = fixed
        .uncompensated
        .iter()
        .find(|u| u.target_uuid == "h-agg")
        .expect("the aggregate is named");
    assert_eq!(entry.reason, UncompensatedReason::AggregateTarget);
    assert_within(
        "w · the value the creature fixes",
        entry
            .dropped_mean
            .expect("a fixed source names a magnitude"),
        0.5 * logistic,
        STRUCTURAL_FOLD_TOL,
    );

    // Reporting a magnitude is not a licence to return something invalid, and
    // reporting is all that happened: the creature the caller gets back is the
    // one the aggregate form of the same cut computes.
    assert_valid("supplied_mean", &supplied.creature);
    assert_valid("bare", &bare.creature);
    assert_valid("fixed", &fixed.creature);
    assert_same_function_within(
        "reporting the magnitude changed nothing else",
        CONVERSION_TOL,
        &bare.creature,
        &removal_by_hand(&json, "h-1"),
    );
}

/// `h-1` has nothing to sum, so it activates to `LOGISTIC(0.4)` on every record
/// — and it feeds an aggregate, which takes no fold for it.
const ZERO_INWARD_AGGREGATE_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.4,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-agg","bias":0.2,"squash":"MEAN"},
    {"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":0.75,"fromUUID":"input-0","toUUID":"h-agg"},
    {"weight":0.25,"fromUUID":"input-1","toUUID":"h-agg"},
    {"weight":0.5,"fromUUID":"h-1","toUUID":"h-agg"},
    {"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"}
  ]
}"#;

#[test]
fn an_aggregate_left_reducing_two_terms_keeps_its_squash() {
    let result = pruned(&creature(ZERO_INWARD_AGGREGATE_JSON), "h-1", None);

    assert_eq!(
        squash_of(&result.creature, "h-agg"),
        "MEAN",
        "an aggregate still reducing two terms was rewritten"
    );
    assert!(
        result.converted_neurons.is_empty(),
        "{:?}",
        result.converted_neurons
    );
    assert_eq!(result.transform, TransformClass::Approximate);
    assert_valid("two_terms_left", &result.creature);
}

#[test]
fn no_dropped_magnitude_refuses_a_neuron_removal() {
    // `|W · μ| = 1.2e9` against a creature whose other terms are fractions is
    // still answered: the scorer judges the loss, this crate reports it.
    let huge = mean_only(6e8);
    let result = pruned(&creature(TWO_TARGETS_JSON), "h-1", Some(&huge));

    assert_within(
        "the whole fold lands in the bias",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * 6e8,
        1e-6,
    );
    assert_eq!(result.transform, TransformClass::Approximate);
}
