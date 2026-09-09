//! Observation-width contract on `CreatureExport` (Issue #550).
//!
//! `input` is the authoritative observation count and `output` the target
//! count. `neurons` lists only non-input neurons, so `input` can never be
//! re-derived once lost. The rule mirrored from NEAT-AI (TS)
//! `CreatureValidate.ts`: `input < 1` or `output < 1` is never accepted —
//! not on parse, not on compile, not on serialise — and fails with a typed
//! error. No default, no fallback.
//!
//! The rule is bounded at both ends (Issue #622): `input` is a *declared* count
//! with no backing data, so a width past `MAX_NODE_COUNT` — a creature that
//! could never be addressed by the `u16` source index — is refused here, before
//! any entry point allocates one map entry per declared input.

use std::error::Error;

use neat_core::if_graft::{GraftError, validate_creature_topology};
use neat_core::network::MAX_NODE_COUNT;
use neat_core::{
    CleanupError, CreatureError, CreatureExport, NeuronExport, SynapseExport, cleanup_creature,
    compile_creature, creature_to_json, creature_to_json_pretty, parse_creature_json,
};

/// Zero-input creature JSON exactly as written in the issue's acceptance
/// criterion.
const ZERO_INPUT_JSON: &str = r#"{"input":0,"output":1,"neurons":[],"synapses":[]}"#;
const ZERO_OUTPUT_JSON: &str = r#"{"input":1,"output":0,"neurons":[],"synapses":[]}"#;

fn output_neuron() -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: "output".to_string(),
        uuid: "output-0".to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    }
}

/// A structurally valid creature apart from the declared widths, so any
/// rejection can only come from the width rule.
fn creature_with_widths(input: usize, output: usize) -> CreatureExport {
    CreatureExport {
        memetic: None,
        input,
        output,
        neurons: vec![output_neuron()],
        synapses: vec![SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: 1.0,
            synapse_type: None,
        }],
        semantic_version: None,
        forward_only: false,
    }
}

// ---------------------------------------------------------------------------
// parse_creature_json
// ---------------------------------------------------------------------------

