//! Synapse pruning with typed-role and `IF`-aware rewrites (Issue #591).
//!
//! [`neat_core::prune_synapse`] removes **one** typed edge — the
//! `(from, to, role)` triple — compensates the target that read it when the
//! caller's statistics allow, rewrites whatever `IF` structure the removal made
//! statically determined, runs the Issue #589 cleanup fixed point, and
//! validates the stable result before returning it.
//!
//! Three oracles do the work here, and none is a second copy of the
//! implementation:
//!
//! - **a zero-weight twin.** Every `IF` rewrite claims to compute the same
//!   number as the creature it started from. The test builds that creature
//!   independently — the *original* with the removed edge's weight set to `0`,
//!   which is the same function as the original without that edge — compiles
//!   both, and activates them. Only a genuinely exact rewrite passes.
//! - **arithmetic derived in the test.** Every fold, weight share and residual
//!   variance is computed here from the documented formula and the fixture's
//!   own numbers, never read back out of the code under test.
//! - **the TypeScript captures** in [`neat_core::PRUNE_PARITY_CASES`] — NEAT-AI's
//!   own output for the same removals (Issue #588).

use neat_core::prune_fixtures::{
    CASCADE_ORPHAN_FEEDERS, CONSTANT_MOVES_INTO_PREFIX, EDGE_ROLE_IDENTITY,
    EDGE_SOURCE_BECOMES_DEAD, EDGE_TARGET_BECOMES_CONSTANT,
};
use neat_core::{
    CleanupError, CleanupOptions, CreatureExport, IfRepair, ProxyStats, PruneError, PruneResult,
    PruneStats, SynapseKey, SynapseType, TransformClass, UncompensatedReason, ValidateOptions,
    cleanup_creature, cleanup_creature_with, compile_creature, creature_validate,
    parse_creature_json, prune_synapse, validate_creature_topology,
};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// `f32` activation slack: the creatures carry `f64` weights and the compiled
/// network computes in `f32`, so an exact comparison would fail on rounding.
const ACTIVATION_TOL: f32 = 1e-5;

/// `f64` slack for a fold derived in the test: `0.3 + 2.0 * 0.6` is the same
/// number by two different orders of operations only to within an ulp or two.
const FOLD_TOL: f64 = 1e-12;

/// Slack for a fold of a **structural** activation, which is whatever the
/// forward pass computes — and it computes in `f32`.
const STRUCTURAL_FOLD_TOL: f64 = 1e-6;

// --- fixtures ---------------------------------------------------------------

/// `h-1` feeds two surviving targets; `h-2` also feeds the output, so it can
/// stand as a correlated survivor.
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

/// `c-1` is a constant, so what it carried into the output is known exactly
/// from the creature alone.
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

/// `h-agg` is a `MINIMUM`, which reads its smallest inward term rather than a
/// sum, so no bias fold stands in for what the removal took.
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

/// A live `IF`: `h-cond` is its only condition source, `h-a` feeds both
/// branches, and `input-0` reaches the condition so the branch is genuinely
/// dynamic.
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

/// An `IF` whose branches are fed by two sources each, so removing one edge
/// leaves the role populated and nothing is rewritten.
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

/// `h-c` is the `IF`'s only condition source and is fed by exactly one edge.
/// Cutting that edge makes `h-c` structurally constant, so the condition sum
/// is `0.5 > 0` on every record and the negative branch is unreachable.
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

/// The same static-condition shape, with the negative branch fed through a
/// two-step chain so dropping it strands a whole line of feeders.
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

/// The static-condition shape again, with the positive arm written as an
/// **untyped** edge. `IfRoles::tally` and the forward pass both read an untyped
/// inward edge as the positive role, so a flatten onto the positive branch has
/// to keep it.
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

/// A synapse naming a target the creature does not carry, on an edge the
/// request does not touch.
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

// --- helpers ----------------------------------------------------------------

fn creature(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("test fixture parses")
}

fn key(from: &str, to: &str, role: SynapseType) -> SynapseKey {
    SynapseKey {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        role,
    }
}

