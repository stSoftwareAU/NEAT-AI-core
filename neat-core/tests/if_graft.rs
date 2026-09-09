//! Safe IF-node grafting onto a `CreatureExport` (Issue #555).
//!
//! Every assertion is on an observable outcome — the returned creature, the
//! compiled network's activations, or the typed error — never on how the helper
//! got there.

use neat_core::decision_tree::{
    RESIDUAL_THRESHOLD, RESIDUAL_VALUE, linear_base_creature, residual_correction_creature,
};
use neat_core::if_graft::{
    GraftError, IfCorrectionSpec, IfNodeSpec, RelaySpec, graft_if_correction, graft_if_node,
    graft_if_nodes, graft_if_tree, graft_relay_node, validate_creature_topology,
};
use neat_core::topology_ops::{
    BACKWARD_CONNECTION, DUPLICATE_CONNECTION, STRUCTURAL_HIDDEN_NO_OUTWARD,
    STRUCTURAL_IF_MISSING_CONDITION, STRUCTURAL_SYNAPSE_TARGETS_INPUT,
};
use neat_core::{
    CreatureError, CreatureExport, NeuronExport, SynapseExport, SynapseType, ValidateOptions,
    compile_creature, creature_validate,
};

const TOL: f32 = 1e-6;

/// Two inputs, one hidden TANH neuron, one identity output. Deliberately not an
/// IF creature — the graft has to add every role itself.
fn base_creature() -> CreatureExport {
    CreatureExport {
        memetic: None,
        input: 2,
        output: 1,
        neurons: vec![
            NeuronExport {
                id: None,
                neuron_type: "hidden".to_string(),
                uuid: "hidden-1".to_string(),
                bias: 0.0,
                squash: Some("TANH".to_string()),
            },
            NeuronExport {
                id: None,
                neuron_type: "output".to_string(),
                uuid: "output-0".to_string(),
                bias: 0.0,
                squash: Some("IDENTITY".to_string()),
            },
        ],
        synapses: vec![
            SynapseExport {
                from_uuid: "input-0".to_string(),
                to_uuid: "hidden-1".to_string(),
                weight: 1.0,
                synapse_type: None,
            },
            SynapseExport {
                from_uuid: "input-1".to_string(),
                to_uuid: "hidden-1".to_string(),
                weight: 0.5,
                synapse_type: None,
            },
            SynapseExport {
                from_uuid: "hidden-1".to_string(),
                to_uuid: "output-0".to_string(),
                weight: 1.0,
                synapse_type: None,
            },
        ],
        semantic_version: None,
        forward_only: true,
    }
}

/// A well-formed depth-1 IF node: fires when `input-0 > 0.25`.
fn valid_spec() -> IfNodeSpec {
    IfNodeSpec::new("if-1", 0.0)
        .with_constant("if-1-one", 1.0)
        .with_constant("if-1-pos", 1.0)
        .with_constant("if-1-neg", 1.0)
        .with_condition("input-0", 1.0)
        .with_condition("if-1-one", -0.25)
        .with_positive("if-1-pos", 2.0)
        .with_negative("if-1-neg", 0.0)
        .with_target("output-0", 1.0)
}

fn activate(creature: &CreatureExport, inputs: &[f32]) -> f32 {
    let mut net = compile_creature(creature).expect("grafted creature compiles");
    net.activate(inputs, creature.output)[0]
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn grafted_if_node_adds_its_branch_to_the_target() {
    let base = base_creature();
    let grafted = graft_if_node(&base, &valid_spec()).expect("graft succeeds");

    // Above the split the positive leaf (2.0) is added; at or below it the
    // zero-valued negative leaf contributes nothing.
    let above = [0.5f32, 0.0];
    let below = [0.1f32, 0.0];
    assert!((activate(&grafted, &above) - (activate(&base, &above) + 2.0)).abs() <= TOL);
    assert!((activate(&grafted, &below) - activate(&base, &below)).abs() <= TOL);
}

#[test]
fn graft_leaves_the_source_creature_untouched() {
    let base = base_creature();
    let before = base.clone();
    let _ = graft_if_node(&base, &valid_spec()).expect("graft succeeds");
    assert_eq!(base, before);
}

#[test]
fn grafted_node_is_placed_after_its_sources_and_before_its_targets() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_constant("if-1-one", 1.0)
        .with_constant("if-1-pos", 1.0)
        .with_constant("if-1-neg", 1.0)
        .with_condition("hidden-1", 1.0)
        .with_condition("if-1-one", -0.1)
        .with_positive("if-1-pos", 1.0)
        .with_negative("if-1-neg", 0.0)
        .with_target("output-0", 1.0);
    let grafted = graft_if_node(&base_creature(), &spec).expect("graft succeeds");

    let order: Vec<&str> = grafted
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect::<Vec<_>>();
    let pos = |u: &str| order.iter().position(|o| *o == u).expect("neuron present");
    assert!(pos("hidden-1") < pos("if-1"), "order was {order:?}");
    assert!(pos("if-1") < pos("output-0"), "order was {order:?}");
    for c in ["if-1-one", "if-1-pos", "if-1-neg"] {
        assert!(
            pos(c) < pos("if-1"),
            "constant {c} after the node: {order:?}"
        );
    }
}

