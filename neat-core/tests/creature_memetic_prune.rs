//! Pruning a memetic record back to the structure a creature still has —
//! rule 31's inverse (NEAT-AI-Lamarck#197).
//!
//! Rule 31 (`MEMETIC`) refuses a creature whose memetic biases or weights name
//! a neuron or a synapse it no longer carries, so **every** consumer that
//! removes structure has to prune the record with it. Without a shared prune,
//! each downstream repo hand-rolls one — and the blunt version (`memetic =
//! None`) throws away `MemeticExport::extra`, the `generation` / `score` /
//! `ancestry` history the record exists to carry. This crate owns rule 31, so
//! it owns its inverse.
//!
//! Every assertion here is on the record the prune hands back: which keys
//! survived, which went, and that the extras are untouched. The
//! `creature_validate` assertions are the *consequence*, never the only oracle.

use neat_core::{
    CreatureExport, MemeticExport, MemeticWeightExport, MemeticWeightRowExport, MemeticWeights,
    NeuronExport, SynapseExport, ValidateOptions, creature_validate, parse_creature_json,
};
use std::collections::BTreeMap;

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// 2 inputs, `input-0 -> h1`, `input-1 -> h1`, `h1 -> o1`.
fn creature() -> CreatureExport {
    parse_creature_json(
        r#"{
          "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
          "neurons":[
            {"id":7,"type":"hidden","uuid":"h1","bias":0.25,"squash":"IDENTITY"},
            {"type":"output","uuid":"o1","bias":0.0,"squash":"IDENTITY"}
          ],
          "synapses":[
            {"fromUUID":"input-0","toUUID":"h1","weight":1.0},
            {"fromUUID":"input-1","toUUID":"h1","weight":0.5},
            {"fromUUID":"h1","toUUID":"o1","weight":1.0}
          ]
        }"#,
    )
    .unwrap()
}

fn row(from: &str, to: &str, weight: f64) -> MemeticWeightRowExport {
    MemeticWeightRowExport {
        from_uuid: Some(from.to_string()),
        to_uuid: Some(to.to_string()),
        weight: Some(weight),
    }
}

fn rows_of(memetic: &MemeticExport) -> Vec<(String, String)> {
    match &memetic.weights {
        MemeticWeights::Rows(rows) => rows
            .iter()
            .map(|r| {
                (
                    r.from_uuid.clone().unwrap_or_default(),
                    r.to_uuid.clone().unwrap_or_default(),
                )
            })
            .collect(),
        MemeticWeights::ById(_) => panic!("fixture is the row form"),
    }
}

fn by_id_of(memetic: &MemeticExport) -> Vec<(String, Vec<i64>)> {
    match &memetic.weights {
        MemeticWeights::ById(by_id) => by_id
            .iter()
            .map(|(key, entries)| {
                (
                    key.clone(),
                    entries.iter().filter_map(|e| e.to_id).collect::<Vec<_>>(),
                )
            })
            .collect(),
        MemeticWeights::Rows(_) => panic!("fixture is the map form"),
    }
}

/// The row form: a weight naming a removed edge goes, the rest stays, and the
/// extras the record carries are untouched.
#[test]
fn prune_drops_the_row_naming_a_removed_edge() {
    let mut host = creature();
    let mut extra = serde_json::Map::new();
    extra.insert("generation".into(), serde_json::json!(11));
    extra.insert("score".into(), serde_json::json!(0.42));
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01), ("gone".to_string(), 0.02)]),
        weights: MemeticWeights::Rows(vec![
            row("input-0", "h1", 0.9),
            row("input-1", "h1", 0.8),
            row("h1", "o1", 1.1),
        ]),
        extra,
    });
    // Remove `input-0 -> h1`, exactly as a neuron split does.
    host.synapses
        .retain(|s| !(s.from_uuid == "input-0" && s.to_uuid == "h1"));

    host.prune_memetic();

    let memetic = host.memetic.as_ref().expect("the record itself survives");
    assert_eq!(
        rows_of(memetic),
        vec![
            ("input-1".to_string(), "h1".to_string()),
            ("h1".to_string(), "o1".to_string()),
        ],
        "only the row naming the removed edge is dropped"
    );
    assert_eq!(
        memetic.biases.keys().collect::<Vec<_>>(),
        vec!["h1"],
        "`gone` names no neuron; `h1` does"
    );
    assert_eq!(memetic.extra["generation"], serde_json::json!(11));
    assert_eq!(memetic.extra["score"], serde_json::json!(0.42));
    creature_validate(&host, &OPTIONS).expect("a pruned creature satisfies rule 31");
}