fn pruned(creature: &CreatureExport, key: &SynapseKey, stats: Option<&PruneStats>) -> PruneResult {
    prune_synapse(creature, key, stats).unwrap_or_else(|e| {
        panic!(
            "pruning {} -> {} ({:?}) failed: {e}",
            key.from_uuid, key.to_uuid, key.role
        )
    })
}

fn has_neuron(creature: &CreatureExport, uuid: &str) -> bool {
    creature.neurons.iter().any(|n| n.uuid == uuid)
}

fn neuron<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a neat_core::NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"))
}

fn weight(creature: &CreatureExport, from: &str, to: &str) -> f64 {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from && s.to_uuid == to)
        .unwrap_or_else(|| panic!("no synapse {from} -> {to}"))
        .weight
}

fn role_weight(creature: &CreatureExport, from: &str, to: &str, role: Option<&str>) -> f64 {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from && s.to_uuid == to && s.synapse_type.as_deref() == role)
        .unwrap_or_else(|| panic!("no synapse {from} -> {to} ({role:?})"))
        .weight
}

fn has_edge(creature: &CreatureExport, from: &str, to: &str) -> bool {
    creature
        .synapses
        .iter()
        .any(|s| s.from_uuid == from && s.to_uuid == to)
}

/// The creature the removal is *equivalent to*: the original with the removed
/// edge's weight set to `0`.
///
/// A term of `0 · a` contributes nothing to any sum the forward pass takes, so
/// this is the same function of the inputs as the original without that edge —
/// derived from the creature, never from the code under test.
fn with_edge_zeroed(
    creature: &CreatureExport,
    from: &str,
    to: &str,
    role: Option<&str>,
) -> CreatureExport {
    let mut twin = creature.clone();
    let mut found = false;
    for synapse in &mut twin.synapses {
        if synapse.from_uuid == from
            && synapse.to_uuid == to
            && synapse.synapse_type.as_deref() == role
        {
            synapse.weight = 0.0;
            found = true;
        }
    }
    assert!(found, "no synapse {from} -> {to} ({role:?}) to zero");
    twin
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

fn probe_inputs(width: usize) -> Vec<Vec<f32>> {
    let seeds: [f32; 5] = [-1.5, -0.25, 0.0, 0.75, 2.0];
    seeds
        .iter()
        .map(|s| (0..width).map(|i| s + i as f32 * 0.125).collect())
        .collect()
}

fn outputs(creature: &CreatureExport, inputs: &[f32]) -> Vec<f32> {
    let mut net = compile_creature(creature).expect("creature compiles");
    net.activate(inputs, creature.output)
}

/// Assert two creatures are the same function of the inputs.
fn assert_same_function(name: &str, left: &CreatureExport, right: &CreatureExport) {
    assert_eq!(left.input, right.input, "{name}: observation width moved");
    assert_eq!(left.output, right.output, "{name}: target width moved");
    for probe in probe_inputs(left.input) {
        let a = outputs(left, &probe);
        let b = outputs(right, &probe);
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert!(
                (x - y).abs() <= ACTIVATION_TOL * (1.0 + x.abs()),
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

fn assert_valid(name: &str, creature: &CreatureExport) {
    creature_validate(creature, &OPTIONS)
        .unwrap_or_else(|f| panic!("{name}: pruned creature is invalid: {f}"));
    validate_creature_topology(creature)
        .unwrap_or_else(|e| panic!("{name}: pruned creature failed the topology gate: {e}"));
}

fn mean_only(mean: f64) -> PruneStats {
    PruneStats {
        mean_activation: mean,
        variance: None,
        proxy: None,
    }
}

// --- the requested edge, and only it -----------------------------------------

#[test]
fn only_the_requested_role_of_a_repeated_pair_is_removed() {
    let before = EDGE_ROLE_IDENTITY.before();
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Negative), None);

    assert_eq!(
        result.creature,
        EDGE_ROLE_IDENTITY.after(),
        "the typed removal did not reproduce the TypeScript capture"
    );
    assert_close(
        "the surviving positive row",
        role_weight(&result.creature, "h-a", "if-1", Some("positive")),
        2.0,
    );
    assert!(
        !result.creature.synapses.iter().any(|s| s.from_uuid == "h-a"
            && s.to_uuid == "if-1"
            && s.synapse_type.as_deref() == Some("negative")),
        "the requested negative role survived"
    );
    assert_valid("only_the_requested_role", &result.creature);
}

#[test]
fn the_result_names_the_edge_it_removed_and_no_neuron() {
    let before = creature(ORDINARY_JSON);
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        None,
    );

    assert_eq!(result.removed_neuron, None);
    assert_eq!(
        result.removed_synapses,
        vec![key("h-1", "output-0", SynapseType::Standard)]
    );
    assert!(!has_edge(&result.creature, "h-1", "output-0"));
    assert!(has_neuron(&result.creature, "h-1"), "h-1 still feeds h-2");
    assert_valid("the_result_names_the_edge", &result.creature);
}

#[test]
fn a_role_the_pair_does_not_carry_is_refused() {
    let before = EDGE_ROLE_IDENTITY.before();
    match prune_synapse(&before, &key("h-a", "if-1", SynapseType::Condition), None) {
        Err(PruneError::UnknownSynapse {
            from_uuid,
            to_uuid,
            role,
        }) => {
            assert_eq!(from_uuid, "h-a");
            assert_eq!(to_uuid, "if-1");
            assert_eq!(role, SynapseType::Condition);
        }
        other => panic!("an absent role was not refused: {other:?}"),
    }
}

#[test]
fn a_pair_the_creature_does_not_carry_is_refused() {
    let before = creature(ORDINARY_JSON);
    match prune_synapse(&before, &key("h-2", "h-1", SynapseType::Standard), None) {
        Err(PruneError::UnknownSynapse {
            from_uuid, to_uuid, ..
        }) => {
            assert_eq!(from_uuid, "h-2");
            assert_eq!(to_uuid, "h-1");
        }
        other => panic!("an absent pair was not refused: {other:?}"),
    }
}

#[test]
fn a_target_the_creature_does_not_carry_is_refused() {
    let before = creature(DANGLING_TARGET_JSON);
    match prune_synapse(&before, &key("h-1", "h-ghost", SynapseType::Standard), None) {
        Err(PruneError::Cleanup(CleanupError::UnknownEndpoint { uuid })) => {
            assert_eq!(uuid, "h-ghost");
        }
        other => panic!("a dangling target was not refused: {other:?}"),
    }
}

// --- input-sourced and output-target edges are candidates --------------------

#[test]
fn an_input_sourced_edge_into_an_output_is_a_candidate() {
    let before = creature(ORDINARY_JSON);
    let result = pruned(
        &before,
        &key("input-0", "output-0", SynapseType::Standard),
        None,
    );

    assert!(!has_edge(&result.creature, "input-0", "output-0"));
    assert_eq!(result.creature.input, 2, "the declared width moved");
    assert_different_function("input_edge_removed", &before, &result.creature);
    assert_valid("an_input_sourced_edge", &result.creature);
}

#[test]
fn an_input_sourced_edge_into_a_hidden_neuron_is_a_candidate() {
    let before = EDGE_TARGET_BECOMES_CONSTANT.before();
    let result = pruned(&before, &key("input-0", "h-1", SynapseType::Standard), None);

    // TypeScript keeps the folded value in the constant's bias; this crate
    // holds the support-node invariants and keeps it in the weights, so the
    // capture is graded on the numbers rather than the bytes.
    assert_same_function(
        "edge_target_becomes_constant",
        &EDGE_TARGET_BECOMES_CONSTANT.after(),
        &result.creature,
    );
    assert_eq!(
        neuron(&result.creature, "h-1").neuron_type,
        "constant",
        "the target kept its outward edge but lost its last inward one"
    );
    assert_valid("an_input_sourced_edge_into_a_hidden", &result.creature);
}

#[test]
fn a_hidden_to_output_edge_is_a_candidate() {
    let before = creature(ORDINARY_JSON);
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        None,
    );

    assert!(!has_edge(&result.creature, "h-1", "output-0"));
    assert!(has_neuron(&result.creature, "h-1"), "h-1 still feeds h-2");
    assert!(
        has_edge(&result.creature, "h-1", "h-2"),
        "h-1's other edge stays"
    );
    assert_different_function("hidden_to_output_removed", &before, &result.creature);
    assert_valid("a_hidden_to_output_edge", &result.creature);
}

