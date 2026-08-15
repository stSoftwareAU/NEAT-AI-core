//! Unknown-field passthrough for `CreatureExport` — issue NEAT-AI#3748.
//!
//! The TypeScript engine owns the creature wire format and adds fields ahead of
//! these Rust structs. Before this fix every key the structs did not declare was
//! silently dropped on parse and absent on re-serialise, so each new TypeScript
//! field caused a fresh round of data loss (NEAT-AI#3746 was the `tags` /
//! `uuid` / `memetic` instance of it).
//!
//! These tests pin the general fix — a `#[serde(flatten)]` catch-all at all
//! three levels — and the exact guarantee it delivers:
//!
//! 1. unknown keys survive `parse -> serialise` and, when they already sit in
//!    canonical position, byte-identically;
//! 2. a creature with no unknown keys serialises exactly as it did before; and
//! 3. a declared field is never emitted twice — once typed and once via the
//!    catch-all.

use neat_core::{creature_to_json, parse_creature_json};

/// Fixture carrying an invented key at all three levels — creature, neuron and
/// synapse — in canonical position: after every declared field, keys sorted.
///
/// Canonical position is what makes byte-identity assertable; the
/// `unknown_keys_are_re_emitted_in_canonical_order` test below pins what
/// happens to a creature whose unknown keys arrive elsewhere.
const FUTURE_FIELD_CREATURE_JSON: &str = concat!(
    r#"{"input":1,"output":1,"neurons":["#,
    r#"{"type":"hidden","uuid":"h-1","bias":0.5,"squash":"Swish","#,
    r#""futureField":{"a":1}},"#,
    r#"{"type":"output","uuid":"o-1","bias":0.25,"squash":"IDENTITY"}],"#,
    r#""synapses":[{"fromUUID":"input-0","toUUID":"h-1","weight":1.5,"#,
    r#""futureField":{"a":1}},"#,
    r#"{"fromUUID":"h-1","toUUID":"o-1","weight":-0.75}],"#,
    r#""semanticVersion":"2.0.0","forwardOnly":true,"#,
    r#""futureField":{"a":1},"zFutureFlag":true}"#,
);

/// Golden fixture: the exact bytes a creature with no unknown keys serialised
/// to before the catch-all was added. Any drift — an `"extra":{}` key, a
/// reordering, a new `null` — breaks this.
const PLAIN_CREATURE_JSON: &str = concat!(
    r#"{"input":1,"output":1,"neurons":["#,
    r#"{"type":"hidden","uuid":"h-1","bias":0.5,"squash":"Swish"},"#,
    r#"{"type":"output","uuid":"o-1","bias":0.25,"squash":"IDENTITY"}],"#,
    r#""synapses":[{"fromUUID":"input-0","toUUID":"h-1","weight":1.5},"#,
    r#"{"fromUUID":"h-1","toUUID":"o-1","weight":-0.75}],"#,
    r#""semanticVersion":"2.0.0","forwardOnly":true}"#,
);

/// Every typed metadata field from NEAT-AI#3747 *plus* an unknown key at each
/// level, to prove the catch-all and the typed optionals compose.
const TAGGED_AND_FUTURE_CREATURE_JSON: &str = concat!(
    r#"{"input":1,"output":1,"neurons":["#,
    r#"{"type":"hidden","uuid":"h-1","bias":0.5,"squash":"Swish","#,
    r#""tags":[{"name":"intelligentDesign","value":"Swish -> SOFTSIGN"}],"#,
    r#""futureField":{"a":1}},"#,
    r#"{"type":"output","uuid":"o-1","bias":0.25,"squash":"IDENTITY"}],"#,
    r#""synapses":[{"fromUUID":"input-0","toUUID":"h-1","weight":1.5,"#,
    r#""type":"condition","tags":[{"name":"origin","value":"crispr"}],"#,
    r#""futureField":{"a":1}},"#,
    r#"{"fromUUID":"h-1","toUUID":"o-1","weight":-0.75}],"#,
    r#""semanticVersion":"2.0.0","forwardOnly":true,"#,
    r#""uuid":"c-9","tags":[{"name":"approach","value":"fine"}],"#,
    r#""memetic":{"generation":1,"score":0.47130383936547315,"biases":{},"#,
    r#""weights":{"input-0":[{"toUUID":"h-1","weight":-4.8619145999999995}]}},"#,
    r#""futureField":{"a":1}}"#,
);

/// Count non-overlapping occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn unknown_fields_round_trip_byte_identical() {
    let creature =
        parse_creature_json(FUTURE_FIELD_CREATURE_JSON).expect("parse creature with unknown keys");

    // The unknown keys are readable, not merely echoed.
    assert_eq!(
        creature.extra["futureField"]["a"], 1,
        "creature-level unknown key is parsed"
    );
    assert_eq!(creature.extra["zFutureFlag"], true);
    assert_eq!(creature.neurons[0].extra["futureField"]["a"], 1);
    assert_eq!(creature.synapses[0].extra["futureField"]["a"], 1);

    // Neurons and synapses without unknown keys carry an empty map, not a
    // sibling's leftovers.
    assert!(creature.neurons[1].extra.is_empty());
    assert!(creature.synapses[1].extra.is_empty());

    let serialised = creature_to_json(&creature).expect("serialise");
    assert_eq!(
        serialised, FUTURE_FIELD_CREATURE_JSON,
        "unknown keys in canonical position round trip byte-identically"
    );

    // And the rewrite is a fixed point: a second pass changes nothing.
    let reparsed = parse_creature_json(&serialised).expect("re-parse");
    assert_eq!(
        creature_to_json(&reparsed).expect("re-serialise"),
        FUTURE_FIELD_CREATURE_JSON
    );
}