#[test]
fn grafted_node_carries_the_if_squash_and_all_three_roles() {
    let grafted = graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds");
    let node = grafted
        .neurons
        .iter()
        .find(|n| n.uuid == "if-1")
        .expect("node present");
    assert_eq!(node.squash.as_deref(), Some("IF"));
    assert_eq!(node.neuron_type, "hidden");

    let net = compile_creature(&grafted).expect("compiles");
    let mut counts = [0usize; 4];
    for s in net.synapses() {
        counts[SynapseType::from(s.synapse_type) as usize] += 1;
    }
    assert_eq!(counts[SynapseType::Condition as usize], 2);
    assert_eq!(counts[SynapseType::Positive as usize], 1);
    assert_eq!(counts[SynapseType::Negative as usize], 1);
}

#[test]
fn graft_if_tree_applies_every_node_in_order() {
    let first = valid_spec();
    let second = IfNodeSpec::new("if-2", 0.0)
        .with_constant("if-2-one", 1.0)
        .with_constant("if-2-pos", 1.0)
        .with_constant("if-2-neg", 1.0)
        .with_condition("if-1", 1.0)
        .with_condition("if-2-one", -1.0)
        .with_positive("if-2-pos", 5.0)
        .with_negative("if-2-neg", 0.0)
        .with_target("output-0", 1.0);

    let grafted = graft_if_tree(&base_creature(), &[first, second]).expect("tree graft succeeds");
    let order: Vec<&str> = grafted.neurons.iter().map(|n| n.uuid.as_str()).collect();
    let pos = |u: &str| order.iter().position(|o| *o == u).expect("neuron present");
    assert!(pos("if-1") < pos("if-2"), "order was {order:?}");
    assert!(pos("if-2") < pos("output-0"), "order was {order:?}");

    // `if-1` yields 2.0 above its split, which clears `if-2`'s threshold of 1.0
    // and adds a further 5.0 on top.
    let base = base_creature();
    let above = [0.5f32, 0.0];
    assert!((activate(&grafted, &above) - (activate(&base, &above) + 2.0 + 5.0)).abs() <= TOL);
}

#[test]
fn graft_if_tree_is_all_or_nothing() {
    let good = valid_spec();
    let bad = IfNodeSpec::new("if-2", 0.0)
        .with_condition("no-such-neuron", 1.0)
        .with_positive("hidden-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("output-0", 1.0);
    let err = graft_if_tree(&base_creature(), &[good, bad]).expect_err("second node is invalid");
    assert!(matches!(err, GraftError::UnknownSourceUuid(ref u) if u == "no-such-neuron"));
}

// ---------------------------------------------------------------------------
// Depth-1 correction convenience — the canonical residual fixture
// ---------------------------------------------------------------------------

#[test]
fn graft_if_correction_reproduces_the_canonical_residual_fixture() {
    let spec = IfCorrectionSpec {
        uuid: "residual-0".to_string(),
        feature_uuid: "input-0".to_string(),
        threshold: RESIDUAL_THRESHOLD,
        positive_value: RESIDUAL_VALUE,
        negative_value: 0.0,
        target_uuid: "output-0".to_string(),
        target_weight: 1.0,
    };
    let grafted = graft_if_correction(&linear_base_creature(), &spec).expect("graft succeeds");
    assert_eq!(grafted, residual_correction_creature());
}

#[test]
fn graft_if_correction_fires_only_above_the_threshold() {
    let spec = IfCorrectionSpec {
        uuid: "residual-0".to_string(),
        feature_uuid: "input-0".to_string(),
        threshold: 0.75,
        positive_value: 1.5,
        negative_value: 0.0,
        target_uuid: "output-0".to_string(),
        target_weight: 1.0,
    };
    let base = linear_base_creature();
    let grafted = graft_if_correction(&base, &spec).expect("graft succeeds");

    assert!((activate(&grafted, &[1.0]) - 3.5).abs() <= TOL);
    // Strict `>`: the threshold itself takes the negative (zero) leaf.
    assert!((activate(&grafted, &[0.75]) - 1.5).abs() <= TOL);
    assert!((activate(&grafted, &[0.25]) - 0.5).abs() <= TOL);
}

// ---------------------------------------------------------------------------
// Fail closed — every rejection returns a typed error and no creature
// ---------------------------------------------------------------------------

fn reject(spec: IfNodeSpec) -> GraftError {
    graft_if_node(&base_creature(), &spec).expect_err("graft must be rejected")
}

#[test]
fn rejects_a_uuid_that_already_exists() {
    let spec = valid_spec();
    let clashing = IfNodeSpec {
        uuid: "hidden-1".to_string(),
        ..spec
    };
    assert!(matches!(reject(clashing), GraftError::DuplicateUuid(ref u) if u == "hidden-1"));
}

#[test]
fn rejects_a_constant_uuid_that_already_exists() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_constant("output-0", 1.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("output-0", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(reject(spec), GraftError::DuplicateUuid(ref u) if u == "output-0"));
}

#[test]
fn rejects_an_unknown_source() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("ghost", 1.0)
        .with_positive("input-0", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(reject(spec), GraftError::UnknownSourceUuid(ref u) if u == "ghost"));
}

