//! Typed-error contract tests for the public creature/network APIs (Issue #115).
//!
//! These verify that the crate-root functions which previously returned
//! `Result<_, String>` now return custom error types implementing
//! `std::error::Error`, can be matched on by variant, and preserve the
//! underlying `serde_json::Error` as a `source()` chain.

use std::error::Error;

use neat_core::{
    CompiledNetwork, CreatureError, NetworkError, PcEngineError, PredictiveCodingEngine,
    compile_creature, creature_to_json, creature_to_json_pretty, parse_creature_json,
    parse_squash_name,
};

/// A valid minimal creature: 1 input, 1 output, single synapse.
fn minimal_creature_json() -> &'static str {
    r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}
        ]
    }"#
}

#[test]
fn parse_creature_json_error_is_std_error_with_serde_source() {
    let err = parse_creature_json("{ not valid json").unwrap_err();
    // Matches on a typed variant rather than inspecting a String.
    assert!(matches!(err, CreatureError::Json(_)));
    // The underlying serde_json::Error is preserved as the error source.
    let source = err.source().expect("Json variant must expose a source");
    assert!(source.downcast_ref::<serde_json::Error>().is_some());
}

#[test]
fn parse_squash_name_unknown_returns_typed_variant() {
    let err = parse_squash_name("NOT_A_REAL_SQUASH").unwrap_err();
    match err {
        CreatureError::UnknownSquash(name) => assert_eq!(name, "NOT_A_REAL_SQUASH"),
        other => panic!("expected UnknownSquash, got {other:?}"),
    }
    // Unknown squash is a structural failure, not a JSON failure: no source chain.
    assert!(
        CreatureError::UnknownSquash("x".to_string())
            .source()
            .is_none()
    );
}

#[test]
fn compile_creature_output_mismatch_returns_typed_variant() {
    let json = r#"{
        "input": 1,
        "output": 2,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": []
    }"#;
    let creature = parse_creature_json(json).unwrap();
    let err = compile_creature(&creature).err().expect("should fail");
    match err {
        CreatureError::OutputCountMismatch { expected, found } => {
            assert_eq!(expected, 2);
            assert_eq!(found, 1);
        }
        other => panic!("expected OutputCountMismatch, got {other:?}"),
    }
}

#[test]
fn compile_creature_unknown_source_uuid_returns_typed_variant() {
    let json = r#"{
        "input": 1,
        "output": 1,
        "neurons": [
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "nonexistent", "toUUID": "output-0", "weight": 1.0}
        ]
    }"#;
    let creature = parse_creature_json(json).unwrap();
    let err = compile_creature(&creature).err().expect("should fail");
    match err {
        CreatureError::UnknownSourceUuid(uuid) => assert_eq!(uuid, "nonexistent"),
        other => panic!("expected UnknownSourceUuid, got {other:?}"),
    }
}

#[test]
fn creature_round_trip_helpers_return_ok() {
    let creature = parse_creature_json(minimal_creature_json()).unwrap();
    // Serialisation helpers return Result<String, CreatureError> and succeed here.
    let compact: Result<String, CreatureError> = creature_to_json(&creature);
    let pretty: Result<String, CreatureError> = creature_to_json_pretty(&creature);
    assert!(compact.unwrap().contains("\"input\":1"));
    assert!(pretty.unwrap().contains("\"input\""));
}

#[test]
fn creature_error_propagates_via_question_mark() {
    // A caller with its own Box<dyn Error> can `?`-propagate a CreatureError.
    fn parse(json: &str) -> Result<usize, Box<dyn Error>> {
        let creature = parse_creature_json(json)?;
        Ok(creature.input)
    }
    assert_eq!(parse(minimal_creature_json()).unwrap(), 1);
    assert!(parse("{ broken").is_err());
}

#[test]
fn compiled_network_new_truncated_returns_typed_error() {
    // Buffer too short for the 8-byte header. `CompiledNetwork` is not `Debug`,
    // so extract the error via `.err()` rather than `unwrap_err()`.
    let err: NetworkError = CompiledNetwork::new(&[0u8; 4]).err().expect("should fail");
    assert!(matches!(err, NetworkError::TruncatedData { .. }));
    // Display preserves a human-readable description.
    assert!(err.to_string().contains("too short"));
    // Implements std::error::Error.
    let _as_err: &dyn Error = &err;
}

#[test]
fn pc_engine_new_truncated_returns_typed_error() {
    // Buffer too short for the 24-byte PC header. `PredictiveCodingEngine` is
    // not `Debug`, so extract the error via `.err()`.
    let err: PcEngineError = PredictiveCodingEngine::new(&[0u8; 8])
        .err()
        .expect("should fail");
    assert!(matches!(err, PcEngineError::TruncatedData { .. }));
    assert!(err.to_string().contains("too short"));
    let _as_err: &dyn Error = &err;
}