#[test]
fn no_extras_serialisation_unchanged() {
    let creature = parse_creature_json(PLAIN_CREATURE_JSON).expect("parse plain creature");
    assert!(creature.extra.is_empty());

    let serialised = creature_to_json(&creature).expect("serialise");
    assert_eq!(
        serialised, PLAIN_CREATURE_JSON,
        "a creature with no unknown keys serialises exactly as it did before"
    );
    assert!(
        !serialised.contains("extra"),
        "the catch-all map must never surface as a key of its own: {serialised}"
    );
    assert!(
        !serialised.contains("null"),
        "an absent optional stays absent rather than becoming null: {serialised}"
    );
}

#[test]
fn typed_fields_not_duplicated_via_extra() {
    let creature = parse_creature_json(TAGGED_AND_FUTURE_CREATURE_JSON)
        .expect("parse creature with typed metadata and unknown keys");

    // The typed fields are claimed by their declared field, so the catch-all
    // sees only the genuinely unknown key.
    assert_eq!(
        creature.extra.keys().collect::<Vec<_>>(),
        vec!["futureField"],
        "typed fields must not also land in the catch-all"
    );
    assert_eq!(creature.uuid.as_deref(), Some("c-9"));
    assert!(creature.tags.is_some());
    assert!(creature.memetic.is_some());
    assert_eq!(
        creature.neurons[0].extra.keys().collect::<Vec<_>>(),
        vec!["futureField"]
    );
    assert!(creature.neurons[0].tags.is_some());
    assert_eq!(
        creature.synapses[0].extra.keys().collect::<Vec<_>>(),
        vec!["futureField"]
    );
    assert!(creature.synapses[0].tags.is_some());

    let serialised = creature_to_json(&creature).expect("serialise");

    // Each typed key is emitted exactly once — never once typed and once more
    // through the flattened map.
    assert_eq!(count_occurrences(&serialised, r#""uuid":"c-9""#), 1);
    assert_eq!(count_occurrences(&serialised, r#""memetic":"#), 1);
    assert_eq!(count_occurrences(&serialised, r#""approach""#), 1);
    assert_eq!(count_occurrences(&serialised, r#""intelligentDesign""#), 1);
    assert_eq!(count_occurrences(&serialised, r#""origin""#), 1);
    assert_eq!(count_occurrences(&serialised, r#""futureField""#), 3);

    assert_eq!(
        serialised, TAGGED_AND_FUTURE_CREATURE_JSON,
        "typed metadata and unknown keys round trip together byte-identically"
    );
}

#[test]
fn flattening_preserves_f64_precision_of_declared_fields() {
    // A `#[serde(flatten)]` field routes the whole struct through serde's
    // buffering deserialiser, so pin that declared numbers still arrive — and
    // leave — with full f64 precision rather than being rounded in transit.
    let json = concat!(
        r#"{"input":1,"output":1,"neurons":["#,
        r#"{"type":"output","uuid":"o-1","bias":-0.30000000000000004,"squash":"IDENTITY"}],"#,
        r#""synapses":[{"fromUUID":"input-0","toUUID":"o-1","weight":-4.8619145999999995}],"#,
        r#""forwardOnly":false,"futureField":1}"#,
    );
    let creature = parse_creature_json(json).expect("parse high-precision creature");
    assert_eq!(creature.neurons[0].bias, -0.30000000000000004);
    assert_eq!(creature.synapses[0].weight, -4.8619145999999995);
    assert_eq!(creature_to_json(&creature).expect("serialise"), json);
}

#[test]
fn unknown_keys_are_re_emitted_in_canonical_order() {
    // Documented normalisation (see the `creature` module docs): unknown keys
    // move after the declared fields and are emitted in sorted key order —
    // nested unknown objects included. Nothing is lost, but the bytes move.
    let json = concat!(
        r#"{"zTrailing":1,"input":1,"output":1,"neurons":["#,
        r#"{"type":"output","uuid":"o-1","bias":0.0,"squash":"IDENTITY"}],"#,
        r#""aLeading":{"z":1,"a":2},"synapses":[],"forwardOnly":false}"#,
    );
    let creature = parse_creature_json(json).expect("parse creature with interleaved unknown keys");

    let serialised = creature_to_json(&creature).expect("serialise");
    assert_eq!(
        serialised,
        concat!(
            r#"{"input":1,"output":1,"neurons":["#,
            r#"{"type":"output","uuid":"o-1","bias":0.0,"squash":"IDENTITY"}],"#,
            r#""synapses":[],"forwardOnly":false,"#,
            r#""aLeading":{"a":2,"z":1},"zTrailing":1}"#,
        ),
        "unknown keys are preserved, moved after the declared fields, and sorted"
    );

    // Deterministic: serialising the same value twice gives the same bytes.
    assert_eq!(
        creature_to_json(&creature).expect("serialise twice"),
        serialised
    );
}
