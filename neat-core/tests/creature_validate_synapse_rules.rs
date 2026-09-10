//! Issue #561 — the synapse, forward-only and memetic half of
//! `creature_validate`, ported from NEAT-AI `src/architecture/CreatureValidate.ts`.
//!
//! Each rule gets a positive and a negative case, and the messages are asserted
//! verbatim: NEAT-AI's own error-message tests read this text, so a drifted
//! string is a broken contract rather than cosmetic.

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightExport, MemeticWeights};
use neat_core::creature_validate::{
    FailureClass, ValidateOptions, ValidationStats, creature_validate, reason,
    validate_synapse_and_memetic_rules,
};
use neat_core::{
    CreatureExport, NeuronExport, SynapseExport, stump_creature, validate_no_duplicate_synapses,
};

// ---------------------------------------------------------------------------
// Fixtures — index space is `0..input` implicit inputs, then `neurons` in order.
// ---------------------------------------------------------------------------

fn neuron(neuron_type: &str, uuid: &str, id: i64) -> NeuronExport {
    NeuronExport {
        id: Some(id),
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias: 0.5,
        squash: Some("IDENTITY".to_string()),
    }
}

fn edge(from: &str, to: &str) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight: 1.0,
        synapse_type: None,
    }
}

/// `input-0 -> h -> o`; indices `0`, `1`, `2`.
fn linear_creature() -> CreatureExport {
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![neuron("hidden", "h", 1), neuron("output", "o", -1)],
        synapses: vec![edge("input-0", "h"), edge("h", "o")],
        semantic_version: None,
        forward_only: false,
        memetic: None,
    }
}

/// `input-0 -> h1 -> h2 -> o`; indices `0`, `1`, `2`, `3`.
fn two_hidden_creature() -> CreatureExport {
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![
            neuron("hidden", "h1", 1),
            neuron("hidden", "h2", 2),
            neuron("output", "o", -1),
        ],
        synapses: vec![edge("input-0", "h1"), edge("h1", "h2"), edge("h2", "o")],
        semantic_version: None,
        forward_only: false,
        memetic: None,
    }
}

/// Run the half under test from a fresh `stats`, returning the tally.
fn run(creature: &CreatureExport, options: &ValidateOptions) -> ValidationStats {
    let mut stats = ValidationStats::default();
    validate_synapse_and_memetic_rules(creature, options, &mut stats)
        .unwrap_or_else(|failure| panic!("expected a valid creature, was: {failure}"));
    stats
}

/// Run the half under test expecting the first violated rule.
fn run_err(
    creature: &CreatureExport,
    options: &ValidateOptions,
) -> neat_core::creature_validate::ValidationFailure {
    let mut stats = ValidationStats::default();
    validate_synapse_and_memetic_rules(creature, options, &mut stats)
        .expect_err("expected a validation failure")
}

// ---------------------------------------------------------------------------
// Rule 23 — a synapse may not point at an input neuron.
// ---------------------------------------------------------------------------

#[test]
fn a_feed_forward_creature_passes_and_tallies_its_synapses() {
    let stats = run(&linear_creature(), &ValidateOptions::default());
    assert_eq!(stats.connections, 2, "one tally per synapse walked");
}

#[test]
fn a_synapse_pointing_at_an_input_neuron_is_an_invalid_connection() {
    let mut creature = linear_creature();
    creature.synapses.push(edge("h", "input-0"));

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::INVALID_CONNECTION);
    assert_eq!(failure.message, "2) connection points to an input node");
    assert_eq!(failure.synapse_index, Some(2));
}

// ---------------------------------------------------------------------------
// Rule 24 — a self connection fails only under `forward_only`.
// ---------------------------------------------------------------------------

#[test]
fn a_self_connection_is_legal_by_default() {
    let mut creature = linear_creature();
    creature.synapses.insert(1, edge("h", "h"));

    let stats = run(&creature, &ValidateOptions::default());
    assert_eq!(stats.connections, 3);
}

#[test]
fn a_self_connection_is_rejected_under_forward_only() {
    let mut creature = linear_creature();
    creature.synapses.insert(1, edge("h", "h"));

    let failure = run_err(
        &creature,
        &ValidateOptions {
            forward_only: true,
            ..ValidateOptions::default()
        },
    );

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::SELF_CONNECTION);
    assert_eq!(failure.message, "1) Self connection synapse h -> h");
    assert_eq!(failure.synapse_index, Some(1));
}

