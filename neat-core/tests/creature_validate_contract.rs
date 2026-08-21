//! Issue #559 — the `creature_validate` contract: options semantics, the
//! success value, the structured failure and the input format.
//!
//! These are **contract** tests. The rule bodies are ported by the follow-up
//! issues (NEAT-AI#3801 / #3802); what is pinned here is the interface those
//! ports must fill in — the option resolution NEAT-AI's TypeScript performs
//! before any rule runs, the exact `reason` string sets, the fail-loud
//! constructor guard, and the creature JSON shape the validator reads.

use neat_core::creature::{MemeticExport, MemeticWeightExport};
use neat_core::creature_validate::{
    FailureClass, TOPOLOGY_REASONS, VALIDATION_REASONS, ValidateOptions, ValidationFailure,
    ValidationStats, creature_validate, reason,
};
use neat_core::{CreatureExport, NeuronExport, creature_to_json, parse_creature_json};

// ---------------------------------------------------------------------------
// 1. Options — the resolution TypeScript performs before any rule runs.
// ---------------------------------------------------------------------------

/// `const feedbackLoop = forwardOnly ? false : options?.feedbackLoop;` — a
/// caller asking for both gets the forward-only answer, not its own.
#[test]
fn forward_only_overrides_an_explicit_feedback_loop_request() {
    let options = ValidateOptions {
        forward_only: true,
        feedback_loop: Some(true),
        ..ValidateOptions::default()
    };

    assert_eq!(options.resolved_feedback_loop(), Some(false));
    assert!(options.rejects_recursive_synapses());
}

/// Without `forward_only`, `feedback_loop` passes through untouched and only
/// an explicit `Some(false)` rejects `from > to`.
#[test]
fn only_an_explicit_false_feedback_loop_rejects_recursive_synapses() {
    let allowed_by_default = ValidateOptions::default();
    assert_eq!(allowed_by_default.resolved_feedback_loop(), None);
    assert!(!allowed_by_default.rejects_recursive_synapses());

    let allowed_explicitly = ValidateOptions {
        feedback_loop: Some(true),
        ..ValidateOptions::default()
    };
    assert_eq!(allowed_explicitly.resolved_feedback_loop(), Some(true));
    assert!(!allowed_explicitly.rejects_recursive_synapses());

    let rejected = ValidateOptions {
        feedback_loop: Some(false),
        ..ValidateOptions::default()
    };
    assert_eq!(rejected.resolved_feedback_loop(), Some(false));
    assert!(rejected.rejects_recursive_synapses());
}

/// The two count options are deliberately asymmetric in TypeScript:
/// `if (options && options.neurons)` is a **truthiness** test, so a zero
/// expected neuron count is skipped, while `Number.isInteger(options.connections)`
/// checks zero like any other integer.
#[test]
fn zero_expected_neuron_count_is_skipped_but_zero_connections_is_checked() {
    let zeros = ValidateOptions {
        neurons: Some(0),
        connections: Some(0),
        ..ValidateOptions::default()
    };

    assert_eq!(zeros.expected_neurons(), None);
    assert_eq!(zeros.expected_connections(), Some(0));

    let counted = ValidateOptions {
        neurons: Some(7),
        connections: Some(9),
        ..ValidateOptions::default()
    };
    assert_eq!(counted.expected_neurons(), Some(7));
    assert_eq!(counted.expected_connections(), Some(9));

    let unset = ValidateOptions::default();
    assert_eq!(unset.expected_neurons(), None);
    assert_eq!(unset.expected_connections(), None);
}

// ---------------------------------------------------------------------------
// 2. Success value.
// ---------------------------------------------------------------------------

/// `ValidationStats` mirrors the `stats` object `creatureValidate` returns, and
/// starts at zero for every counter the way the TypeScript literal does.
#[test]
fn validation_stats_start_at_zero_and_carry_all_five_counters() {
    assert_eq!(
        ValidationStats::default(),
        ValidationStats {
            input: 0,
            constant: 0,
            hidden: 0,
            output: 0,
            connections: 0,
        }
    );

    let stats = ValidationStats {
        input: 2,
        constant: 1,
        hidden: 3,
        output: 1,
        connections: 8,
    };
    assert_eq!(stats.neurons(), 7, "input + constant + hidden + output");
}

// ---------------------------------------------------------------------------
// 3. Failure value.
// ---------------------------------------------------------------------------