#[test]
fn rejects_an_unknown_target() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("ghost", 1.0);
    assert!(matches!(reject(spec), GraftError::UnknownTargetUuid(ref u) if u == "ghost"));
}

#[test]
fn rejects_a_synapse_that_targets_an_input() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("input-0", 1.0);
    assert!(matches!(reject(spec), GraftError::TargetIsInput(ref u) if u == "input-0"));
}

#[test]
fn rejects_a_synapse_that_targets_a_constant() {
    let with_constant = graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds");
    let spec = IfNodeSpec::new("if-2", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("if-1-one", 1.0);
    let err = graft_if_node(&with_constant, &spec).expect_err("must be rejected");
    assert!(matches!(err, GraftError::TargetIsConstant(ref u) if u == "if-1-one"));
}

#[test]
fn rejects_a_node_missing_any_if_role() {
    let missing_condition = IfNodeSpec::new("if-1", 0.0)
        .with_positive("input-0", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(missing_condition),
        GraftError::MissingConditionSynapse
    ));

    let missing_positive = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(missing_positive),
        GraftError::MissingPositiveSynapse
    ));

    let missing_negative = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(missing_negative),
        GraftError::MissingNegativeSynapse
    ));
}

#[test]
fn rejects_a_node_with_no_outward_connection() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0);
    assert!(matches!(reject(spec), GraftError::NoTargets));
}

/// Issue #577 changed this case: one source feeding two **branches** of the
/// grafted `IF` node is now a legal creature, because the node keeps a sum per
/// role. What is still refused is a repeat of one role, below.
#[test]
fn accepts_one_source_under_two_branches_of_the_if_node() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-0", 2.0)
        .with_negative("input-1", 3.0)
        .with_target("output-0", 1.0);
    let grafted = graft_if_node(&base_creature(), &spec).expect("an IF node may repeat a source");

    // `input-0 = 0.5` makes the condition sum 0.5 > 0, so the positive branch
    // fires with the *same* source: 0.5 * 2.0 = 1.0 added to the base output.
    let base = base_creature();
    let above = [0.5f32, 0.0];
    assert!((activate(&grafted, &above) - (activate(&base, &above) + 1.0)).abs() <= TOL);

    // Below the split the negative branch reads `input-1` instead.
    let below = [-0.5f32, 2.0];
    assert!((activate(&grafted, &below) - (activate(&base, &below) + 6.0)).abs() <= TOL);

    creature_validate(&grafted, &validate_options(&grafted))
        .expect("and the shared validator accepts it");
}

#[test]
fn rejects_two_synapses_between_the_same_pair_in_one_role() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_condition("input-0", 2.0)
        .with_positive("input-1", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(spec),
        GraftError::DuplicateEdge { ref from, ref to } if from == "input-0" && to == "if-1"
    ));
}

#[test]
fn rejects_a_self_edge() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("if-1", 1.0)
        .with_positive("input-0", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(reject(spec), GraftError::SelfEdge(ref u) if u == "if-1"));
}

#[test]
fn rejects_non_finite_weights_and_biases() {
    let bad_weight = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", f64::NAN)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(bad_weight),
        GraftError::NonFiniteWeight { .. }
    ));

    let bad_bias = IfNodeSpec::new("if-1", f64::INFINITY)
        .with_condition("input-0", 1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(bad_bias),
        GraftError::NonFiniteBias { ref uuid } if uuid == "if-1"
    ));

    let bad_constant = IfNodeSpec::new("if-1", 0.0)
        .with_constant("if-1-one", f64::NAN)
        .with_condition("input-0", 1.0)
        .with_condition("if-1-one", -1.0)
        .with_positive("input-1", 1.0)
        .with_negative("hidden-1", 1.0)
        .with_target("output-0", 1.0);
    assert!(matches!(
        reject(bad_constant),
        GraftError::NonFiniteBias { ref uuid } if uuid == "if-1-one"
    ));
}