#[test]
fn losing_a_hidden_neurons_last_outgoing_edge_removes_it_and_its_feeders() {
    let before = creature(ORDINARY_JSON);
    let result = pruned(
        &before,
        &key("h-2", "output-0", SynapseType::Standard),
        None,
    );

    assert!(
        !has_neuron(&result.creature, "h-2"),
        "nothing reads h-2 now"
    );
    assert!(!has_edge(&result.creature, "input-1", "h-2"));
    assert!(!has_edge(&result.creature, "h-1", "h-2"));
    assert!(
        has_neuron(&result.creature, "h-1"),
        "h-1 still feeds the output, so the cascade stops at h-2"
    );
    assert_eq!(result.cascade_neurons, vec!["h-2".to_string()]);
    assert_valid(
        "losing_a_hidden_neurons_last_outgoing_edge",
        &result.creature,
    );
}

// --- the structural rewrites -------------------------------------------------

#[test]
fn a_source_left_with_nothing_to_feed_is_removed_with_its_feeders() {
    let before = EDGE_SOURCE_BECOMES_DEAD.before();
    let result = pruned(&before, &key("h-1", "h-2", SynapseType::Standard), None);

    assert_eq!(
        result.creature,
        EDGE_SOURCE_BECOMES_DEAD.after(),
        "the dead-source cleanup did not reproduce the TypeScript capture"
    );
    assert_eq!(result.cascade_neurons, vec!["h-1".to_string()]);
    assert_valid("a_source_left_with_nothing_to_feed", &result.creature);
}

