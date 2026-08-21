//! Issue #562 — replay NEAT-AI's `creatureValidate` conformance corpus against
//! the Rust port.
//!
//! The corpus (NEAT-AI#3801) is the executable definition of what the
//! TypeScript does today. It is **vendored**, not fetched: the bytes and the
//! commit they came from are in
//! `tests/fixtures/creature_validate/README.md`, so drift between the two
//! stacks shows up as a diff rather than as a test that quietly changed
//! meaning.
//!
//! # The two shapes
//!
//! The corpus describes creatures in NEAT-AI's **runtime** shape — every
//! neuron listed, inputs included, ids and `from`/`to` as integers. This crate
//! validates the **wire** shape ([`CreatureExport`]): inputs are implicit,
//! neurons are named by UUID and ids are derived. [`to_export`] is the one
//! adapter between them, and it is deliberately dumb — it drops the input
//! neurons, resolves each synapse index to a UUID, and hands the result
//! straight to serde.
//!
//! # No case is skipped
//!
//! Ten cases describe something the wire shape cannot express — a non-integer
//! id serde rejects before any rule runs, an input neuron carrying its own id,
//! the host-only `neuron.index` check. They are not dropped: each is declared
//! in [`DIVERGENCES`] with *why*, and with what this crate does instead, and
//! the test asserts that behaviour too. A divergent case that starts
//! converting, or whose Rust outcome changes, fails here just as loudly as a
//! mismatched one — and an entry that no longer diverges fails as a stale
//! declaration.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use neat_core::{CreatureExport, ValidateOptions, ValidationStats, creature_validate};
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Where the vendored corpus lives.
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/creature_validate")
}

// ---------------------------------------------------------------------------
// The corpus, as it is written.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CorpusFile {
    #[allow(dead_code)]
    group: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    rule: String,
    creature: Value,
    #[serde(default)]
    options: Option<CaseOptions>,
    expect: Expect,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaseOptions {
    neurons: Option<usize>,
    connections: Option<usize>,
    feedback_loop: Option<bool>,
    forward_only: Option<bool>,
}

