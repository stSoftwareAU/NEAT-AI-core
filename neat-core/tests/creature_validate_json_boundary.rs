//! Issue #562 — the JSON ABI `creature_validate` crosses the WASM boundary on.
//!
//! The shim in `wasm_exports.rs` is three lines over
//! [`neat_core::creature_validate_json`]; everything worth testing lives in
//! that native function, so these tests run in the ordinary `cargo test` gate
//! rather than only in a browser.
//!
//! Two properties are pinned here:
//!
//! - **the answer is the native answer** — the same class, `reason`, message
//!   and indices [`neat_core::creature_validate`] returns, in JSON;
//! - **nothing panics** — a panic in WASM aborts the module, taking the host's
//!   session with it, so every malformed, truncated, hostile or absurdly large
//!   payload must come back as a structured `MALFORMED_REQUEST` failure.

use neat_core::{MALFORMED_REQUEST, MAX_REQUEST_NEURONS, ValidateResponse, creature_validate_json};

/// A creature that breaks no rule: one input, one hidden, one output.
const VALID_CREATURE: &str = r#"{
    "input": 1,
    "output": 1,
    "neurons": [
        { "type": "hidden", "uuid": "h1", "bias": 0.5, "squash": "IDENTITY" },
        { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
    ],
    "synapses": [
        { "fromUUID": "input-0", "toUUID": "h1", "weight": 1.0 },
        { "fromUUID": "h1", "toUUID": "output-0", "weight": 1.0 }
    ]
}"#;