#[test]
fn a_dead_source_cascades_through_every_level_it_orphans() {
    let before = CASCADE_ORPHAN_FEEDERS.before();
    let result = pruned(
        &before,
        &key("h-x", "output-0", SynapseType::Standard),
        None,
    );

    assert_eq!(
        result.creature,
        CASCADE_ORPHAN_FEEDERS.after(),
        "the multi-level cascade did not reach the capture"
    );
    let mut cascaded = result.cascade_neurons.clone();
    cascaded.sort();
    assert_eq!(
        cascaded,
        vec![
            "h-a".to_string(),
            "h-b".to_string(),
            "h-c".to_string(),
            "h-d".to_string(),
            "h-x".to_string()
        ],
        "the cascade stopped short of a level"
    );
    assert_valid("a_dead_source_cascades", &result.creature);
}

#[test]
fn a_converted_constant_moves_into_the_constant_prefix() {
    let before = CONSTANT_MOVES_INTO_PREFIX.before();
    let result = pruned(&before, &key("input-0", "h-2", SynapseType::Standard), None);

    assert_same_function(
        "constant_moves_into_prefix",
        &CONSTANT_MOVES_INTO_PREFIX.after(),
        &result.creature,
    );
    let types: Vec<&str> = result
        .creature
        .neurons
        .iter()
        .map(|n| n.neuron_type.as_str())
        .collect();
    let first_hidden = types.iter().position(|t| *t == "hidden");
    let last_constant = types.iter().rposition(|t| *t == "constant");
    assert!(
        first_hidden.is_none() || last_constant.is_none() || last_constant < first_hidden,
        "a constant follows a hidden neuron: {types:?}"
    );
    assert_valid("a_converted_constant_moves", &result.creature);
}

// --- compensation ------------------------------------------------------------

#[test]
fn without_statistics_an_ordinary_target_is_reported_uncompensated() {
    let before = creature(ORDINARY_JSON);
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        None,
    );

    assert_eq!(result.bias_folds, vec![]);
    assert_eq!(result.uncompensated.len(), 1);
    let entry = &result.uncompensated[0];
    assert_eq!(entry.target_uuid, "output-0");
    assert_eq!(entry.reason, UncompensatedReason::NoStatistics);
    assert_close("the weight it carried", entry.weight_sum, 2.0);
    assert_eq!(result.transform, TransformClass::Approximate);
    assert_close(
        "the target's bias is untouched",
        neuron(&result.creature, "output-0").bias,
        0.3,
    );
}