/// The permitted `reason` values are the TypeScript unions verbatim, in
/// declaration order — NEAT-AI rehydrates its own error type from the string,
/// so a drifted name silently loses the failure mode.
#[test]
fn reason_sets_reproduce_the_typescript_unions_verbatim() {
    assert_eq!(
        VALIDATION_REASONS,
        [
            "OTHER",
            "NEURON_ORDER",
            "NO_OUTWARD_CONNECTIONS",
            "NO_INWARD_CONNECTIONS",
            "IF_CONDITIONS",
            "RECURSIVE_SYNAPSE",
            "SELF_CONNECTION",
            "DUPLICATE_SYNAPSE",
            "MEMETIC",
        ],
        "src/errors/ValidationError.ts ValidationErrorName"
    );
    assert_eq!(
        TOPOLOGY_REASONS,
        [
            "INVALID_NEURON_TYPE",
            "INVALID_NEURON_BIAS",
            "INVALID_SQUASH",
            "INVALID_SYNAPSE_WEIGHT",
            "INVALID_SYNAPSE_REFERENCE",
            "MISSING_SQUASH",
            "INVALID_CONNECTION",
            "INVALID_STATE",
            "DUPLICATE_UUID",
            "MISSING_NEURON",
            "MISSING_NEURON_UUID",
            "SORT_FAILURE",
            "EXCESSIVE_ERRORS",
        ],
        "src/errors/TopologyError.ts TopologyErrorReason"
    );

    assert_eq!(
        FailureClass::Validation.permitted_reasons(),
        VALIDATION_REASONS.as_slice()
    );
    assert_eq!(
        FailureClass::Topology.permitted_reasons(),
        TOPOLOGY_REASONS.as_slice()
    );
}

/// Each class accepts only its own union — the named constants are typed by
/// where they live, so a topology reason never reaches a `ValidationError`.
#[test]
fn each_class_permits_only_its_own_union() {
    assert!(FailureClass::Validation.permits(reason::MEMETIC));
    assert!(!FailureClass::Validation.permits(reason::SORT_FAILURE));

    assert!(FailureClass::Topology.permits(reason::SORT_FAILURE));
    assert!(!FailureClass::Topology.permits(reason::MEMETIC));

    assert!(!FailureClass::Validation.permits("NOT_A_REASON"));
    assert!(!FailureClass::Topology.permits("NOT_A_REASON"));
}

/// A failure carries the class, the verbatim reason, the human-readable text
/// and the index of whatever it was looking at.
#[test]
fn failure_carries_class_reason_message_and_the_offending_index() {
    let failure = ValidationFailure::validation(
        reason::RECURSIVE_SYNAPSE,
        "3) Recursive synapse hidden-1 -> hidden-0",
    )
    .at_synapse(3);

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, "RECURSIVE_SYNAPSE");
    assert_eq!(failure.message, "3) Recursive synapse hidden-1 -> hidden-0");
    assert_eq!(failure.synapse_index, Some(3));
    assert_eq!(failure.neuron_index, None);

    let neuron_failure = ValidationFailure::topology(
        reason::INVALID_SQUASH,
        "Node uuid-c 'constant' has squash: TANH",
    )
    .at_neuron(4);

    assert_eq!(neuron_failure.class, FailureClass::Topology);
    assert_eq!(neuron_failure.reason, "INVALID_SQUASH");
    assert_eq!(neuron_failure.neuron_index, Some(4));
    assert_eq!(neuron_failure.synapse_index, None);
}

/// `Display` names the TypeScript error class and reason so a Rust-side log
/// line reads the same as the TypeScript one it mirrors.
#[test]
fn failure_display_names_the_typescript_error_class_and_reason() {
    let failure = ValidationFailure::validation(reason::NO_INWARD_CONNECTIONS, "hidden-2 is orphaned");

    assert_eq!(
        failure.to_string(),
        "ValidationError(NO_INWARD_CONNECTIONS): hidden-2 is orphaned"
    );
    assert_eq!(
        ValidationFailure::topology(reason::SORT_FAILURE, "5) synapses not sorted").to_string(),
        "TopologyError(SORT_FAILURE): 5) synapses not sorted"
    );
}

/// A reason outside the class's union is a programming error and fails loud
/// rather than crossing the WASM boundary as a string NEAT-AI cannot rehydrate.
#[test]
#[should_panic(expected = "not a ValidationError reason")]
fn a_topology_reason_on_a_validation_failure_fails_loud() {
    let _ = ValidationFailure::validation(reason::INVALID_CONNECTION, "wrong union");
}

#[test]
#[should_panic(expected = "not a TopologyError reason")]
fn an_unlisted_reason_on_a_topology_failure_fails_loud() {
    let _ = ValidationFailure::topology("MADE_UP", "not in either union");
}

// ---------------------------------------------------------------------------
// 4. Entry point — contract only; no rule body has been ported yet.
// ---------------------------------------------------------------------------

/// Until the rule bodies land, `creature_validate` certifies nothing: it
/// returns a structured `OTHER` failure rather than an empty `Ok`, so a
/// consumer that wires it up early cannot mistake "not implemented" for
/// "valid". The porting issues replace this test with the real rule coverage.
#[test]
fn the_entry_point_refuses_to_certify_a_creature_until_the_rules_are_ported() {
    let creature = neat_core::stump_creature();
    let failure = creature_validate(&creature, &ValidateOptions::default())
        .expect_err("the contract stub must not report a creature valid");

    assert_eq!(failure.class, FailureClass::Validation);
    assert_eq!(failure.reason, reason::OTHER);
    assert!(
        failure.message.contains("not ported"),
        "message should say why nothing was checked, was: {}",
        failure.message
    );
}

// ---------------------------------------------------------------------------
// 5. Input format — the `CreatureExport` extension.
// ---------------------------------------------------------------------------