/// The id-keyed map form: an entry naming a removed edge goes, a key naming no
/// neuron goes with its entries, and a still-live entry stays.
#[test]
fn prune_drops_dangling_map_entries_and_unresolvable_keys() {
    let mut host = creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::new(),
        weights: MemeticWeights::ById(BTreeMap::from([
            (
                "7".to_string(),
                vec![MemeticWeightExport {
                    to_id: Some(-1),
                    weight: Some(1.1),
                }],
            ),
            (
                "404".to_string(),
                vec![MemeticWeightExport {
                    to_id: Some(-1),
                    weight: Some(0.3),
                }],
            ),
        ])),
        extra: serde_json::Map::new(),
    });

    // Nothing removed yet: `7 -> -1` is `h1 -> o1`, so it survives; key `404`
    // names no neuron at all.
    host.prune_memetic();
    assert_eq!(
        by_id_of(host.memetic.as_ref().unwrap()),
        vec![("7".to_string(), vec![-1])]
    );
    creature_validate(&host, &OPTIONS).expect("a pruned creature satisfies rule 31");

    // Now remove `h1 -> o1`: the last entry goes, the key stays as an empty
    // list, and the record is still there.
    host.synapses.retain(|s| s.to_uuid != "o1");
    host.prune_memetic();
    assert_eq!(
        by_id_of(host.memetic.as_ref().unwrap()),
        vec![("7".to_string(), vec![])]
    );
}

/// A neuron removal takes its bias delta, and every weight naming it, with it.
#[test]
fn prune_follows_a_removed_neuron() {
    let mut host = creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01), ("o1".to_string(), 0.03)]),
        weights: MemeticWeights::Rows(vec![row("input-0", "h1", 0.9), row("h1", "o1", 1.1)]),
        extra: serde_json::Map::new(),
    });
    // Drop `h1` and every edge touching it — a neuron removal.
    host.neurons.retain(|n| n.uuid != "h1");
    host.synapses
        .retain(|s| s.from_uuid != "h1" && s.to_uuid != "h1");
    host.synapses.push(SynapseExport {
        from_uuid: "input-0".into(),
        to_uuid: "o1".into(),
        weight: 1.0,
        synapse_type: None,
    });

    host.prune_memetic();

    let memetic = host.memetic.as_ref().unwrap();
    assert_eq!(memetic.biases.keys().collect::<Vec<_>>(), vec!["o1"]);
    assert!(
        rows_of(memetic).is_empty(),
        "both rows named the removed neuron: {:?}",
        rows_of(memetic)
    );
    creature_validate(&host, &OPTIONS).expect("a pruned creature satisfies rule 31");
}

/// Appending structure resolves every existing key, so a prune must be a no-op
/// there — pruning would throw away valid fine-tuning history.
#[test]
fn prune_is_a_no_op_when_nothing_was_removed() {
    let mut host = creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01)]),
        weights: MemeticWeights::Rows(vec![row("input-0", "h1", 0.9)]),
        extra: serde_json::Map::from_iter([("ancestry".to_string(), serde_json::json!(["a"]))]),
    });
    let before = host.memetic.clone();

    // A pure append: a second edge into the output.
    host.neurons.push(NeuronExport {
        id: None,
        neuron_type: "hidden".into(),
        uuid: "h2".into(),
        bias: 0.0,
        squash: Some("IDENTITY".into()),
    });
    host.prune_memetic();

    assert_eq!(host.memetic, before, "an append prunes nothing");
}

/// A creature with no memetic record is left exactly as it was, and an
/// emptied-out record is kept rather than cleared: `memetic = None` is a
/// different fact from "fine-tuned, then rewired".
#[test]
fn prune_keeps_the_record_but_not_a_missing_one() {
    let mut none = creature();
    none.prune_memetic();
    assert!(none.memetic.is_none());

    let mut host = creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01)]),
        weights: MemeticWeights::Rows(vec![row("input-0", "h1", 0.9)]),
        extra: serde_json::Map::from_iter([("generation".to_string(), serde_json::json!(4))]),
    });
    host.neurons.clear();
    host.synapses.clear();
    host.prune_memetic();

    let memetic = host.memetic.as_ref().expect("the record survives");
    assert!(memetic.biases.is_empty());
    assert!(rows_of(memetic).is_empty());
    assert_eq!(
        memetic.extra["generation"],
        serde_json::json!(4),
        "the history the record exists to carry is never the thing pruned"
    );
}

/// Pruning twice changes nothing the first pass left — a second removal in the
/// same pipeline must not compound.
#[test]
fn prune_is_idempotent() {
    let mut once = creature();
    once.memetic = Some(MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01), ("gone".to_string(), 0.02)]),
        weights: MemeticWeights::Rows(vec![row("input-0", "h1", 0.9), row("gone", "o1", 0.5)]),
        extra: serde_json::Map::new(),
    });
    once.prune_memetic();
    let mut twice = once.clone();
    twice.prune_memetic();
    assert_eq!(once, twice);
}