#[test]
fn a_supplied_mean_folds_the_removed_term_into_the_targets_bias() {
    let before = creature(ORDINARY_JSON);
    let stats = mean_only(0.6);
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    );

    // W · μ = 2.0 · 0.6, on top of the target's own 0.3.
    assert_close(
        "the folded bias",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * 0.6,
    );
    assert_eq!(result.bias_folds.len(), 1);
    let fold = &result.bias_folds[0];
    assert_eq!(fold.target_uuid, "output-0");
    assert_close("the fold delta", fold.delta, 2.0 * 0.6);
    assert!(!fold.exact, "a measured mean is never exact");
    assert_eq!(result.transform, TransformClass::Approximate);
}

#[test]
fn a_supplied_variance_reports_the_residual_the_fold_could_not_carry() {
    let before = creature(ORDINARY_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: Some(0.04),
        proxy: None,
    };
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    );

    // W² σ² = 2.0² · 0.04.
    assert_close(
        "the residual variance",
        result.bias_folds[0]
            .residual_variance
            .expect("a supplied variance is reported"),
        4.0 * 0.04,
    );
}

#[test]
fn a_correlated_survivor_carries_the_part_it_predicts() {
    let before = creature(ORDINARY_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: Some(0.04),
        proxy: Some(ProxyStats {
            uuid: "h-2".to_string(),
            mean_activation: 0.5,
            variance: 0.02,
            covariance: 0.01,
        }),
    };
    let result = pruned(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    );

    // β = cov / σₛ² = 0.01 / 0.02 = 0.5; the survivor's edge takes β · W.
    let beta = 0.01 / 0.02;
    assert_eq!(result.weight_shares.len(), 1);
    assert_close(
        "the survivor's share",
        result.weight_shares[0].delta,
        beta * 2.0,
    );
    assert_close(
        "the survivor's edge",
        weight(&result.creature, "h-2", "output-0"),
        1.0 + beta * 2.0,
    );
    assert_close(
        "the bias takes what the survivor does not deliver",
        neuron(&result.creature, "output-0").bias,
        0.3 + 2.0 * (0.6 - beta * 0.5),
    );
    assert_close(
        "the residual variance",
        result.bias_folds[0]
            .residual_variance
            .expect("a supplied variance is reported"),
        4.0 * (0.04 - 0.01 * 0.01 / 0.02),
    );
}

#[test]
fn a_constant_source_folds_exactly_without_any_statistic() {
    let before = creature(CONSTANT_SOURCE_JSON);
    let result = pruned(
        &before,
        &key("c-1", "output-0", SynapseType::Standard),
        None,
    );

    // The constant is worth 0.5 on every record and carried 0.2 of it.
    assert_within(
        "the exact fold",
        neuron(&result.creature, "output-0").bias,
        0.25 + 0.2 * 0.5,
        STRUCTURAL_FOLD_TOL,
    );
    assert!(result.bias_folds[0].exact, "a structural fold is exact");
    assert_eq!(result.transform, TransformClass::Exact);
    assert_same_function("constant_source_fold", &before, &result.creature);
    assert!(!has_neuron(&result.creature, "c-1"), "the constant is dead");
}

#[test]
fn a_supplied_mean_never_overrides_the_structural_value() {
    let before = creature(CONSTANT_SOURCE_JSON);
    let stats = mean_only(9.0);
    let result = pruned(
        &before,
        &key("c-1", "output-0", SynapseType::Standard),
        Some(&stats),
    );

    assert_within(
        "the structural fold wins",
        neuron(&result.creature, "output-0").bias,
        0.25 + 0.2 * 0.5,
        STRUCTURAL_FOLD_TOL,
    );
    assert_eq!(result.transform, TransformClass::Exact);
}

