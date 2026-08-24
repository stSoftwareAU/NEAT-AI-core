//! Duplicate synapse rejection, keyed by `(fromUUID, toUUID, type)`
//! (Issues #556, #577).
//!
//! NEAT-AI's TypeScript loader keys synapses by that triple, so a creature
//! carrying the same triple twice loses every copy but one before it is ever
//! scored. `compile_creature` used to accept all copies and **sum** them, so
//! the same JSON scored differently under the two engines — observed in
//! production as `rust_scorer` 0.356183 against `Creature.scoreDir` 0.353147.
//!
//! Which copy TypeScript keeps is an artefact of its map insertion order, so
//! there is no safe value for Rust to reproduce. The contract is therefore to
//! **fail closed**: a repeated triple is a typed
//! [`CreatureError::DuplicateSynapse`], never a silent divergence.
//!
//! The **role** is part of the key only where it changes the answer: an `IF`
//! neuron keeps a separate sum per role, so one source may feed two of its
//! branches. Every other squash sums its inward synapses regardless of role, so
//! two synapses from one source are exactly one with the summed weight —
//! meaningless redundancy, rejected as
//! [`CreatureError::TypedDuplicateSynapse`] (Issue #577).

use std::error::Error;

use neat_core::{
    CreatureError, CreatureExport, NeuronExport, SynapseExport, compile_creature,
    parse_creature_json, validate_no_duplicate_synapses,
};

fn synapse(from: &str, to: &str, weight: f64, synapse_type: Option<&str>) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: synapse_type.map(str::to_string),
    }
}

fn neuron(neuron_type: &str, uuid: &str, bias: f64, squash: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias,
        squash: Some(squash.to_string()),
    }
}

/// One constant neuron feeding an `IF` neuron three times — condition,
/// positive and negative — so all three synapses share the pair
/// `("c", "if-0")` and differ only in their role.
///
/// Issue #556 rejected this shape because the key was the pair alone; under
/// `(from, to, type)` keying (Issue #577) it is the canonical way to build an
/// `IF` node without a constant per branch, so it compiles and each role lands
/// in its own sum.
fn if_triple_from_one_constant() -> CreatureExport {
    CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons: vec![
            neuron("constant", "c", 1.0, "IDENTITY"),
            neuron("hidden", "if-0", 0.0, "IF"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        synapses: vec![
            synapse("c", "if-0", 1.0, Some("condition")),
            synapse("c", "if-0", 2.0, Some("positive")),
            synapse("c", "if-0", -3.0, Some("negative")),
            synapse("if-0", "output-0", 1.0, None),
        ],
        semantic_version: None,
        forward_only: true,
    }
}

/// The same creature with its `condition` edge written twice: an exact repeat
/// of `("c", "if-0", condition)`, which no keying makes meaningful.
fn repeated_condition_role() -> CreatureExport {
    let mut creature = if_triple_from_one_constant();
    creature
        .synapses
        .insert(1, synapse("c", "if-0", 4.0, Some("condition")));
    creature
}

// ---------------------------------------------------------------------------
// Rejection
// ---------------------------------------------------------------------------

#[test]
fn compile_rejects_a_repeated_role_from_one_source() {
    match compile_creature(&repeated_condition_role()) {
        Err(CreatureError::DuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "c");
            assert_eq!(to_uuid, "if-0");
        }
        Err(other) => panic!("expected DuplicateSynapse, got {other:?}"),
        Ok(_) => panic!("a repeated (from, to, type) triple must not compile"),
    }
}

/// The other half of the Issue #577 rule: two roles from one source are only
/// meaningful into an `IF` target. An IDENTITY neuron sums every inward synapse
/// regardless of role, so the pair is redundancy with no meaning — and carries
/// a **distinct** error, so a caller can tell "you repeated yourself" from
/// "that target cannot mean what you wrote".
#[test]
fn compile_rejects_two_roles_from_one_source_into_a_non_if_target() {
    let creature = CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons: vec![
            neuron("hidden", "h", 0.0, "IDENTITY"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        synapses: vec![
            synapse("input-0", "h", 1.0, Some("positive")),
            synapse("input-0", "h", 2.0, Some("negative")),
            synapse("h", "output-0", 1.0, None),
        ],
        semantic_version: None,
        forward_only: true,
    };
    match compile_creature(&creature) {
        Err(CreatureError::TypedDuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "input-0");
            assert_eq!(to_uuid, "h");
        }
        Err(other) => panic!("expected TypedDuplicateSynapse, got {other:?}"),
        Ok(_) => panic!("only an IF target may carry two roles from one source"),
    }
}

#[test]
fn compile_rejects_a_repeated_pair_with_identical_weight_and_type() {
    // Same pair, same weight, no type: the plainest possible duplicate. Rust
    // would have applied 1.0 + 1.0 = 2.0 where TypeScript applies 1.0.
    let json = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0},
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}
        ]
    }"#;
    let creature = parse_creature_json(json).expect("duplicates are a compile-time rule");
    match compile_creature(&creature) {
        Err(CreatureError::DuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "input-0");
            assert_eq!(to_uuid, "output-0");
        }
        Err(other) => panic!("expected DuplicateSynapse, got {other:?}"),
        Ok(_) => panic!("a repeated (from, to) pair must not compile"),
    }
}