// ---------------------------------------------------------------------------
// Rule 25 — synapses are sorted by `(from, to)`.
// ---------------------------------------------------------------------------

#[test]
fn a_backwards_step_in_from_is_a_sort_failure() {
    let mut creature = linear_creature();
    creature.synapses.swap(0, 1);

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::SORT_FAILURE);
    assert_eq!(failure.message, "1) synapses not sorted");
    assert_eq!(failure.synapse_index, Some(1));
}

#[test]
fn a_backwards_step_in_to_within_one_from_names_the_last_to() {
    let mut creature = two_hidden_creature();
    // from 0 targets h2 (index 2) before h1 (index 1).
    creature.synapses = vec![
        edge("input-0", "h2"),
        edge("input-0", "h1"),
        edge("h1", "h2"),
        edge("h2", "o"),
    ];

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.reason, reason::SORT_FAILURE);
    assert_eq!(failure.message, "1) synapses not sorted 0->1 last to: 2");
}

#[test]
fn the_last_to_resets_when_from_advances() {
    let mut creature = two_hidden_creature();
    // `input-0 -> h2` then `h1 -> h2`: the second `to` is not a regression
    // because `from` advanced, so `last_to` was reset to -1.
    creature.synapses = vec![
        edge("input-0", "h1"),
        edge("input-0", "h2"),
        edge("h1", "h2"),
        edge("h2", "o"),
    ];

    assert_eq!(run(&creature, &ValidateOptions::default()).connections, 4);
}

// ---------------------------------------------------------------------------
// Rule 26 — duplicate `(from, to)` pair (Issue #556 parity).
// ---------------------------------------------------------------------------

#[test]
fn an_adjacent_duplicate_pair_is_an_invalid_connection() {
    let mut creature = linear_creature();
    creature.synapses.insert(1, edge("input-0", "h"));

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::INVALID_CONNECTION);
    assert_eq!(failure.message, "1) duplicate synapse input-0 -> h");
    assert_eq!(failure.synapse_index, Some(1));
}

/// A duplicate that is *not* adjacent cannot reach the duplicate branch — the
/// sorted-adjacency walk stops on the sort regression that separating the two
/// copies necessarily creates. Both paths still reject the creature, which is
/// the invariant Issue #556 cares about; only the reported reason differs.
#[test]
fn a_non_adjacent_duplicate_pair_is_still_rejected_by_both_paths() {
    let mut creature = two_hidden_creature();
    creature.synapses = vec![
        edge("input-0", "h1"),
        edge("input-0", "h2"),
        edge("input-0", "h1"),
        edge("h1", "h2"),
        edge("h2", "o"),
    ];

    let failure = run_err(&creature, &ValidateOptions::default());
    assert_eq!(failure.reason, reason::SORT_FAILURE);
    assert_eq!(failure.message, "2) synapses not sorted 0->1 last to: 2");

    assert!(
        validate_no_duplicate_synapses(&creature).is_err(),
        "Issue #556's pair check rejects the same creature regardless of order"
    );
}

// ---------------------------------------------------------------------------
// Rule 26 — the role plays no part in it (Issue #572).
// ---------------------------------------------------------------------------

/// An `IF` neuron fed **two synapses of each role**, one per distinct constant:
/// six inward pairs, none of them repeated. Rule 12 wants one of each role and
/// rule 26 wants each `(from, to)` pair at most once — both hold here, so the
/// creature is legal even though three roles each arrive twice.
fn doubled_role_if_creature() -> CreatureExport {
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![
            constant("k-cond-a", 1),
            constant("k-cond-b", 2),
            constant("k-pos-a", 3),
            constant("k-pos-b", 4),
            constant("k-neg-a", 5),
            constant("k-neg-b", 6),
            neuron("hidden", "if-0", 7),
            neuron("output", "o", -1),
        ],
        synapses: vec![
            role_edge("k-cond-a", "if-0", "condition"),
            role_edge("k-cond-b", "if-0", "condition"),
            role_edge("k-pos-a", "if-0", "positive"),
            role_edge("k-pos-b", "if-0", "positive"),
            role_edge("k-neg-a", "if-0", "negative"),
            role_edge("k-neg-b", "if-0", "negative"),
            edge("if-0", "o"),
        ],
        semantic_version: None,
        forward_only: false,
        memetic: None,
    }
}

/// A constant carries no squash (rule 15) and the `bias = 1.0` the `IF`
/// branches read as their value.
fn constant(uuid: &str, id: i64) -> NeuronExport {
    NeuronExport {
        id: Some(id),
        neuron_type: "constant".to_string(),
        uuid: uuid.to_string(),
        bias: 1.0,
        squash: None,
    }
}

