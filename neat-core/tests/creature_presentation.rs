//! Issue #765 — the persisted-creature presentation corpus parses and compiles.
//!
//! `tests/fixtures/creature_presentation/canonical/` holds creature JSON in the
//! presentation contract form (see that directory's README). This crate does
//! not own the byte layout — the Deno gate `tests/creature_presentation_test.ts`
//! does — but every canonical file must still be a creature this crate reads:
//! [`parse_creature_json`] accepts it and [`compile_creature`] builds a network
//! whose shape matches what the file declares. Listing the directory against a
//! fixed set of names means an emptied or renamed corpus fails rather than
//! passing over zero files.
//!
//! It also pins `variants/serde_typed_pretty/memetic_by_id.json` to the real Rust
//! writer: that variant is documented as `creature_to_json_pretty` applied to the
//! canonical `memetic_by_id.json`, and `docs/CREATURE_JSON_PRESENTATION.md` relies on
//! that to describe today's Rust output. The test below fails if the writer's bytes
//! move, so the fixture and the doc cannot silently go stale.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use neat_core::creature_presentation::{
    PresentationError, creature_export_presentation_json, creature_presentation_json,
};
use neat_core::network::MAX_NODE_COUNT;
use neat_core::{
    CreatureError, MemeticExport, compile_creature, creature_to_json_pretty, parse_creature_json,
};
use serde_json::{Value, json};

/// The corpus the contract promises; adding or removing a file is a deliberate edit here.
const EXPECTED_CANONICAL: [&str; 3] = [
    "if_forest_tagged.json",
    "memetic_by_id.json",
    "number_forms.json",
];

fn canonical_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/creature_presentation/canonical")
}

#[test]
fn canonical_corpus_is_exactly_the_registered_files() {
    let found: BTreeSet<String> = fs::read_dir(canonical_dir())
        .expect("canonical corpus directory is readable")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".json"))
        .collect();
    let expected: BTreeSet<String> = EXPECTED_CANONICAL
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn every_canonical_creature_parses_and_compiles_to_the_declared_shape() {
    for name in EXPECTED_CANONICAL {
        let path = canonical_dir().join(name);
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));

        let creature = parse_creature_json(&text).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let network =
            compile_creature(&creature).unwrap_or_else(|e| panic!("{name}: compile: {e}"));

        // Inputs are implicit, so the network holds `input` plus every listed neuron.
        assert_eq!(network.num_inputs(), creature.input, "{name}: input count");
        assert_eq!(
            network.num_neurons(),
            creature.input + creature.neurons.len(),
            "{name}: neuron count"
        );
        // One compiled synapse per declared synapse: each toUUID names a listed neuron.
        assert_eq!(
            network.num_synapses(),
            creature.synapses.len(),
            "{name}: synapse count"
        );
        let outputs = creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "output")
            .count();
        assert_eq!(outputs, creature.output, "{name}: output neurons");
    }
}

/// The committed `serde_typed_pretty` variant of `memetic_by_id.json` is exactly what
/// `creature_to_json_pretty` writes for the canonical file, byte for byte. The variant
/// and the presentation doc's description of the Rust writer both depend on this, so
/// a change in the writer's output must fail here rather than drift silently.
#[test]
fn typed_serde_variant_is_what_creature_to_json_pretty_writes_today() {
    let canonical = canonical_dir().join("memetic_by_id.json");
    let canonical_text =
        fs::read_to_string(&canonical).expect("canonical memetic_by_id.json is readable");
    let creature =
        parse_creature_json(&canonical_text).expect("canonical memetic_by_id.json parses");

    let written = creature_to_json_pretty(&creature).expect("creature_to_json_pretty succeeds");

    let variant_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/creature_presentation/variants/serde_typed_pretty/memetic_by_id.json",
    );
    let variant_text = fs::read_to_string(&variant_path)
        .expect("serde_typed_pretty memetic_by_id.json is readable");

    assert_eq!(written, variant_text);
}

// ---------------------------------------------------------------------------
// Issue #766 — the contract serialiser.
// ---------------------------------------------------------------------------

