//! The core-side contract a caller's compaction deadline relies on (Issue #778).
//!
//! No compaction rule in NEAT-AI `src/compact` carries a deadline. The only
//! deadline is host orchestration in
//! `src/architecture/training/TrainingTeardown.ts::finaliseTraining` at NEAT-AI
//! revision 60d6b4e5: its `skipCompaction` callback is asked once before
//! `compactUnused` and once more before the variant search. When it answers
//! `true`, the teardown returns without a compacted copy. Either way the
//! teardown ages the newborn grace of the trained creature exactly once
//! (`ageNewbornGrace(creature)`, `ageNewbornGrace(bestTraceJSON)`), and ages a
//! compacted copy only when `compactUnused` has not already aged it
//! (`if (!compactWasAged && compact)`). So a deadline decides whether a
//! compacted copy exists, never how much grace is spent. That deadline stays
//! with the caller; it is not ported here.
//!
//! What the caller's deadline needs from `neat-core` is pinned here instead:
//!
//! - a stage re-run after an abandoned attempt gives a bit-identical result, so
//!   there is no hidden state for the abandoned attempt to leave behind;
//! - the one grace round the caller spends through `age_newborn_grace` is the
//!   round a compacting `compactUnused` pass writes into its copy, so firing the
//!   deadline never changes a surviving neuron's grace budget.
//!
//! The frozen captures are `golden/compact_legacy_unused.json`, the same fourteen
//! `compactUnused` runs `compact_legacy_unused.rs` replays. The loading helpers
//! are copied from there, as integration test files cannot share them.

use std::collections::HashMap;

use neat_core::{
    AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, CreatureExport, NeuronExport, NeuronTraceStats,
    NewbornGraceAgeing, SynapseExport, UnitySlot, UnusedNeuronCandidate, UnusedNeuronChoice,
    age_newborn_grace, aggressive_prune, choose_unused_neuron, remove_neuron,
};
use serde_json::Value;

/// How many captures the golden holds. A dropped or added case fails the count.
const CAPTURES: usize = 14;

fn golden() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy_unused.json"
    ))
    .unwrap();
    let records: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(
        records.len(),
        CAPTURES,
        "the golden holds fourteen captures"
    );
    records
}

fn golden_record(name: &str) -> Value {
    golden()
        .into_iter()
        .find(|record| record["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no golden record named {name}"))
}