#[test]
fn rejects_a_graft_that_cannot_be_placed_forward_only() {
    // Source `output-0` sits after target `hidden-1`, so no insertion point
    // keeps every edge pointing forwards.
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("output-0", 1.0)
        .with_positive("input-0", 1.0)
        .with_negative("input-1", 1.0)
        .with_target("hidden-1", 1.0);
    assert!(matches!(
        reject(spec),
        GraftError::ForwardOrderViolation { ref source, ref target }
            if source == "output-0" && target == "hidden-1"
    ));
}

#[test]
fn rejects_a_base_creature_that_is_already_malformed() {
    let mut base = base_creature();
    // A hidden neuron with no outward connection is structurally invalid.
    base.synapses.retain(|s| s.from_uuid != "hidden-1");
    let err = graft_if_node(&base, &valid_spec()).expect_err("malformed base is rejected");
    assert!(matches!(err, GraftError::MalformedStructure { .. }));
}

// ---------------------------------------------------------------------------
// The gate itself — the post-check that stops a malformed creature escaping is
// unreachable through `graft_if_node` by construction, so it is exercised
// directly with synthetic creatures (AGENTS.md oracle rule 5).
// ---------------------------------------------------------------------------

#[test]
fn gate_accepts_every_canonical_fixture() {
    for creature in [
        linear_base_creature(),
        residual_correction_creature(),
        neat_core::decision_tree::stump_creature(),
        neat_core::decision_tree::depth2_tree_creature(),
        base_creature(),
    ] {
        validate_creature_topology(&creature).expect("canonical creature passes the gate");
    }
}

/// A declared observation width past the `u16` index ceiling is refused by the
/// gate itself, before the UUID map it would size is built (Issue #622).
#[test]
fn gate_rejects_a_declared_input_past_the_node_ceiling() {
    let mut creature = base_creature();
    creature.input = 100_000_000;
    // `base_creature` lists two neurons, and the error carries the declared
    // node count — the width plus those two.
    let expected = 100_000_000 + creature.neurons.len();
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(
        matches!(
            err,
            GraftError::Creature(CreatureError::TooManyNodes { count }) if count == expected
        ),
        "expected a typed TooManyNodes refusal naming {expected} nodes, got {err:?}"
    );
}

#[test]
fn gate_rejects_a_hidden_neuron_with_no_outward_connection() {
    let mut creature = base_creature();
    creature.synapses.retain(|s| s.from_uuid != "hidden-1");
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(
        err,
        GraftError::MalformedStructure { code, .. } if code == STRUCTURAL_HIDDEN_NO_OUTWARD
    ));
}

#[test]
fn gate_rejects_an_if_neuron_that_lost_a_role() {
    let mut creature = graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds");
    for s in &mut creature.synapses {
        if s.to_uuid == "if-1" && s.synapse_type.as_deref() == Some("condition") {
            s.synapse_type = None;
        }
    }
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(
        err,
        GraftError::MalformedStructure { code, .. } if code == STRUCTURAL_IF_MISSING_CONDITION
    ));
}

#[test]
fn gate_rejects_a_synapse_that_targets_an_input() {
    let mut creature = base_creature();
    // Recurrent, so the ordering gate stands aside and the structural gate is
    // what has to catch a synapse pointing back into an input neuron.
    creature.forward_only = false;
    creature.synapses.push(SynapseExport {
        from_uuid: "hidden-1".to_string(),
        to_uuid: "input-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    });
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(
        err,
        GraftError::MalformedStructure { code, .. } if code == STRUCTURAL_SYNAPSE_TARGETS_INPUT
    ));
}

#[test]
fn gate_rejects_a_duplicate_connection() {
    let mut creature = base_creature();
    creature.synapses.push(SynapseExport {
        from_uuid: "hidden-1".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 0.25,
        synapse_type: None,
    });
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(
        err,
        GraftError::MalformedTopology { code, .. } if code == DUPLICATE_CONNECTION
    ));
}

#[test]
fn gate_rejects_a_backward_edge_only_when_forward_only() {
    let mut creature = base_creature();
    creature.synapses.push(SynapseExport {
        from_uuid: "output-0".to_string(),
        to_uuid: "hidden-1".to_string(),
        weight: 0.25,
        synapse_type: None,
    });
    let err = validate_creature_topology(&creature).expect_err("rejected while forward-only");
    assert!(matches!(
        err,
        GraftError::MalformedTopology { code, .. } if code == BACKWARD_CONNECTION
    ));

    // A recurrent creature legitimately carries the same edge.
    creature.forward_only = false;
    validate_creature_topology(&creature).expect("recurrent creature is accepted");
}

#[test]
fn gate_rejects_an_unresolvable_synapse_uuid() {
    let mut creature = base_creature();
    creature.synapses[0].from_uuid = "ghost".to_string();
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(err, GraftError::UnknownSourceUuid(ref u) if u == "ghost"));
}

