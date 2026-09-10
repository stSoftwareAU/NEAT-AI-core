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
use neat_core::range::{apply_get_range, apply_limit_range};
use neat_core::{
    CleanupError, CleanupOptions, CreatureExport, IfRepair, ProxyStats, PruneError, PruneResult,
    PruneStats, SquashConversion, SquashType, SynapseKey, SynapseType, TransformClass,
    UncompensatedReason, ValidateOptions, cleanup_creature, cleanup_creature_with,
    compile_creature, creature_validate, parse_creature_json, prune_synapse,
    validate_creature_topology,
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

/// Relative slack for a rewrite that must compute the **same** number: the two
/// arms of the forward pass it is measured across agree to `f32` rounding, and
/// nothing looser is being claimed (Ockham #197).
const CONVERSION_TOL: f32 = 1e-6;

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

/// An `IF` with a varying condition, in a creature that already carries a
/// support constant. Restoring an emptied branch role has to hang off that
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
fn a_restored_branch_role_reuses_the_support_constant_already_there() {
    let before = creature(IF_WITH_SUPPORT_CONSTANT_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Positive), None);

    // Constants are support nodes, not something a repair may proliferate.
    let constants: Vec<&str> = result
        .creature
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "constant")
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(
        constants,
        vec!["c-1"],
        "the restored role minted a constant instead of reusing c-1"
    );
    assert_eq!(result.restored_if_roles.len(), 1);
    assert_eq!(result.restored_if_roles[0].from_uuid, "c-1");

    let twin = with_edge_zeroed(&before, "h-a", "if-1", Some("positive"));
    assert_same_function("restored_role_reuses_support", &twin, &result.creature);
    assert_valid("a_restored_branch_role_reuses", &result.creature);
}

#[test]
fn an_untyped_if_arm_answers_to_the_positive_role() {
    // `IfRoles::tally` and the forward pass both read an untyped inward edge
    // as the positive branch, so a request for `Positive` has to name it.
    // Reading the two spellings apart would refuse the very removal Issue #591
    // exists to stop refusing.
    let before = creature(IF_STATIC_UNTYPED_ARM_JSON);
    let result = pruned(&before, &key("h-p", "if-1", SynapseType::Positive), None);

    assert!(
        !has_edge(&result.creature, "h-p", "if-1"),
        "the untyped positive arm survived a request naming its role"
    );
    let twin = with_edge_zeroed(&before, "h-p", "if-1", None);
    assert_same_function("untyped_arm_by_positive_role", &twin, &result.creature);
    assert_valid("an_untyped_if_arm_answers_to_positive", &result.creature);
}

#[test]
fn a_standard_request_names_the_positive_row_of_an_if() {
    // The same identity read from the other side: `Standard` and `positive`
    // are one branch, so either spelling of the request names the same edge.
    let before = creature(IF_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Standard), None);

    assert_close(
        "the negative row of the same pair is untouched",
        role_weight(&result.creature, "h-a", "if-1", Some("negative")),
        -3.0,
    );
    let twin = with_edge_zeroed(&before, "h-a", "if-1", Some("positive"));
    assert_same_function("positive_row_by_standard_role", &twin, &result.creature);
    assert_valid(
        "a_standard_request_names_the_positive_row",
        &result.creature,
    );
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

// --- aggregates the cut leaves with inward edges (Ockham #197) ---------------

/// The aggregate fixture, parameterised by the squash `h-agg` declares and the
/// bias it carries.
///
/// `h-1` feeds both `h-agg` and the output, so cutting `h-1 → h-agg` leaves
/// `h-agg` with exactly **one** inward edge (`input-1 → h-agg` at `0.75`) and
/// leaves `h-1` itself alive. Every source is an `IDENTITY`, so what the
/// creature computes is plain arithmetic a test can derive.
fn aggregate_json(squash: &str, bias: f64) -> String {
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

/// The same fixture with a third inward edge into `h-agg`, so cutting
/// `h-1 → h-agg` leaves **two** and nothing is rewritten.
fn aggregate_three_edges_json(squash: &str, bias: f64) -> String {
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
        {{"weight":0.25,"fromUUID":"input-0","toUUID":"h-agg"}},
        {{"weight":0.5,"fromUUID":"h-1","toUUID":"h-agg"}},
        {{"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}},
        {{"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"}}
      ]
    }}"#
    )
}

