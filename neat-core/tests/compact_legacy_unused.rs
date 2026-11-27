//! Frozen replay of the legacy `compactUnused` neuron choice and removal (Issue #778).
//!
//! `golden/compact_legacy_unused.json` holds fourteen captures of the frozen TypeScript
//! `compactUnused` at NEAT-AI 60d6b4e5, written by `scripts/legacy_compaction_unused.ts`
//! (see `docs/research/compaction-unused-choice-baseline.md`). Each capture is replayed
//! through the two halves of the port: `choose_unused_neuron` must pick the neuron the
//! capture picked, and `remove_neuron` must reproduce the captured creature bit for bit
//! while drawing exactly the captured reconnect draws and minting the constant the
//! capture keeps. A second call minted one constant more in TypeScript, a temporary
//! that it discarded; the port mints the survivor only.
//!
//! The replay builds its input from `aligned`, the creature the TypeScript iterates,
//! and reads `traced` for the input and output counts and for the caller's
//! `compact-unity-slot` tags.

use std::cell::Cell;

use neat_core::{
    CreatureExport, NeuronExport, NeuronTraceStats, SynapseExport, UnitySlot,
    UnusedNeuronCandidate, UnusedNeuronChoice, choose_unused_neuron, remove_neuron,
};
use serde_json::Value;

/// The NEAT-AI revision every capture was taken at.
const REVISION: &str = "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab";

/// The fourteen captured cases, in capture order. A dropped or renamed case fails the count.
const CASES: [&str; 14] = [
    "newborn_no_grace",
    "newborn_grace_1",
    "newborn_grace_2",
    "newborn_grace_0_spent",
    "newborn_nothing_to_compact",
    "unused_clipped",
    "cascade_cosine_clipped",
    "orphan_cleanup_feeder",
    "orphan_cleanup_cascade",
    "fix_if_compact",
    "compact_constant_first",
    "compact_constant_second",
    "compact_keep_order_first",
    "compact_keep_order_second",
];

fn golden() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy_unused.json"
    ))
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn golden_record(name: &str) -> Value {
    golden()
        .into_iter()
        .find(|record| record["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no golden record named {name}"))
}

fn usize_list(value: &Value) -> Vec<usize> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| usize::try_from(v.as_u64().unwrap()).unwrap())
        .collect()
}

fn f64_list(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

/// Runtime index as TypeScript numbers it: inputs first, then the listed neurons.
fn runtime_index(input: usize, uuids: &[&str], uuid: &str) -> usize {
    match uuid.strip_prefix("input-") {
        Some(index) => index.parse().unwrap(),
        None => input + uuids.iter().position(|listed| *listed == uuid).unwrap(),
    }
}

/// The capture does not record `forwardOnly`. A creature is forward-only when every
/// synapse runs from a lower to a higher runtime index, so the flag is derived from
/// the captured wiring.
fn wiring_is_forward(record: &Value) -> bool {
    let input = usize::try_from(record["traced"]["input"].as_u64().unwrap()).unwrap();
    let uuids: Vec<&str> = record["aligned"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    record["aligned"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| {
            runtime_index(input, &uuids, s["fromUUID"].as_str().unwrap())
                < runtime_index(input, &uuids, s["toUUID"].as_str().unwrap())
        })
}

/// The creature the TypeScript iterates, from `aligned`, in its captured order. That
/// order is already sorted by runtime `(from, to)`, which `remove_neuron` expects.
fn creature_from_aligned(record: &Value) -> CreatureExport {
    let aligned = &record["aligned"];
    let neurons = aligned["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| NeuronExport {
            id: None,
            neuron_type: n["type"].as_str().unwrap().to_string(),
            uuid: n["uuid"].as_str().unwrap().to_string(),
            bias: n["bias"].as_f64().unwrap(),
            squash: n["squash"].as_str().map(str::to_string),
        })
        .collect();
    let synapses = aligned["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| SynapseExport {
            from_uuid: s["fromUUID"].as_str().unwrap().to_string(),
            to_uuid: s["toUUID"].as_str().unwrap().to_string(),
            weight: s["weight"].as_f64().unwrap(),
            synapse_type: s["type"].as_str().map(str::to_string),
        })
        .collect();
    CreatureExport {
        input: usize::try_from(record["traced"]["input"].as_u64().unwrap()).unwrap(),
        output: usize::try_from(record["traced"]["output"].as_u64().unwrap()).unwrap(),
        neurons,
        synapses,
        semantic_version: None,
        forward_only: wiring_is_forward(record),
        memetic: None,
    }
}