/// Variants the corpus promises, as (directory, file); the canonical twin has the same file name.
const EXPECTED_VARIANTS: [(&str, &str); 3] = [
    ("jq_indent_1", "number_forms.json"),
    ("serde_typed_pretty", "memetic_by_id.json"),
    ("serde_value_pretty", "number_forms.json"),
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/creature_presentation")
}

fn read_canonical(name: &str) -> String {
    fs::read_to_string(canonical_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn read_variant(dir: &str, name: &str) -> String {
    fs::read_to_string(corpus_dir().join("variants").join(dir).join(name))
        .unwrap_or_else(|e| panic!("{dir}/{name}: {e}"))
}

fn as_value(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture is JSON")
}

fn write(doc: &Value) -> String {
    creature_presentation_json(doc).expect("document is written")
}

#[test]
fn variant_directory_lists_exactly_the_registered_variants() {
    let mut found = BTreeSet::new();
    for dir in fs::read_dir(corpus_dir().join("variants")).expect("variants dir") {
        let dir = dir.expect("entry");
        for file in fs::read_dir(dir.path()).expect("variant dir") {
            let file = file.expect("entry");
            found.insert((
                dir.file_name().to_string_lossy().into_owned(),
                file.file_name().to_string_lossy().into_owned(),
            ));
        }
    }
    let expected: BTreeSet<(String, String)> = EXPECTED_VARIANTS
        .iter()
        .map(|(d, f)| ((*d).to_string(), (*f).to_string()))
        .collect();
    assert_eq!(found, expected);
}

/// A canonical file is its own fixed point: parse then write reproduces its bytes,
/// and writing the output again changes nothing.
#[test]
fn canonical_files_are_rewritten_byte_for_byte() {
    for name in EXPECTED_CANONICAL {
        let text = read_canonical(name);
        let written = write(&as_value(&text));
        assert_eq!(written, text, "{name}: rewrite differs");
        let again = write(&as_value(&written));
        assert_eq!(again, written, "{name}: second rewrite differs");
    }
}

/// `memetic_by_id.json` carries nothing the typed structs drop (no creature uuid, tags
/// or frozen — checked here), so the typed entry reproduces it exactly.
#[test]
fn typed_entry_writes_the_tagless_canonical_file_byte_for_byte() {
    let text = read_canonical("memetic_by_id.json");
    let value = as_value(&text);
    let root = value.as_object().expect("object");
    assert!(!root.contains_key("uuid") && !root.contains_key("tags"));
    for n in value["neurons"].as_array().expect("neurons") {
        assert!(!n.as_object().expect("object").contains_key("frozen"));
        assert!(!n.as_object().expect("object").contains_key("tags"));
    }
    let creature = parse_creature_json(&text).expect("parses");
    let written = creature_export_presentation_json(&creature).expect("written");
    assert_eq!(written, text);
}

/// Another writer's spelling of a document is normalised to the canonical bytes.
#[test]
fn every_variant_is_normalised_to_its_canonical_bytes() {
    for (dir, name) in EXPECTED_VARIANTS {
        let written = write(&as_value(&read_variant(dir, name)));
        assert_eq!(written, read_canonical(name), "{dir}/{name}");
    }
}

/// Compare two documents by an independent route: numbers by f64 bits, strings exactly,
/// object key SETS (order ignored), arrays by length and order. Shares no code with the
/// writer. Returns how many negative zeros it saw on the left.
fn assert_same_document(a: &Value, b: &Value, path: &str) -> usize {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().expect("f64"), y.as_f64().expect("f64"));
            assert_eq!(x.to_bits(), y.to_bits(), "{path}: {x} vs {y}");
            usize::from(x == 0.0 && x.is_sign_negative())
        }
        (Value::String(x), Value::String(y)) => {
            assert_eq!(x, y, "{path}");
            0
        }
        (Value::Bool(x), Value::Bool(y)) => {
            assert_eq!(x, y, "{path}");
            0
        }
        (Value::Null, Value::Null) => 0,
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{path}: array length");
            x.iter()
                .zip(y)
                .enumerate()
                .map(|(i, (p, q))| assert_same_document(p, q, &format!("{path}/{i}")))
                .sum()
        }
        (Value::Object(x), Value::Object(y)) => {
            let kx: BTreeSet<&String> = x.keys().collect();
            let ky: BTreeSet<&String> = y.keys().collect();
            assert_eq!(kx, ky, "{path}: key set");
            x.iter()
                .map(|(k, v)| assert_same_document(v, &y[k], &format!("{path}/{k}")))
                .sum()
        }
        _ => panic!("{path}: different JSON types"),
    }
}