/// `h-agg` is fed by a constant and by `h-1`, so the cut leaves it one edge
/// whose source the creature itself fixes.
fn aggregate_constant_fed_json(squash: &str) -> String {
    format!(
        r#"{{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {{"type":"constant","uuid":"c-1","bias":1.0}},
        {{"type":"hidden","uuid":"h-1","bias":0.1,"squash":"IDENTITY"}},
        {{"type":"hidden","uuid":"h-agg","bias":0.2,"squash":"{squash}"}},
        {{"type":"output","uuid":"output-0","bias":0.3,"squash":"IDENTITY"}}
      ],
      "synapses":[
        {{"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"}},
        {{"weight":0.75,"fromUUID":"c-1","toUUID":"h-agg"}},
        {{"weight":0.5,"fromUUID":"h-1","toUUID":"h-agg"}},
        {{"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"}},
        {{"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"}}
      ]
    }}"#
    )
}

/// The creature the cut leaves, built **by hand** and still declaring the
/// aggregate squash.
///
/// This is the oracle for every conversion: the claim is that a single-edge
/// aggregate and its point-wise replacement are the same function, so the
/// replacement is graded against the aggregate form itself — reached here by a
/// plain `retain`, never by the code under test. The two sides run down
/// *different* arms of the forward pass, so a fault in either moves one of them.
fn cut_by_hand(json: &str, from: &str, to: &str) -> CreatureExport {
    let mut twin = creature(json);
    let before = twin.synapses.len();
    twin.synapses
        .retain(|s| !(s.from_uuid == from && s.to_uuid == to));
    assert_eq!(before - 1, twin.synapses.len(), "no {from} -> {to} to cut");
    twin
}

/// The same creature with one neuron's squash rewritten — used to show that a
/// conversion the rules refuse would genuinely have changed the answer.
fn with_squash(creature: &CreatureExport, uuid: &str, squash: &str) -> CreatureExport {
    let mut twin = creature.clone();
    let target = twin
        .neurons
        .iter_mut()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"));
    target.squash = Some(squash.to_string());
    twin
}

fn squash_of(creature: &CreatureExport, uuid: &str) -> String {
    neuron(creature, uuid)
        .squash
        .clone()
        .unwrap_or_else(|| "IDENTITY".to_string())
}

/// What `aggregate_json`'s output computes once `h-agg` is left with the one
/// `input-1 → h-agg` edge, derived from the documented forward-pass arms.
///
/// `h-1` is `IDENTITY(x0 + 0.1)`, the output sums `2·h-1 + 1·h-agg + 0.3`, and
/// the aggregate's own one-term value is whichever arm its squash takes.
fn expected_single_edge_output(squash: &str, bias: f64, probe: &[f32]) -> f64 {
    let x0 = f64::from(probe[0]);
    let x1 = f64::from(probe[1]);
    let term = 0.75 * x1;
    let aggregate = match squash {
        // The extreme of one term is that term; a mean over one term is it too.
        "MINIMUM" | "MAXIMUM" | "MEAN" => term + bias,
        // `HYPOT` takes the root of the sum of squares and *then* adds its bias.
        "HYPOT" => term.abs() + bias,
        // `HYPOTv2` squares `bias + w·a`, so the bias is inside the root.
        "HYPOTv2" => (bias + term).abs(),
        other => panic!("no derivation for {other}"),
    };
    0.3 + 2.0 * (x0 + 0.1) + aggregate
}

/// Assert the creature computes the derived number on every probe record.
fn assert_computes(name: &str, creature: &CreatureExport, expected: impl Fn(&[f32]) -> f64) {
    for probe in probe_inputs(creature.input) {
        let actual = f64::from(outputs(creature, &probe)[0]);
        let want = expected(&probe);
        assert_within(
            &format!("{name} on {probe:?}"),
            actual,
            want,
            1e-6 * want.abs().max(1.0),
        );
    }
}

#[test]
fn a_minimum_maximum_or_mean_left_with_one_edge_becomes_identity() {
    for squash in ["MINIMUM", "MAXIMUM", "MEAN"] {
        let json = aggregate_json(squash, 0.2);
        let result = pruned(
            &creature(&json),
            &key("h-1", "h-agg", SynapseType::Standard),
            None,
        );

        assert_valid(squash, &result.creature);
        assert_eq!(
            squash_of(&result.creature, "h-agg"),
            "IDENTITY",
            "{squash} left with one edge was not rewritten"
        );
        assert_eq!(
            result.converted_neurons,
            vec![SquashConversion {
                uuid: "h-agg".to_string(),
                from: squash,
                to: "IDENTITY",
            }],
            "{squash}: the conversion was not reported"
        );
        assert_close(
            &format!("{squash}: the bias is untouched"),
            neuron(&result.creature, "h-agg").bias,
            0.2,
        );
        assert_same_function_within(
            squash,
            CONVERSION_TOL,
            &result.creature,
            &cut_by_hand(&json, "h-1", "h-agg"),
        );
        assert_computes(squash, &result.creature, |probe| {
            expected_single_edge_output(squash, 0.2, probe)
        });
    }
}

