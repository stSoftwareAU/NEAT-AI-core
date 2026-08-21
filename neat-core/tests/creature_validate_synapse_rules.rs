//! Issue #561 — the synapse, forward-only and memetic half of
//! `creature_validate`, ported from NEAT-AI `src/architecture/CreatureValidate.ts`.
//!
//! Each rule gets a positive and a negative case, and the messages are asserted
//! verbatim: NEAT-AI's own error-message tests read this text, so a drifted
//! string is a broken contract rather than cosmetic.

use std::collections::BTreeMap;

use neat_core::creature::{MemeticExport, MemeticWeightExport, MemeticWeights};
use neat_core::creature_validate::{
    FailureClass, ValidateOptions, ValidationStats, reason, validate_synapse_and_memetic_rules,
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