#[test]
fn rewriting_loses_no_value_including_the_sign_of_zero() {
    let mut negative_zeros = 0;
    let mut inputs: Vec<(String, String)> = EXPECTED_CANONICAL
        .iter()
        .map(|n| ((*n).to_string(), read_canonical(n)))
        .collect();
    for (d, n) in EXPECTED_VARIANTS {
        inputs.push((format!("{d}/{n}"), read_variant(d, n)));
    }
    for (name, text) in inputs {
        let original = as_value(&text);
        let rewritten = as_value(&write(&original));
        negative_zeros += assert_same_document(&original, &rewritten, &name);
    }
    // Positive control: serde_json reads a literal `-0` as -0.0, and the corpus holds some,
    // so the sign check above is not vacuous.
    let parsed: Value = serde_json::from_str("-0").expect("literal");
    assert!(parsed.as_f64().expect("f64").is_sign_negative());
    assert!(negative_zeros >= 2, "corpus must hold -0: {negative_zeros}");
    // The writer itself keeps the sign.
    assert!(write(&as_value(&read_canonical("memetic_by_id.json"))).contains(": -0\n"));
}

fn minimal_creature() -> Value {
    json!({
        "input": 1,
        "output": 1,
        "neurons": [{"type": "output", "uuid": "output-0", "bias": 0, "squash": "IDENTITY"}],
        "synapses": [{"weight": 1, "fromUUID": "input-0", "toUUID": "output-0"}]
    })
}

/// Write numbers through the document entry, in an unmodelled top-level array.
fn written_numbers(values: &[f64]) -> Vec<String> {
    let mut doc = minimal_creature();
    let numbers: Vec<Value> = values
        .iter()
        .map(|v| Value::Number(serde_json::Number::from_f64(*v).expect("finite")))
        .collect();
    doc["sweep"] = Value::Array(numbers);
    let text = write(&doc);
    let lines: Vec<&str> = text.lines().collect();
    let open = lines
        .iter()
        .position(|l| *l == " \"sweep\": [")
        .expect("sweep");
    // Elements are at depth 2 (two spaces), one per line, comma-terminated except the last.
    lines[open + 1..open + 1 + values.len()]
        .iter()
        .map(|l| l.trim_start().trim_end_matches(',').to_string())
        .collect()
}

fn next_up(x: f64) -> f64 {
    f64::from_bits(x.to_bits() + 1)
}
fn next_down(x: f64) -> f64 {
    f64::from_bits(x.to_bits() - 1)
}