#[test]
fn a_hypot_at_zero_bias_and_a_hypot_v2_become_absolute() {
    for (squash, bias) in [("HYPOT", 0.0), ("HYPOTv2", -0.35)] {
        let json = aggregate_json(squash, bias);
        let result = pruned(
            &creature(&json),
            &key("h-1", "h-agg", SynapseType::Standard),
            None,
        );

        assert_valid(squash, &result.creature);
        assert_eq!(
            squash_of(&result.creature, "h-agg"),
            "ABSOLUTE",
            "{squash} left with one edge was not rewritten"
        );
        assert_eq!(
            result.converted_neurons,
            vec![SquashConversion {
                uuid: "h-agg".to_string(),
                from: squash,
                to: "ABSOLUTE",
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
            &cut_by_hand(&json, "h-1", "h-agg"),
        );
        assert_computes(squash, &result.creature, |probe| {
            expected_single_edge_output(squash, bias, probe)
        });
    }
}

#[test]
fn a_hypot_with_a_non_zero_bias_keeps_its_squash() {
    let json = aggregate_json("HYPOT", 0.2);
    let result = pruned(
        &creature(&json),
        &key("h-1", "h-agg", SynapseType::Standard),
        None,
    );

    assert_valid("hypot_with_bias", &result.creature);
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
    assert_computes("hypot_with_bias", &result.creature, |probe| {
        expected_single_edge_output("HYPOT", 0.2, probe)
    });

    // Why it is kept: `HYPOT` adds its bias to `|w·a|` where `ABSOLUTE` folds it
    // inside, so the rewrite would have changed the answer.
    let kept = cut_by_hand(&json, "h-1", "h-agg");
    assert_different_function(
        "ABSOLUTE is not HYPOT at a non-zero bias",
        &kept,
        &with_squash(&kept, "h-agg", "ABSOLUTE"),
    );
}

#[test]
fn an_if_left_with_one_edge_is_never_converted() {
    // Rule 12 wants one edge of each role and `IfRepair` owns what to do when
    // the removal leaves an `IF` short of that, so this module keeps its hands
    // off: an `IF` reads its condition sum to pick a branch and a point-wise
    // squash cannot stand in for that.
    let before = creature(IF_JSON);
    let result = pruned(&before, &key("h-a", "if-1", SynapseType::Negative), None);

    assert!(
        result.converted_neurons.is_empty(),
        "an IF was converted: {:?}",
        result.converted_neurons
    );
    assert_valid("if_is_never_converted", &result.creature);
}

#[test]
fn a_converted_target_folds_its_last_edge_exactly() {
    // Step one leaves `h-agg` with the constant's edge alone, which the
    // conversion turns into an `IDENTITY` sum.
    let json = aggregate_constant_fed_json("MINIMUM");
    let first = pruned(
        &creature(&json),
        &key("h-1", "h-agg", SynapseType::Standard),
        None,
    );
    assert_eq!(squash_of(&first.creature, "h-agg"), "IDENTITY");

    // Step two removes that last edge. Its source is a constant, so the term is
    // the creature's own to prove — and now that the target sums rather than
    // aggregates, it folds into the bias exactly.
    let second = pruned(
        &first.creature,
        &key("c-1", "h-agg", SynapseType::Standard),
        None,
    );

    assert!(
        second.uncompensated.is_empty(),
        "the fold was refused: {:?}",
        second.uncompensated
    );
    assert_eq!(second.bias_folds.len(), 1);
    assert!(second.bias_folds[0].exact, "{:?}", second.bias_folds[0]);
    assert_eq!(second.transform, TransformClass::Exact);
    assert_same_function_within(
        "the last edge folds exactly",
        CONVERSION_TOL,
        &first.creature,
        &second.creature,
    );
    assert_valid("a_converted_target_folds", &second.creature);
}

#[test]
fn an_aggregate_left_with_two_edges_keeps_its_squash_and_reports_the_dropped_term() {
    let json = aggregate_three_edges_json("MEAN", 0.2);
    let stats = mean_only(0.6);
    let result = pruned(
        &creature(&json),
        &key("h-1", "h-agg", SynapseType::Standard),
        Some(&stats),
    );

    assert_valid("two_edges", &result.creature);
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
    assert!(result.bias_folds.is_empty(), "an aggregate takes no fold");
    assert_close(
        "the aggregate's bias is untouched",
        neuron(&result.creature, "h-agg").bias,
        0.2,
    );

    assert_eq!(result.uncompensated.len(), 1);
    let entry = &result.uncompensated[0];
    assert_eq!(entry.target_uuid, "h-agg");
    assert_eq!(entry.reason, UncompensatedReason::AggregateTarget);
    assert_eq!(entry.squash, "MEAN");
    assert_close("the weight it carried", entry.weight_sum, 0.5);
    // `w · μ` — what the scorer is being asked to judge, not a refusal.
    assert_close(
        "the dropped term's magnitude",
        entry
            .dropped_mean
            .expect("a supplied mean names a magnitude"),
        0.5 * 0.6,
    );
    assert_eq!(result.transform, TransformClass::Approximate);

    // The term really is gone: a `MEAN` over two terms is not a `MEAN` over
    // three, so the creature no longer computes what it did.
    assert_different_function(
        "the dropped term changed the answer",
        &creature(&json),
        &result.creature,
    );
}

#[test]
fn the_dropped_term_magnitude_is_reported_only_where_a_number_proves_it() {
    // No statistic, and a source the creature cannot fix: there is no number,
    // and none is invented.
    let bare = pruned(
        &creature(ORDINARY_JSON),
        &key("h-1", "output-0", SynapseType::Standard),
        None,
    );
    assert_eq!(bare.uncompensated.len(), 1);
    assert_eq!(
        bare.uncompensated[0].reason,
        UncompensatedReason::NoStatistics
    );
    assert_eq!(bare.uncompensated[0].dropped_mean, None);
    assert_close(
        "the bias is untouched",
        neuron(&bare.creature, "output-0").bias,
        0.3,
    );

    // A source the creature itself fixes names the magnitude with no statistic
    // at all: the constant is worth `1.0` on every record and carried `0.75`.
    let fixed = pruned(
        &creature(&aggregate_three_edges_json("MINIMUM", 0.2)),
        &key("input-1", "h-agg", SynapseType::Standard),
        None,
    );
    assert_eq!(fixed.uncompensated.len(), 1);
    assert_eq!(
        fixed.uncompensated[0].reason,
        UncompensatedReason::AggregateTarget
    );
    assert_eq!(
        fixed.uncompensated[0].dropped_mean, None,
        "an observation neuron varies with the record"
    );

    let constant = pruned(
        &creature(&aggregate_constant_fed_json("MINIMUM")),
        &key("c-1", "h-agg", SynapseType::Standard),
        None,
    );
    assert_eq!(constant.uncompensated.len(), 1);
    assert_eq!(
        constant.uncompensated[0].reason,
        UncompensatedReason::AggregateTarget
    );
    assert_close(
        "w · the value the creature fixes",
        constant.uncompensated[0]
            .dropped_mean
            .expect("a fixed source names a magnitude"),
        0.75 * 1.0,
    );
}

#[test]
fn no_dropped_magnitude_refuses_a_prune() {
    // `|w · μ| = 1e9` is an enormous shortfall for a creature whose other terms
    // are fractions, and it is still answered: the scorer judges, this crate
    // reports. Nothing here is a numeric bound.
    let huge = mean_only(5e8);

    let folded = pruned(
        &creature(ORDINARY_JSON),
        &key("h-1", "output-0", SynapseType::Standard),
        Some(&huge),
    );
    assert_within(
        "the whole fold lands in the bias",
        neuron(&folded.creature, "output-0").bias,
        0.3 + 2.0 * 5e8,
        1e-6,
    );

    let aggregate = pruned(
        &creature(&aggregate_three_edges_json("MEAN", 0.2)),
        &key("h-1", "h-agg", SynapseType::Standard),
        Some(&huge),
    );
    let dropped = aggregate.uncompensated[0]
        .dropped_mean
        .expect("a supplied mean names a magnitude");
    assert_within(
        "the reported magnitude",
        dropped,
        0.5 * 5e8,
        1e-12 * 0.5 * 5e8,
    );
    assert_eq!(aggregate.transform, TransformClass::Approximate);
}

#[test]
fn unusable_statistics_still_refuse_an_aggregate_prune() {
    // No magnitude refuses a prune, but statistics this crate cannot make sense
    // of still do — on an aggregate target exactly as anywhere else.
    let before = creature(&aggregate_three_edges_json("MEAN", 0.2));
    let edge = key("h-1", "h-agg", SynapseType::Standard);

    match prune_synapse(&before, &edge, Some(&mean_only(f64::INFINITY))) {
        Err(PruneError::NonFiniteStatistic { field, .. }) => assert_eq!(field, "mean_activation"),
        other => panic!("an infinite mean was not refused: {other:?}"),
    }
    let negative = PruneStats {
        mean_activation: 0.6,
        variance: Some(-1.0),
        proxy: None,
    };
    match prune_synapse(&before, &edge, Some(&negative)) {
        Err(PruneError::NegativeVariance { variance, .. }) => {
            assert_close("the refused variance", variance, -1.0)
        }
        other => panic!("a negative variance was not refused: {other:?}"),
    }
    let inconsistent = PruneStats {
        mean_activation: 0.6,
        variance: Some(0.01),
        proxy: Some(ProxyStats {
            uuid: "h-1".to_string(),
            mean_activation: 0.5,
            variance: 0.01,
            covariance: 1.0,
        }),
    };
    match prune_synapse(&before, &edge, Some(&inconsistent)) {
        Err(PruneError::InconsistentCovariance { uuid, .. }) => assert_eq!(uuid, "h-1"),
        other => panic!("an impossible covariance was not refused: {other:?}"),
    }
    let unknown_proxy = PruneStats {
        mean_activation: 0.6,
        variance: None,
        proxy: Some(ProxyStats {
            uuid: "no-such-neuron".to_string(),
            mean_activation: 0.5,
            variance: 0.01,
            covariance: 0.001,
        }),
    };
    match prune_synapse(&before, &edge, Some(&unknown_proxy)) {
        Err(PruneError::UnknownProxy { uuid }) => assert_eq!(uuid, "no-such-neuron"),
        other => panic!("an unknown proxy was not refused: {other:?}"),
    }
}

#[test]
fn the_replacement_clamps_to_the_bounds_the_rules_rely_on() {
    // The guard inside `prune_rewrite` reads `apply_get_range`, so asserting one
    // range against the other would move both sides of the comparison when that
    // table changes. The independent oracle is the documented bound *literals*:
    // `F32_LARGE` either side for the unbounded squashes, and a `0` floor for
    // `ABSOLUTE` and `HYPOTv2`.
    const LARGE: f32 = 3.4028235e38;
    for squash in [
        SquashType::Minimum,
        SquashType::Maximum,
        SquashType::Mean,
        SquashType::Identity,
        SquashType::Hypotenuse,
    ] {
        assert_eq!(
            apply_get_range(squash),
            (-LARGE, LARGE),
            "{squash:?} is documented as unbounded either side"
        );
    }
    for squash in [SquashType::Absolute, SquashType::HypotenuseV2] {
        assert_eq!(
            apply_get_range(squash),
            (0.0, LARGE),
            "{squash:?} is documented as floored at zero"
        );
    }
}

#[test]
fn the_replacement_clamps_every_converted_activation_the_same_way() {
    // A conversion is only exact if the forward pass's own output clamp treats
    // the replacement as it treated the aggregate, so the rule table is only
    // allowed to name a replacement that clamps identically.
    let probes = [
        -3.3e38_f32,
        -2.5,
        2.5,
        3.3e38,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    for (from, to) in [
        (SquashType::Minimum, SquashType::Identity),
        (SquashType::Maximum, SquashType::Identity),
        (SquashType::Mean, SquashType::Identity),
        (SquashType::HypotenuseV2, SquashType::Absolute),
    ] {
        for value in probes {
            assert_eq!(
                apply_limit_range(from, value).to_bits(),
                apply_limit_range(to, value).to_bits(),
                "{from:?} -> {to:?} clamps {value} differently"
            );
        }
    }

    // `HYPOT → ABSOLUTE` is the one rule whose replacement clamps more tightly
    // — `ABSOLUTE` floors at `0` — which is exactly why it is taken only at
    // bias `0`, where the activation is `|w·a|` and the floor cannot bite.
    for value in [0.0_f32, 2.5, 3.3e38, f32::INFINITY, f32::NAN] {
        assert_eq!(
            apply_limit_range(SquashType::Hypotenuse, value).to_bits(),
            apply_limit_range(SquashType::Absolute, value).to_bits(),
            "HYPOT -> ABSOLUTE clamps the non-negative {value} differently"
        );
    }
    assert_ne!(
        apply_limit_range(SquashType::Hypotenuse, -2.5).to_bits(),
        apply_limit_range(SquashType::Absolute, -2.5).to_bits(),
        "the ABSOLUTE floor is what the bias-0 condition protects"
    );
}
