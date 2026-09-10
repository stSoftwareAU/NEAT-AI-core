//! A creature weight must parse to the **exact** `f64` its JSON literal names,
//! so the Rust and TypeScript engines load the same network.
//!
//! `serde_json`'s default number parser is a fast approximation that lands up
//! to 1 ULP away from the nearest `f64`; the exact algorithm is behind its
//! opt-in `float_roundtrip` feature. JavaScript `JSON.parse` — and Rust's own
//! `f64::from_str` — are always exact, so without that feature the Backprop,
//! scorer and Lamarck engines trained and scored a *slightly different network*
//! from the one NEAT-AI's TypeScript loaded, silently: nothing failed, the
//! number was just marginally wrong.
//!
//! The oracle here is deliberately **not** `serde_json`. Every expectation is
//! either the `f64` bit pattern the test itself started from, or
//! `f64::from_str` over the same literal text — two routes that share no code
//! with the parser under test, so a fault in it moves only one side of the
//! assertion.

use std::collections::BTreeMap;

use neat_core::creature::MemeticWeights;
use neat_core::{CreatureExport, creature_to_json, parse_creature_json};

// ---------------------------------------------------------------------------
// Literals the fast parser gets wrong.
// ---------------------------------------------------------------------------

/// Decimal literals whose nearest `f64` the approximate parser misses by 1 ULP.
///
/// The first is the weight from the reported defect — synapse 10511 of a
/// production sampler fixture creature, `input-542 -> neuron-1514601746`. The
/// rest were found by round-tripping shortest-form `f64` text through the
/// unfixed parser, and cover ordinary magnitudes as well as the extremes, so
/// the fix cannot be mistaken for a special case on one exponent range.
const HARD_LITERALS: &[&str] = &[
    "2.2985736498644322e-8",
    "-0.20221894534048165",
    "-104913124.37877665",
    "-187348849289.48602",
    "3.518437208883201171875e13",
    "7.038531e-26",
    "8.988465674311579e307",
    "2.2250738585072011e-308",
    "5.911867584488277e-25",
    "-2.3359371920453957e-123",
];

// ---------------------------------------------------------------------------
// Fixtures — `input-0 -> h -> output-0`, one literal under test per creature.
// ---------------------------------------------------------------------------

/// A creature whose synapse `h -> output-0` carries `weight`, whose hidden
/// neuron carries `bias`, and whose memetic block repeats both.
fn creature_json(weight: &str, bias: &str) -> String {
    format!(
        r#"{{
  "input": 1,
  "output": 1,
  "neurons": [
    {{ "type": "hidden", "uuid": "h", "bias": {bias}, "squash": "TANH" }},
    {{ "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }}
  ],
  "synapses": [
    {{ "fromUUID": "input-0", "toUUID": "h", "weight": 1.0 }},
    {{ "fromUUID": "h", "toUUID": "output-0", "weight": {weight} }}
  ],
  "memetic": {{
    "generation": 7,
    "biases": {{ "h": {bias} }},
    "weights": [
      {{ "fromUUID": "h", "toUUID": "output-0", "weight": {weight} }}
    ]
  }}
}}"#
    )
}

fn parse(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("the creature parses")
}

/// The `f64` the literal text names, by a route that never touches
/// `serde_json`.
fn exact(literal: &str) -> f64 {
    literal
        .parse::<f64>()
        .expect("the literal is valid float text")
}

/// Bit-level equality — `-0.0 == 0.0` and `NaN != NaN` under `==`, and a 1 ULP
/// drift is exactly what this file exists to catch, so compare the payloads.
fn assert_same_f64(actual: f64, expected: f64, what: &str) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{what}: parsed {actual:e} (0x{:016x}) but the literal names {expected:e} (0x{:016x})",
        actual.to_bits(),
        expected.to_bits()
    );
}

/// The memetic block's single row weight.
fn memetic_row_weight(creature: &CreatureExport) -> f64 {
    let memetic = creature.memetic.as_ref().expect("the memetic block parsed");
    match &memetic.weights {
        MemeticWeights::Rows(rows) => rows[0].weight.expect("the row carries a weight"),
        MemeticWeights::ById(_) => panic!("the fixture writes the row form"),
    }
}

/// The memetic block's biases, keyed by neuron.
fn memetic_biases(creature: &CreatureExport) -> &BTreeMap<String, f64> {
    &creature
        .memetic
        .as_ref()
        .expect("the memetic block parsed")
        .biases
}

// ---------------------------------------------------------------------------
// Every `f64` field on the wire parses exactly.
// ---------------------------------------------------------------------------

#[test]
fn synapse_weight_parses_to_the_f64_the_literal_names() {
    for literal in HARD_LITERALS {
        let creature = parse(&creature_json(literal, "0.25"));
        assert_same_f64(creature.synapses[1].weight, exact(literal), literal);
    }
}

#[test]
fn neuron_bias_parses_to_the_f64_the_literal_names() {
    for literal in HARD_LITERALS {
        let creature = parse(&creature_json("1.0", literal));
        assert_same_f64(creature.neurons[0].bias, exact(literal), literal);
    }
}

#[test]
fn memetic_weight_and_bias_parse_to_the_f64_the_literals_name() {
    for literal in HARD_LITERALS {
        let creature = parse(&creature_json(literal, literal));
        assert_same_f64(memetic_row_weight(&creature), exact(literal), literal);
        assert_same_f64(memetic_biases(&creature)["h"], exact(literal), literal);
    }
}

// ---------------------------------------------------------------------------
// The round-trip contract the module docs state (Issue #30).
// ---------------------------------------------------------------------------

#[test]
fn parse_serialise_parse_preserves_every_hard_weight() {
    for literal in HARD_LITERALS {
        let first = parse(&creature_json(literal, literal));
        let written = creature_to_json(&first).expect("the creature serialises");
        let second = parse_creature_json(&written).expect("the re-serialised creature parses");

        assert_eq!(
            first, second,
            "{literal}: parse -> serialise -> parse changed the creature"
        );
        assert_same_f64(second.synapses[1].weight, exact(literal), literal);
    }
}

// ---------------------------------------------------------------------------
// The general case: any `f64`, not the ten above.
// ---------------------------------------------------------------------------

/// Deterministic xorshift64 — a fixed seed, so a failure is reproducible and
/// the sweep is not a flaky test.
fn xorshift64(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Shortest-form text of **any** finite `f64` must parse back to that exact
/// `f64`.
///
/// This is the property the fix has to hold in general, and the oracle is the
/// `f64` the sweep started from — no parser, fast or exact, is consulted for
/// the expected value. Roughly 30% of these bit patterns drift by 1 ULP
/// through the approximate parser, so the sweep is red long before it ends
/// against unfixed code.
#[test]
fn shortest_form_text_of_any_f64_parses_back_to_that_f64() {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut checked = 0usize;

    for _ in 0..4_000 {
        let value = f64::from_bits(xorshift64(&mut state));
        if !value.is_finite() {
            continue;
        }
        // `{:?}` emits the shortest text that names this exact `f64`, and is
        // always valid JSON number syntax for a finite value.
        let literal = format!("{value:?}");
        let creature = parse(&creature_json(&literal, &literal));

        assert_same_f64(creature.synapses[1].weight, value, &literal);
        assert_same_f64(creature.neurons[0].bias, value, &literal);
        checked += 1;
    }

    assert!(
        checked > 3_000,
        "the sweep must actually exercise the parser, not skip its way to green \
         (only {checked} finite values reached it)"
    );
}