#[test]
fn numbers_are_written_as_ecmascript_number_to_string() {
    // (value, expected text, rule)
    let table: Vec<(f64, &str, &str)> = vec![
        (1.0, "1", "integer-valued: no .0"),
        (1e-6, "0.000001", "n = -5 > -6: decimal"),
        (1e-7, "1e-7", "n = -6, not > -6: exponent, signed"),
        (
            1e20,
            "100000000000000000000",
            "k <= n <= 21: digits then zeros",
        ),
        (1e21, "1e+21", "n = 22 > 21: exponent, '+' sign"),
        (
            123456789012345680000.0,
            "123456789012345680000",
            "n = 21: decimal",
        ),
        (5e-324, "5e-324", "smallest subnormal: exponent"),
        (
            f64::MAX,
            "1.7976931348623157e+308",
            "largest: exponent with fraction",
        ),
        (
            2.2985736498644322e-8,
            "2.2985736498644322e-8",
            "17 digits kept",
        ),
        (0.0, "0", "zero"),
        (-0.0, "-0", "contract keeps the sign of zero"),
        (0.1, "0.1", "0 < n <= 21 handled as 0.ddd; n = 0"),
        (-1.5e-9, "-1.5e-9", "sign prefix, exponent form"),
        (1.5, "1.5", "0 < n <= 21: split at n"),
        (0.0000015, "0.0000015", "n = -5: decimal"),
        (
            999999999999999900000.0,
            "999999999999999900000",
            "n = 21: decimal",
        ),
        (
            next_down(1e21),
            "999999999999999900000",
            "largest double below 1e21 stays decimal",
        ),
        (
            next_up(1e21),
            "1.0000000000000001e+21",
            "just above 1e21: exponent",
        ),
        (
            next_down(1e-6),
            "9.999999999999997e-7",
            "just below 1e-6: exponent",
        ),
        (
            next_up(1e-6),
            "0.0000010000000000000002",
            "just above 1e-6: decimal",
        ),
        (
            f64::MIN_POSITIVE,
            "2.2250738585072014e-308",
            "smallest normal",
        ),
        (
            f64::from_bits(0x000f_ffff_ffff_ffff),
            "2.225073858507201e-308",
            "largest subnormal",
        ),
        (-f64::MAX, "-1.7976931348623157e+308", "negative extreme"),
    ];
    let values: Vec<f64> = table.iter().map(|t| t.0).collect();
    let got = written_numbers(&values);
    for ((v, expected, rule), text) in table.iter().zip(&got) {
        assert_eq!(text, expected, "{v:e}: {rule}");
    }
}

/// Fixed-seed xorshift64*, so the sweep is deterministic.
fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[test]
fn number_text_round_trips_and_has_the_ecmascript_shape_across_a_sweep() {
    let mut values: Vec<f64> = Vec::new();
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    while values.len() < 40_000 {
        let v = f64::from_bits(xorshift(&mut state));
        if v.is_finite() {
            values.push(v);
        }
    }
    // Magnitudes near every decade boundary, and the thresholds' neighbours.
    for e in -30..=30 {
        let p = 10f64.powi(e);
        for v in [p, next_up(p), next_down(p), 1.5 * p, 9.999 * p] {
            values.push(v);
            values.push(-v);
        }
    }
    for base in [1e21, 1e-6, 1e-7, 1e20, f64::MIN_POSITIVE, f64::MAX, 5e-324] {
        values.extend([base, next_up(base), next_down(base)]);
    }
    for i in 1..2000u64 {
        values.push(f64::from_bits(i)); // subnormals
        values.push(f64::from_bits(f64::MAX.to_bits() - i)); // extremes
    }

    for chunk in values.chunks(5000) {
        for (v, text) in chunk.iter().zip(written_numbers(chunk)) {
            let back = f64::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(back.to_bits(), v.to_bits(), "{v:e} -> {text}");
            assert!(!text.contains('E'), "{text}: uppercase E");
            assert!(!text.ends_with(".0"), "{text}: .0 suffix");
            if let Some((_, exp)) = text.split_once('e') {
                assert!(
                    exp.starts_with('+') || exp.starts_with('-'),
                    "{text}: unsigned exponent"
                );
            }
            let a = v.abs();
            let decimal = *v == 0.0 || (1e-6..1e21).contains(&a);
            assert_eq!(!text.contains('e'), decimal, "{v:e} -> {text}: notation");
        }
    }
}

#[test]
fn changing_one_number_changes_exactly_one_line() {
    let text = read_canonical("memetic_by_id.json");
    let mut doc = as_value(&text);
    // A number is one line, so one edited weight is one edited line.
    doc["synapses"][0]["weight"] = json!(0.5000001);
    let new = write(&doc);
    let (old_lines, new_lines): (Vec<&str>, Vec<&str>) =
        (text.lines().collect(), new.lines().collect());
    assert_eq!(old_lines.len(), new_lines.len());
    let differing: Vec<usize> = (0..old_lines.len())
        .filter(|i| old_lines[*i] != new_lines[*i])
        .collect();
    assert_eq!(differing.len(), 1, "{differing:?}");
    assert_eq!(new_lines[differing[0]], "   \"weight\": 0.5000001,");
}