fn role_edge(from: &str, to: &str, role: &str) -> SynapseExport {
    SynapseExport {
        synapse_type: Some(role.to_string()),
        ..edge(from, to)
    }
}

/// The whole entry point, not just this half: a repeated role must clear the
/// `IF` rule as well as the duplicate one. A downstream production report reads
/// as though same-role fan-in into one neuron were forbidden — it is not; only
/// an exact repeat of the ordered `(from, to)` pair is.
#[test]
fn same_role_fan_in_from_distinct_sources_breaks_no_rule() {
    let mut creature = doubled_role_if_creature();
    creature.neurons[6].squash = Some("IF".to_string());

    let stats = creature_validate(&creature, &ValidateOptions::default())
        .expect("repeated roles from distinct sources are a legal creature");

    assert_eq!(stats.constant, 6);
    assert_eq!(stats.connections, 7, "one tally per synapse walked");
}

/// The same creature with one of those pairs repeated: same role, same source,
/// same target — and now rule 26 stops it.
#[test]
fn repeating_one_pair_of_that_creature_is_still_a_duplicate() {
    let mut creature = doubled_role_if_creature();
    creature
        .synapses
        .insert(1, role_edge("k-cond-a", "if-0", "condition"));

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::INVALID_CONNECTION);
    assert_eq!(failure.message, "1) duplicate synapse k-cond-a -> if-0");
    assert_eq!(failure.synapse_index, Some(1));
}

// ---------------------------------------------------------------------------
// Rules 25 and 26 — the role completes the key, and only an `IF` target may
// use it (Issue #577).
// ---------------------------------------------------------------------------

/// `k` (a constant) feeds `if-0` under all three roles, and `if-0` feeds the
/// output: one source, three branches, no relay neuron and no constant per
/// branch. Sorted by `(from, to, type)`, which is condition, negative,
/// positive.
fn shared_source_if_creature() -> CreatureExport {
    let mut if_neuron = neuron("hidden", "if-0", 2);
    if_neuron.squash = Some("IF".to_string());
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![constant("k", 1), if_neuron, neuron("output", "o", -1)],
        synapses: vec![
            role_edge("k", "if-0", "condition"),
            role_edge("k", "if-0", "negative"),
            role_edge("k", "if-0", "positive"),
            edge("if-0", "o"),
        ],
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

#[test]
fn one_source_may_carry_every_role_into_an_if_target() {
    let creature = shared_source_if_creature();
    let stats = run(&creature, &ValidateOptions::default());
    assert_eq!(stats.connections, 4, "one tally per synapse walked");

    // The whole entry point agrees, forward-only leg included: the index-level
    // topology gate has to key the pair by role too, or it would call the
    // second edge a duplicate connection.
    let options = ValidateOptions {
        forward_only: true,
        ..ValidateOptions::default()
    };
    creature_validate(&creature, &options).expect("a forward-only creature is valid too");
    assert!(
        validate_no_duplicate_synapses(&creature).is_ok(),
        "and so does the order-independent pair rule"
    );
}

#[test]
fn a_repeated_role_into_an_if_target_is_still_an_invalid_connection() {
    let mut creature = shared_source_if_creature();
    creature
        .synapses
        .insert(1, role_edge("k", "if-0", "condition"));

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::INVALID_CONNECTION);
    assert_eq!(failure.message, "1) duplicate synapse k -> if-0");
    assert_eq!(failure.synapse_index, Some(1));
}

/// Rule 25's third leg: within a repeated pair the roles ascend, so the total
/// order stays total.
#[test]
fn roles_out_of_order_within_one_pair_are_a_sort_failure() {
    let mut creature = shared_source_if_creature();
    creature.synapses.swap(0, 1);

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::SORT_FAILURE);
    assert_eq!(
        failure.message,
        "1) synapses not sorted 1->2 type: condition last type: negative"
    );
    assert_eq!(failure.synapse_index, Some(1));
}

/// The distinct code the issue asks for: "that target cannot mean what you
/// wrote" is a `ValidationError` / `DUPLICATE_SYNAPSE`, not the
/// `TopologyError` / `INVALID_CONNECTION` a plain repeat carries.
#[test]
fn two_roles_into_a_non_if_target_are_a_duplicate_synapse() {
    let mut creature = shared_source_if_creature();
    // The same wiring, but the target sums every inward synapse regardless of
    // role, so the second edge says nothing the first does not.
    creature.neurons[1].squash = Some("IDENTITY".to_string());

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::DUPLICATE_SYNAPSE);
    assert_eq!(
        failure.message,
        "1) synapse k -> if-0 repeats a source into a non-'IF' neuron"
    );
    assert_eq!(failure.synapse_index, Some(1));
}