fn name_of(record: &Value) -> &str {
    record["name"].as_str().unwrap()
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

/// The capture does not record `forwardOnly`; it is derived from the wiring.
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

/// The creature the TypeScript iterates, from `aligned`, in its captured order.
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

/// The trace of one aligned neuron, `None` when absent.
fn trace_of(neuron: &Value) -> Option<NeuronTraceStats> {
    neuron
        .get("trace")
        .filter(|t| !t.is_null())
        .map(|t| NeuronTraceStats {
            count: t["count"].as_u64().unwrap(),
            maximum_activation: t["maximumActivation"].as_f64().unwrap(),
            minimum_activation: t["minimumActivation"].as_f64().unwrap(),
            total_activation: t["totalActivation"].as_f64(),
        })
}

/// One candidate per aligned neuron, with the grace the capture recorded.
fn candidates_from_aligned(record: &Value) -> Vec<UnusedNeuronCandidate> {
    let grace = record["graceRemaining"].as_array().unwrap();
    let neurons = record["aligned"]["neurons"].as_array().unwrap();
    assert_eq!(grace.len(), neurons.len(), "one grace entry per neuron");
    neurons
        .iter()
        .zip(grace)
        .map(|(neuron, grace)| UnusedNeuronCandidate {
            trace: trace_of(neuron),
            newborn_grace_remaining: u32::try_from(grace.as_u64().unwrap()).unwrap(),
        })
        .collect()
}

fn choose_with(
    record: &Value,
    creature: &CreatureExport,
    candidates: &[UnusedNeuronCandidate],
) -> Option<UnusedNeuronChoice> {
    let shuffled = usize_list(&record["shuffled"]);
    let plank = record["plankConstant"].as_f64().unwrap();
    choose_unused_neuron(creature, candidates, &shuffled, plank)
        .unwrap_or_else(|error| panic!("{}: {error}", name_of(record)))
}

fn unity_slot_tag(neuron: &Value) -> Option<u8> {
    neuron["tags"]
        .as_array()?
        .iter()
        .find(|tag| tag["name"].as_str() == Some("compact-unity-slot"))
        .map(|tag| tag["value"].as_str().unwrap().parse().unwrap())
}

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

/// The uuid the capture's surviving minted constant carries, or a fresh name
/// when nothing minted survives (see `compact_legacy_unused.rs`).
fn survivor_uuid(record: &Value) -> String {
    let input: Vec<&str> = record["traced"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|neuron| neuron["uuid"].as_str().unwrap())
        .collect();
    record["result"]["after"]["neurons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|neuron| !input.contains(&neuron["uuid"].as_str().unwrap()))
        .find(|neuron| unity_slot_tag(neuron).is_some())
        .map_or_else(
            || "unity-constant-0".to_string(),
            |neuron| neuron["uuid"].as_str().unwrap().to_string(),
        )
}

/// One `remove_neuron` call with a fresh draw sequence from the capture.
fn remove_chosen(
    record: &Value,
    creature: &CreatureExport,
    choice: &UnusedNeuronChoice,
) -> CreatureExport {
    let name = name_of(record);
    let draws = f64_list(&record["removalDraws"]);
    let mut next = draws.into_iter();
    let survivor = survivor_uuid(record);
    remove_neuron(
        creature,
        &choice.uuid,
        choice.activation,
        &unity_slots_from_input(record),
        || survivor,
        || {
            next.next()
                .unwrap_or_else(|| panic!("{name}: removal drew past the captured draws"))
        },
    )
    .unwrap_or_else(|error| panic!("{name}: {error}"))
    .unwrap_or_else(|| panic!("{name}: removal refused the chosen neuron"))
    .creature
}

/// Bit-for-bit equality of two creatures: every field, weights and biases by
/// their `f64` bit patterns rather than `==`, so `-0.0` and `0.0` differ.
fn assert_bit_equal(context: &str, actual: &CreatureExport, expected: &CreatureExport) {
    assert_eq!(actual.input, expected.input, "{context}: input");
    assert_eq!(actual.output, expected.output, "{context}: output");
    assert_eq!(
        actual.forward_only, expected.forward_only,
        "{context}: forwardOnly"
    );
    assert_eq!(
        actual.semantic_version, expected.semantic_version,
        "{context}: semanticVersion"
    );
    assert_eq!(actual.memetic, expected.memetic, "{context}: memetic");
    assert_eq!(
        actual.neurons.len(),
        expected.neurons.len(),
        "{context}: neuron count"
    );
    for (a, e) in actual.neurons.iter().zip(&expected.neurons) {
        assert_eq!(
            (&a.id, &a.neuron_type, &a.uuid, &a.squash),
            (&e.id, &e.neuron_type, &e.uuid, &e.squash),
            "{context}: neuron"
        );
        assert_eq!(
            a.bias.to_bits(),
            e.bias.to_bits(),
            "{context}: bias of {}",
            a.uuid
        );
    }
    assert_eq!(
        actual.synapses.len(),
        expected.synapses.len(),
        "{context}: synapse count"
    );
    for (a, e) in actual.synapses.iter().zip(&expected.synapses) {
        assert_eq!(
            (&a.from_uuid, &a.to_uuid, &a.synapse_type),
            (&e.from_uuid, &e.to_uuid, &e.synapse_type),
            "{context}: synapse"
        );
        assert_eq!(
            a.weight.to_bits(),
            e.weight.to_bits(),
            "{context}: weight {} -> {}",
            a.from_uuid,
            a.to_uuid
        );
    }
}

/// Compares a creature with a captured `result.after`, in order, bit for bit.
fn assert_matches_capture(context: &str, actual: &CreatureExport, expected: &Value) {
    let neurons = expected["neurons"].as_array().unwrap();
    assert_eq!(
        actual.neurons.len(),
        neurons.len(),
        "{context}: neuron count"
    );
    for (a, e) in actual.neurons.iter().zip(neurons) {
        assert_eq!(a.uuid, e["uuid"].as_str().unwrap(), "{context}: uuid");
        assert_eq!(
            a.neuron_type,
            e["type"].as_str().unwrap(),
            "{context}: type"
        );
        assert_eq!(
            a.squash.as_deref(),
            e["squash"].as_str(),
            "{context}: squash"
        );
        assert_eq!(
            a.bias.to_bits(),
            e["bias"].as_f64().unwrap().to_bits(),
            "{context}: bias of {}",
            a.uuid
        );
    }
    let synapses = expected["synapses"].as_array().unwrap();
    assert_eq!(
        actual.synapses.len(),
        synapses.len(),
        "{context}: synapse count"
    );
    for (a, e) in actual.synapses.iter().zip(synapses) {
        assert_eq!(
            a.from_uuid,
            e["fromUUID"].as_str().unwrap(),
            "{context}: source"
        );
        assert_eq!(
            a.to_uuid,
            e["toUUID"].as_str().unwrap(),
            "{context}: target"
        );
        assert_eq!(
            a.synapse_type.as_deref(),
            e["type"].as_str(),
            "{context}: role"
        );
        assert_eq!(
            a.weight.to_bits(),
            e["weight"].as_f64().unwrap().to_bits(),
            "{context}: weight {} -> {}",
            a.from_uuid,
            a.to_uuid
        );
    }
}

#[test]
fn rerunning_a_stage_after_an_abandoned_attempt_is_bit_identical() {
    let records = golden();

    // remove_neuron: abandon an attempt on every capture, run another capture's
    // removal in between, then re-run. The re-run must equal both the abandoned
    // attempt and the frozen capture.
    let compacting: Vec<&Value> = records.iter().filter(|r| !r["result"].is_null()).collect();
    assert_eq!(compacting.len(), CAPTURES - 1);
    for (i, record) in compacting.iter().enumerate() {
        let name = name_of(record);
        let creature = creature_from_aligned(record);
        let choice = choose_with(record, &creature, &candidates_from_aligned(record))
            .unwrap_or_else(|| panic!("{name}: a neuron is chosen"));
        let abandoned = remove_chosen(record, &creature, &choice);

        let other = compacting[(i + 1) % compacting.len()];
        let other_creature = creature_from_aligned(other);
        let other_choice =
            choose_with(other, &other_creature, &candidates_from_aligned(other)).unwrap();
        drop(remove_chosen(other, &other_creature, &other_choice));

        let rerun_choice = choose_with(record, &creature, &candidates_from_aligned(record))
            .unwrap_or_else(|| panic!("{name}: a neuron is chosen again"));
        assert_eq!(rerun_choice.uuid, choice.uuid, "{name}: same choice");
        assert_eq!(
            rerun_choice.activation.to_bits(),
            choice.activation.to_bits(),
            "{name}: same activation"
        );
        let rerun = remove_chosen(record, &creature, &rerun_choice);
        assert_bit_equal(&format!("{name}: re-run removal"), &rerun, &abandoned);
        assert_matches_capture(
            &format!("{name}: re-run removal"),
            &rerun,
            &record["result"]["after"],
        );
        assert_ne!(rerun, creature, "{name}: the removal changed the creature");
    }

    // aggressive_prune on newborn_grace_1. Its synapses, in aligned order, carry
    // weights 1, 0.5, 1, 1e-6 and 0.9. Only hidden-quiet -> output-0 (1e-6) is
    // below AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD (1e-3), it carries no role, and
    // output-0 keeps the 0.5 and 0.9 edges, so exactly that one synapse goes.
    let record = golden_record("newborn_grace_1");
    let creature = creature_from_aligned(&record);
    let weights: Vec<f64> = creature.synapses.iter().map(|s| s.weight).collect();
    assert_eq!(weights, [1.0, 0.5, 1.0, 1e-6, 0.9], "the fixture's weights");
    let mut expected = creature.clone();
    expected.synapses.remove(3);

    let abandoned = aggressive_prune(&creature, AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, &[]).unwrap();
    // An unrelated call with a different threshold and a frozen index between
    // the abandoned attempt and the re-run. At 0.95 every inbound edge of
    // output-0 (0.5, 1e-6, 0.9) is tiny, so its first in order, 0.5, survives;
    // 1e-6 and 0.9 go. The frozen first edge and the other 1.0 edge stay.
    let unrelated = aggressive_prune(&creature, 0.95, &[0]).unwrap();
    assert_eq!(unrelated.removed_synapses, 2, "1e-6 and 0.9 are removed");
    let rerun = aggressive_prune(&creature, AGGRESSIVE_PRUNE_WEIGHT_THRESHOLD, &[]).unwrap();

    assert_eq!(
        rerun.removed_synapses, 1,
        "one synapse is below the threshold"
    );
    assert_eq!(abandoned.removed_synapses, rerun.removed_synapses);
    assert_bit_equal(
        "aggressive_prune: re-run",
        &rerun.creature,
        &abandoned.creature,
    );
    assert_bit_equal("aggressive_prune: re-run", &rerun.creature, &expected);
    assert_ne!(rerun.creature, creature, "the prune changed the creature");
}

/// The value of a neuron's first `newborn-grace` tag, as `getTag` returns it.
fn grace_tag(neuron: &Value) -> Option<String> {
    neuron["tags"]
        .as_array()?
        .iter()
        .find(|tag| tag["name"].as_str() == Some("newborn-grace"))
        .map(|tag| tag["value"].as_str().unwrap().to_string())
}

/// Applies an ageing decision to a tag value, as the caller writes it back.
fn apply(tag: Option<String>, ageing: NewbornGraceAgeing) -> Option<String> {
    match ageing {
        NewbornGraceAgeing::Keep => tag,
        NewbornGraceAgeing::Remove => None,
        NewbornGraceAgeing::Set(value) => Some(value),
    }
}

/// The caller's one `ageNewbornGrace` round over every neuron of the trained
/// creature, which `finaliseTraining` runs whether or not the deadline fired.
fn caller_ages_grace(tags: &HashMap<String, Option<String>>) -> HashMap<String, Option<String>> {
    tags.iter()
        .map(|(uuid, tag)| {
            let aged = apply(tag.clone(), age_newborn_grace(tag.as_deref()));
            (uuid.clone(), aged)
        })
        .collect()
}

fn tags_before(record: &Value) -> HashMap<String, Option<String>> {
    record["traced"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["uuid"].as_str().unwrap().to_string(), grace_tag(n)))
        .collect()
}

/// Firing the deadline only decides whether a compacted copy exists. The
/// trained creature is aged once by the caller on both paths, and the frozen
/// compacted copy was aged inside `compactUnused`; for every neuron the two
/// share, they must carry the same `newborn-grace` tag, so the grace budget is
/// the same whichever copy the caller ends up keeping.
#[test]
fn the_callers_grace_round_matches_the_round_a_compacting_pass_writes() {
    // Hand-derived cases, from parseInt then `remaining - 1`:
    //   "3"   -> 3 rounds, one spent -> "2"
    //   " 4x" -> parseInt skips the space and stops at x: 4 -> "3"
    //   "1"   -> the last round is spent, the tag goes
    //   "0"   -> no round left, the stale tag goes
    //   "abc" -> no digits, 0 rounds, the tag goes
    //   ""    -> an empty tag is left as it is
    //   None  -> nothing to age
    assert_eq!(
        age_newborn_grace(Some("3")),
        NewbornGraceAgeing::Set("2".to_string())
    );
    assert_eq!(
        age_newborn_grace(Some(" 4x")),
        NewbornGraceAgeing::Set("3".to_string())
    );
    assert_eq!(age_newborn_grace(Some("1")), NewbornGraceAgeing::Remove);
    assert_eq!(age_newborn_grace(Some("0")), NewbornGraceAgeing::Remove);
    assert_eq!(age_newborn_grace(Some("abc")), NewbornGraceAgeing::Remove);
    assert_eq!(age_newborn_grace(Some("")), NewbornGraceAgeing::Keep);
    assert_eq!(age_newborn_grace(None), NewbornGraceAgeing::Keep);

    let (mut kept, mut removed, mut set) = (0_usize, 0_usize, 0_usize);
    let mut compared = 0_usize;
    for record in &golden() {
        let name = name_of(record);
        if record["result"].is_null() {
            continue;
        }
        let before = tags_before(record);
        let aged = caller_ages_grace(&before);
        for survivor in record["result"]["after"]["neurons"].as_array().unwrap() {
            let uuid = survivor["uuid"].as_str().unwrap();
            let Some(tag) = before.get(uuid) else {
                // A constant the pass minted has no before-tag to age.
                continue;
            };
            match age_newborn_grace(tag.as_deref()) {
                NewbornGraceAgeing::Keep => kept += 1,
                NewbornGraceAgeing::Remove => removed += 1,
                NewbornGraceAgeing::Set(_) => set += 1,
            }
            assert_eq!(
                aged[uuid],
                grace_tag(survivor),
                "{name}: the caller ages {uuid} as the frozen pass did"
            );
            compared += 1;
        }
    }
    assert!(set >= 1, "a survivor's grace was reduced");
    assert!(removed >= 1, "a survivor's grace was removed");
    assert!(kept >= 1, "a survivor's tag was kept");
    assert_eq!(compared, kept + removed + set);

    // Exactly one round, not two: newborn_grace_2's hidden-quiet goes from "2"
    // to "1" on the frozen pass. The caller's one round writes "1"; ageing the
    // already-aged compacted copy again (what `!compactWasAged` prevents) would
    // remove the tag, which the frozen pass did not.
    let record = golden_record("newborn_grace_2");
    let once = caller_ages_grace(&tags_before(&record));
    assert_eq!(once["hidden-quiet"], Some("1".to_string()));
    let twice = caller_ages_grace(&once);
    assert_eq!(twice["hidden-quiet"], None);
}
