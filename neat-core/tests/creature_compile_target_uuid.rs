//! `compile_creature` refuses a synapse whose `toUUID` names no listed neuron
//! (Issue #682).
//!
//! The destination used to get no check at all: synapses were grouped by
//! `toUUID` and read back **per listed neuron**, so an edge naming a
//! destination the creature does not carry was never looked up and simply
//! vanished from the compiled network while `compile_creature` returned `Ok`.
//! The creature and the network the fleet scores then disagreed with nothing
//! saying so. These tests pin the typed refusal that replaced the silent drop,
//! and — as the oracle against over-refusal — that a well-formed creature still
//! compiles every edge it declares.

use std::error::Error;

use neat_core::{CreatureError, compile_creature, parse_creature_json};

/// Two neurons (`h-1` hidden, `out-0` output) over one input, and whatever
/// synapse rows the caller names. Keeps every test differing only in its edges.
fn creature_json(synapses: &str) -> String {
    format!(
        r#"{{
            "input": 1,
            "output": 1,
            "neurons": [
                {{"type": "hidden", "uuid": "h-1", "bias": 0.0, "squash": "IDENTITY"}},
                {{"type": "output", "uuid": "out-0", "bias": 0.0, "squash": "IDENTITY"}}
            ],
            "synapses": [{synapses}]
        }}"#
    )
}

fn syn(from: &str, to: &str) -> String {
    format!(r#"{{"fromUUID": "{from}", "toUUID": "{to}", "weight": 1.0}}"#)
}

fn compile_error(synapses: &str) -> CreatureError {
    let creature = parse_creature_json(&creature_json(synapses)).expect("fixture parses");
    compile_creature(&creature)
        .err()
        .expect("an unresolvable destination must be refused")
}

#[test]
fn a_well_formed_creature_compiles_every_edge_it_declares() {
    let edges = format!("{}, {}", syn("input-0", "h-1"), syn("h-1", "out-0"));
    let creature = parse_creature_json(&creature_json(&edges)).expect("fixture parses");
    let network = compile_creature(&creature).expect("both destinations are listed neurons");
    // One compiled synapse per declared edge — the count the silent drop lowered.
    assert_eq!(network.synapses().len(), 2);
}

#[test]
fn a_destination_naming_no_listed_neuron_is_refused() {
    let edges = format!("{}, {}", syn("input-0", "h-1"), syn("h-1", "nope"));
    match compile_error(&edges) {
        CreatureError::UnknownTargetUuid(uuid) => assert_eq!(uuid, "nope"),
        other => panic!("expected UnknownTargetUuid, got {other:?}"),
    }
}

#[test]
fn a_destination_that_is_an_input_neuron_is_refused() {
    // The transposed `fromUUID`/`toUUID` pair from the issue: `input-0` resolves
    // through the UUID map but is never in `neurons`, so nothing would read the
    // edge back.
    let edges = format!("{}, {}", syn("h-1", "input-0"), syn("h-1", "out-0"));
    match compile_error(&edges) {
        CreatureError::UnknownTargetUuid(uuid) => assert_eq!(uuid, "input-0"),
        other => panic!("expected UnknownTargetUuid, got {other:?}"),
    }
}

#[test]
fn the_refusal_names_the_first_unresolvable_destination_in_declaration_order() {
    // Two bad destinations: the answer is the earlier row, not whichever a hash
    // map happened to yield.
    let edges = format!("{}, {}", syn("input-0", "ghost"), syn("h-1", "phantom"));
    match compile_error(&edges) {
        CreatureError::UnknownTargetUuid(uuid) => assert_eq!(uuid, "ghost"),
        other => panic!("expected UnknownTargetUuid, got {other:?}"),
    }
}

#[test]
fn an_unresolvable_destination_is_refused_before_an_unresolvable_source() {
    // Both endpoints dangle on the same row; the destination check runs over the
    // whole synapse list before the neuron walk, so it is what speaks.
    let edges = syn("missing-source", "missing-target");
    match compile_error(&edges) {
        CreatureError::UnknownTargetUuid(uuid) => assert_eq!(uuid, "missing-target"),
        other => panic!("expected UnknownTargetUuid, got {other:?}"),
    }
}

#[test]
fn the_refusal_reads_as_a_std_error_naming_the_uuid() {
    let err = CreatureError::UnknownTargetUuid("nope".to_string());
    assert_eq!(err.to_string(), "Unknown target neuron UUID: nope");
    // A structural failure, not a JSON one: no source chain, exactly as the
    // mirroring `UnknownSourceUuid` behaves.
    assert!(err.source().is_none());
}