// ---------------------------------------------------------------------------
// Rule 27 — recursive synapse, `feedback_loop` tri-state.
// ---------------------------------------------------------------------------

/// `input-0 -> h -> o` plus the backward `o -> h`, still sorted by `from`.
fn recursive_creature() -> CreatureExport {
    let mut creature = linear_creature();
    creature.synapses.push(edge("o", "h"));
    creature
}

#[test]
fn a_recursive_synapse_is_legal_when_feedback_loop_is_unset() {
    let stats = run(&recursive_creature(), &ValidateOptions::default());
    assert_eq!(stats.connections, 3);
}

#[test]
fn a_recursive_synapse_is_legal_when_feedback_loop_is_explicitly_true() {
    let options = ValidateOptions {
        feedback_loop: Some(true),
        ..ValidateOptions::default()
    };
    assert_eq!(run(&recursive_creature(), &options).connections, 3);
}

#[test]
fn a_recursive_synapse_is_rejected_when_feedback_loop_is_explicitly_false() {
    let options = ValidateOptions {
        feedback_loop: Some(false),
        ..ValidateOptions::default()
    };

    let failure = run_err(&recursive_creature(), &options);

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::RECURSIVE_SYNAPSE);
    assert_eq!(failure.message, "2) Recursive synapse output-0 -> h");
    assert_eq!(failure.synapse_index, Some(2));
}

/// `forwardOnly` forces `feedbackLoop` to false, so a caller asking for both
/// gets the forward-only answer.
#[test]
fn forward_only_rejects_a_recursive_synapse_even_when_feedback_loop_was_requested() {
    let options = ValidateOptions {
        forward_only: true,
        feedback_loop: Some(true),
        ..ValidateOptions::default()
    };

    let failure = run_err(&recursive_creature(), &options);
    assert_eq!(failure.reason, reason::RECURSIVE_SYNAPSE);
}

// ---------------------------------------------------------------------------
// Rule 28 — the `connections` count option, checked after the walk.
// ---------------------------------------------------------------------------

#[test]
fn a_matching_connection_count_passes() {
    let options = ValidateOptions {
        connections: Some(2),
        ..ValidateOptions::default()
    };
    assert_eq!(run(&linear_creature(), &options).connections, 2);
}

#[test]
fn a_mismatched_connection_count_is_reported_after_the_walk() {
    let options = ValidateOptions {
        connections: Some(5),
        ..ValidateOptions::default()
    };

    let failure = run_err(&linear_creature(), &options);

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::OTHER);
    assert_eq!(failure.message, "Synapses length: 2 expected: 5");
    assert_eq!(failure.synapse_index, None);
}

// ---------------------------------------------------------------------------
// Rules 29 / 30 — the forward-only leg reuses `topology_ops.rs`.
// ---------------------------------------------------------------------------

#[test]
fn a_forward_only_decision_stump_passes_the_whole_half() {
    let options = ValidateOptions {
        forward_only: true,
        ..ValidateOptions::default()
    };

    assert_eq!(run(&stump_creature(), &options).connections, 4);
}

/// A hidden neuron with an inward but no outward synapse breaks structural
/// integrity — the check `validate_structural_integrity` already owns.
#[test]
fn forward_only_structural_integrity_catches_a_dead_end_hidden_neuron() {
    let mut creature = linear_creature();
    creature.synapses = vec![edge("input-0", "h"), edge("input-0", "o")];

    // Legal without `forward_only`: the structural leg does not run at all.
    assert_eq!(run(&creature, &ValidateOptions::default()).connections, 2);

    let failure = run_err(
        &creature,
        &ValidateOptions {
            forward_only: true,
            ..ValidateOptions::default()
        },
    );

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::OTHER);
    assert_eq!(
        failure.message,
        "WASM structural validation failed: Hidden neuron has no outward connections at neuron 1"
    );
    assert_eq!(failure.neuron_index, Some(1));
}

// ---------------------------------------------------------------------------
// Input format — an endpoint naming no neuron.
// ---------------------------------------------------------------------------