#[test]
fn an_aggregate_target_is_never_given_a_bias_fold() {
    let before = creature(AGGREGATE_TARGET_JSON);
    let stats = mean_only(0.6);
    let result = pruned(
        &before,
        &key("h-1", "h-agg", SynapseType::Standard),
        Some(&stats),
    );

    assert_eq!(result.bias_folds, vec![]);
    assert_eq!(result.uncompensated.len(), 1);
    let entry = &result.uncompensated[0];
    assert_eq!(entry.target_uuid, "h-agg");
    assert_eq!(entry.reason, UncompensatedReason::AggregateTarget);
    assert_eq!(entry.squash, "MINIMUM");
    assert_close(
        "the aggregate's bias is untouched",
        neuron(&result.creature, "h-agg").bias,
        0.2,
    );
    assert_eq!(result.transform, TransformClass::Approximate);
}

#[test]
fn an_if_target_is_reported_per_role_rather_than_as_a_sum() {
    let before = creature(IF_SHARED_BRANCHES_JSON);
    let stats = mean_only(0.6);
    let result = pruned(
        &before,
        &key("h-a", "if-1", SynapseType::Positive),
        Some(&stats),
    );

    assert_eq!(result.bias_folds, vec![]);
    assert_eq!(result.uncompensated.len(), 1);
    let entry = &result.uncompensated[0];
    assert_eq!(entry.target_uuid, "if-1");
    assert_eq!(entry.role, SynapseType::Positive);
    assert_eq!(entry.reason, UncompensatedReason::AggregateTarget);
}

#[test]
fn statistics_that_are_not_numbers_are_refused_before_anything_is_cut() {
    let before = creature(ORDINARY_JSON);
    let stats = mean_only(f64::NAN);
    match prune_synapse(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    ) {
        Err(PruneError::NonFiniteStatistic { field, .. }) => assert_eq!(field, "mean_activation"),
        other => panic!("a NaN mean was not refused: {other:?}"),
    }
}

#[test]
fn a_proxy_that_does_not_feed_the_target_is_refused() {
    let before = creature(ORDINARY_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: None,
        proxy: Some(ProxyStats {
            uuid: "input-1".to_string(),
            mean_activation: 0.5,
            variance: 0.02,
            covariance: 0.01,
        }),
    };
    match prune_synapse(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    ) {
        Err(PruneError::MissingProxyEdge { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "input-1");
            assert_eq!(to_uuid, "output-0");
        }
        other => panic!("a proxy with no edge was not refused: {other:?}"),
    }
}

// --- IF rewrites -------------------------------------------------------------

#[test]
fn losing_the_last_condition_leaves_the_negative_branch_and_drops_the_positive() {
    let before = creature(IF_JSON);
    let result = pruned(
        &before,
        &key("h-cond", "if-1", SynapseType::Condition),
        None,
    );

    // With no condition edge the condition sum is 0, which is not > 0, so the
    // forward pass takes the negative branch on every record.
    let twin = with_edge_zeroed(&before, "h-cond", "if-1", Some("condition"));
    assert_same_function("last_condition_removed", &twin, &result.creature);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IDENTITY"),
        "a statically-negative IF is an IDENTITY sum of its negative arm"
    );
    assert_close(
        "the negative arm survives at its own weight",
        weight(&result.creature, "h-a", "if-1"),
        -3.0,
    );
    assert!(
        !has_neuron(&result.creature, "h-cond"),
        "the condition source is dead structure"
    );
    assert_valid("losing_the_last_condition", &result.creature);
}

#[test]
fn losing_the_last_positive_keeps_the_branch_and_stays_valid() {
    let before = creature(IF_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Positive), None);

    let twin = with_edge_zeroed(&before, "h-a", "if-1", Some("positive"));
    assert_same_function("last_positive_removed", &twin, &result.creature);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IF"),
        "the condition still varies, so the IF still branches"
    );
    assert_valid("losing_the_last_positive", &result.creature);
}

#[test]
fn losing_the_last_negative_keeps_the_branch_and_stays_valid() {
    let before = creature(IF_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Negative), None);

    let twin = with_edge_zeroed(&before, "h-a", "if-1", Some("negative"));
    assert_same_function("last_negative_removed", &twin, &result.creature);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IF"),
        "the condition still varies, so the IF still branches"
    );
    assert_valid("losing_the_last_negative", &result.creature);
}

