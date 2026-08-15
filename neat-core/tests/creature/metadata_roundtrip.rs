//! Metadata fidelity tests for `CreatureExport` — issue NEAT-AI#3747.
//!
//! The TypeScript engine is the wire-format contract: it carries per-neuron
//! `tags`, per-synapse `tags`, and top-level `uuid` / `tags` / `memetic`. Every
//! Rust rewrite of a creature used to strip all of those silently. These tests
//! pin the two halves of the fix:
//!
//! 1. a creature carrying the metadata survives `parse -> serialise`
//!    **byte-identically**, memetic key order included; and
//! 2. a creature carrying none of it serialises exactly as before — no new
//!    `null` keys, no key-order drift.

use neat_core::{creature_to_json, parse_creature_json};

/// Compact fixture in `CreatureExport` declaration order, carrying every
/// metadata field the Rust path used to drop.
///
/// The `memetic` keys are deliberately **not** alphabetical
/// (`generation`, `score`, `biases`, `weights`) so a representation that
/// re-orders object keys cannot pass the byte-identical assertion.
const TAGGED_CREATURE_JSON: &str = concat!(
    r#"{"input":1,"output":1,"neurons":["#,
    r#"{"type":"hidden","uuid":"h-1","bias":0.5,"squash":"Swish","#,
    r#""tags":[{"name":"intelligentDesign","value":"Swish -> SOFTSIGN"}]},"#,
    r#"{"type":"output","uuid":"o-1","bias":0.25,"squash":"IDENTITY"}],"#,
    r#""synapses":[{"fromUUID":"input-0","toUUID":"h-1","weight":1.5,"#,
    r#""type":"condition","tags":[{"name":"origin","value":"crispr"}]},"#,
    r#"{"fromUUID":"h-1","toUUID":"o-1","weight":-0.75}],"#,
    r#""semanticVersion":"2.0.0","forwardOnly":true,"#,
    r#""uuid":"c-9","tags":[{"name":"approach","value":"fine"}],"#,
    r#""memetic":{"generation":1,"score":0.47130383936547315,"biases":{},"#,
    r#""weights":{"input-0":[{"toUUID":"h-1","weight":-4.8619145999999995}]}}}"#,
);

/// Same topology with none of the optional metadata — the shape existing
/// consumers already serialise today.
const PLAIN_CREATURE_JSON: &str = concat!(
    r#"{"input":1,"output":1,"neurons":["#,
    r#"{"type":"hidden","uuid":"h-1","bias":0.5,"squash":"Swish"},"#,
    r#"{"type":"output","uuid":"o-1","bias":0.25,"squash":"IDENTITY"}],"#,
    r#""synapses":[{"fromUUID":"input-0","toUUID":"h-1","weight":1.5},"#,
    r#"{"fromUUID":"h-1","toUUID":"o-1","weight":-0.75}],"#,
    r#""semanticVersion":"2.0.0","forwardOnly":true}"#,
);

#[test]
fn roundtrip_preserves_tags_uuid_memetic() {
    let creature = parse_creature_json(TAGGED_CREATURE_JSON).expect("parse tagged creature");

    // Per-neuron tags: the intelligentDesign pedigree that used to be dropped.
    let hidden_tags = creature.neurons[0]
        .tags
        .as_ref()
        .expect("hidden neuron keeps its tags");
    assert_eq!(hidden_tags.len(), 1);
    assert_eq!(hidden_tags[0].name, "intelligentDesign");
    assert_eq!(hidden_tags[0].value, "Swish -> SOFTSIGN");
    assert!(
        creature.neurons[1].tags.is_none(),
        "an untagged neuron stays untagged"
    );

    // Per-synapse tags.
    let synapse_tags = creature.synapses[0]
        .tags
        .as_ref()
        .expect("synapse keeps its tags");
    assert_eq!(synapse_tags.len(), 1);
    assert_eq!(synapse_tags[0].name, "origin");
    assert_eq!(synapse_tags[0].value, "crispr");
    assert!(creature.synapses[1].tags.is_none());

    // Top-level uuid, tags and memetic.
    assert_eq!(creature.uuid.as_deref(), Some("c-9"));
    let creature_tags = creature.tags.as_ref().expect("creature keeps its tags");
    assert_eq!(creature_tags.len(), 1);
    assert_eq!(creature_tags[0].name, "approach");
    assert_eq!(creature_tags[0].value, "fine");
    let memetic = creature.memetic.as_ref().expect("creature keeps memetic");
    assert!(
        memetic.as_json_str().contains(r#""generation":1"#),
        "memetic payload survives: {}",
        memetic.as_json_str()
    );

    // The contract that matters downstream: nothing is added, dropped, or
    // re-ordered on the way back out.
    let serialised = creature_to_json(&creature).expect("serialise tagged creature");
    assert_eq!(
        serialised, TAGGED_CREATURE_JSON,
        "tagged creature must round trip byte-identically"
    );
}

#[test]
fn roundtrip_plain_creature_byte_identical() {
    let creature = parse_creature_json(PLAIN_CREATURE_JSON).expect("parse plain creature");

    assert!(creature.uuid.is_none());
    assert!(creature.tags.is_none());
    assert!(creature.memetic.is_none());

    let serialised = creature_to_json(&creature).expect("serialise plain creature");
    assert_eq!(
        serialised, PLAIN_CREATURE_JSON,
        "a creature without metadata must serialise exactly as before"
    );
    assert!(
        !serialised.contains("null"),
        "absent optional fields must stay absent, never become null: {serialised}"
    );
}

#[test]
fn memetic_object_key_order_is_preserved_verbatim() {
    // Guards the failure mode a key-sorting representation would introduce:
    // rewriting a model file would re-order every memetic block and churn the
    // diff even when no value changed.
    let creature = parse_creature_json(TAGGED_CREATURE_JSON).expect("parse tagged creature");
    let memetic = creature.memetic.as_ref().expect("memetic present");
    assert_eq!(
        memetic.as_json_str(),
        concat!(
            r#"{"generation":1,"score":0.47130383936547315,"biases":{},"#,
            r#""weights":{"input-0":[{"toUUID":"h-1","weight":-4.8619145999999995}]}}"#,
        )
    );
}

#[test]
fn tagged_creature_survives_a_second_roundtrip() {
    // parse -> serialise -> parse -> serialise must be a fixed point, so a
    // creature rewritten repeatedly by the Rust path never drifts.
    let once = creature_to_json(&parse_creature_json(TAGGED_CREATURE_JSON).expect("parse"))
        .expect("serialise once");
    let twice =
        creature_to_json(&parse_creature_json(&once).expect("re-parse")).expect("serialise");
    assert_eq!(once, twice);
    assert_eq!(twice, TAGGED_CREATURE_JSON);
}