#[test]
fn a_synapse_endpoint_naming_no_neuron_is_an_invalid_reference() {
    let mut creature = linear_creature();
    creature.synapses.push(edge("h", "ghost"));

    let failure = run_err(&creature, &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Topology);
    assert_eq!(failure.reason, reason::INVALID_SYNAPSE_REFERENCE);
    assert_eq!(
        failure.message,
        "2) synapse to ghost does not name a neuron"
    );
    assert_eq!(failure.synapse_index, Some(2));
}

// ---------------------------------------------------------------------------
// Rule 31 — memetic cross-references.
// ---------------------------------------------------------------------------

fn memetic_creature(memetic: MemeticExport) -> CreatureExport {
    let mut creature = linear_creature();
    creature.memetic = Some(memetic);
    creature
}

fn weights(key: &str, entries: Vec<MemeticWeightExport>) -> MemeticExport {
    MemeticExport {
        weights: MemeticWeights::ById(BTreeMap::from([(key.to_string(), entries)])),
        ..MemeticExport::default()
    }
}

fn weight(to_id: i64, value: f64) -> MemeticWeightExport {
    MemeticWeightExport {
        to_id: Some(to_id),
        weight: Some(value),
    }
}

#[test]
fn a_memetic_block_that_resolves_passes() {
    let mut memetic = weights("1", vec![weight(-1, 0.25)]);
    memetic.biases.insert("-1".to_string(), 0.5);

    let stats = run(&memetic_creature(memetic), &ValidateOptions::default());
    assert_eq!(stats.connections, 2);
}

#[test]
fn a_memetic_bias_for_an_unknown_neuron_id_is_rejected() {
    let mut memetic = MemeticExport::default();
    memetic.biases.insert("99".to_string(), 0.5);

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Neuron with id 99 not found in the creature."
    );
}

#[test]
fn a_memetic_weights_key_that_is_not_a_neuron_id_is_rejected() {
    let memetic = weights("99", vec![weight(-1, 0.25)]);

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Synapse with id 99 not found in the creature."
    );
}

#[test]
fn a_memetic_entry_without_a_to_id_is_rejected() {
    let memetic = weights(
        "1",
        vec![MemeticWeightExport {
            to_id: None,
            weight: Some(0.25),
        }],
    );

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 1 to id undefined is invalid."
    );
}

#[test]
fn a_memetic_entry_without_a_weight_is_rejected_with_its_index() {
    let memetic = weights(
        "1",
        vec![
            weight(-1, 0.25),
            MemeticWeightExport {
                to_id: Some(-1),
                weight: None,
            },
        ],
    );

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 1 to id -1 has invalid weight at index 1."
    );
}

#[test]
fn a_memetic_to_id_that_is_not_a_neuron_is_rejected() {
    let memetic = weights("1", vec![weight(42, 0.25)]);

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(failure.message, "Memetic from id 1 has no valid neuron.");
}

#[test]
fn a_memetic_pair_with_no_matching_synapse_is_rejected() {
    // Both ids resolve (input id 0 and output id -1) but no `0 -> -1` synapse
    // exists: `input-0` feeds the hidden neuron, not the output.
    let memetic = weights("0", vec![weight(-1, 0.25)]);

    let failure = run_err(&memetic_creature(memetic), &ValidateOptions::default());

    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message,
        "Memetic from id 0 to id -1 has no matching synapses."
    );
}

/// The synapse set is keyed by **neuron id**, not by index. Here the hidden
/// neuron's id (`7`) is deliberately not its index (`1`) and the output's id
/// (`-1`) is not its index (`2`), so index-based matching would fail to find
/// the `7 -> -1` synapse this memetic entry names.
#[test]
fn memetic_matching_is_by_neuron_id_not_by_index() {
    let mut creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![neuron("hidden", "h", 7), neuron("output", "o", -1)],
        synapses: vec![edge("input-0", "h"), edge("h", "o")],
        semantic_version: None,
        forward_only: false,
        memetic: None,
    };
    creature.memetic = Some(weights("7", vec![weight(-1, 0.25)]));

    assert_eq!(run(&creature, &ValidateOptions::default()).connections, 2);

    // The index-shaped pair `1 -> 2` must *not* satisfy the rule.
    creature.memetic = Some(weights("1", vec![weight(2, 0.25)]));
    let failure = run_err(&creature, &ValidateOptions::default());
    assert_eq!(failure.reason, reason::MEMETIC);
    assert_eq!(
        failure.message, "Synapse with id 1 not found in the creature.",
        "id 1 belongs to no neuron — only indices look like that"
    );
}