#[test]
fn a_shared_branch_keeps_the_if_untouched() {
    let before = creature(IF_SHARED_BRANCHES_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Positive), None);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IF"),
        "h-b still feeds the positive branch"
    );
    assert_close(
        "the other positive row is untouched",
        role_weight(&result.creature, "h-b", "if-1", Some("positive")),
        3.0,
    );
    assert_close(
        "h-a's negative row is untouched",
        role_weight(&result.creature, "h-a", "if-1", Some("negative")),
        -1.0,
    );
    assert_valid("a_shared_branch", &result.creature);
}

#[test]
fn a_statically_true_condition_drops_the_unreachable_branch_and_cascades() {
    let before = creature(IF_STATIC_AFTER_CUT_JSON);
    let result = pruned(&before, &key("input-0", "h-c", SynapseType::Standard), None);

    // h-c sums nothing once the edge is gone, so it is worth IDENTITY(0.5) on
    // every record and the condition sum is 0.5 > 0: the positive branch is
    // taken always, and the negative branch is unreachable.
    let twin = with_edge_zeroed(&before, "input-0", "h-c", None);
    assert_same_function("static_if_choice", &twin, &result.creature);

    assert_eq!(
        neuron(&result.creature, "if-1").squash.as_deref(),
        Some("IDENTITY"),
        "a statically-decided IF no longer branches"
    );
    assert!(
        !has_neuron(&result.creature, "h-n"),
        "the unreachable negative branch's only source is dead structure"
    );
    assert!(
        !has_neuron(&result.creature, "h-c"),
        "the condition source is dead once the condition is gone"
    );
    assert_close(
        "the positive arm survives at its own weight",
        weight(&result.creature, "h-p", "if-1"),
        2.0,
    );
    assert_valid("a_statically_true_condition", &result.creature);
}

#[test]
fn a_flattened_positive_branch_keeps_the_untyped_edges_that_feed_it() {
    let before = creature(IF_STATIC_UNTYPED_ARM_JSON);
    let result = pruned(&before, &key("input-0", "h-c", SynapseType::Standard), None);

    // The condition settles at `0.5 > 0`, so the positive branch is the one the
    // forward pass takes — and an untyped edge is a positive-branch edge.
    let twin = with_edge_zeroed(&before, "input-0", "h-c", None);
    assert_same_function("static_if_untyped_arm", &twin, &result.creature);

    assert!(
        has_edge(&result.creature, "h-p", "if-1"),
        "the untyped positive arm was dropped with the unreachable branch"
    );
    assert_close(
        "the untyped arm survives at its own weight",
        weight(&result.creature, "h-p", "if-1"),
        2.0,
    );
    assert_valid("a_flattened_positive_branch_untyped", &result.creature);
}

#[test]
fn dropping_an_unreachable_branch_cascades_through_every_level_it_strands() {
    let before = creature(IF_STATIC_MULTILEVEL_JSON);
    let result = pruned(&before, &key("input-0", "h-c", SynapseType::Standard), None);

    let twin = with_edge_zeroed(&before, "input-0", "h-c", None);
    assert_same_function("static_if_multilevel", &twin, &result.creature);

    for stranded in ["h-c", "h-n", "h-n2"] {
        assert!(
            !has_neuron(&result.creature, stranded),
            "{stranded} still stands after its only reader went"
        );
    }
    assert!(
        has_neuron(&result.creature, "h-p"),
        "the taken branch stays"
    );
    assert_close(
        "the positive arm survives at its own weight",
        weight(&result.creature, "h-p", "if-1"),
        2.0,
    );
    assert_valid("dropping_an_unreachable_branch", &result.creature);
}