#[test]
fn adding_a_creature_tag_changes_only_the_tags_array() {
    let text = read_canonical("if_forest_tagged.json");
    let mut doc = as_value(&text);
    doc["tags"]
        .as_array_mut()
        .expect("tags")
        .push(json!({"name": "added", "value": "yes"}));
    let new = write(&doc);
    let old_lines: Vec<&str> = text.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    // The new tag is 4 lines: open brace, two members, close brace.
    assert_eq!(new_lines.len(), old_lines.len() + 4);

    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old_lines.len() - prefix;
    let suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let changed = prefix..new_lines.len() - suffix;
    // The previous last tag's `}` gains a comma (1 changed line), then the 3 lines up to the
    // new closing `}`, which matches the old closing `}` as suffix: 4 new-side lines in all.
    assert_eq!(changed.len(), 4, "{:?}", &new_lines[changed.clone()]);
    assert_eq!(old_lines.len() - suffix - prefix, 0);
    assert_eq!(new_lines[changed.start], "  },");

    let open = new_lines
        .iter()
        .position(|l| *l == " \"tags\": [")
        .expect("creature tags");
    let close = open
        + new_lines[open..]
            .iter()
            .position(|l| *l == " ],")
            .expect("close");
    assert!(
        changed.start > open && changed.end <= close,
        "{changed:?} vs {open}..{close}"
    );
}

/// Compiled scoring is identical before and after the rewrite, bit for bit.
#[test]
fn rewriting_does_not_change_what_a_creature_scores() {
    let mut files: Vec<(String, String)> = EXPECTED_CANONICAL
        .iter()
        .map(|n| ((*n).to_string(), read_canonical(n)))
        .collect();
    for (d, n) in EXPECTED_VARIANTS {
        files.push((format!("{d}/{n}"), read_variant(d, n)));
    }
    for (name, text) in files {
        let rewritten = write(&as_value(&text));
        let original = parse_creature_json(&text).expect("original parses");
        let after = parse_creature_json(&rewritten).expect("rewritten parses");
        let mut a = compile_creature(&original).expect("compiles");
        let mut b = compile_creature(&after).expect("compiles");
        for step in 0..6 {
            let inputs: Vec<f32> = (0..original.input)
                .map(|i| ((step * 7 + i * 3) as f32 - 9.0) * 0.37)
                .collect();
            let x = a.activate(&inputs, original.output);
            let y = b.activate(&inputs, original.output);
            assert!(!x.is_empty(), "{name}: has outputs");
            let (xb, yb): (Vec<u32>, Vec<u32>) = (
                x.iter().map(|v| v.to_bits()).collect(),
                y.iter().map(|v| v.to_bits()).collect(),
            );
            assert_eq!(xb, yb, "{name}: step {step}");
        }
    }
}

// ---- refusals -------------------------------------------------------------

fn with(mut doc: Value, key: &str, value: Value) -> Value {
    doc[key] = value;
    doc
}

fn minimal_typed() -> neat_core::CreatureExport {
    parse_creature_json(&minimal_creature().to_string()).expect("minimal creature parses")
}

#[test]
fn zero_input_is_refused_by_both_entries_and_one_input_is_accepted() {
    let doc = with(minimal_creature(), "input", json!(0));
    assert!(matches!(
        creature_presentation_json(&doc),
        Err(PresentationError::Creature(
            CreatureError::InvalidInputCount { found: 0 }
        ))
    ));
    let mut typed = minimal_typed();
    typed.input = 0;
    assert!(matches!(
        creature_export_presentation_json(&typed),
        Err(PresentationError::Creature(
            CreatureError::InvalidInputCount { found: 0 }
        ))
    ));
    // Control: the same fixture with a legal width is written.
    assert!(creature_presentation_json(&minimal_creature()).is_ok());
    assert!(creature_export_presentation_json(&minimal_typed()).is_ok());
}