#[test]
fn duplicate_error_display_names_both_endpoints() {
    let err = compile_creature(&repeated_condition_role())
        .err()
        .expect("a repeated (from, to, type) triple must not compile");
    let text = err.to_string();
    assert!(text.contains('c'), "message must name the source: {text}");
    assert!(
        text.contains("if-0"),
        "message must name the target: {text}"
    );
    // A structural failure, not a JSON one: no serde source chain.
    assert!(err.source().is_none());
}

#[test]
fn compile_reports_the_first_repeated_pair_in_declaration_order() {
    // Two independent duplicate pairs; the earlier repeat is the one reported,
    // so the message points at the first place a reader should look.
    let mut creature = if_triple_from_one_constant();
    creature.synapses = vec![
        synapse("c", "if-0", 1.0, Some("condition")),
        synapse("if-0", "output-0", 1.0, None),
        synapse("if-0", "output-0", 2.0, None),
        synapse("c", "if-0", 2.0, Some("positive")),
    ];
    match compile_creature(&creature) {
        Err(CreatureError::DuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "if-0");
            assert_eq!(to_uuid, "output-0");
        }
        Err(other) => panic!("expected DuplicateSynapse, got {other:?}"),
        Ok(_) => panic!("a repeated (from, to) pair must not compile"),
    }
}

/// A repeat of one role is a repeat whatever the target is: two `positive`
/// synapses from the same constant are `("c", "if-0", positive)` twice, and an
/// `IF` neuron sums them into the one branch it would have summed a single
/// synapse into.
#[test]
fn compile_rejects_a_repeated_pair_that_shares_one_role() {
    let mut creature = if_triple_from_one_constant();
    creature.synapses = vec![
        synapse("c", "if-0", 1.0, Some("positive")),
        synapse("c", "if-0", 2.0, Some("positive")),
        synapse("if-0", "output-0", 1.0, None),
    ];
    match compile_creature(&creature) {
        Err(CreatureError::DuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "c");
            assert_eq!(to_uuid, "if-0");
        }
        Err(other) => panic!("expected DuplicateSynapse, got {other:?}"),
        Ok(_) => panic!("a repeated (from, to) pair must not compile"),
    }
}

// ---------------------------------------------------------------------------
// Acceptance — the rule must not reject legitimate topologies
// ---------------------------------------------------------------------------

/// The Issue #577 payoff: one constant carries all three roles into an `IF`
/// neuron, so the node needs neither three constants nor an IDENTITY relay to
/// reach a second branch.
#[test]
fn compile_accepts_an_if_neuron_fed_three_times_by_one_constant() {
    let creature = if_triple_from_one_constant();
    assert!(
        validate_no_duplicate_synapses(&creature).is_ok(),
        "one source per role is one synapse per (from, to, type) key"
    );
    let mut network = compile_creature(&creature).expect("an IF target may repeat a source");

    // `c` activates at its bias of 1.0, so condition = 1.0 * 1.0 > 0 and the
    // positive branch is taken: 1.0 * 2.0 + bias 0.0 = 2.0, passed through
    // `output-0` unchanged.
    let output = network.activate(&[0.0], 1);
    assert!(
        (output[0] - 2.0).abs() < 1e-5,
        "expected 2.0, got {}",
        output[0]
    );
}

/// The negative branch of the same creature, so the second role is proven to
/// land in its own sum rather than being folded into the first.
#[test]
fn a_repeated_source_keeps_one_sum_per_role() {
    let mut creature = if_triple_from_one_constant();
    // Flip the condition weight so the condition sum is -1.0, not 1.0.
    creature.synapses[0].weight = -1.0;
    let mut network = compile_creature(&creature).expect("an IF target may repeat a source");

    // condition = 1.0 * -1.0 <= 0, so the negative branch is taken:
    // 1.0 * -3.0 + bias 0.0 = -3.0.
    let output = network.activate(&[0.0], 1);
    assert!(
        (output[0] + 3.0).abs() < 1e-5,
        "expected -3.0, got {}",
        output[0]
    );
}

#[test]
fn compile_accepts_distinct_pairs_that_share_one_endpoint() {
    // `input-0` fans out to two targets and `output-0` fans in from two
    // sources: every pair is distinct, so nothing here is a duplicate.
    let json = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "hidden", "uuid": "h", "bias": 0.0, "squash": "IDENTITY"},
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "h", "weight": 2.0},
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 3.0},
            {"fromUUID": "h", "toUUID": "output-0", "weight": 5.0}
        ]
    }"#;
    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).expect("distinct pairs must compile");

    // h = identity(1.5 * 2.0) = 3.0
    // output = identity(1.5 * 3.0 + 3.0 * 5.0) = 4.5 + 15.0 = 19.5
    let output = network.activate(&[1.5], 1);
    assert!(
        (output[0] - 19.5).abs() < 1e-5,
        "expected 19.5, got {}",
        output[0]
    );
}