#[test]
fn gate_rejects_a_widthless_creature() {
    let mut creature = base_creature();
    creature.input = 0;
    let err = validate_creature_topology(&creature).expect_err("rejected");
    assert!(matches!(err, GraftError::Creature(_)));
}

// ---------------------------------------------------------------------------
// The shared definition of a valid creature (Issue #562) — what the helper
// hands a consumer has to satisfy `creature_validate`, not merely compile.
// ---------------------------------------------------------------------------

/// The options a consumer gates a grafted creature with: counts change by
/// construction, and the creature's own `forwardOnly` decides the ordering
/// rules.
fn validate_options(creature: &CreatureExport) -> ValidateOptions {
    ValidateOptions {
        neurons: None,
        connections: None,
        feedback_loop: None,
        forward_only: creature.forward_only,
    }
}

fn assert_valid(creature: &CreatureExport, what: &str) {
    if let Err(failure) = creature_validate(creature, &validate_options(creature)) {
        panic!(
            "{what} is not a valid creature: {} ({}): {}",
            failure.class, failure.reason, failure.message
        );
    }
}

#[test]
fn every_grafted_creature_satisfies_creature_validate() {
    assert_valid(
        &graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds"),
        "a single grafted node",
    );

    let second = IfNodeSpec::new("if-2", 0.0)
        .with_constant("if-2-one", 1.0)
        .with_constant("if-2-pos", 1.0)
        .with_constant("if-2-neg", 1.0)
        .with_condition("if-1", 1.0)
        .with_condition("if-2-one", -1.0)
        .with_positive("if-2-pos", 5.0)
        .with_negative("if-2-neg", 0.0)
        .with_target("output-0", 1.0);
    assert_valid(
        &graft_if_tree(&base_creature(), &[valid_spec(), second]).expect("tree graft succeeds"),
        "a grafted tree",
    );

    let spec = IfCorrectionSpec {
        uuid: "residual-0".to_string(),
        feature_uuid: "input-0".to_string(),
        threshold: RESIDUAL_THRESHOLD,
        positive_value: RESIDUAL_VALUE,
        negative_value: 0.0,
        target_uuid: "output-0".to_string(),
        target_weight: 1.0,
    };
    assert_valid(
        &graft_if_correction(&linear_base_creature(), &spec).expect("graft succeeds"),
        "a grafted depth-1 correction",
    );
}

#[test]
fn a_graft_off_a_hidden_source_keeps_its_constants_ahead_of_every_hidden_neuron() {
    // The node's condition reads `hidden-1`, so the node itself must follow it —
    // but a constant may never follow a hidden neuron (rule 11), so the three
    // constants this graft introduces still belong at the front.
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_constant("if-1-one", 1.0)
        .with_constant("if-1-pos", 1.0)
        .with_constant("if-1-neg", 1.0)
        .with_condition("hidden-1", 1.0)
        .with_condition("if-1-one", -0.1)
        .with_positive("if-1-pos", 1.0)
        .with_negative("if-1-neg", 0.0)
        .with_target("output-0", 1.0);
    let grafted = graft_if_node(&base_creature(), &spec).expect("graft succeeds");
    assert_valid(&grafted, "a graft whose condition reads a hidden neuron");

    let order: Vec<&str> = grafted.neurons.iter().map(|n| n.uuid.as_str()).collect();
    let pos = |u: &str| order.iter().position(|o| *o == u).expect("neuron present");
    for c in ["if-1-one", "if-1-pos", "if-1-neg"] {
        assert!(
            pos(c) < pos("hidden-1"),
            "constant {c} after a hidden neuron: {order:?}"
        );
    }
    assert!(pos("hidden-1") < pos("if-1"), "order was {order:?}");
}

#[test]
fn grafted_synapses_are_in_canonical_from_to_order() {
    let grafted = graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds");
    let index: std::collections::HashMap<&str, usize> = grafted
        .neurons
        .iter()
        .enumerate()
        .map(|(i, n)| (n.uuid.as_str(), grafted.input + i))
        .collect();
    let resolve = |uuid: &str| -> usize {
        index.get(uuid).copied().unwrap_or_else(|| {
            uuid.strip_prefix("input-")
                .and_then(|n| n.parse::<usize>().ok())
                .expect("every endpoint resolves")
        })
    };
    let keys: Vec<(usize, usize)> = grafted
        .synapses
        .iter()
        .map(|s| (resolve(&s.from_uuid), resolve(&s.to_uuid)))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(
        keys, sorted,
        "synapses are not in canonical (from, to) order"
    );
}

// ---------------------------------------------------------------------------
// Typed outward edges, batched grafts and the identity relay
// (NEAT-AI-Forests #48) — the two shapes a consumer could not express with
// `graft_if_node` alone: a child feeding its parent's branch, and a correction
// entering both branches of an `IF` target.
// ---------------------------------------------------------------------------