#[test]
fn parse_rejects_zero_input_with_invalid_input_count() {
    match parse_creature_json(ZERO_INPUT_JSON) {
        Err(CreatureError::InvalidInputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidInputCount, got {other:?}"),
        Ok(c) => panic!("input: 0 must not parse, got {c:?}"),
    }
}

#[test]
fn parse_rejects_zero_output_with_invalid_output_count() {
    match parse_creature_json(ZERO_OUTPUT_JSON) {
        Err(CreatureError::InvalidOutputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidOutputCount, got {other:?}"),
        Ok(c) => panic!("output: 0 must not parse, got {c:?}"),
    }
}

#[test]
fn parse_reports_input_before_output_when_both_are_zero() {
    // The input width is the one that can never be recovered, so it is
    // reported first when both are missing.
    let json = r#"{"input":0,"output":0,"neurons":[],"synapses":[]}"#;
    match parse_creature_json(json) {
        Err(CreatureError::InvalidInputCount { found }) => assert_eq!(found, 0),
        other => panic!("expected InvalidInputCount, got {other:?}"),
    }
}

#[test]
fn parse_rejects_missing_input_as_json_error_naming_the_field() {
    // No `#[serde(default)]`: an absent key is a deserialisation failure, not
    // a silent zero.
    let json = r#"{"output":1,"neurons":[],"synapses":[]}"#;
    match parse_creature_json(json) {
        Err(CreatureError::Json(e)) => {
            let text = e.to_string();
            assert!(
                text.contains("missing field `input`"),
                "serde error must name the missing field, got: {text}"
            );
        }
        other => panic!("expected Json error for missing input, got {other:?}"),
    }
}

#[test]
fn parse_rejects_missing_output_as_json_error_naming_the_field() {
    let json = r#"{"input":1,"neurons":[],"synapses":[]}"#;
    match parse_creature_json(json) {
        Err(CreatureError::Json(e)) => {
            let text = e.to_string();
            assert!(
                text.contains("missing field `output`"),
                "serde error must name the missing field, got: {text}"
            );
        }
        other => panic!("expected Json error for missing output, got {other:?}"),
    }
}

#[test]
fn parse_rejects_negative_input_as_json_error() {
    // `input` is `usize`; `-1` cannot deserialise and must not wrap or clamp.
    let json = r#"{"input":-1,"output":1,"neurons":[],"synapses":[]}"#;
    match parse_creature_json(json) {
        Err(CreatureError::Json(e)) => {
            let text = e.to_string();
            assert!(
                text.contains("invalid value: integer `-1`"),
                "serde error must report the negative literal, got: {text}"
            );
        }
        other => panic!("expected Json error for input: -1, got {other:?}"),
    }
}

#[test]
fn parse_rejects_negative_output_as_json_error() {
    let json = r#"{"input":1,"output":-1,"neurons":[],"synapses":[]}"#;
    match parse_creature_json(json) {
        Err(CreatureError::Json(e)) => {
            let text = e.to_string();
            assert!(
                text.contains("invalid value: integer `-1`"),
                "serde error must report the negative literal, got: {text}"
            );
        }
        other => panic!("expected Json error for output: -1, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// compile_creature — a directly built CreatureExport bypasses parse, so the
// compiler must fail just as loudly.
// ---------------------------------------------------------------------------

#[test]
fn compile_rejects_directly_built_zero_input_before_any_other_check() {
    // Deliberately also carries an output-count mismatch (output: 2, one output
    // neuron) so the test proves the width check runs *first*.
    let mut creature = creature_with_widths(0, 2);
    creature.synapses.clear();
    match compile_creature(&creature) {
        Err(CreatureError::InvalidInputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidInputCount, got {other:?}"),
        Ok(_) => panic!("input: 0 must not compile"),
    }
}

#[test]
fn compile_rejects_directly_built_zero_output() {
    // Zero declared outputs with zero output neurons used to pass the
    // declared-vs-typed check (`0 == 0`). It must now be a typed rejection.
    let creature = CreatureExport {
        memetic: None,
        input: 1,
        output: 0,
        neurons: Vec::new(),
        synapses: Vec::new(),
        semantic_version: None,
        forward_only: false,
    };
    match compile_creature(&creature) {
        Err(CreatureError::InvalidOutputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidOutputCount, got {other:?}"),
        Ok(_) => panic!("output: 0 must not compile"),
    }
}

// ---------------------------------------------------------------------------
// creature_to_json / creature_to_json_pretty — never *write* a widthless
// creature.
// ---------------------------------------------------------------------------

#[test]
fn serialise_refuses_zero_input() {
    let creature = creature_with_widths(0, 1);
    match creature_to_json(&creature) {
        Err(CreatureError::InvalidInputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidInputCount, got {other:?}"),
        Ok(json) => panic!("input: 0 must not serialise, got {json}"),
    }
}

#[test]
fn serialise_refuses_zero_output() {
    let creature = creature_with_widths(1, 0);
    match creature_to_json(&creature) {
        Err(CreatureError::InvalidOutputCount { found }) => assert_eq!(found, 0),
        Err(other) => panic!("expected InvalidOutputCount, got {other:?}"),
        Ok(json) => panic!("output: 0 must not serialise, got {json}"),
    }
}

#[test]
fn pretty_serialise_refuses_zero_input_and_zero_output() {
    match creature_to_json_pretty(&creature_with_widths(0, 1)) {
        Err(CreatureError::InvalidInputCount { found }) => assert_eq!(found, 0),
        other => panic!("expected InvalidInputCount, got {other:?}"),
    }
    match creature_to_json_pretty(&creature_with_widths(1, 0)) {
        Err(CreatureError::InvalidOutputCount { found }) => assert_eq!(found, 0),
        other => panic!("expected InvalidOutputCount, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error surface
// ---------------------------------------------------------------------------

#[test]
fn width_error_display_mirrors_typescript_creature_validate() {
    // NEAT-AI `src/architecture/CreatureValidate.ts` wording, so logs from the
    // TS and Rust stacks line up.
    assert_eq!(
        CreatureError::InvalidInputCount { found: 0 }.to_string(),
        "Must have at least one input neurons was: 0"
    );
    assert_eq!(
        CreatureError::InvalidOutputCount { found: 0 }.to_string(),
        "Must have at least one output neurons was: 0"
    );
}

#[test]
fn width_errors_are_structural_with_no_source_chain() {
    // Width failures are structural, not JSON failures: no `source()`.
    assert!(
        CreatureError::InvalidInputCount { found: 0 }
            .source()
            .is_none()
    );
    assert!(
        CreatureError::InvalidOutputCount { found: 0 }
            .source()
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Round trip — a valid creature keeps its width byte-for-byte.
// ---------------------------------------------------------------------------

#[test]
fn valid_creature_round_trips_input_and_output_byte_identically() {
    // Production-shaped widths (a real production creature carries `input: 2511,
    // output: 1`), written in the exact canonical field order and spacing that
    // `creature_to_json` emits, so the whole document must come back
    // byte-identical.
    let canonical = concat!(
        r#"{"input":2511,"output":1,"#,
        r#""neurons":[{"type":"output","uuid":"output-0","bias":0.5,"squash":"IDENTITY"}],"#,
        r#""synapses":[{"fromUUID":"input-2510","toUUID":"output-0","weight":1.5}],"#,
        r#""forwardOnly":false}"#
    );

    let creature = parse_creature_json(canonical).expect("valid creature must parse");
    assert_eq!(creature.input, 2511);
    assert_eq!(creature.output, 1);

    let network = compile_creature(&creature).expect("valid creature must compile");
    // 2511 inputs + 1 output neuron.
    assert_eq!(network.num_inputs(), 2511);
    assert_eq!(network.num_neurons(), 2511 + 1);

    let serialised = creature_to_json(&creature).expect("valid creature must serialise");
    assert_eq!(serialised, canonical);

    let reparsed = parse_creature_json(&serialised).expect("serialised form must parse");
    assert_eq!(reparsed.input, 2511);
    assert_eq!(reparsed.output, 1);
    assert_eq!(reparsed, creature);
}

// ---------------------------------------------------------------------------
// Upper bound — the declared width is bounded before it is walked (Issue #622)
// ---------------------------------------------------------------------------

/// The exact payload from the issue: under 100 bytes, declaring a hundred
/// million inputs no creature can carry.
const HUGE_INPUT_JSON: &str = r#"{"input":100000000,"output":1,"neurons":[{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],"synapses":[]}"#;

/// `HUGE_INPUT_JSON` and `creature_with_widths(100_000_000, 1)` both list one
/// neuron, so the declared node count the error carries is the width plus that
/// neuron — the same meaning `TooManyNodes` has after compilation.
const HUGE_NODE_COUNT: usize = 100_000_001;

/// One entry point that must refuse the over-wide declaration, with the case
/// name it is reported and cited under.
///
/// Driving every caller of `validate_creature_width` from one table is what
/// stops a *new* entry point being added with the floor wired up and the
/// ceiling forgotten — the two halves of the rule are one check, so they are
/// one table.
struct CeilingSite {
    /// Case name, reported by
    /// [`every_entry_point_refuses_a_declared_input_past_the_node_ceiling`].
    name: &'static str,
    /// Drives the entry point over a creature declaring `input` inputs beside
    /// one output neuron, returning the declared node count the typed refusal
    /// named, or `None` if the entry point accepted the creature.
    refuse: fn(usize) -> Option<usize>,
    /// Whether this entry point goes on to build the per-input index once the
    /// width passes. The three that do also carry the *node* ceiling — inputs
    /// plus listed neurons — so they refuse a creature declaring exactly
    /// `MAX_NODE_COUNT` inputs beside an output neuron, and are excluded from
    /// the accepting-edge case below for that reason rather than the width one.
    walks_the_width: bool,
}

/// Every route that accepts or emits a creature and then indexes it by the
/// declared width — the four call sites of `validate_creature_width` named in
/// AGENTS.md, plus the two the graft and prune boundaries add.
const CEILING_SITES: &[CeilingSite] = &[
    CeilingSite {
        name: "parse_creature_json_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| {
            let json = format!(
                r#"{{"input":{input},"output":1,"neurons":[{{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}}],"synapses":[]}}"#
            );
            match parse_creature_json(&json) {
                Err(CreatureError::TooManyNodes { count }) => Some(count),
                Err(other) => panic!("expected TooManyNodes, got {other:?}"),
                Ok(_) => None,
            }
        },
        walks_the_width: false,
    },
    CeilingSite {
        name: "compile_creature_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| match compile_creature(&creature_with_widths(input, 1)) {
            Err(CreatureError::TooManyNodes { count }) => Some(count),
            Err(other) => panic!("expected TooManyNodes, got {other:?}"),
            Ok(_) => None,
        },
        walks_the_width: true,
    },
    CeilingSite {
        name: "creature_to_json_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| match creature_to_json(&creature_with_widths(input, 1)) {
            Err(CreatureError::TooManyNodes { count }) => Some(count),
            Err(other) => panic!("expected TooManyNodes, got {other:?}"),
            Ok(_) => None,
        },
        walks_the_width: false,
    },
    CeilingSite {
        name: "creature_to_json_pretty_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| match creature_to_json_pretty(&creature_with_widths(input, 1)) {
            Err(CreatureError::TooManyNodes { count }) => Some(count),
            Err(other) => panic!("expected TooManyNodes, got {other:?}"),
            Ok(_) => None,
        },
        walks_the_width: false,
    },
    CeilingSite {
        name: "validate_creature_topology_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| match validate_creature_topology(&creature_with_widths(input, 1)) {
            Err(GraftError::Creature(CreatureError::TooManyNodes { count })) => Some(count),
            Err(other) => panic!("expected TooManyNodes, got {other:?}"),
            Ok(()) => None,
        },
        walks_the_width: true,
    },
    CeilingSite {
        name: "cleanup_creature_refuses_a_declared_input_past_the_node_ceiling",
        refuse: |input| match cleanup_creature(&creature_with_widths(input, 1)) {
            Err(CleanupError::Creature(CreatureError::TooManyNodes { count })) => Some(count),
            Err(other) => panic!("expected TooManyNodes, got {other:?}"),
            Ok(_) => None,
        },
        walks_the_width: true,
    },
];

#[test]
fn every_entry_point_refuses_a_declared_input_past_the_node_ceiling() {
    for site in CEILING_SITES {
        let Some(count) = (site.refuse)(100_000_000) else {
            panic!(
                "{}: a declared input of 100000000 must be refused",
                site.name
            );
        };
        assert_eq!(
            count, HUGE_NODE_COUNT,
            "{}: the refusal must name the declared node count",
            site.name
        );
    }
}

#[test]
fn every_entry_point_accepts_the_widest_addressable_declaration() {
    // The accepting edge of the same comparison, driven from the same table:
    // `MAX_NODE_COUNT` inputs is the widest observation a `u16` source index
    // can name, so no entry point may refuse it on *width* grounds. Only the
    // sites that stop at the width are asked; the three that walk it go on to
    // add the listed output neuron and refuse 65 537 *nodes*, which is the
    // Issue #177 ceiling and a different question from the rule under test.
    for site in CEILING_SITES.iter().filter(|site| !site.walks_the_width) {
        assert!(
            (site.refuse)(MAX_NODE_COUNT).is_none(),
            "{}: {MAX_NODE_COUNT} inputs is addressable and must not be refused",
            site.name
        );
    }
}

#[test]
fn the_issues_exact_payload_is_refused_rather_than_walked() {
    // The literal reproducer from Issue #622, byte for byte: under 200 bytes of
    // JSON that used to buy a hundred million owned `String` map keys.
    match parse_creature_json(HUGE_INPUT_JSON) {
        Err(CreatureError::TooManyNodes { count }) => assert_eq!(count, HUGE_NODE_COUNT),
        Err(other) => panic!("expected TooManyNodes, got {other:?}"),
        Ok(c) => panic!("input: 100000000 must not parse, got input {}", c.input),
    }
}

#[test]
fn the_input_ceiling_is_reported_before_the_output_floor() {
    // Both halves of the width rule check `input` first, floor and ceiling
    // alike, because `input` is the value that can never be recovered from the
    // rest of the export. A creature that is both over-wide and output-less
    // therefore reports the width it declared, not the output it omitted —
    // pinned here so the order cannot be shuffled unnoticed.
    let mut creature = creature_with_widths(100_000_000, 0);
    creature.neurons.clear();
    creature.synapses.clear();
    match compile_creature(&creature) {
        Err(CreatureError::TooManyNodes { count }) => assert_eq!(count, 100_000_000),
        Err(other) => panic!("expected TooManyNodes ahead of the output floor, got {other:?}"),
        Ok(_) => panic!("an over-wide, output-less creature must not compile"),
    }
}

#[test]
fn the_width_ceiling_error_reads_as_a_node_count() {
    // The Display text is shared with the post-compilation check, so the number
    // it names must be a node count on this path too.
    match compile_creature(&creature_with_widths(100_000_000, 1)) {
        Err(e) => assert_eq!(
            e.to_string(),
            "Creature has 100000001 nodes, exceeding the maximum of 65536 \
             addressable by a u16 source index"
        ),
        Ok(_) => panic!("input: 100000000 must not compile"),
    }
}

#[test]
fn a_declared_input_at_the_node_ceiling_is_still_accepted() {
    // The ceiling is inclusive: `MAX_NODE_COUNT` inputs is the widest creature
    // the u16 index space can address, and the width rule must not narrow it.
    // Parsing costs nothing per declared input, so this stays cheap.
    let json = format!(
        r#"{{"input":{MAX_NODE_COUNT},"output":1,"neurons":[{{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}}],"synapses":[]}}"#
    );
    let creature = parse_creature_json(&json).expect("the ceiling itself must parse");
    assert_eq!(creature.input, MAX_NODE_COUNT);
}
