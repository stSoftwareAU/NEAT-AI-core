//! The packed shape and the runtime JSON shape never disagree about a creature
//! (NEAT-AI#3832).
//!
//! `creature_validate_packed` exists only to be faster. It answers the same
//! question as [`creature_validate_json`] over the same rules, so the one
//! thing that can go wrong is a *shape* bug: a code the encoder writes and the
//! decoder reads back as something else, an option that does not survive the
//! flags, a defect the buffer cannot carry. Any of those would show up as the
//! two shapes disagreeing — the packed one calling a broken creature healthy,
//! or sending the host to fetch a failure the JSON shape does not report.
//!
//! So this replays NEAT-AI's own `creatureValidate` conformance corpus — the
//! same corpus `creature_validate_runtime_conformance.rs` replays — through
//! both shapes and asserts they agree, case by case:
//!
//! | The runtime shape says | The packed shape must say |
//! |------------------------|---------------------------|
//! | healthy, with counters | healthy, with the **same** counters |
//! | a rule was broken | `detailRequired` — go ask the JSON shape |
//!
//! A packed answer is never `malformed` for a corpus creature: the encoder
//! writes what the decoder reads, and a boundary fault there would be a bug in
//! this crate rather than a verdict.

use std::fs;
use std::path::{Path, PathBuf};

use neat_core::{
    PackedValidateResponse, RuntimeCreature, ValidateOptions, ValidateResponse,
    creature_validate_json, creature_validate_packed, encode_packed_request,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Where the vendored corpus lives — the same directory the runtime replay
/// reads, so the two cannot drift onto different corpora.
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/creature_validate")
}

#[derive(Debug, Deserialize)]
struct CorpusFile {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    creature: Value,
    #[serde(default)]
    options: Option<Value>,
}

/// The options bag as the corpus writes it, so the two shapes are asked the
/// same question rather than each defaulting its own way.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaseOptions {
    #[serde(default)]
    neurons: Option<usize>,
    #[serde(default)]
    connections: Option<usize>,
    #[serde(default)]
    feedback_loop: Option<bool>,
    #[serde(default)]
    forward_only: Option<bool>,
}

impl From<CaseOptions> for ValidateOptions {
    fn from(options: CaseOptions) -> Self {
        Self {
            neurons: options.neurons,
            connections: options.connections,
            feedback_loop: options.feedback_loop,
            forward_only: options.forward_only.unwrap_or(false),
        }
    }
}

fn load_corpus() -> Vec<Case> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("the vendored corpus directory must exist")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter(|path| path.file_name().is_some_and(|name| name != "coverage.json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the corpus must not be empty");

    let mut cases = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).expect("readable corpus file");
        let file: CorpusFile = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        cases.extend(file.cases);
    }
    cases
}

/// Ask the JSON shape, exactly as a host would.
fn json_answer(case: &Case) -> ValidateResponse {
    let request = match &case.options {
        Some(options) => json!({ "runtimeCreature": case.creature, "options": options }),
        None => json!({ "runtimeCreature": case.creature }),
    };
    let text = creature_validate_json(&request.to_string());
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("unreadable response: {error}"))
}

/// Ask the packed shape the same question, encoding the case through the
/// crate's own encoder rather than restating the layout here.
fn packed_answer(case: &Case) -> PackedValidateResponse {
    let creature: RuntimeCreature = serde_json::from_value(case.creature.clone())
        .unwrap_or_else(|error| panic!("{}: unreadable creature: {error}", case.name));
    let options: ValidateOptions = case
        .options
        .clone()
        .map(|options| {
            serde_json::from_value::<CaseOptions>(options)
                .unwrap_or_else(|error| panic!("{}: unreadable options: {error}", case.name))
        })
        .unwrap_or_default()
        .into();

    // The memetic record travels alongside the buffer rather than inside it.
    let memetic = case
        .creature
        .get("memetic")
        .filter(|memetic| !memetic.is_null())
        .map(ToString::to_string)
        .unwrap_or_default();

    let text = creature_validate_packed(&encode_packed_request(&creature, &options), &memetic);
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("unreadable response: {error}"))
}