#[test]
fn the_result_names_every_if_neuron_the_rewrite_touched() {
    let before = creature(IF_JSON);

    let flattened = pruned(
        &before,
        &key("h-cond", "if-1", SynapseType::Condition),
        None,
    );
    assert_eq!(flattened.static_if_neurons.len(), 1);
    assert_eq!(flattened.static_if_neurons[0].uuid, "if-1");
    assert_eq!(
        flattened.static_if_neurons[0].branch,
        SynapseType::Negative,
        "a condition sum of 0 is not > 0, so the negative arm is the one taken"
    );
    assert_eq!(flattened.restored_if_roles, vec![]);
    assert_eq!(
        flattened.downgraded_if_neurons,
        Vec::<String>::new(),
        "the exact rewrite replaces the downgrade, it does not accompany it"
    );

    let restored = pruned(&before, &key("h-a", "if-1", SynapseType::Positive), None);
    assert_eq!(restored.static_if_neurons, vec![]);
    assert_eq!(restored.restored_if_roles.len(), 1);
    let edge = &restored.restored_if_roles[0];
    assert_eq!(edge.to_uuid, "if-1");
    assert_eq!(edge.role, SynapseType::Positive);
    assert_close(
        "the restored role carries nothing",
        role_weight(
            &restored.creature,
            &edge.from_uuid,
            "if-1",
            Some("positive"),
        ),
        0.0,
    );
    // The creature named no constant, so the rewrite had to add one — and the
    // result says so rather than leaving it to be discovered.
    assert_eq!(
        neuron(&restored.creature, &edge.from_uuid).neuron_type,
        "constant"
    );
    assert!(
        !has_neuron(&before, &edge.from_uuid),
        "the support constant is new structure, not one the caller wrote"
    );
}

#[test]
fn the_default_cleanup_policy_still_downgrades_an_if_short_a_role() {
    // The same cut, under each policy: parity keeps the blanket downgrade, the
    // prune's policy rewrites exactly. `cleanup_creature` must not have moved.
    let mut cut = creature(IF_JSON);
    cut.synapses
        .retain(|s| !(s.from_uuid == "h-cond" && s.to_uuid == "if-1"));

    let parity = cleanup_creature(&cut).expect("the parity policy cleans up");
    assert_eq!(parity.downgraded_if_neurons, vec!["if-1".to_string()]);
    assert_eq!(parity.static_if_neurons, vec![]);
    assert_eq!(parity.restored_if_roles, vec![]);
    // IDENTITY sums both arms it inherited: 2.0 + -3.0.
    assert_close(
        "the downgrade coalesces the arms",
        weight(&parity.creature, "h-a", "if-1"),
        -1.0,
    );

    let exact = cleanup_creature_with(
        &cut,
        CleanupOptions {
            if_repair: IfRepair::Rewrite,
        },
    )
    .expect("the exact policy cleans up");
    assert_eq!(exact.downgraded_if_neurons, Vec::<String>::new());
    assert_eq!(exact.static_if_neurons.len(), 1);
    assert_close(
        "only the arm the condition takes survives",
        weight(&exact.creature, "h-a", "if-1"),
        -3.0,
    );
}

#[test]
fn a_proxy_that_is_the_removed_edges_own_source_is_refused() {
    let before = creature(ORDINARY_JSON);
    let stats = PruneStats {
        mean_activation: 0.6,
        variance: None,
        proxy: Some(ProxyStats {
            uuid: "h-1".to_string(),
            mean_activation: 0.5,
            variance: 0.02,
            covariance: 0.01,
        }),
    };
    // The only edge `h-1` could carry the share on into `output-0` is the one
    // being removed, so the request cannot be carried out as described.
    match prune_synapse(
        &before,
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&stats),
    ) {
        Err(PruneError::MissingProxyEdge { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "h-1");
            assert_eq!(to_uuid, "output-0");
        }
        other => panic!("a self-proxy was not refused: {other:?}"),
    }
}

#[test]
fn an_output_left_with_nothing_to_sum_still_comes_back_valid() {
    let before = creature(CONSTANT_SOURCE_JSON);
    // `output-0` keeps only the constant's edge once the observation edge goes,
    // and loses that too when the constant folds — an output with no inward
    // edge is the declared width, not dead structure.
    let result = pruned(
        &before,
        &key("input-0", "output-0", SynapseType::Standard),
        None,
    );

    assert_eq!(result.creature.output, 1, "the declared width moved");
    assert!(has_neuron(&result.creature, "output-0"));
    assert_valid("an_output_left_with_nothing_to_sum", &result.creature);
}