/// One candidate per aligned neuron: its trace (`None` when absent) and its grace.
fn candidates_from_aligned(record: &Value) -> Vec<UnusedNeuronCandidate> {
    let grace = record["graceRemaining"].as_array().unwrap();
    let neurons = record["aligned"]["neurons"].as_array().unwrap();
    assert_eq!(
        grace.len(),
        neurons.len(),
        "one grace entry per aligned neuron"
    );
    neurons
        .iter()
        .zip(grace)
        .map(|(neuron, grace)| {
            let trace = neuron
                .get("trace")
                .filter(|t| !t.is_null())
                .map(|t| NeuronTraceStats {
                    count: t["count"].as_u64().unwrap(),
                    maximum_activation: t["maximumActivation"].as_f64().unwrap(),
                    minimum_activation: t["minimumActivation"].as_f64().unwrap(),
                    total_activation: t["totalActivation"].as_f64(),
                });
            UnusedNeuronCandidate {
                trace,
                newborn_grace_remaining: u32::try_from(grace.as_u64().unwrap()).unwrap(),
            }
        })
        .collect()
}

/// `choose_unused_neuron` on a captured creature with the given visit order.
fn choose(record: &Value, shuffled: &[usize]) -> Option<UnusedNeuronChoice> {
    let name = record["name"].as_str().unwrap();
    let creature = creature_from_aligned(record);
    let candidates = candidates_from_aligned(record);
    let plank = record["plankConstant"].as_f64().unwrap();
    choose_unused_neuron(&creature, &candidates, shuffled, plank)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The slot a neuron's `compact-unity-slot` tag holds, when it carries one.
fn unity_slot_tag(neuron: &Value) -> Option<u8> {
    neuron["tags"]
        .as_array()?
        .iter()
        .find(|tag| tag["name"].as_str() == Some("compact-unity-slot"))
        .map(|tag| tag["value"].as_str().unwrap().parse().unwrap())
}

/// The `compact-unity-slot` tags on the captured input, which the caller passes to
/// `remove_neuron`. `aligned` carries no tags, so they are read from `traced`.
fn unity_slots_from_input(record: &Value) -> Vec<UnitySlot> {
    record["traced"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|neuron| {
            unity_slot_tag(neuron).map(|slot| UnitySlot {
                uuid: neuron["uuid"].as_str().unwrap().to_string(),
                slot,
            })
        })
        .collect()
}

/// The unity constant `remove_neuron` must name: the neuron the TypeScript minted
/// and tagged in `result.after`. A removal that minted nothing names nothing.
fn expected_unity_constant(record: &Value) -> Option<UnitySlot> {
    if record["minted"].as_u64().unwrap() == 0 {
        return None;
    }
    let input: Vec<&str> = record["traced"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|neuron| neuron["uuid"].as_str().unwrap())
        .collect();
    record["result"]["after"]["neurons"]
        .as_array()?
        .iter()
        .filter(|neuron| !input.contains(&neuron["uuid"].as_str().unwrap()))
        .find_map(|neuron| {
            unity_slot_tag(neuron).map(|slot| UnitySlot {
                uuid: neuron["uuid"].as_str().unwrap().to_string(),
                slot,
            })
        })
}

/// Compares the rewritten creature with the captured `result.after`. Neuron and
/// synapse order are both checked, and weights and biases bit for bit. Tags are not
/// compared: `CreatureExport` carries none, and the caller writes the `unused` tag.
fn assert_same_creature(name: &str, actual: &CreatureExport, expected: &Value) {
    let expected_neurons = expected["neurons"].as_array().unwrap();
    assert_eq!(
        actual.neurons.len(),
        expected_neurons.len(),
        "{name}: neuron count"
    );
    for (actual, expected) in actual.neurons.iter().zip(expected_neurons) {
        assert_eq!(
            actual.neuron_type,
            expected["type"].as_str().unwrap(),
            "{name}: type of {}",
            actual.uuid
        );
        assert_eq!(
            actual.uuid,
            expected["uuid"].as_str().unwrap(),
            "{name}: uuid"
        );
        assert_eq!(
            actual.squash.as_deref(),
            expected["squash"].as_str(),
            "{name}: squash of {}",
            actual.uuid
        );
        assert_eq!(
            actual.bias.to_bits(),
            expected["bias"].as_f64().unwrap().to_bits(),
            "{name}: bias of {}",
            actual.uuid
        );
    }

    let expected_synapses = expected["synapses"].as_array().unwrap();
    assert_eq!(
        actual.synapses.len(),
        expected_synapses.len(),
        "{name}: synapse count"
    );
    for (actual, expected) in actual.synapses.iter().zip(expected_synapses) {
        assert_eq!(
            actual.from_uuid,
            expected["fromUUID"].as_str().unwrap(),
            "{name}: source"
        );
        assert_eq!(
            actual.to_uuid,
            expected["toUUID"].as_str().unwrap(),
            "{name}: target"
        );
        assert_eq!(
            actual.weight.to_bits(),
            expected["weight"].as_f64().unwrap().to_bits(),
            "{name}: weight {} -> {}",
            actual.from_uuid,
            actual.to_uuid
        );
        assert_eq!(
            actual.synapse_type.as_deref(),
            expected["type"].as_str(),
            "{name}: role {} -> {}",
            actual.from_uuid,
            actual.to_uuid
        );
    }
}

#[test]
fn frozen_compact_unused_captures_replay_exactly() {
    let records = golden();
    let names: Vec<&str> = records
        .iter()
        .map(|record| record["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names, CASES,
        "the golden holds exactly the fourteen frozen cases"
    );

    for record in &records {
        let name = record["name"].as_str().unwrap();
        assert_eq!(
            record["revision"].as_str(),
            Some(REVISION),
            "{name}: captured revision"
        );
        let creature = creature_from_aligned(record);
        let shuffled = usize_list(&record["shuffled"]);
        let choice = choose(record, &shuffled);

        if record["result"].is_null() {
            // TypeScript returned undefined here (`src/compact/CompactUnused.ts`).
            // Both hidden neurons carry newborn grace (`graceRemaining` 2 and 2), so the
            // loop skips each at line 81. `output-0` is not hidden, so line 75 skips it.
            // `neuronForRemoval` stays undefined, the removal block never runs, and the
            // final `else` at lines 167-168 returns undefined. No candidate is left, so
            // the port chooses nothing. There is no choice to remove, so the "creature
            // unchanged" alternative does not apply.
            assert!(
                record["chosenUUID"].is_null(),
                "{name}: capture chose nothing"
            );
            assert_eq!(
                choice, None,
                "{name}: no candidate is left, so nothing is chosen"
            );
            continue;
        }

        let choice = choice.unwrap_or_else(|| panic!("{name}: the capture chose a neuron"));
        let expected_tag = record["result"]["unusedTag"].as_str().unwrap();
        assert_eq!(choice.uuid, expected_tag, "{name}: chosen neuron");

        let unity_slots = unity_slots_from_input(record);
        let draws = f64_list(&record["removalDraws"]);
        let draws_used = Cell::new(0_usize);
        let mints = Cell::new(0_u64);
        // The uuid the port mints is the one the capture kept. On a second call
        // TypeScript minted a temporary constant first and then discarded it, so
        // the survivor is not `unity-constant-0`. It is the captured after-neuron
        // that carries a slot tag. When no minted constant survives, nothing in the
        // capture names one, and any fresh name will do.
        let survivor = expected_unity_constant(record)
            .map_or_else(|| "unity-constant-0".to_string(), |constant| constant.uuid);
        let removal = remove_neuron(
            &creature,
            &choice.uuid,
            choice.activation,
            &unity_slots,
            || {
                mints.set(mints.get() + 1);
                survivor.clone()
            },
            || {
                let i = draws_used.get();
                let value = *draws.get(i).unwrap_or_else(|| {
                    panic!(
                        "{name}: removal drew more than the {} captured draws",
                        draws.len()
                    )
                });
                draws_used.set(i + 1);
                value
            },
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .unwrap_or_else(|| panic!("{name}: removal refused the chosen neuron"));

        assert_eq!(
            draws_used.get(),
            draws.len(),
            "{name}: reconnect draws used"
        );
        // A removal that takes the constant branch mints exactly one constant. On a
        // second call TypeScript minted two, a temporary constant and the survivor;
        // the port mints the survivor alone.
        let captured_mints = record["minted"].as_u64().unwrap();
        assert_eq!(
            mints.get(),
            u64::from(captured_mints > 0),
            "{name}: constants minted"
        );
        if captured_mints == 2 {
            assert_eq!(
                mints.get(),
                1,
                "{name}: the port mints once where TypeScript minted twice"
            );
            assert_eq!(
                removal.unity_constant.as_ref().map(|constant| constant.slot),
                Some(1),
                "{name}: the survivor holds slot 1"
            );
        }
        assert_eq!(
            removal.unity_constant,
            expected_unity_constant(record),
            "{name}: the unity constant named by its after-neuron's slot tag"
        );
        assert_same_creature(name, &removal.creature, &record["result"]["after"]);
    }
}

#[test]
fn the_frozen_choice_depends_on_the_callers_visit_order() {
    // cascade_cosine_clipped has two eligible hidden neurons, and both effects are
    // negative. The loop stops at the first negative effect it visits, so the visit
    // order decides the choice:
    //   hidden-4 = |1 - 1| * min(0.7, 1) - 1e-7 * 1 = -1e-7 (in-degree 1, from hidden-3)
    //   hidden-3 = |-0.54 - (-0.54)| * min(0.5, 1) - 1e-7 * 2 = -2e-7 (in-degree 2)
    // The loop walks the visit order from its last entry, so the captured order
    // [0, 3, 2, 1] reaches hidden-4 first, and the reversed order reaches hidden-3 first.
    let record = golden_record("cascade_cosine_clipped");
    let captured = usize_list(&record["shuffled"]);
    assert_eq!(captured, [0, 3, 2, 1], "the captured visit order");

    let mut reversed = captured.clone();
    reversed.reverse();

    let chosen = |order: &[usize]| choose(&record, order).expect("a neuron is chosen").uuid;
    assert_eq!(
        chosen(&captured),
        "hidden-4",
        "the capture's order picks hidden-4"
    );
    assert_eq!(
        chosen(&reversed),
        "hidden-3",
        "the reversed order picks hidden-3"
    );
}