fn request(creature: &str, options: &str) -> String {
    format!(r#"{{ "creature": {creature}, "options": {options} }}"#)
}

fn respond(request: &str) -> ValidateResponse {
    let json = creature_validate_json(request);
    serde_json::from_str(&json).unwrap_or_else(|error| {
        panic!("the boundary must answer with parseable JSON, got {json:?}: {error}")
    })
}

// ---------------------------------------------------------------------------
// The success half.
// ---------------------------------------------------------------------------

#[test]
fn a_valid_creature_answers_with_the_five_counters() {
    let response = respond(&request(VALID_CREATURE, "{}"));

    assert!(response.ok, "{response:?}");
    assert!(response.failure.is_none(), "{response:?}");
    let stats = response.stats.expect("a successful answer carries stats");
    assert_eq!(
        (
            stats.input,
            stats.constant,
            stats.hidden,
            stats.output,
            stats.connections
        ),
        (1, 0, 1, 1, 2)
    );
}

#[test]
fn the_options_bag_reaches_the_rules() {
    // `neurons` counts the implicit inputs too, so 3 is the match and 2 is not.
    let matching = respond(&request(VALID_CREATURE, r#"{ "neurons": 3 }"#));
    assert!(matching.ok, "{matching:?}");

    let mismatched = respond(&request(VALID_CREATURE, r#"{ "neurons": 2 }"#));
    let failure = mismatched.failure.expect("a mismatch is a failure");
    assert_eq!(failure.class, "ValidationError");
    assert_eq!(failure.reason, "OTHER");
    assert_eq!(failure.message, "Neurons length: 3 expected: 2");
    assert!(!failure.malformed, "a rule failure is not a boundary fault");
}

#[test]
fn an_absent_options_bag_is_the_default_bag() {
    let bare = format!(r#"{{ "creature": {VALID_CREATURE} }}"#);
    assert!(respond(&bare).ok);
}

#[test]
fn forward_only_travels_over_the_boundary() {
    // A backward synapse is legal by default and rejected under forwardOnly.
    let recurrent = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            { "type": "hidden", "uuid": "h1", "bias": 0.0, "squash": "IDENTITY" },
            { "type": "hidden", "uuid": "h2", "bias": 0.0, "squash": "IDENTITY" },
            { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
        ],
        "synapses": [
            { "fromUUID": "input-0", "toUUID": "h1", "weight": 1.0 },
            { "fromUUID": "h1", "toUUID": "h2", "weight": 1.0 },
            { "fromUUID": "h2", "toUUID": "h1", "weight": 0.5 },
            { "fromUUID": "h2", "toUUID": "output-0", "weight": 1.0 }
        ]
    }"#;

    assert!(respond(&request(recurrent, "{}")).ok);

    let rejected = respond(&request(recurrent, r#"{ "forwardOnly": true }"#));
    let failure = rejected.failure.expect("forwardOnly rejects h2 -> h1");
    assert_eq!(failure.class, "ValidationError");
    assert_eq!(failure.reason, "RECURSIVE_SYNAPSE");
    assert_eq!(failure.message, "2) Recursive synapse h2 -> h1");
    assert_eq!(failure.synapse_index, Some(2));
    assert_eq!(failure.neuron_index, None);
}

#[test]
fn a_rule_failure_carries_the_neuron_it_stopped_on() {
    let orphan = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            { "type": "hidden", "uuid": "h1", "bias": 0.5, "squash": "IDENTITY" },
            { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
        ],
        "synapses": [ { "fromUUID": "input-0", "toUUID": "h1", "weight": 1.0 } ]
    }"#;

    let response = respond(&request(orphan, "{}"));
    assert!(!response.ok);
    assert!(response.stats.is_none(), "{response:?}");
    let failure = response.failure.expect("a hidden neuron nothing reads");
    assert_eq!(failure.class, "ValidationError");
    assert_eq!(failure.reason, "NO_OUTWARD_CONNECTIONS");
    assert_eq!(
        failure.message,
        "hidden neuron h1 has no outward connections"
    );
    assert_eq!(failure.neuron_index, Some(1));
    assert_eq!(failure.synapse_index, None);
}

// ---------------------------------------------------------------------------
// Malformed input — the half that must never panic.
// ---------------------------------------------------------------------------

#[track_caller]
fn assert_malformed(payload: &str, what: &str) {
    let response = respond(payload);
    assert!(!response.ok, "{what}: expected a failure, got {response:?}");
    assert!(response.stats.is_none(), "{what}: {response:?}");
    let failure = response
        .failure
        .unwrap_or_else(|| panic!("{what}: a failed answer carries a failure"));
    assert!(
        failure.malformed,
        "{what}: a boundary fault must be flagged malformed, got {failure:?}"
    );
    assert!(
        failure.message.starts_with(MALFORMED_REQUEST),
        "{what}: message must lead with {MALFORMED_REQUEST}, got {:?}",
        failure.message
    );
    assert!(
        failure.class == "ValidationError" && failure.reason == "OTHER",
        "{what}: {failure:?}"
    );
}

#[test]
fn garbage_payloads_fail_loudly_instead_of_panicking() {
    let valid = request(VALID_CREATURE, "{}");
    let hostile: [(&str, String); 14] = [
        ("empty", String::new()),
        ("not json", "this is not JSON".to_string()),
        ("bare scalar", "42".to_string()),
        ("bare null", "null".to_string()),
        ("array, not object", "[1, 2, 3]".to_string()),
        ("no creature key", r#"{ "options": {} }"#.to_string()),
        (
            "creature is a string",
            r#"{ "creature": "nope" }"#.to_string(),
        ),
        (
            "truncated mid-creature",
            valid[..valid.len() / 2].to_string(),
        ),
        ("truncated to one brace", "{".to_string()),
        ("trailing garbage", format!("{valid} and then some")),
        (
            "creature missing input",
            r#"{ "creature": { "output": 1, "neurons": [], "synapses": [] } }"#.to_string(),
        ),
        (
            "negative input",
            request(
                &VALID_CREATURE.replace("\"input\": 1", "\"input\": -1"),
                "{}",
            ),
        ),
        (
            "non-integer input",
            request(
                &VALID_CREATURE.replace("\"input\": 1", "\"input\": 1.5"),
                "{}",
            ),
        ),
        (
            "neuron bias is a string",
            request(
                &VALID_CREATURE.replace("\"bias\": 0.5", "\"bias\": \"NaN\""),
                "{}",
            ),
        ),
    ];

    for (what, payload) in &hostile {
        assert_malformed(payload, what);
    }
}

#[test]
fn a_misspelt_option_is_a_loud_failure_not_a_silent_default() {
    // Silently ignoring `forwardonly` would validate a production creature
    // under the wrong rules and call it healthy.
    assert_malformed(
        &request(VALID_CREATURE, r#"{ "forwardonly": true }"#),
        "misspelt option key",
    );
}

#[test]
fn an_absurd_neuron_count_is_refused_before_anything_is_allocated() {
    // A creature declaring more inputs than memory can hold would abort the
    // module on the first `Vec::with_capacity` — the allocation happens before
    // any rule runs, so the ceiling is checked at the boundary.
    let huge = format!(
        r#"{{ "creature": {{ "input": {}, "output": 1, "neurons": [], "synapses": [] }} }}"#,
        u64::from(u32::MAX) * 4
    );
    assert_malformed(&huge, "absurd input width");

    let over_ceiling = format!(
        r#"{{ "creature": {{ "input": {}, "output": 1, "neurons": [], "synapses": [] }} }}"#,
        MAX_REQUEST_NEURONS + 1
    );
    assert_malformed(&over_ceiling, "one neuron past the ceiling");
}

#[test]
fn deeply_nested_json_is_refused_rather_than_overflowing_the_stack() {
    let payload = format!(
        r#"{{ "creature": {}{} }}"#,
        "[".repeat(2_000),
        "]".repeat(2_000)
    );
    assert_malformed(&payload, "deeply nested creature");
}

#[test]
fn a_creature_with_no_neurons_reports_a_rule_rather_than_a_boundary_fault() {
    // `input: 0` must reach rule 2 and answer in NEAT-AI's own words — the
    // boundary deserialises with serde alone so the width check in
    // `parse_creature_json` cannot shadow the rule.
    let response =
        respond(r#"{ "creature": { "input": 0, "output": 1, "neurons": [], "synapses": [] } }"#);
    let failure = response
        .failure
        .expect("rule 2 rejects a widthless creature");
    assert!(!failure.malformed, "{failure:?}");
    assert_eq!(failure.class, "ValidationError");
    assert_eq!(failure.reason, "OTHER");
    assert_eq!(
        failure.message,
        "Must have at least one input neurons was: 0"
    );
}

#[test]
fn a_creature_at_the_ceiling_is_still_validated() {
    // The ceiling is a boundary guard, not a rule: a creature that reaches it
    // is validated and reports its own first broken rule.
    let neurons: Vec<String> = (0..MAX_REQUEST_NEURONS - 2)
        .map(|i| {
            format!(r#"{{ "type": "hidden", "uuid": "h{i}", "bias": 0.0, "squash": "IDENTITY" }}"#)
        })
        .collect();
    let creature = format!(
        r#"{{ "input": 1, "output": 1, "neurons": [{}, {{ "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }}], "synapses": [] }}"#,
        neurons.join(", ")
    );

    let response = respond(&request(&creature, "{}"));
    let failure = response
        .failure
        .expect("unwired hidden neurons are invalid");
    assert!(!failure.malformed, "{failure:?}");
    assert_eq!(failure.reason, "NO_INWARD_CONNECTIONS");
}