#[test]
fn compile_accepts_an_if_neuron_fed_by_three_distinct_constants() {
    // The canonical IF shape the duplicate repro was a corruption of: one
    // constant per branch, so the three pairs are distinct.
    let creature = CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons: vec![
            neuron("constant", "c-cond", 1.0, "IDENTITY"),
            neuron("constant", "c-pos", 2.0, "IDENTITY"),
            neuron("constant", "c-neg", 3.0, "IDENTITY"),
            neuron("hidden", "if-0", 0.5, "IF"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        synapses: vec![
            synapse("c-cond", "if-0", 1.0, Some("condition")),
            synapse("c-pos", "if-0", 2.0, Some("positive")),
            synapse("c-neg", "if-0", -3.0, Some("negative")),
            synapse("if-0", "output-0", 1.0, None),
        ],
        semantic_version: None,
        forward_only: true,
    };
    let mut network = compile_creature(&creature).expect("distinct IF branches must compile");

    // condition = 1.0 * 1.0 = 1.0 > 0, so the positive branch is taken:
    // if-0 = 2.0 * 2.0 + bias 0.5 = 4.5; output = identity(4.5 * 1.0) = 4.5
    let output = network.activate(&[0.0], 1);
    assert!(
        (output[0] - 4.5).abs() < 1e-5,
        "expected 4.5, got {}",
        output[0]
    );
}

/// A **repeated role** into one neuron is legal as long as the sources differ:
/// the key is `(from, to, type)`, so distinct sources make distinct keys
/// whatever the roles say. Two `positive` synapses into the same `IF` neuron
/// therefore compile, and both contribute to the branch they name.
#[test]
fn compile_accepts_repeated_roles_into_one_neuron_when_the_sources_differ() {
    let creature = CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons: vec![
            neuron("constant", "k-cond", 1.0, "IDENTITY"),
            neuron("constant", "k-pos-a", 1.0, "IDENTITY"),
            neuron("constant", "k-pos-b", 1.0, "IDENTITY"),
            neuron("constant", "k-neg", 1.0, "IDENTITY"),
            neuron("hidden", "if-0", 0.5, "IF"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        synapses: vec![
            synapse("k-cond", "if-0", 1.0, Some("condition")),
            synapse("k-pos-a", "if-0", 2.0, Some("positive")),
            synapse("k-pos-b", "if-0", 3.0, Some("positive")),
            synapse("k-neg", "if-0", -4.0, Some("negative")),
            synapse("if-0", "output-0", 1.0, None),
        ],
        semantic_version: None,
        forward_only: true,
    };
    assert!(
        validate_no_duplicate_synapses(&creature).is_ok(),
        "distinct sources make distinct pairs, whatever the roles say"
    );
    let mut network = compile_creature(&creature).expect("repeated roles must compile");

    // Each constant activates at its own bias of 1.0. condition = 1.0 * 1.0 =
    // 1.0 > 0, so the positive branch is taken and *both* positive synapses
    // contribute: 1.0 * 2.0 + 1.0 * 3.0 + bias 0.5 = 5.5.
    let output = network.activate(&[0.0], 1);
    assert!(
        (output[0] - 5.5).abs() < 1e-5,
        "expected 5.5, got {}",
        output[0]
    );
}

#[test]
fn compile_accepts_a_creature_with_no_synapses_at_all() {
    let json = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.25, "squash": "IDENTITY"}
        ],
        "synapses": []
    }"#;
    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).expect("an empty synapse list has no duplicates");
    let output = network.activate(&[1.0], 1);
    assert!(
        (output[0] - 0.25).abs() < 1e-5,
        "expected 0.25 bias passthrough"
    );
}

// ---------------------------------------------------------------------------
// Consumer boundary
// ---------------------------------------------------------------------------

#[test]
fn validate_no_duplicate_synapses_answers_for_a_hand_built_creature() {
    // Consumers that build a `CreatureExport` in Rust — never touching
    // `parse_creature_json` — can apply the same rule at their own boundary.
    let duplicated = repeated_condition_role();
    match validate_no_duplicate_synapses(&duplicated) {
        Err(CreatureError::DuplicateSynapse { from_uuid, to_uuid }) => {
            assert_eq!(from_uuid, "c");
            assert_eq!(to_uuid, "if-0");
        }
        other => panic!("expected DuplicateSynapse, got {other:?}"),
    }

    let mut cleaned = duplicated;
    cleaned.synapses = vec![
        synapse("c", "if-0", 1.0, Some("condition")),
        synapse("if-0", "output-0", 1.0, None),
    ];
    assert!(validate_no_duplicate_synapses(&cleaned).is_ok());
}