/// Three inputs and an `IF` **output** — the production champion's shape. An
/// untyped edge into it would feed the positive branch only, so a correction
/// that must apply on both sides needs typed outward edges.
fn if_output_creature() -> CreatureExport {
    CreatureExport {
        memetic: None,
        input: 3,
        output: 1,
        neurons: vec![NeuronExport {
            id: None,
            neuron_type: "output".to_string(),
            uuid: "output-0".to_string(),
            bias: 0.0,
            squash: Some("IF".to_string()),
        }],
        synapses: vec![
            SynapseExport {
                from_uuid: "input-0".to_string(),
                to_uuid: "output-0".to_string(),
                weight: 1.0,
                synapse_type: Some("condition".to_string()),
            },
            SynapseExport {
                from_uuid: "input-1".to_string(),
                to_uuid: "output-0".to_string(),
                weight: 2.0,
                synapse_type: Some("positive".to_string()),
            },
            SynapseExport {
                from_uuid: "input-2".to_string(),
                to_uuid: "output-0".to_string(),
                weight: -1.0,
                synapse_type: Some("negative".to_string()),
            },
        ],
        semantic_version: None,
        forward_only: true,
    }
}

/// The correction node used against [`if_output_creature`]: `input-1 > 0.5`
/// contributes `0.75`, otherwise `-0.25`.
fn correction_spec(target_role: SynapseType) -> IfNodeSpec {
    IfNodeSpec::new("corr", 0.0)
        .with_constant("corr-one", 1.0)
        .with_constant("corr-pos", 1.0)
        .with_constant("corr-neg", 1.0)
        .with_condition("input-1", 1.0)
        .with_condition("corr-one", -0.5)
        .with_positive("corr-pos", 0.75)
        .with_negative("corr-neg", -0.25)
        .with_target_role("output-0", 1.0, target_role)
}

/// What the correction node itself emits for a record — derived from the spec
/// above, not from the graft, so a wiring fault moves only one side.
fn correction_value(record: &[f32]) -> f32 {
    if record[1] > 0.5 { 0.75 } else { -0.25 }
}

#[test]
fn a_typed_outward_edge_reaches_the_named_branch_of_an_if_target() {
    let base = if_output_creature();
    let grafted = graft_if_node(&base, &correction_spec(SynapseType::Positive))
        .expect("typed graft succeeds");
    assert_valid(&grafted, "a graft with a typed outward edge");

    let edge = grafted
        .synapses
        .iter()
        .find(|s| s.from_uuid == "corr" && s.to_uuid == "output-0")
        .expect("outward edge present");
    assert_eq!(edge.synapse_type.as_deref(), Some("positive"));

    // The condition of the output is `input-0`, so the positive branch is the
    // one taken when `input-0 > 0`: there the correction lands, and only there.
    let above = [1.0f32, 1.0, 1.0];
    let below = [-1.0f32, 1.0, 1.0];
    assert!(
        (activate(&grafted, &above) - (activate(&base, &above) + correction_value(&above))).abs()
            <= TOL
    );
    assert!((activate(&grafted, &below) - activate(&base, &below)).abs() <= TOL);
}

#[test]
fn an_outward_edge_with_no_role_stays_untyped() {
    let grafted = graft_if_node(&base_creature(), &valid_spec()).expect("graft succeeds");
    let edge = grafted
        .synapses
        .iter()
        .find(|s| s.from_uuid == "if-1" && s.to_uuid == "output-0")
        .expect("outward edge present");
    assert_eq!(edge.synapse_type, None);
}

#[test]
fn rejects_an_explicit_role_on_an_inbound_edge() {
    let mut spec = valid_spec();
    spec.condition[0].role = SynapseType::Negative;
    let err = graft_if_node(&base_creature(), &spec).expect_err("must be rejected");
    assert!(
        matches!(err, GraftError::InboundEdgeHasRole { ref from, ref to } if from == "input-0" && to == "if-1"),
        "unexpected error: {err}"
    );
}

/// A child that only its parent reads, described before that parent.
fn child_spec() -> IfNodeSpec {
    IfNodeSpec::new("child", 0.0)
        .with_constant("child-one", 1.0)
        .with_constant("child-pos", 1.0)
        .with_constant("child-neg", 1.0)
        .with_condition("input-0", 1.0)
        .with_condition("child-one", -0.25)
        .with_positive("child-pos", 3.0)
        .with_negative("child-neg", 1.0)
}

/// The parent whose positive branch reads `child`.
fn parent_spec() -> IfNodeSpec {
    IfNodeSpec::new("parent", 0.0)
        .with_constant("parent-one", 1.0)
        .with_constant("parent-neg", 1.0)
        .with_condition("input-1", 1.0)
        .with_condition("parent-one", -0.1)
        .with_positive("child", 1.0)
        .with_negative("parent-neg", 0.0)
        .with_target("output-0", 1.0)
}

