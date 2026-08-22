//! Replays NEAT-AI's `creatureValidate` conformance corpus through the
//! **runtime** request shape (NEAT-AI#3803).
//!
//! `creature_validate_conformance.rs` replays the same corpus through the
//! export form, where ten cases describe something the wire shape cannot carry
//! and are declared as divergences. That is the gap this shape closes: the
//! corpus creatures *are* the runtime shape, so a case is handed to the
//! validator as written — no implicit inputs, no derived ids, no widening of a
//! non-finite bias into "absent".
//!
//! Only one case cannot be answered here, and it is not a rule at all:
//! `neuron-index-mismatch` reads `neuron.index`, an in-memory cache that stays
//! host-side (NEAT-AI#3802). Every other case must match exactly — class,
//! `reason`, message text and, for a creature that passes, all five counters.
//! A case that stops matching fails here by name.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use neat_core::{ValidateResponse, creature_validate_json};
use serde::Deserialize;
use serde_json::{Value, json};

/// Where the vendored corpus lives.
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
    expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expect {
    outcome: String,
    error: Option<String>,
    reason: Option<String>,
    message_contains: Option<String>,
    stats: Option<Value>,
}

/// The one case this shape cannot answer: `neuron.index` is a host-side cache,
/// not a rule (NEAT-AI#3802), so no creature description can carry it.
const HOST_ONLY_CASES: [&str; 1] = ["neuron-index-mismatch"];

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

/// Ask the boundary about one case, exactly as a host would.
fn answer(case: &Case) -> ValidateResponse {
    let request = match &case.options {
        Some(options) => json!({ "runtimeCreature": case.creature, "options": options }),
        None => json!({ "runtimeCreature": case.creature }),
    };
    let text = creature_validate_json(&request.to_string());
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("unreadable response: {error}"))
}

#[test]
fn every_corpus_case_matches_through_the_runtime_shape() {
    let mut replayed = 0usize;
    for case in load_corpus() {
        if HOST_ONLY_CASES.contains(&case.name.as_str()) {
            continue;
        }
        replayed += 1;
        let response = answer(&case);

        if case.expect.outcome == "ok" {
            assert!(
                response.ok,
                "{}: expected the creature to pass, got {:?}",
                case.name, response.failure
            );
            let stats = serde_json::to_value(response.stats.expect("a passing case carries stats"))
                .expect("stats serialise");
            assert_eq!(
                stats,
                *case
                    .expect
                    .stats
                    .as_ref()
                    .expect("an ok case declares stats"),
                "{}: counters differ",
                case.name
            );
            continue;
        }

        let failure = response
            .failure
            .unwrap_or_else(|| panic!("{}: expected a failure, the creature passed", case.name));
        assert!(
            !failure.malformed,
            "{}: the payload never reached a rule — {}",
            case.name, failure.message
        );
        assert_eq!(
            failure.class,
            *case
                .expect
                .error
                .as_ref()
                .expect("a throwing case declares a class"),
            "{}: error class differs (message: {})",
            case.name,
            failure.message
        );
        assert_eq!(
            failure.reason,
            *case
                .expect
                .reason
                .as_ref()
                .expect("a throwing case declares a reason"),
            "{}: reason differs (message: {})",
            case.name,
            failure.message
        );
        let expected_text = case
            .expect
            .message_contains
            .as_ref()
            .expect("a throwing case declares its message");
        assert!(
            failure.message.contains(expected_text),
            "{}: message {:?} does not contain {:?}",
            case.name,
            failure.message,
            expected_text
        );
    }

    assert!(
        replayed >= 40,
        "only {replayed} cases replayed — the corpus should not have shrunk"
    );
}

#[test]
fn the_host_only_declarations_are_still_host_only() {
    let corpus = load_corpus();
    let names: BTreeSet<&str> = corpus.iter().map(|case| case.name.as_str()).collect();

    for declared in HOST_ONLY_CASES {
        assert!(
            names.contains(declared),
            "{declared} is declared host-only but no longer exists in the corpus"
        );
        let case = corpus
            .iter()
            .find(|case| case.name == declared)
            .expect("the declared case exists");
        // A stale declaration must fail loudly: if this shape *can* answer the
        // case, it belongs in the replay above rather than in the exemption.
        let response = answer(case);
        assert!(
            response.ok,
            "{declared} is declared host-only, but the rules now report it: {:?}",
            response.failure
        );
    }
}