#[test]
fn every_corpus_case_reaches_the_same_verdict_through_both_shapes() {
    let mut replayed = 0usize;

    for case in load_corpus() {
        replayed += 1;
        let json = json_answer(&case);
        let packed = packed_answer(&case);

        assert!(
            packed.failure.is_none(),
            "{}: the packed buffer never reached a rule — {:?}",
            case.name,
            packed.failure
        );

        assert_eq!(
            packed.ok, json.ok,
            "{}: the two shapes disagree — JSON says {}, packed says {} (JSON failure: {:?})",
            case.name, json.ok, packed.ok, json.failure
        );

        if json.ok {
            assert_eq!(
                packed.stats, json.stats,
                "{}: the two shapes counted differently",
                case.name
            );
            assert_eq!(
                packed.detail_required, None,
                "{}: a healthy creature needs no detail",
                case.name
            );
        } else {
            assert_eq!(
                packed.detail_required,
                Some(true),
                "{}: a broken creature must send the host to the JSON shape",
                case.name
            );
        }
    }

    assert!(
        replayed >= 40,
        "only {replayed} cases replayed — the corpus should not have shrunk"
    );
}

/// The corpus is only evidence if it exercises both answers through the packed
/// shape — a replay where everything passed would assert nothing about the
/// `detailRequired` half.
#[test]
fn the_corpus_reaches_both_answers_through_the_packed_shape() {
    let mut healthy = 0;
    let mut broken = 0;

    for case in load_corpus() {
        if packed_answer(&case).ok {
            healthy += 1;
        } else {
            broken += 1;
        }
    }

    assert!(healthy >= 5, "only {healthy} corpus creatures passed");
    assert!(broken >= 20, "only {broken} corpus creatures were rejected");
}

// ---------------------------------------------------------------------------
// Issue #577 — the role is part of the key in every request shape.
// ---------------------------------------------------------------------------

/// One constant feeding an `IF` neuron under all three roles, written in the
/// runtime shape: three synapses sharing the pair `(1, 2)`.
///
/// The corpus predates the rule, so this is the case that proves the role
/// buffer really is threaded through the packed layout and the runtime one —
/// under `(from, to)` keying every shape would call the second synapse a
/// duplicate.
fn shared_source_if_case(target_squash: &str) -> Case {
    Case {
        name: format!("shared-source-if-{target_squash}"),
        creature: json!({
            "input": 1,
            "output": 1,
            "neurons": [
                { "type": "input", "id": 0, "uuid": "input-0", "bias": 0.0 },
                { "type": "constant", "id": 1, "uuid": "k", "bias": 1.0 },
                { "type": "hidden", "id": 2, "uuid": "if-0", "bias": 0.0, "squash": target_squash },
                { "type": "output", "id": -1, "uuid": "o", "bias": 0.0, "squash": "IDENTITY" }
            ],
            "synapses": [
                { "from": 0, "to": 2, "weight": 1.0, "type": "condition" },
                { "from": 1, "to": 2, "weight": 1.0, "type": "condition" },
                { "from": 1, "to": 2, "weight": 2.0, "type": "negative" },
                { "from": 1, "to": 2, "weight": 3.0, "type": "positive" },
                { "from": 2, "to": 3, "weight": 1.0 }
            ]
        }),
        options: None,
    }
}

#[test]
fn one_source_carries_every_role_into_an_if_target_through_both_shapes() {
    let case = shared_source_if_case("IF");

    let json = json_answer(&case);
    assert!(json.ok, "the JSON shape rejected it: {:?}", json.failure);
    let packed = packed_answer(&case);
    assert!(packed.ok, "the packed shape disagreed with the JSON shape");
    assert_eq!(
        packed.stats, json.stats,
        "the two shapes counted differently"
    );
}

#[test]
fn the_same_wiring_into_a_non_if_target_is_rejected_through_both_shapes() {
    let case = shared_source_if_case("IDENTITY");

    let json = json_answer(&case);
    assert!(!json.ok, "a non-IF target reads no roles");
    let failure = json.failure.expect("a rejected creature carries a failure");
    assert_eq!(failure.reason, "DUPLICATE_SYNAPSE");

    let packed = packed_answer(&case);
    assert!(!packed.ok, "the packed shape disagreed with the JSON shape");
    assert_eq!(packed.detail_required, Some(true));
}
