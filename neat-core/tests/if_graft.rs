//! Safe IF-node grafting onto a `CreatureExport` (Issue #555).
//!
//! Every assertion is on an observable outcome — the returned creature, the
//! compiled network's activations, or the typed error — never on how the helper
//! got there.

use neat_core::decision_tree::{
    RESIDUAL_THRESHOLD, RESIDUAL_VALUE, linear_base_creature, residual_correction_creature,
};
use neat_core::if_graft::{
    GraftError, IfCorrectionSpec, IfNodeSpec, graft_if_correction, graft_if_node, graft_if_tree,
    validate_creature_topology,
};
use neat_core::topology_ops::{
    BACKWARD_CONNECTION, DUPLICATE_CONNECTION, STRUCTURAL_HIDDEN_NO_OUTWARD,
    STRUCTURAL_IF_MISSING_CONDITION, STRUCTURAL_SYNAPSE_TARGETS_INPUT,
};
use neat_core::{CreatureExport, NeuronExport, SynapseExport, SynapseType, compile_creature};

const TOL: f32 = 1e-6;

/// Two inputs, one hidden TANH neuron, one identity output. Deliberately not an
/// IF creature — the graft has to add every role itself.
fn base_creature() -> CreatureExport {
    CreatureExport {
        input: 2,
        output: 1,
        neurons: vec![
            NeuronExport {
                neuron_type: "hidden".to_string(),
                uuid: "hidden-1".to_string(),
                bias: 0.0,
                squash: Some("TANH".to_string()),
            },
            NeuronExport {
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
    for s in &net.synapses {
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

#[test]
fn rejects_two_synapses_between_the_same_pair() {
    let spec = IfNodeSpec::new("if-1", 0.0)
        .with_condition("input-0", 1.0)
        .with_positive("input-0", 1.0)
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