/// `MemeticExport::prune_to` is the same prune against a creature the caller
/// holds separately — the record need not be attached to be pruned.
#[test]
fn prune_to_prunes_a_detached_record() {
    let mut host = creature();
    host.synapses
        .retain(|s| !(s.from_uuid == "input-0" && s.to_uuid == "h1"));
    let mut memetic = MemeticExport {
        biases: BTreeMap::from([("h1".to_string(), 0.01)]),
        weights: MemeticWeights::Rows(vec![row("input-0", "h1", 0.9), row("h1", "o1", 1.1)]),
        extra: serde_json::Map::new(),
    };

    memetic.prune_to(&host);

    assert_eq!(
        rows_of(&memetic),
        vec![("h1".to_string(), "o1".to_string())]
    );
    assert!(
        host.memetic.is_none(),
        "prune_to does not attach the record"
    );
}

/// A declared observation width no payload backs — the implicit input neurons
/// are not listed, so `"input": 1000000` is a hundred bytes of JSON.
const WIDE_INPUT: usize = 1_000_000;

/// `input-999999 -> o1`, declaring a million observations (Issue #650).
fn wide_creature() -> CreatureExport {
    CreatureExport {
        memetic: None,
        input: WIDE_INPUT,
        output: 1,
        neurons: vec![NeuronExport {
            id: None,
            neuron_type: "output".into(),
            uuid: "o1".into(),
            bias: 0.0,
            squash: Some("IDENTITY".into()),
        }],
        synapses: vec![SynapseExport {
            from_uuid: format!("input-{}", WIDE_INPUT - 1),
            to_uuid: "o1".into(),
            weight: 1.0,
            synapse_type: None,
        }],
        semantic_version: None,
        forward_only: false,
    }
}

/// The implicit input neurons are derived arithmetically rather than
/// materialised one view each (Issue #650), so the prune must still answer
/// exactly as it did: a reference to an input *inside* the declared width
/// resolves — by either vocabulary — and one past it does not.
#[test]
fn prune_resolves_implicit_inputs_across_a_wide_declared_width() {
    let mut host = wide_creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::from([
            (format!("input-{}", WIDE_INPUT - 1), 0.01),
            ((WIDE_INPUT - 1).to_string(), 0.02),
            (format!("input-{WIDE_INPUT}"), 0.03),
            (WIDE_INPUT.to_string(), 0.04),
        ]),
        weights: MemeticWeights::Rows(vec![
            row(&format!("input-{}", WIDE_INPUT - 1), "o1", 0.9),
            row("input-0", "o1", 0.8),
            row(&format!("input-{WIDE_INPUT}"), "o1", 0.7),
        ]),
        extra: serde_json::Map::new(),
    });

    host.prune_memetic();

    let memetic = host.memetic.as_ref().expect("the record itself survives");
    assert_eq!(
        memetic.biases.keys().collect::<Vec<_>>(),
        vec!["999999", "input-999999"],
        "both vocabularies resolve the last declared input; neither resolves one past it"
    );
    assert_eq!(
        rows_of(memetic),
        vec![("input-999999".to_string(), "o1".to_string())],
        "input-0 exists but carries no synapse to o1, and input-1000000 does not exist"
    );
}

/// The same arithmetic, through the id-keyed map form: the key and the `toId`
/// both resolve against a width no payload backs.
#[test]
fn prune_resolves_implicit_input_ids_in_the_map_form() {
    let mut host = wide_creature();
    host.memetic = Some(MemeticExport {
        biases: BTreeMap::new(),
        weights: MemeticWeights::ById(BTreeMap::from([
            (
                (WIDE_INPUT - 1).to_string(),
                vec![
                    // `o1` is the first output, so its runtime id is -1.
                    MemeticWeightExport {
                        to_id: Some(-1),
                        weight: Some(0.9),
                    },
                    // Input 0 exists, but `input-999999 -> input-0` does not.
                    MemeticWeightExport {
                        to_id: Some(0),
                        weight: Some(0.8),
                    },
                ],
            ),
            // A key one past the declared width names no neuron at all.
            (
                WIDE_INPUT.to_string(),
                vec![MemeticWeightExport {
                    to_id: Some(-1),
                    weight: Some(0.7),
                }],
            ),
        ])),
        extra: serde_json::Map::new(),
    });

    host.prune_memetic();

    let memetic = host.memetic.as_ref().expect("the record itself survives");
    assert_eq!(
        by_id_of(memetic),
        vec![("999999".to_string(), vec![-1])],
        "the live edge survives; the absent pair and the out-of-range key go"
    );
}