/// The tree the two specs above describe, evaluated independently of the graft.
fn tree_value(record: &[f32]) -> f32 {
    if record[1] > 0.1 {
        if record[0] > 0.25 { 3.0 } else { 1.0 }
    } else {
        0.0
    }
}

#[test]
fn a_batched_graft_wires_a_child_into_its_parents_branch() {
    let base = base_creature();
    let grafted =
        graft_if_nodes(&base, &[child_spec(), parent_spec()]).expect("batched graft succeeds");
    assert_valid(&grafted, "a batched nested-tree graft");

    let order: Vec<&str> = grafted.neurons.iter().map(|n| n.uuid.as_str()).collect();
    let pos = |u: &str| order.iter().position(|o| *o == u).expect("neuron present");
    assert!(pos("child") < pos("parent"), "order was {order:?}");

    let edge = grafted
        .synapses
        .iter()
        .find(|s| s.from_uuid == "child" && s.to_uuid == "parent")
        .expect("child feeds its parent");
    assert_eq!(edge.synapse_type.as_deref(), Some("positive"));

    for record in [
        [0.5f32, 0.5],
        [0.1, 0.5],
        [0.5, 0.0],
        [0.1, 0.0],
        [0.26, 0.11],
    ] {
        let delta = activate(&grafted, &record) - activate(&base, &record);
        assert!(
            (delta - tree_value(&record)).abs() <= TOL,
            "record {record:?}: delta {delta} vs expected {}",
            tree_value(&record)
        );
    }
}

#[test]
fn a_batched_graft_still_refuses_a_node_nothing_ever_reads() {
    // `child` has no target and no later node names it as a branch source.
    let err = graft_if_nodes(&base_creature(), &[child_spec()]).expect_err("must be rejected");
    assert!(matches!(err, GraftError::NoTargets), "unexpected: {err}");
}

#[test]
fn a_batched_graft_is_all_or_nothing() {
    let base = base_creature();
    let broken = parent_spec().with_target("no-such-neuron", 1.0);
    let err = graft_if_nodes(&base, &[child_spec(), broken]).expect_err("second node is invalid");
    assert!(
        matches!(err, GraftError::UnknownTargetUuid(ref u) if u == "no-such-neuron"),
        "unexpected: {err}"
    );
    assert_eq!(base, base_creature(), "the source creature was mutated");
}

#[test]
fn a_batched_graft_of_nothing_returns_the_creature_unchanged() {
    let base = base_creature();
    assert_eq!(graft_if_nodes(&base, &[]).expect("empty batch"), base);
}

/// A creature carrying four bias-1 constants, all read by the output, where a
/// graft reads only the first three.
fn four_constant_creature() -> CreatureExport {
    let mut neurons: Vec<NeuronExport> = ["c-a", "c-b", "c-c", "c-d"]
        .iter()
        .map(|uuid| NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: (*uuid).to_string(),
            bias: 1.0,
            squash: None,
        })
        .collect();
    neurons.push(NeuronExport {
        id: None,
        neuron_type: "output".to_string(),
        uuid: "output-0".to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    });
    let mut synapses = vec![SynapseExport {
        from_uuid: "input-0".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    }];
    synapses.extend(
        ["c-a", "c-b", "c-c", "c-d"]
            .iter()
            .map(|uuid| SynapseExport {
                from_uuid: (*uuid).to_string(),
                to_uuid: "output-0".to_string(),
                weight: 0.0,
                synapse_type: None,
            }),
    );
    CreatureExport {
        memetic: None,
        input: 2,
        output: 1,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
    }
}

#[test]
fn a_grafted_node_is_listed_after_every_constant_the_creature_carries() {
    // Its sources reach `c-c` only, but listing the node there would leave
    // `c-d` after a hidden neuron, which rule 11 refuses.
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_condition("c-a", -0.25)
        .with_positive("c-b", 2.0)
        .with_negative("c-c", 0.0)
        .with_target("output-0", 1.0);
    let grafted = graft_if_node(&four_constant_creature(), &spec).expect("graft succeeds");
    assert_valid(&grafted, "a graft onto a creature with a trailing constant");

    let order: Vec<&str> = grafted.neurons.iter().map(|n| n.uuid.as_str()).collect();
    let pos = |u: &str| order.iter().position(|o| *o == u).expect("neuron present");
    assert!(pos("c-d") < pos("if-1"), "order was {order:?}");
}