#[test]
fn zero_output_is_refused_by_both_entries_and_one_output_is_accepted() {
    let doc = with(minimal_creature(), "output", json!(0));
    assert!(matches!(
        creature_presentation_json(&doc),
        Err(PresentationError::Creature(
            CreatureError::InvalidOutputCount { found: 0 }
        ))
    ));
    let mut typed = minimal_typed();
    typed.output = 0;
    assert!(matches!(
        creature_export_presentation_json(&typed),
        Err(PresentationError::Creature(
            CreatureError::InvalidOutputCount { found: 0 }
        ))
    ));
}

#[test]
fn an_input_width_past_the_node_ceiling_is_refused_and_the_ceiling_itself_is_accepted() {
    let doc = with(minimal_creature(), "input", json!(MAX_NODE_COUNT + 1));
    assert!(matches!(
        creature_presentation_json(&doc),
        Err(PresentationError::Creature(
            CreatureError::TooManyNodes { .. }
        ))
    ));
    let mut typed = minimal_typed();
    typed.input = MAX_NODE_COUNT + 1;
    assert!(matches!(
        creature_export_presentation_json(&typed),
        Err(PresentationError::Creature(
            CreatureError::TooManyNodes { .. }
        ))
    ));
    // Control: the ceiling is inclusive.
    let at_ceiling = with(minimal_creature(), "input", json!(MAX_NODE_COUNT));
    assert!(creature_presentation_json(&at_ceiling).is_ok());
}

#[test]
fn a_non_finite_typed_number_is_refused_with_its_path() {
    let mut typed = minimal_typed();
    typed.synapses[0].weight = f64::NAN;
    match creature_export_presentation_json(&typed) {
        Err(PresentationError::NonFiniteNumber { path }) => assert_eq!(path, "/synapses/0/weight"),
        other => panic!("expected NonFiniteNumber, got {other:?}"),
    }
    let mut typed = minimal_typed();
    typed.neurons[0].bias = f64::INFINITY;
    match creature_export_presentation_json(&typed) {
        Err(PresentationError::NonFiniteNumber { path }) => assert_eq!(path, "/neurons/0/bias"),
        other => panic!("expected NonFiniteNumber, got {other:?}"),
    }
    let mut typed = minimal_typed();
    let mut memetic = MemeticExport::default();
    memetic.biases.insert("3".to_string(), f64::NEG_INFINITY);
    typed.memetic = Some(memetic);
    match creature_export_presentation_json(&typed) {
        Err(PresentationError::NonFiniteNumber { path }) => assert_eq!(path, "/memetic/biases/3"),
        other => panic!("expected NonFiniteNumber, got {other:?}"),
    }
    // Control: with the memetic bias made finite the same creature is written.
    let mut typed = minimal_typed();
    let mut memetic = MemeticExport::default();
    memetic.biases.insert("3".to_string(), 0.5);
    typed.memetic = Some(memetic);
    assert!(creature_export_presentation_json(&typed).is_ok());
}

#[test]
fn a_null_in_a_document_is_refused_with_its_path() {
    // serde_json turns NaN into null, so this is a weight that was already lost.
    let mut doc = minimal_creature();
    doc["synapses"][0]["weight"] = json!(f64::NAN);
    match creature_presentation_json(&doc) {
        Err(PresentationError::NullValue { path }) => assert_eq!(path, "/synapses/0/weight"),
        other => panic!("expected NullValue, got {other:?}"),
    }
    // Control: the same document with a real weight is written.
    doc["synapses"][0]["weight"] = json!(0.5);
    assert!(creature_presentation_json(&doc).is_ok());
}

#[test]
fn a_document_that_is_not_a_creature_is_refused_as_json() {
    let mut doc = minimal_creature();
    doc.as_object_mut().expect("object").remove("neurons");
    assert!(matches!(
        creature_presentation_json(&doc),
        Err(PresentationError::Creature(CreatureError::Json(_)))
    ));
    // Control: with `neurons` restored it is a creature.
    assert!(creature_presentation_json(&minimal_creature()).is_ok());
}