impl From<&CaseOptions> for ValidateOptions {
    fn from(options: &CaseOptions) -> Self {
        Self {
            neurons: options.neurons,
            connections: options.connections,
            feedback_loop: options.feedback_loop,
            forward_only: options.forward_only.unwrap_or(false),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expect {
    outcome: String,
    error: Option<String>,
    reason: Option<String>,
    message_contains: Option<String>,
    stats: Option<ExpectedStats>,
}

#[derive(Debug, Deserialize)]
struct ExpectedStats {
    input: u32,
    constant: u32,
    hidden: u32,
    output: u32,
    connections: u32,
}

/// The `coverage.json` manifest: every validation site, in source order.
#[derive(Debug, Deserialize)]
struct Coverage {
    sites: Vec<Site>,
}

#[derive(Debug, Deserialize)]
struct Site {
    id: String,
    status: String,
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

fn load_coverage() -> Coverage {
    let text = fs::read_to_string(fixture_dir().join("coverage.json")).expect("readable manifest");
    serde_json::from_str(&text).expect("coverage.json parses")
}

// ---------------------------------------------------------------------------
// Runtime shape -> wire shape.
// ---------------------------------------------------------------------------

/// Convert one corpus creature into the [`CreatureExport`] this crate validates.
///
/// The mapping is the one `compile_creature` already fixes: indices `0..input`
/// are the implicit input neurons (`input-N`), index `input + i` is
/// `neurons[i]`, and a synapse's `from` / `to` become those neurons' UUIDs.
///
/// `Err` means the wire shape cannot carry the case — the message says what
/// stopped it, and the case must be declared in [`DIVERGENCES`].
fn to_export(creature: &Value) -> Result<CreatureExport, String> {
    let object = creature.as_object().ok_or("creature is not an object")?;
    let input = object
        .get("input")
        .and_then(Value::as_u64)
        .ok_or("`input` is not a non-negative integer the wire form can hold")?
        as usize;

    let neurons = object
        .get("neurons")
        .and_then(Value::as_array)
        .ok_or("`neurons` is not an array")?;

    // Every neuron's wire label, indexed as the runtime shape indexes them, so
    // the synapses can be resolved afterwards.
    let mut labels: Vec<String> = Vec::with_capacity(neurons.len());
    let mut exported: Vec<Value> = Vec::new();
    // Non-finite biases: JSON has no literal for them, so the corpus writes
    // "NaN" / "Infinity" / "-Infinity" and this patches them back in after
    // serde has parsed the rest.
    let mut bias_patches: Vec<(usize, f64)> = Vec::new();

    for (index, neuron) in neurons.iter().enumerate() {
        let neuron = neuron.as_object().ok_or("a neuron is not an object")?;
        let declared_type = neuron.get("type").and_then(Value::as_str).unwrap_or("");

        // The leading run of input-typed neurons is what the wire form makes
        // implicit; it is dropped here and rebuilt from `creature.input`.
        if declared_type == "input" {
            if !exported.is_empty() {
                return Err(format!(
                    "neuron {index} is an input neuron behind a computational one; \
                     the wire form lists no input neurons at all"
                ));
            }
            if neuron.get("id").and_then(Value::as_i64) != Some(index as i64) {
                return Err(format!(
                    "input neuron {index} declares an id that is not its index; \
                     implicit input neurons take `id == index` by construction"
                ));
            }
            labels.push(format!("input-{index}"));
            continue;
        }

        let uuid = neuron.get("uuid").and_then(Value::as_str).unwrap_or("");
        labels.push(uuid.to_string());

        let mut wire = Map::new();
        wire.insert("type".to_string(), json!(declared_type));
        wire.insert("uuid".to_string(), json!(uuid));

        // Ids: an output's is derived as `-(outputIndex + 1)` and every other
        // neuron with no id takes a hash of its UUID, so a case that says
        // otherwise is describing something the wire form overrides.
        let declared_id = neuron.get("id").unwrap_or(&Value::Null).clone();
        if declared_type == "output" {
            let ordinal = exported
                .iter()
                .filter(|n| n["type"] == json!("output"))
                .count() as i64;
            if declared_id != json!(-(ordinal + 1)) {
                return Err(format!(
                    "output neuron {index} declares id {declared_id}, but the wire form \
                     derives -{} whatever the file says",
                    ordinal + 1
                ));
            }
        } else if declared_id.is_null() {
            return Err(format!(
                "neuron {index} declares no id; the wire form derives one from its UUID, \
                 so the missing-id rule cannot be reached this way"
            ));
        }
        if !declared_id.is_null() {
            wire.insert("id".to_string(), declared_id);
        }

        match neuron.get("bias") {
            Some(Value::String(text)) => {
                let value = match text.as_str() {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    other => return Err(format!("neuron {index} has a bias of {other:?}")),
                };
                bias_patches.push((exported.len(), value));
                wire.insert("bias".to_string(), json!(0.0));
            }
            Some(bias) => {
                wire.insert("bias".to_string(), bias.clone());
            }
            None => {
                wire.insert("bias".to_string(), Value::Null);
            }
        }

        if let Some(squash) = neuron.get("squash") {
            wire.insert("squash".to_string(), squash.clone());
        }

        exported.push(Value::Object(wire));
    }

    let mut synapses = Vec::new();
    for synapse in object
        .get("synapses")
        .and_then(Value::as_array)
        .ok_or("`synapses` is not an array")?
    {
        let label = |key: &str| -> Result<String, String> {
            let index = synapse
                .get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("synapse `{key}` is not an index"))?
                as usize;
            labels
                .get(index)
                .cloned()
                .ok_or_else(|| format!("synapse `{key}` names no neuron: {index}"))
        };

        let mut wire = Map::new();
        wire.insert("fromUUID".to_string(), json!(label("from")?));
        wire.insert("toUUID".to_string(), json!(label("to")?));
        wire.insert(
            "weight".to_string(),
            synapse.get("weight").cloned().unwrap_or(Value::Null),
        );
        if let Some(kind) = synapse.get("type") {
            wire.insert("type".to_string(), kind.clone());
        }
        synapses.push(Value::Object(wire));
    }

    let mut wire = Map::new();
    wire.insert("input".to_string(), json!(input));
    wire.insert(
        "output".to_string(),
        object.get("output").cloned().unwrap_or(Value::Null),
    );
    wire.insert("neurons".to_string(), Value::Array(exported));
    wire.insert("synapses".to_string(), Value::Array(synapses));
    if let Some(forward_only) = object.get("forwardOnly") {
        wire.insert("forwardOnly".to_string(), forward_only.clone());
    }
    if let Some(memetic) = object.get("memetic") {
        wire.insert("memetic".to_string(), memetic.clone());
    }

    let mut creature: CreatureExport = serde_json::from_value(Value::Object(wire))
        .map_err(|error| format!("the wire form rejects this creature: {error}"))?;

    for (index, bias) in bias_patches {
        creature.neurons[index].bias = bias;
    }

    Ok(creature)
}

// ---------------------------------------------------------------------------
// The declared divergences.
// ---------------------------------------------------------------------------

/// What this crate does with a case the wire shape cannot describe.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RustOutcome {
    /// The creature never reaches a rule — serde or the shape adapter stops it.
    Unrepresentable,
    /// The creature reaches the rules and breaks none, with these counters:
    /// `input`, `constant`, `hidden`, `output`, `connections`.
    Accepted([u32; 5]),
}

/// One corpus case the Rust port cannot reproduce, and why.
struct Declared {
    /// The corpus case name.
    case: &'static str,
    /// Why the wire shape cannot carry it.
    why: &'static str,
    /// What this crate does instead — pinned so a change here still fails.
    outcome: RustOutcome,
}

/// Every case the port does not reproduce.
///
/// Each is a consequence of the input format the contract fixed in Issue #559,
/// not of a rule that was dropped — the rules themselves are all ported. The
/// four `Unrepresentable` parse-boundary entries are the format working as
/// designed: a malformed creature fails at the wire boundary instead of
/// reaching a validator that would have to describe it.
const DIVERGENCES: [Declared; 10] = [
    // -- The wire form rejects the creature before any rule runs. -----------
    Declared {
        case: "input-count-not-integer",
        why: "`input` is `usize` on the wire; 1.5 cannot be written, let alone validated",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "output-count-not-integer",
        why: "`output` is `usize` on the wire; 2.5 is a serde error, not a rule failure",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "neuron-id-not-integer",
        why: "the case states -1.5 on an *output* neuron, whose id the wire form derives \
              anyway; and `NeuronExport::id` is `i64`, so -1.5 is a serde error either way \
              (module docs, rule 5)",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "hidden-bias-undefined-shadowed",
        why: "`NeuronExport::bias` is a required number; `undefined` cannot be written",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "memetic-weights-not-an-array",
        why: "memetic `weights` values are typed arrays; a bare object is a serde error \
              (module docs, rule 31)",
        outcome: RustOutcome::Unrepresentable,
    },
    // -- Input neurons are implicit, so their identity cannot be misstated. --
    Declared {
        case: "input-neuron-id-not-index",
        why: "implicit input neurons take `id == index` by construction, so rule 7 \
              cannot fail on the wire",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "input-neuron-past-input-count",
        why: "the wire form lists no input neurons, so none can sit past the declared \
              width (rule 10)",
        outcome: RustOutcome::Unrepresentable,
    },
    Declared {
        case: "stats-input-count-mismatch",
        why: "the wire form carries exactly `input` implicit inputs, so the count cannot \
              disagree with itself (rule 21) — the second input is simply implied",
        outcome: RustOutcome::Accepted([2, 0, 1, 1, 2]),
    },
    // -- Ids are derived by the loader before the rules ever see them. -------
    Declared {
        case: "neuron-missing-id",
        why: "an output's id is derived as -(outputIndex + 1) whatever the file says, so \
              rule 4 cannot be reached through an output neuron",
        outcome: RustOutcome::Unrepresentable,
    },
    // -- Host-only. ---------------------------------------------------------
    Declared {
        case: "neuron-index-mismatch",
        why: "`neuron.index` is one of the checks that stay host-side (NEAT-AI#3802); the \
              wire form carries no index, so the key is ignored and the creature is valid",
        outcome: RustOutcome::Accepted([1, 0, 0, 1, 1]),
    },
];

fn declared(case: &str) -> Option<&'static Declared> {
    DIVERGENCES.iter().find(|entry| entry.case == case)
}

// ---------------------------------------------------------------------------
// The replay.
// ---------------------------------------------------------------------------

fn assert_matches_corpus(
    case: &Case,
    result: &Result<ValidationStats, neat_core::ValidationFailure>,
) {
    let name = &case.name;
    match case.expect.outcome.as_str() {
        "ok" => {
            let stats = result
                .as_ref()
                .unwrap_or_else(|failure| panic!("{name}: expected `ok`, got {failure}"));
            let expected = case
                .expect
                .stats
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: an `ok` case must pin its stats"));
            assert_eq!(
                (
                    stats.input,
                    stats.constant,
                    stats.hidden,
                    stats.output,
                    stats.connections
                ),
                (
                    expected.input,
                    expected.constant,
                    expected.hidden,
                    expected.output,
                    expected.connections
                ),
                "{name}: stats disagree with the TypeScript"
            );
        }
        "throws" => {
            let failure = match result {
                Err(failure) => failure,
                Ok(stats) => panic!("{name}: expected a failure, got {stats:?}"),
            };
            let error = case.expect.error.as_deref().unwrap_or_default();
            let reason = case.expect.reason.as_deref().unwrap_or_default();
            let message = case.expect.message_contains.as_deref().unwrap_or_default();

            assert_eq!(
                failure.class.as_str(),
                error,
                "{name}: wrong error class ({failure})"
            );
            assert_eq!(failure.reason, reason, "{name}: wrong reason ({failure})");
            assert!(
                failure.message.contains(message),
                "{name}: message {:?} does not contain {message:?}",
                failure.message
            );
        }
        other => panic!("{name}: unknown outcome {other:?}"),
    }
}

/// Every corpus case either reproduces the TypeScript, or is a declared
/// divergence whose Rust behaviour is pinned here instead.
#[test]
fn the_corpus_replays_against_creature_validate() {
    let cases = load_corpus();
    assert!(
        cases.len() >= 47,
        "the vendored corpus shrank to {} cases",
        cases.len()
    );

    let mut seen_divergences: BTreeSet<&str> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();

    for case in &cases {
        assert!(
            names.insert(case.name.clone()),
            "duplicate case name {}",
            case.name
        );

        let options = case
            .options
            .as_ref()
            .map(ValidateOptions::from)
            .unwrap_or_default();

        match to_export(&case.creature) {
            Err(why) => {
                let entry = declared(&case.name).unwrap_or_else(|| {
                    panic!(
                        "{}: the wire form cannot carry this case ({why}), and it is not \
                         declared in DIVERGENCES",
                        case.name
                    )
                });
                assert_eq!(
                    entry.outcome,
                    RustOutcome::Unrepresentable,
                    "{}: declared as reachable, but the wire form refused it ({why})",
                    case.name
                );
                seen_divergences.insert(entry.case);
            }
            Ok(creature) => {
                let result = creature_validate(&creature, &options);
                match declared(&case.name) {
                    None => assert_matches_corpus(case, &result),
                    Some(entry) => {
                        seen_divergences.insert(entry.case);
                        match entry.outcome {
                            RustOutcome::Unrepresentable => panic!(
                                "{}: declared unrepresentable ({}), but it converted and \
                                 returned {result:?} — the declaration is stale",
                                case.name, entry.why
                            ),
                            RustOutcome::Accepted(expected) => {
                                let stats = result.unwrap_or_else(|failure| {
                                    panic!(
                                        "{}: declared accepted ({}), got {failure}",
                                        case.name, entry.why
                                    )
                                });
                                assert_eq!(
                                    [
                                        stats.input,
                                        stats.constant,
                                        stats.hidden,
                                        stats.output,
                                        stats.connections
                                    ],
                                    expected,
                                    "{}: the declared Rust behaviour changed ({})",
                                    case.name,
                                    entry.why
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let declared_names: BTreeSet<&str> = DIVERGENCES.iter().map(|entry| entry.case).collect();
    assert_eq!(
        seen_divergences, declared_names,
        "a declared divergence names no corpus case (stale entry)"
    );
}

/// The corpus is vendored whole: every case names a site the manifest declares,
/// and every site the manifest calls covered still has a case.
#[test]
fn every_coverage_site_still_has_a_case() {
    let cases = load_corpus();
    let coverage = load_coverage();

    let sites: BTreeMap<&str, &str> = coverage
        .sites
        .iter()
        .map(|site| (site.id.as_str(), site.status.as_str()))
        .collect();

    let mut covered: BTreeSet<&str> = BTreeSet::new();
    for case in &cases {
        assert!(
            sites.contains_key(case.rule.as_str()),
            "{}: names rule {:?}, which coverage.json does not declare",
            case.name,
            case.rule
        );
        covered.insert(sites.get_key_value(case.rule.as_str()).expect("declared").0);
    }

    let missing: Vec<&str> = sites
        .iter()
        .filter(|(id, status)| **status != "not-expressible" && !covered.contains(*id))
        .map(|(id, _)| *id)
        .collect();
    assert!(
        missing.is_empty(),
        "the vendored corpus lost the cases for {missing:?}"
    );
}

/// Two thirds of the corpus is replayed verbatim; the rest is declared. This
/// pins the split so a future change cannot quietly move a case into the
/// divergence table.
#[test]
fn the_declared_divergences_are_the_only_ones() {
    let cases = load_corpus();
    let divergent = cases
        .iter()
        .filter(|case| declared(&case.name).is_some())
        .count();

    assert_eq!(
        divergent,
        DIVERGENCES.len(),
        "every declared divergence must name a case that is actually in the corpus"
    );
    assert!(
        cases.len() - divergent >= 37,
        "only {} of {} cases replay against the Rust port",
        cases.len() - divergent,
        cases.len()
    );
}