#[test]
fn a_relay_carries_a_second_typed_edge_into_the_same_target() {
    let base = if_output_creature();
    // The correction enters the positive branch directly and the negative one
    // through the relay, so it applies whichever branch the output takes.
    let grafted = graft_if_node(&base, &correction_spec(SynapseType::Positive))
        .expect("typed graft succeeds");
    let relay = RelaySpec::new("corr-relay", 0.0)
        .with_source("corr", 1.0)
        .with_target_role("output-0", 1.0, SynapseType::Negative);
    let grafted = graft_relay_node(&grafted, &relay).expect("relay graft succeeds");
    assert_valid(&grafted, "a graft wired into both branches of an IF output");

    let node = grafted
        .neurons
        .iter()
        .find(|n| n.uuid == "corr-relay")
        .expect("relay present");
    assert_eq!(node.squash.as_deref(), Some("IDENTITY"));
    assert_eq!(node.neuron_type, "hidden");

    for record in [[1.0f32, 1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 0.0, 0.0]] {
        let delta = activate(&grafted, &record) - activate(&base, &record);
        assert!(
            (delta - correction_value(&record)).abs() <= TOL,
            "record {record:?}: delta {delta} vs expected {}",
            correction_value(&record)
        );
    }
}

/// The Issue #577 payoff: the same correction reaches **both** branches of the
/// `IF` output from the node itself, so the relay neuron the case above needs
/// is not created at all. Same behaviour, one fewer neuron and one fewer
/// synapse.
#[test]
fn a_node_reaches_both_branches_of_an_if_target_without_a_relay() {
    let base = if_output_creature();
    let spec = correction_spec(SynapseType::Positive).with_target_role(
        "output-0",
        1.0,
        SynapseType::Negative,
    );
    let grafted = graft_if_node(&base, &spec).expect("both roles into one IF target");
    assert_valid(&grafted, "a node wired into both branches of an IF output");

    let roles: Vec<Option<&str>> = grafted
        .synapses
        .iter()
        .filter(|s| s.from_uuid == "corr" && s.to_uuid == "output-0")
        .map(|s| s.synapse_type.as_deref())
        .collect();
    assert_eq!(
        roles,
        vec![Some("negative"), Some("positive")],
        "both roles are emitted, in canonical (from, to, type) order"
    );

    // Behaviour identical to the relay version, whichever branch the output
    // takes.
    for record in [[1.0f32, 1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 0.0, 0.0]] {
        let delta = activate(&grafted, &record) - activate(&base, &record);
        assert!(
            (delta - correction_value(&record)).abs() <= TOL,
            "record {record:?}: delta {delta} vs expected {}",
            correction_value(&record)
        );
    }

    // The relay-free creature really is the smaller one.
    let relay = RelaySpec::new("corr-relay", 0.0)
        .with_source("corr", 1.0)
        .with_target_role("output-0", 1.0, SynapseType::Negative);
    let with_relay = graft_relay_node(
        &graft_if_node(&base, &correction_spec(SynapseType::Positive)).expect("typed graft"),
        &relay,
    )
    .expect("relay graft succeeds");
    assert_eq!(grafted.neurons.len() + 1, with_relay.neurons.len());
    assert_eq!(grafted.synapses.len() + 1, with_relay.synapses.len());
}

/// The other half of the rule: a destination that sums every inward synapse
/// regardless of role reads two edges from one source as one, so the second is
/// redundancy — refused under its own variant.
#[test]
fn rejects_two_roles_into_a_target_that_is_not_an_if_neuron() {
    let spec = valid_spec().with_target_role("output-0", 1.0, SynapseType::Negative);
    assert!(matches!(
        graft_if_node(&base_creature(), &spec).expect_err("must be rejected"),
        GraftError::TypedDuplicateEdge { ref from, ref to } if from == "if-1" && to == "output-0"
    ));
}

/// And the same source twice in the same role stays a plain duplicate, however
/// many roles the destination reads.
#[test]
fn rejects_one_role_repeated_into_an_if_target() {
    let spec = correction_spec(SynapseType::Positive).with_target_role(
        "output-0",
        2.0,
        SynapseType::Positive,
    );
    assert!(matches!(
        graft_if_node(&if_output_creature(), &spec).expect_err("must be rejected"),
        GraftError::DuplicateEdge { ref from, ref to } if from == "corr" && to == "output-0"
    ));
}

#[test]
fn a_relay_with_no_source_or_no_target_is_refused() {
    let base = graft_if_node(
        &if_output_creature(),
        &correction_spec(SynapseType::Positive),
    )
    .expect("typed graft succeeds");
    let sourceless = RelaySpec::new("corr-relay", 0.0).with_target("output-0", 1.0);
    assert!(matches!(
        graft_relay_node(&base, &sourceless).expect_err("must be rejected"),
        GraftError::NoSources
    ));

    let targetless = RelaySpec::new("corr-relay", 0.0).with_source("corr", 1.0);
    assert!(matches!(
        graft_relay_node(&base, &targetless).expect_err("must be rejected"),
        GraftError::NoTargets
    ));
}