const CREATURE_WITHOUT_EXTENSIONS: &str = r#"{
  "input": 1,
  "output": 1,
  "neurons": [{ "type": "output", "uuid": "out-0", "bias": 0.25, "squash": "IDENTITY" }],
  "synapses": [{ "fromUUID": "input-0", "toUUID": "out-0", "weight": 1.5 }]
}"#;

/// The extension is opt-in on the wire: a creature written before it existed
/// parses unchanged and serialises without either new key.
#[test]
fn a_creature_without_id_or_memetic_parses_and_serialises_unchanged() {
    let creature = parse_creature_json(CREATURE_WITHOUT_EXTENSIONS).expect("parses");

    assert_eq!(creature.neurons[0].id, None);
    assert_eq!(creature.memetic, None);

    let json = creature_to_json(&creature).expect("serialises");
    assert!(!json.contains("\"id\""), "no id key was emitted: {json}");
    assert!(!json.contains("memetic"), "no memetic key was emitted: {json}");
    assert_eq!(
        parse_creature_json(&json).expect("re-parses"),
        creature,
        "round trip is lossless"
    );
}

/// Output neurons carry negative ids (NEAT-AI #1958), so the field is signed
/// and survives a round trip.
#[test]
fn negative_output_neuron_ids_round_trip() {
    let json = r#"{
      "input": 2,
      "output": 1,
      "neurons": [
        { "type": "constant", "uuid": "c-0", "bias": 1.0, "id": 2 },
        { "type": "output", "uuid": "out-0", "bias": 0.0, "squash": "IDENTITY", "id": -1 }
      ],
      "synapses": [{ "fromUUID": "c-0", "toUUID": "out-0", "weight": 1.0 }]
    }"#;

    let creature = parse_creature_json(json).expect("parses");
    assert_eq!(creature.neurons[0].id, Some(2));
    assert_eq!(creature.neurons[1].id, Some(-1));

    let round_tripped = parse_creature_json(&creature_to_json(&creature).expect("serialises"))
        .expect("re-parses");
    assert_eq!(round_tripped, creature);
}

/// The memetic block reaches the validator with its `biases` and `weights`
/// intact, and the keys it does not read (`generation`, `score`, `ancestry`)
/// survive the round trip rather than being dropped on the floor.
#[test]
fn the_memetic_block_round_trips_including_keys_the_validator_does_not_read() {
    let json = r#"{
      "input": 1,
      "output": 1,
      "neurons": [{ "type": "output", "uuid": "out-0", "bias": 0.0, "id": -1 }],
      "synapses": [{ "fromUUID": "input-0", "toUUID": "out-0", "weight": 1.0 }],
      "memetic": {
        "generation": 12,
        "score": 0.5,
        "biases": { "-1": 0.75 },
        "weights": { "0": [{ "toId": -1, "weight": 1.25 }] }
      }
    }"#;

    let creature = parse_creature_json(json).expect("parses");
    let memetic = creature.memetic.as_ref().expect("memetic block present");

    assert_eq!(memetic.biases.get("-1"), Some(&0.75));
    assert_eq!(
        memetic.weights.get("0").map(Vec::as_slice),
        Some(
            [MemeticWeightExport {
                to_id: Some(-1),
                weight: Some(1.25),
            }]
            .as_slice()
        )
    );
    assert_eq!(memetic.extra.get("generation").and_then(|v| v.as_i64()), Some(12));

    let round_tripped = parse_creature_json(&creature_to_json(&creature).expect("serialises"))
        .expect("re-parses");
    assert_eq!(round_tripped, creature, "generation and score survive");
}

/// A memetic weight missing `toId` or `weight` is a `MEMETIC` failure the
/// validator must be able to *report*, so the wire shape has to be able to
/// carry it rather than failing in serde first.
#[test]
fn a_memetic_weight_missing_its_fields_still_parses_so_the_rule_can_report_it() {
    let json = r#"{
      "input": 1,
      "output": 1,
      "neurons": [{ "type": "output", "uuid": "out-0", "bias": 0.0, "id": -1 }],
      "synapses": [{ "fromUUID": "input-0", "toUUID": "out-0", "weight": 1.0 }],
      "memetic": { "biases": {}, "weights": { "0": [{}] } }
    }"#;

    let creature = parse_creature_json(json).expect("parses");
    let weights = &creature.memetic.as_ref().expect("memetic").weights["0"];

    assert_eq!(weights[0].to_id, None);
    assert_eq!(weights[0].weight, None);
}

/// A hand-built `CreatureExport` reaches the same shape without going through
/// JSON — the struct fields are public and default to "absent".
#[test]
fn a_hand_built_creature_can_carry_the_extension() {
    let creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![NeuronExport {
            id: Some(-1),
            neuron_type: "output".to_string(),
            uuid: "out-0".to_string(),
            bias: 0.0,
            squash: Some("IDENTITY".to_string()),
        }],
        synapses: vec![],
        semantic_version: None,
        forward_only: true,
        memetic: Some(MemeticExport::default()),
    };

    let json = creature_to_json(&creature).expect("serialises");
    assert_eq!(parse_creature_json(&json).expect("re-parses"), creature);
}
