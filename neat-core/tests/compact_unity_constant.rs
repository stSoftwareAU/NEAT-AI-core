//! Constant-branch removal of one hidden neuron, ported from `removeNeuron`'s
//! constant arm (Issue #778).
//!
//! The oracle for the frozen cases is `golden/compact_legacy_unity.json`,
//! captured by running the TypeScript at NEAT-AI 60d6b4e5; see
//! `docs/research/compaction-unity-constant-baseline.md`. The behaviour tests at
//! the end are native counterparts of three NEAT-AI `compactUnused` tests, and
//! compare the network's own outputs before and after, so they do not depend on
//! the port's arithmetic.

use neat_core::{
    CreatureExport, MemeticExport, MemeticWeights, NeuronExport, NeuronRemoval, NeuronRemovalError,
    RemoveNeuronError, SynapseExport, UnitySlot, compile_creature, remove_neuron,
    remove_neuron_by_bias_fold,
};
use serde_json::Value;
use std::cell::Cell;
use std::collections::BTreeMap;

/// Points per input axis for the behaviour grids, spanning `[-1, 1]`.
const GRID_POINTS: usize = 9;

/// Outputs read back from every compiled creature in the behaviour tests.
const OUTPUTS: usize = 2;

/// The activation folded in for a `CLIPPED` neuron that is saturated at 1.
///
/// Its pre-activation is `bias + sum(weight * input)`, bounded by the bias
/// `+/-` the sum of `|weight|` over inputs in `[-1, 1]`. The bias is 2 or 2.5 and
/// the inbound weights are `+/-0.3`, so the pre-activation stays in `[1.4, 3.1]`,
/// above the clamp at 1. The trace mean is therefore exactly 1.0.
const SATURATED_ACTIVATION: f64 = 1.0;

fn golden_records() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/compact_legacy_unity.json"
    ))
    .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn golden_record(name: &str) -> Value {
    golden_records()
        .into_iter()
        .find(|record| record["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no golden record named {name}"))
}

fn neuron(kind: &str, uuid: &str, squash: Option<&str>, bias: f64) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias,
        squash: squash.map(str::to_string),
    }
}

fn synapse(from: &str, to: &str, weight: f64, role: Option<&str>) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: role.map(str::to_string),
    }
}

fn edge(from: &str, to: &str, weight: f64) -> SynapseExport {
    synapse(from, to, weight, None)
}

/// Runtime index as TypeScript numbers it: inputs first, then listed neurons.
fn runtime_index(creature: &CreatureExport, uuid: &str) -> usize {
    match uuid.strip_prefix("input-") {
        Some(index) => index.parse().unwrap(),
        None => {
            creature.input
                + creature
                    .neurons
                    .iter()
                    .position(|neuron| neuron.uuid == uuid)
                    .unwrap()
        }
    }
}

/// TypeScript's role code: `standard` 0, `condition` 1, `negative` 2, `positive` 3.
fn role_code(role: Option<&str>) -> u8 {
    match role {
        None => 0,
        Some("condition") => 1,
        Some("negative") => 2,
        Some("positive") => 3,
        Some(other) => panic!("unexpected synapse role {other}"),
    }
}

/// `removeHiddenNeuron` re-sorts the synapses by `(from, to, role)` (TypeScript
/// `compareSynapses`), and `remove_neuron` expects that order on input. The
/// captured fixtures list their synapses unsorted, so every builder sorts them.
fn in_typescript_order(mut creature: CreatureExport) -> CreatureExport {
    let mut keyed: Vec<((usize, usize, u8), SynapseExport)> = creature
        .synapses
        .iter()
        .map(|s| {
            let key = (
                runtime_index(&creature, &s.from_uuid),
                runtime_index(&creature, &s.to_uuid),
                role_code(s.synapse_type.as_deref()),
            );
            (key, s.clone())
        })
        .collect();
    keyed.sort_by_key(|(key, _)| *key);
    creature.synapses = keyed.into_iter().map(|(_, s)| s).collect();
    creature
}

fn creature(
    input: usize,
    neurons: Vec<NeuronExport>,
    synapses: Vec<SynapseExport>,
) -> CreatureExport {
    let output = neurons.iter().filter(|n| n.neuron_type == "output").count();
    in_typescript_order(CreatureExport {
        input,
        output,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    })
}

fn record_creature(input: &Value) -> CreatureExport {
    let neurons = input["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| NeuronExport {
            id: None,
            neuron_type: n["type"].as_str().unwrap().to_string(),
            uuid: n["uuid"].as_str().unwrap().to_string(),
            bias: n["bias"].as_f64().unwrap(),
            squash: n.get("squash").and_then(Value::as_str).map(str::to_string),
        })
        .collect();
    let synapses = input["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| SynapseExport {
            from_uuid: s["fromUUID"].as_str().unwrap().to_string(),
            to_uuid: s["toUUID"].as_str().unwrap().to_string(),
            weight: s["weight"].as_f64().unwrap(),
            synapse_type: s.get("type").and_then(Value::as_str).map(str::to_string),
        })
        .collect();
    in_typescript_order(CreatureExport {
        input: input["input"].as_u64().unwrap() as usize,
        output: input["output"].as_u64().unwrap() as usize,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: false,
        memetic: None,
    })
}

fn record_slots(record: &Value) -> Vec<UnitySlot> {
    record["unitySlots"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(uuid, slot)| UnitySlot {
            uuid: uuid.clone(),
            slot: slot.as_str().unwrap().parse().unwrap(),
        })
        .collect()
}

fn fresh_uuid() -> String {
    "unity-constant-0".to_string()
}

fn never_draw() -> f64 {
    panic!("the constant branch takes no reconnect draw")
}

/// `hidden-0` (IDENTITY) feeds one `IF` output, so removing it takes the
/// constant branch. Output `output-0` has two inbound synapses from inputs.
fn if_target_fixture() -> CreatureExport {
    creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
        ],
    )
}

#[test]
fn frozen_unity_captures_replay_exactly() {
    const REPLAYED: [&str; 5] = [
        "unity_if",
        "unity_mixed_targets",
        "unity_two_roles",
        "unity_after_existing_constants",
        "unity_feeder_cascade",
    ];
    let mut replayed = Vec::new();
    for name in REPLAYED {
        let record = golden_record(name);
        let creature = record_creature(&record["input"]);
        let mints = Cell::new(0_u64);
        let removal = remove_neuron(
            &creature,
            record["remove"].as_str().unwrap(),
            record["activation"].as_f64().unwrap(),
            &record_slots(&record),
            || {
                mints.set(mints.get() + 1);
                fresh_uuid()
            },
            never_draw,
        )
        .expect("a frozen capture is accepted")
        .expect("a frozen capture removes its neuron");

        assert_eq!(
            mints.get(),
            record["minted"].as_u64().unwrap(),
            "{name}: mint calls"
        );

        let expected_neurons = record["after"]["neurons"].as_array().unwrap();
        assert_eq!(
            removal.creature.neurons.len(),
            expected_neurons.len(),
            "{name}: neuron count"
        );
        for (actual, expected) in removal.creature.neurons.iter().zip(expected_neurons) {
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

        // The capture's slot-0 constant is the one the port names.
        let slot_zero: Vec<&str> = expected_neurons
            .iter()
            .filter(|n| n["unitySlot"].as_str() == Some("0"))
            .map(|n| n["uuid"].as_str().unwrap())
            .collect();
        assert_eq!(slot_zero, ["unity-constant-0"], "{name}: capture slot 0");
        assert_eq!(
            removal.unity_constant,
            Some(UnitySlot {
                uuid: "unity-constant-0".to_string(),
                slot: 0,
            }),
            "{name}: unity constant"
        );

        let expected_synapses = record["after"]["synapses"].as_array().unwrap();
        assert_eq!(
            removal.creature.synapses.len(),
            expected_synapses.len(),
            "{name}: synapse count"
        );
        for (actual, expected) in removal.creature.synapses.iter().zip(expected_synapses) {
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
        assert!(
            removal.creature.memetic.is_none(),
            "{name}: memetic cleared"
        );
        replayed.push(name);
    }
    assert_eq!(replayed, REPLAYED);
}

#[test]
fn the_overflow_capture_is_refused_before_any_change() {
    let record = golden_record("unity_overflow");
    let creature = record_creature(&record["input"]);
    let activation = record["activation"].as_f64().unwrap();
    // TypeScript mints the constant, then throws while connecting it. The
    // capture says so; the port refuses before it mints.
    assert_eq!(record["result"]["threw"], "weight must be a number");

    let mints = Cell::new(0_u64);
    let refused = remove_neuron(
        &creature,
        "hidden-0",
        activation,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::NonFiniteConstantWeight {
            target: "output-0".to_string(),
        })
    );
    assert_eq!(mints.get(), 0, "a refused constant takes no UUID");

    // Control: the same creature with activation 1.5 gives 10 * 1.5 = 15, which
    // is finite, so it is accepted.
    let accepted = remove_neuron(&creature, "hidden-0", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(
        accepted.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
}

#[test]
fn reusing_a_tagged_slot_is_refused_and_the_capture_shows_why() {
    let record = golden_record("unity_reuse_tagged_slot");
    let creature = record_creature(&record["input"]);
    let slots = record_slots(&record);
    assert_eq!(
        slots,
        vec![UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        }]
    );

    // The capture's defect: both constants are gone, and hidden-0 carries the
    // condition weight 4.5 = 3 * 1.5, rewired through the removed neuron.
    let after_uuids: Vec<&str> = record["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    assert!(!after_uuids.contains(&"k0"), "{after_uuids:?}");
    assert!(
        !after_uuids.contains(&"unity-constant-0"),
        "{after_uuids:?}"
    );
    assert!(
        record["after"]["synapses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["fromUUID"] == "hidden-0"
                && s["toUUID"] == "output-0"
                && s["type"] == "condition"
                && s["weight"].as_f64() == Some(4.5)),
        "the capture rewires through hidden-0 at weight 4.5"
    );

    let refused = remove_neuron(&creature, "hidden-1", 1.5, &slots, fresh_uuid, never_draw);
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0".to_string(),
        })
    );

    // Control: without the slot tag the same creature takes the fresh path and
    // adds the new constant in slot 0. `k0` feeds only the removed `hidden-1`, so
    // the orphan cascade takes it too, as TypeScript's `removeHiddenNeuron` does.
    let accepted = remove_neuron(&creature, "hidden-1", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert!(!accepted.creature.neurons.iter().any(|n| n.uuid == "k0"));
    assert_eq!(
        accepted.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
}

#[test]
fn a_legacy_slot_zero_id_is_refused_like_a_tagged_one() {
    // `legacy-k` holds slot 0 by its id -1000 alone. `hidden-9` sits between the
    // constants and the removed `hidden-0`, so the insertion point (index 1) names
    // `hidden-9`, not `hidden-0`. That is the defective shape of
    // `unity_reuse_tagged_slot`, where the stale index rewires through another
    // neuron, so the port refuses it. A removed neuron directly after the constants
    // takes the sound reuse path instead.
    let edges = vec![
        edge("input-0", "hidden-0", 1.0),
        edge("hidden-0", "output-0", 2.0),
        edge("input-1", "output-0", 1.0),
        edge("legacy-k", "output-0", 0.5),
        edge("input-1", "hidden-9", 1.0),
        edge("hidden-9", "output-0", 0.5),
    ];
    let legacy = NeuronExport {
        id: Some(-1000),
        ..neuron("constant", "legacy-k", None, 1.0)
    };
    let with_legacy_id = creature(
        2,
        vec![
            legacy,
            neuron("hidden", "hidden-9", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges.clone(),
    );
    let refused = remove_neuron(
        &with_legacy_id,
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "legacy-k".to_string(),
        })
    );

    // Control: the same constant without the legacy id holds no slot.
    let untagged = creature(
        2,
        vec![
            neuron("constant", "legacy-k", None, 1.0),
            neuron("hidden", "hidden-9", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    );
    let accepted = remove_neuron(&untagged, "hidden-0", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert!(
        accepted
            .creature
            .neurons
            .iter()
            .any(|n| n.uuid == "legacy-k")
    );
    assert_eq!(
        accepted.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
}

/// The edges of the held-slot fixtures: `hidden-0` (IDENTITY) feeds the IF target
/// `output-0` at 2.0, and the constant `k0` feeds the same target at 0.25.
fn held_slot_zero_edges() -> Vec<SynapseExport> {
    vec![
        edge("input-0", "hidden-0", 1.0),
        edge("hidden-0", "output-0", 2.0),
        edge("input-1", "output-0", 1.0),
        edge("k0", "output-0", 0.25),
    ]
}

/// `hidden-0` is the first neuron after the leading constants, so the insertion
/// point names it. `holder` is the constant `k0` that holds slot 0 ahead of it.
fn held_slot_zero_fixture(holder: NeuronExport) -> CreatureExport {
    creature(
        2,
        vec![
            holder,
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        held_slot_zero_edges(),
    )
}

fn slot_zero_held_by_k0() -> [UnitySlot; 1] {
    [UnitySlot {
        uuid: "k0".to_string(),
        slot: 0,
    }]
}

fn survivor_in_slot_one() -> Option<UnitySlot> {
    Some(UnitySlot {
        uuid: "unity-constant-0".to_string(),
        slot: 1,
    })
}

/// The sound reuse, checked against its derivation. TypeScript reaches `k0` through
/// the stale index and sets its bias to 1; the new constant takes slot 1 right after
/// `k0`; `k0` keeps its own edge; and the new constant feeds the IF target at
/// 2.0 * 1.5 = 3.0, the role it had. The port mints the survivor once.
fn assert_sound_reuse(removal: &NeuronRemoval, mints: u64) {
    assert_eq!(mints, 1, "the port mints the survivor only");
    assert_eq!(removal.unity_constant, survivor_in_slot_one());
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids, ["k0", "unity-constant-0", "output-0"]);
    assert_eq!(
        removal.creature.neurons[0].bias.to_bits(),
        1.0_f64.to_bits(),
        "k0's bias becomes 1.0"
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0", "output-0", 0.25)),
        "k0 keeps its own edge"
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("unity-constant-0", "output-0", 3.0)),
        "the survivor feeds the IF target at 2.0 * 1.5"
    );
}

#[test]
fn a_tagged_slot_zero_constant_ahead_of_the_removed_neuron_is_reused_as_slot_one() {
    // `k0` carries bias 0.7, so the reset to 1.0 is visible.
    let fixture = held_slot_zero_fixture(neuron("constant", "k0", None, 0.7));
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slot_zero_held_by_k0(),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .expect("the sound reuse is accepted")
    .expect("the removal is applied");
    assert_sound_reuse(&removal, mints.get());
}

#[test]
fn a_legacy_slot_zero_id_ahead_of_the_removed_neuron_is_reused_as_slot_one() {
    let legacy = NeuronExport {
        id: Some(-1000),
        ..neuron("constant", "k0", None, 0.7)
    };
    let fixture = held_slot_zero_fixture(legacy);
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .expect("the sound reuse is accepted")
    .expect("the removal is applied");
    assert_sound_reuse(&removal, mints.get());
}

#[test]
fn a_held_slot_zero_behind_the_removed_neuron_is_refused() {
    // `k0` follows `hidden-0`, so it is not one of the leading constants.
    // TypeScript's loader never hands `removeNeuron` this order (see
    // `unity_reuse_holder_behind`), and the port refuses it.
    let behind = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("constant", "k0", None, 0.7),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        held_slot_zero_edges(),
    );
    let refused = remove_neuron(
        &behind,
        "hidden-0",
        1.5,
        &slot_zero_held_by_k0(),
        fresh_uuid,
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0".to_string(),
        })
    );

    // Control: the same constant moved ahead of `hidden-0` takes the sound reuse.
    let ahead = held_slot_zero_fixture(neuron("constant", "k0", None, 0.7));
    let accepted = remove_neuron(
        &ahead,
        "hidden-0",
        1.5,
        &slot_zero_held_by_k0(),
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.unity_constant, survivor_in_slot_one());
}

/// Two constants lead the list, ahead of `hidden-0`: `k0` and `second`. Both feed
/// the IF target, so TypeScript's dedupe loop would have a synapse to re-source.
fn two_leading_constants_fixture(second: NeuronExport) -> CreatureExport {
    let mut edges = held_slot_zero_edges();
    edges.push(edge("k2", "output-0", 0.125));
    creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            second,
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    )
}

#[test]
fn a_second_tagged_slot_zero_constant_is_refused_and_named() {
    // TypeScript meets `k0` first and `k2` second and re-sources `k2`'s synapses
    // onto `k0`. Both feed `output-0` in the standard role, so the merge would
    // leave `k0` with two identical synapses. The port refuses that merge before
    // minting anything.
    let fixture = two_leading_constants_fixture(neuron("constant", "k2", None, -0.2));
    let slots = |second: u8| {
        [
            UnitySlot {
                uuid: "k0".to_string(),
                slot: 0,
            },
            UnitySlot {
                uuid: "k2".to_string(),
                slot: second,
            },
        ]
    };
    let mints = Cell::new(0_u64);
    let refused = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots(0),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k2".to_string(),
        })
    );
    assert_eq!(mints.get(), 0, "a refusal mints no UUID");

    // Control: the same creature with `k2` tagged slot 2 holds one slot-0 constant
    // only, so the sound reuse runs and `k2` is left alone.
    let accepted = remove_neuron(&fixture, "hidden-0", 1.5, &slots(2), fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(accepted.unity_constant, survivor_in_slot_one());
    assert!(accepted.creature.neurons.iter().any(|n| n.uuid == "k2"));
}

#[test]
fn a_legacy_slot_zero_id_followed_by_a_tagged_slot_zero_constant_is_refused() {
    // `k0` holds slot 0 by id -1000 and `k2` by tag. Merging `k2` into `k0` would
    // duplicate their standard synapse into `output-0`, so it is refused.
    let legacy = NeuronExport {
        id: Some(-1000),
        ..neuron("constant", "k0", None, 0.7)
    };
    let mut fixture = two_leading_constants_fixture(neuron("constant", "k2", None, -0.2));
    fixture.neurons[0] = legacy;
    let slot_zero = |slot: u8| {
        [UnitySlot {
            uuid: "k2".to_string(),
            slot,
        }]
    };
    let refused = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slot_zero(0),
        fresh_uuid,
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k2".to_string(),
        })
    );

    // Control: `k2` tagged slot 2 leaves the legacy-id constant as the only holder.
    let accepted = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slot_zero(2),
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.unity_constant, survivor_in_slot_one());
}

/// `k0` holds slot 0 ahead of `hidden-0`, and `holder` sits between them. When
/// `holder` also holds slot 1 the survivor takes slot 2, and otherwise slot 1.
fn slot_one_fixture(holder: NeuronExport) -> CreatureExport {
    let mut edges = held_slot_zero_edges();
    edges.push(edge("k1", "output-0", 0.25));
    creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            holder,
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    )
}

fn slots_zero_and_one_held() -> [UnitySlot; 2] {
    [
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        },
        UnitySlot {
            uuid: "k1".to_string(),
            slot: 1,
        },
    ]
}

/// The slot-2 reuse, checked against its derivation. `createConstantOne(creature, 1)`
/// returns a stale index naming `hidden-0` exactly as slot 0 did, so `removeNeuron`
/// asks for slot 2. Both holders have their bias set to 1, the survivor takes
/// slot 2 right after them, and it feeds the IF target at 2.0 * 1.5 = 3.0.
fn assert_slot_two_reuse(removal: &NeuronRemoval, mints: u64) {
    assert_eq!(mints, 1, "the port mints the survivor only");
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 2,
        })
    );
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids, ["k0", "k1", "unity-constant-0", "output-0"]);
    for holder in &removal.creature.neurons[..2] {
        assert_eq!(
            holder.bias.to_bits(),
            1.0_f64.to_bits(),
            "{}'s bias becomes 1.0",
            holder.uuid
        );
    }
    for kept in [edge("k0", "output-0", 0.25), edge("k1", "output-0", 0.25)] {
        assert!(removal.creature.synapses.contains(&kept), "{kept:?} kept");
    }
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("unity-constant-0", "output-0", 3.0)),
        "the survivor feeds the IF target at 2.0 * 1.5"
    );
}

#[test]
fn a_tagged_slot_one_constant_ahead_of_the_removed_neuron_moves_the_survivor_to_slot_two() {
    let fixture = slot_one_fixture(neuron("constant", "k1", None, -0.2));
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots_zero_and_one_held(),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .expect("the slot-2 reuse is accepted")
    .expect("the removal is applied");
    assert_slot_two_reuse(&removal, mints.get());

    // Control: without the slot-1 tag `k1` holds nothing, and the survivor takes
    // slot 1 with `k1`'s bias untouched.
    let accepted = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots_zero_and_one_held()[..1],
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.unity_constant, survivor_in_slot_one());
    assert_eq!(
        accepted.creature.neurons[1].bias.to_bits(),
        (-0.2_f64).to_bits()
    );
}

#[test]
fn a_legacy_slot_one_id_ahead_of_the_removed_neuron_moves_the_survivor_to_slot_two() {
    let legacy = NeuronExport {
        id: Some(-1001),
        ..neuron("constant", "k1", None, -0.2)
    };
    let fixture = slot_one_fixture(legacy);
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slot_zero_held_by_k0(),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .expect("the slot-2 reuse is accepted")
    .expect("the removal is applied");
    assert_slot_two_reuse(&removal, mints.get());
}

/// The removal's neurons and synapses equal the capture's `after`, bit for bit
/// on biases and weights, in order, with the memetic record cleared.
fn assert_matches_capture(record: &Value, removal: &NeuronRemoval) {
    let expected_neurons = record["after"]["neurons"].as_array().unwrap();
    let actual: Vec<(String, String, Option<String>, u64)> = removal
        .creature
        .neurons
        .iter()
        .map(|n| {
            (
                n.neuron_type.clone(),
                n.uuid.clone(),
                n.squash.clone(),
                n.bias.to_bits(),
            )
        })
        .collect();
    let expected: Vec<(String, String, Option<String>, u64)> = expected_neurons
        .iter()
        .map(|n| {
            (
                n["type"].as_str().unwrap().to_string(),
                n["uuid"].as_str().unwrap().to_string(),
                n["squash"].as_str().map(str::to_string),
                n["bias"].as_f64().unwrap().to_bits(),
            )
        })
        .collect();
    assert_eq!(actual, expected);

    let actual: Vec<(String, String, u64, Option<String>)> = removal
        .creature
        .synapses
        .iter()
        .map(|s| {
            (
                s.from_uuid.clone(),
                s.to_uuid.clone(),
                s.weight.to_bits(),
                s.synapse_type.clone(),
            )
        })
        .collect();
    let expected: Vec<(String, String, u64, Option<String>)> = record["after"]["synapses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["fromUUID"].as_str().unwrap().to_string(),
                s["toUUID"].as_str().unwrap().to_string(),
                s["weight"].as_f64().unwrap().to_bits(),
                s["type"].as_str().map(str::to_string),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert!(removal.creature.memetic.is_none());
}

#[test]
fn the_frozen_slot_one_held_capture_replays_exactly() {
    let record = golden_record("unity_reuse_slot_one_held");
    assert_eq!(record["result"], true);
    // TypeScript mints a temporary slot-0 and slot-1 constant, removes both, and
    // keeps the third. The port mints only that survivor, so it is handed the
    // capture's name for it.
    assert_eq!(record["minted"], 3);
    let creature = record_creature(&record["input"]);
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &creature,
        record["remove"].as_str().unwrap(),
        record["activation"].as_f64().unwrap(),
        &record_slots(&record),
        || {
            mints.set(mints.get() + 1);
            "unity-constant-2".to_string()
        },
        never_draw,
    )
    .expect("the capture is accepted")
    .expect("the capture removes its neuron");
    assert_eq!(mints.get(), 1);

    assert_matches_capture(&record, &removal);
    // The capture tags the survivor slot 2, which is the slot the port reports.
    let survivor = record["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["uuid"] == "unity-constant-2")
        .unwrap();
    assert_eq!(survivor["unitySlot"], "2");
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-2".to_string(),
            slot: 2,
        })
    );

    assert!(removal.creature.memetic.is_none());
}

#[test]
fn a_held_slot_two_is_refused_and_the_capture_shows_why() {
    let record = golden_record("unity_reuse_slot_two_held");
    // The capture's defect: `removeNeuron` gives up (`false`) and removes nothing,
    // but every holder's bias has already been set to 1, so the creature it leaves
    // scores differently from the one it was handed.
    assert_eq!(record["result"], false);
    let before = record["input"]["neurons"].as_array().unwrap();
    let after = record["after"]["neurons"].as_array().unwrap();
    assert_eq!(after.len(), before.len(), "nothing is removed");
    for holder in ["k0", "k1", "k2"] {
        let was = before.iter().find(|n| n["uuid"] == holder).unwrap();
        let now = after.iter().find(|n| n["uuid"] == holder).unwrap();
        assert_ne!(was["bias"].as_f64(), Some(1.0), "{holder} starts off 1");
        assert_eq!(now["bias"].as_f64(), Some(1.0), "{holder} is left at 1");
    }

    let creature = record_creature(&record["input"]);
    let mints = Cell::new(0_u64);
    let refused = remove_neuron(
        &creature,
        "hidden-0",
        1.5,
        &record_slots(&record),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k2".to_string(),
        })
    );
    assert_eq!(mints.get(), 0, "a refusal takes no UUID");

    // Control: without the slot-2 tag the survivor takes slot 2.
    let slots = record_slots(&record);
    let without_two: Vec<UnitySlot> = slots.into_iter().filter(|s| s.slot != 2).collect();
    let accepted = remove_neuron(
        &creature,
        "hidden-0",
        1.5,
        &without_two,
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.unity_constant.map(|s| s.slot), Some(2));
}

#[test]
fn a_legacy_slot_two_id_is_refused_like_a_tagged_one() {
    let mut edges = held_slot_zero_edges();
    edges.push(edge("k1", "output-0", 0.25));
    edges.push(edge("k2", "output-0", 0.25));
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("constant", "k1", None, -0.2),
            NeuronExport {
                id: Some(-1002),
                ..neuron("constant", "k2", None, 0.3)
            },
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    );
    let refused = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots_zero_and_one_held(),
        fresh_uuid,
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k2".to_string(),
        })
    );
}

#[test]
fn a_slot_one_holder_behind_the_removed_neuron_is_refused_and_named() {
    // `k1` follows `hidden-0`, an order TypeScript's loader never hands
    // `removeNeuron`, so the port refuses it.
    let mut edges = held_slot_zero_edges();
    edges.push(edge("k1", "output-0", 0.25));
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("constant", "k1", None, -0.2),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    );
    let refused = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots_zero_and_one_held(),
        fresh_uuid,
        never_draw,
    );
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k1".to_string(),
        })
    );
}

#[test]
fn one_constant_holding_slots_zero_and_one_is_refused() {
    let fixture = held_slot_zero_fixture(neuron("constant", "k0", None, 0.7));
    let slots = [
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        },
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 1,
        },
    ];
    let refused = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw);
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0".to_string(),
        })
    );
}

#[test]
fn a_second_slot_one_holder_with_a_duplicate_synapse_is_refused_and_named() {
    // `k1` and `k1b` both feed `output-0` in the standard role, so merging `k1b`
    // into `k1` would duplicate that synapse.
    let mut edges = held_slot_zero_edges();
    edges.push(edge("k1", "output-0", 0.25));
    edges.push(edge("k1b", "output-0", 0.5));
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("constant", "k1", None, -0.2),
            neuron("constant", "k1b", None, 0.4),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        edges,
    );
    let mut slots = slots_zero_and_one_held().to_vec();
    slots.push(UnitySlot {
        uuid: "k1b".to_string(),
        slot: 1,
    });
    let refused = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw);
    assert_eq!(
        refused,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k1b".to_string(),
        })
    );
}

#[test]
fn a_slot_one_tag_does_not_block_the_fresh_slot_zero_path() {
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k1", None, -0.2),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
            edge("k1", "output-0", 0.25),
        ],
    );
    let slots = [UnitySlot {
        uuid: "k1".to_string(),
        slot: 1,
    }];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    assert!(removal.creature.neurons.iter().any(|n| n.uuid == "k1"));
}

#[test]
fn a_unity_slot_on_a_non_constant_neuron_is_ignored() {
    // `hidden-1` is a hidden neuron, not a constant, and it is not the neuron
    // being removed. It carries slot 0, the one slot that would trigger the
    // reuse refusal if the tag were read regardless of the neuron's type.
    let fixture = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-1", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-1", "hidden-1", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("hidden-1", "output-0", 1.0),
            edge("input-1", "output-0", 1.0),
        ],
    );
    let slots = [UnitySlot {
        uuid: "hidden-1".to_string(),
        slot: 0,
    }];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    // The fresh path ran: the minted constant replaces `hidden-0`'s edge with
    // weight 2.0 * 1.5 = 3.0, and the tagged hidden neuron is untouched.
    let constant_edge = removal
        .creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == "unity-constant-0")
        .expect("the minted constant feeds the IF target");
    assert_eq!(constant_edge.to_uuid, "output-0");
    assert_eq!(constant_edge.weight.to_bits(), 3.0_f64.to_bits());
    assert!(
        removal
            .creature
            .neurons
            .iter()
            .any(|n| n.uuid == "hidden-1")
    );
    assert!(
        !removal
            .creature
            .neurons
            .iter()
            .any(|n| n.uuid == "hidden-0")
    );
}

fn memetic_record() -> MemeticExport {
    MemeticExport {
        biases: BTreeMap::from([("output-0".to_string(), 0.01)]),
        weights: MemeticWeights::default(),
        extra: serde_json::Map::new(),
    }
}

#[test]
fn a_constant_branch_removal_clears_the_memetic_record_and_a_refusal_keeps_it() {
    let mut base = if_target_fixture();
    base.memetic = Some(memetic_record());

    let removal = remove_neuron(&base, "hidden-0", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    // The constant branch ran (a constant was minted), and it cleared the record.
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    assert_eq!(removal.creature.memetic, None);

    // f64::MAX * 2.0 overflows the constant's weight, so the branch refuses.
    let refused = remove_neuron(&base, "hidden-0", f64::MAX, &[], fresh_uuid, never_draw);
    assert_eq!(
        refused,
        Err(NeuronRemovalError::NonFiniteConstantWeight {
            target: "output-0".to_string(),
        })
    );
    assert_eq!(
        base.memetic,
        Some(memetic_record()),
        "the input is never modified"
    );
}

#[test]
fn a_constant_the_cascade_removes_again_is_not_returned_as_a_slot() {
    // `hidden-0` is a MAXIMUM aggregate whose only outward synapse is its own
    // self-loop, so its one outward target is an aggregate and the constant
    // branch runs. The minted constant then feeds nothing but the removed
    // neuron, so the orphan cascade removes it again.
    let mut fixture = creature(
        1,
        vec![
            neuron("hidden", "hidden-0", Some("MAXIMUM"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 0.0),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "hidden-0", 0.5),
            edge("input-0", "output-0", 1.0),
        ],
    );
    fixture.forward_only = false;

    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(mints.get(), 1, "the constant branch minted a constant");
    assert_eq!(removal.unity_constant, None);
    assert!(
        !removal
            .creature
            .neurons
            .iter()
            .any(|n| n.uuid == "unity-constant-0" || n.uuid == "hidden-0")
    );
    assert!(removal.creature.synapses.iter().all(|s| {
        s.from_uuid != "unity-constant-0"
            && s.to_uuid != "unity-constant-0"
            && s.from_uuid != "hidden-0"
            && s.to_uuid != "hidden-0"
    }));
    assert_eq!(
        removal.creature.synapses,
        vec![edge("input-0", "output-0", 1.0)]
    );
}

#[test]
fn a_unity_slot_outside_zero_to_two_is_refused_before_removal() {
    let slots = [UnitySlot {
        uuid: "hidden-0".to_string(),
        slot: 3,
    }];
    let error = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &slots,
        fresh_uuid,
        never_draw,
    )
    .expect_err("slot 3 is refused");
    assert_eq!(
        error,
        NeuronRemovalError::InvalidUnitySlot {
            uuid: "hidden-0".to_string(),
            slot: 3,
        }
    );
    assert_eq!(
        error.to_string(),
        "Unity slot 3 for neuron hidden-0 is outside 0..=2"
    );

    let accepted = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    );
    assert!(accepted.unwrap().is_some(), "control without the bad slot");
}

#[test]
fn a_unity_slot_two_tag_is_accepted_and_does_not_block_a_fresh_slot_zero() {
    // `createConstantOne(creature, 2)` tags a constant with slot 2. Slot 2 is
    // the top of `0..=2`, so it is accepted; it is not slot 0, so the removal
    // still mints a fresh slot-0 constant and leaves the slot-2 constant alone.
    let fixture = creature(
        2,
        vec![
            neuron("constant", "constant-2", None, 1.0),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-0", "output-0", 0.5),
            edge("input-1", "output-0", 1.0),
        ],
    );
    let slots = [UnitySlot {
        uuid: "constant-2".to_string(),
        slot: 2,
    }];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw)
        .expect("slot 2 is in range")
        .expect("the removal is applied");

    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert!(
        uuids.contains(&"constant-2"),
        "the slot-2 constant survives"
    );
    assert!(
        uuids.contains(&"unity-constant-0"),
        "a fresh slot-0 constant is minted"
    );
}

#[test]
fn a_unity_slot_naming_no_neuron_is_refused_before_removal() {
    let slots = [UnitySlot {
        uuid: "ghost-0".to_string(),
        slot: 0,
    }];
    let error = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &slots,
        fresh_uuid,
        never_draw,
    )
    .expect_err("an unknown slot neuron is refused");
    assert_eq!(
        error,
        NeuronRemovalError::UnknownUnitySlotNeuron {
            uuid: "ghost-0".to_string(),
        }
    );
    assert_eq!(
        error.to_string(),
        "Unity slot names neuron ghost-0, which is not in the creature"
    );

    let accepted = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    );
    assert!(accepted.unwrap().is_some(), "control without the bad slot");
}

#[test]
fn a_minted_uuid_naming_a_neuron_or_an_input_is_refused() {
    for taken in ["output-0", "input-1"] {
        let error = remove_neuron(
            &if_target_fixture(),
            "hidden-0",
            1.5,
            &[],
            || taken.to_string(),
            never_draw,
        )
        .expect_err("a taken UUID is refused");
        assert_eq!(
            error,
            NeuronRemovalError::ConstantUuidTaken {
                uuid: taken.to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            format!("Minted constant UUID {taken} is already used by a neuron or an input")
        );
    }

    let accepted = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    );
    assert!(accepted.unwrap().is_some(), "control with a free UUID");
}

#[test]
fn an_unknown_neuron_is_refused_by_the_shared_bias_fold_validation() {
    let error = remove_neuron(
        &if_target_fixture(),
        "ghost",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    )
    .expect_err("an unknown neuron is refused");
    assert_eq!(
        error,
        NeuronRemovalError::Remove(RemoveNeuronError::UnknownNeuron {
            uuid: "ghost".to_string(),
        })
    );
    assert_eq!(error.to_string(), "No neuron with UUID ghost");

    let accepted = remove_neuron(
        &if_target_fixture(),
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        never_draw,
    );
    assert!(accepted.unwrap().is_some(), "control with a known neuron");
}

#[test]
fn a_target_without_an_aggregate_squash_delegates_to_the_bias_fold() {
    // output-0 is IDENTITY, so no constant is needed. Its bias 0.25 takes the
    // fold 2.0 * 1.5 = 3.0 from hidden-0, giving 3.25. Two inputs still feed
    // output-0 afterwards, so no reconnect draw is taken.
    let fixture = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-0", "output-0", 0.5),
            edge("input-1", "output-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
        ],
    );
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .unwrap()
    .unwrap();

    let delegated = remove_neuron_by_bias_fold(&fixture, "hidden-0", 1.5, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(removal.creature, delegated);
    assert_eq!(removal.unity_constant, None);
    assert_eq!(mints.get(), 0, "the bias fold mints no constant");
    let output = removal
        .creature
        .neurons
        .iter()
        .find(|n| n.uuid == "output-0")
        .unwrap();
    assert_eq!(output.bias, 3.25);
}

/// `hidden-0` is the only synapse into `output-0` (IDENTITY), so the fold
/// leaves that target short of two inbound synapses and takes one reconnect
/// draw. Two inputs make a draw `u` pick input `floor(u * 2)`.
fn reconnect_fixture() -> CreatureExport {
    creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
        ],
    )
}

#[test]
fn the_callers_reconnect_draw_picks_the_input_of_the_delegated_bias_fold() {
    // A draw of 0.75 over two inputs is floor(0.75 * 2) = 1, so the zero-weight
    // reconnect edge comes from input-1; a draw of 0.25 is floor(0.5) = 0 and
    // gives input-0. The two differ, so a fixed draw inside `remove_neuron`
    // could not satisfy both.
    for (draw, source) in [(0.75, "input-1"), (0.25, "input-0")] {
        let calls = Cell::new(0_u32);
        let removal = remove_neuron(
            &reconnect_fixture(),
            "hidden-0",
            1.5,
            &[],
            fresh_uuid,
            || {
                calls.set(calls.get() + 1);
                draw
            },
        )
        .unwrap()
        .unwrap();

        assert_eq!(calls.get(), 1, "one reconnect draw for draw {draw}");
        assert_eq!(removal.unity_constant, None);
        assert_eq!(
            removal.creature.synapses,
            vec![edge(source, "output-0", 0.0)],
            "draw {draw} reconnects from {source}"
        );
        let output = removal
            .creature
            .neurons
            .iter()
            .find(|n| n.uuid == "output-0")
            .unwrap();
        assert_eq!(output.bias, 3.25, "0.25 + 2.0 * 1.5");
    }
}

#[test]
fn a_reconnect_draw_outside_zero_to_one_is_refused_with_its_message() {
    for draw in [1.0, -0.5] {
        let error = remove_neuron(
            &reconnect_fixture(),
            "hidden-0",
            1.5,
            &[],
            fresh_uuid,
            || draw,
        )
        .expect_err("a draw outside [0, 1) is refused");
        assert_eq!(
            error,
            NeuronRemovalError::Remove(RemoveNeuronError::DrawOutOfRange { draw })
        );
        assert_eq!(
            error.to_string(),
            format!("Reconnect draw {draw} is outside [0, 1)")
        );
    }

    let accepted = remove_neuron(
        &reconnect_fixture(),
        "hidden-0",
        1.5,
        &[],
        fresh_uuid,
        || 0.0,
    );
    assert!(accepted.unwrap().is_some(), "control with an in-range draw");
}

#[test]
fn a_non_finite_fold_on_a_bias_fold_target_returns_none_and_mints_nothing() {
    // Bias f64::MAX plus the fold f64::MAX * 1.0 overflows to infinity, so the
    // fold is refused and nothing changes.
    let fixture = creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), f64::MAX),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-0", "output-0", 0.5),
            edge("input-1", "output-0", 1.0),
            edge("hidden-0", "output-0", 1.0),
        ],
    );
    let mints = Cell::new(0_u64);
    let result = remove_neuron(
        &fixture,
        "hidden-0",
        f64::MAX,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(result, Ok(None));
    assert_eq!(mints.get(), 0);
}

#[test]
fn the_constant_edges_follow_typescript_order_whatever_order_they_are_listed_in() {
    // Two edges from hidden-0 into the IF output-0, listed positive first:
    // condition 0.5 and positive -2.0. TypeScript's outward connections come
    // back in role order, so the constant's edges must too. With activation 2.0
    // the weights are 0.5 * 2.0 = 1.0 and -2.0 * 2.0 = -4.0.
    let neurons = vec![
        neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
        neuron("output", "output-0", Some("IF"), 0.25),
    ];
    let build = |outward: [SynapseExport; 2]| CreatureExport {
        input: 2,
        output: 1,
        neurons: neurons.clone(),
        synapses: [
            edge("input-0", "hidden-0", 1.0),
            edge("input-1", "output-0", 1.0),
            outward[0].clone(),
            outward[1].clone(),
        ]
        .to_vec(),
        semantic_version: None,
        forward_only: true,
        memetic: None,
    };
    let positive = synapse("hidden-0", "output-0", -2.0, Some("positive"));
    let condition = synapse("hidden-0", "output-0", 0.5, Some("condition"));

    let listed_positive_first = remove_neuron(
        &build([positive.clone(), condition.clone()]),
        "hidden-0",
        2.0,
        &[],
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    let listed_condition_first = remove_neuron(
        &build([condition, positive]),
        "hidden-0",
        2.0,
        &[],
        fresh_uuid,
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        listed_positive_first.creature,
        listed_condition_first.creature
    );

    let constant_edges: Vec<(&str, Option<&str>)> = listed_positive_first
        .creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == "unity-constant-0")
        .map(|s| (s.to_uuid.as_str(), s.synapse_type.as_deref()))
        .collect();
    assert_eq!(
        constant_edges,
        [
            ("output-0", Some("condition")),
            ("output-0", Some("positive"))
        ]
    );
    let weights: Vec<f64> = listed_positive_first
        .creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == "unity-constant-0")
        .map(|s| s.weight)
        .collect();
    assert_eq!(weights, [1.0, -4.0]);
}

/// Removes `hidden-3` by the constant branch at the saturated activation.
fn remove_saturated_hidden_3(before: &CreatureExport) -> NeuronRemoval {
    let removal = remove_neuron(
        before,
        "hidden-3",
        SATURATED_ACTIVATION,
        &[],
        fresh_uuid,
        never_draw,
    )
    .expect("the fixture is accepted")
    .expect("the fixture removes hidden-3");
    assert!(
        removal.unity_constant.is_some(),
        "the constant branch ran, so the test cannot pass vacuously"
    );
    assert!(
        removal
            .creature
            .neurons
            .iter()
            .all(|n| n.uuid != "hidden-3"),
        "hidden-3 is gone"
    );
    removal
}

/// Both creatures give the same outputs on every grid point over `[-1, 1]^3`,
/// within `1e-6` relative (absolute below 1). The grid has `GRID_POINTS` values
/// per axis, so `GRID_POINTS.pow(3)` points in all.
fn assert_same_outputs_on_grid(before: &CreatureExport, after: &CreatureExport) {
    let mut before = compile_creature(before).expect("fixture compiles");
    let mut after = compile_creature(after).expect("compacted creature compiles");
    let axis: Vec<f32> = (0..GRID_POINTS)
        .map(|k| -1.0 + 2.0 * k as f32 / (GRID_POINTS - 1) as f32)
        .collect();
    let mut checked = 0;
    for &a in &axis {
        for &b in &axis {
            for &c in &axis {
                let sample = [a, b, c];
                let expected = before.activate(&sample, OUTPUTS);
                let actual = after.activate(&sample, OUTPUTS);
                assert_eq!(actual.len(), expected.len());
                for (e, x) in expected.iter().zip(&actual) {
                    assert!(
                        (e - x).abs() <= 1e-6 * e.abs().max(1.0),
                        "{sample:?}: {e} vs {x}"
                    );
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, GRID_POINTS.pow(3));
}

/// The `FixIF` creature: `hidden-3` feeds `output-0` and the `IF` neuron
/// `hidden-4`, so removing it takes the constant branch.
fn fixif_creature() -> CreatureExport {
    creature(
        3,
        vec![
            neuron("hidden", "hidden-3", Some("CLIPPED"), 2.0),
            neuron("hidden", "hidden-4", Some("IF"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 1.0),
            neuron("output", "output-1", Some("IDENTITY"), 0.0),
        ],
        vec![
            edge("input-0", "hidden-3", -0.3),
            synapse("input-0", "hidden-4", -0.3, Some("condition")),
            edge("input-1", "hidden-3", 0.3),
            synapse("input-1", "hidden-4", 0.3, Some("positive")),
            edge("hidden-3", "output-0", 0.6),
            synapse("hidden-3", "hidden-4", 0.3, Some("negative")),
            edge("hidden-4", "output-1", 0.7),
            edge("input-2", "output-1", 0.8),
        ],
    )
}

#[test]
fn removing_a_saturated_neuron_into_an_if_target_preserves_behaviour() {
    let before = fixif_creature();
    let removal = remove_saturated_hidden_3(&before);
    assert_same_outputs_on_grid(&before, &removal.creature);
}

#[test]
fn removing_a_saturated_neuron_with_an_inverse_feeder_preserves_behaviour() {
    // `CompactConstant`: hidden-3 also feeds the INVERSE neuron hidden-3b, which
    // the constant must take over with weight 0.3 * 1.0.
    let before = creature(
        3,
        vec![
            neuron("hidden", "hidden-3", Some("CLIPPED"), 2.5),
            neuron("hidden", "hidden-3b", Some("INVERSE"), -0.1),
            neuron("hidden", "hidden-4", Some("IF"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 1.0),
            neuron("output", "output-1", Some("IDENTITY"), 0.0),
        ],
        vec![
            edge("input-0", "hidden-3", -0.3),
            synapse("input-0", "hidden-4", -0.3, Some("condition")),
            edge("hidden-3", "hidden-3b", 0.3),
            synapse("hidden-3", "hidden-4", 0.3, Some("positive")),
            edge("hidden-3", "output-0", 0.6),
            synapse("hidden-3b", "hidden-4", 0.3, Some("negative")),
            edge("hidden-4", "output-1", 0.7),
            edge("input-2", "output-1", 0.8),
        ],
    );
    let removal = remove_saturated_hidden_3(&before);
    assert_same_outputs_on_grid(&before, &removal.creature);
}

#[test]
fn the_new_constant_lands_right_after_an_untagged_constant() {
    // `CompactKeepOrder`: the untagged constant `first-one` already sits first.
    // The new constant goes after the leading constants, so it lands at index 1.
    let before = creature(
        3,
        vec![
            neuron("constant", "first-one", None, 1.0),
            neuron("hidden", "hidden-3", Some("CLIPPED"), 2.5),
            neuron("hidden", "hidden-3b", Some("INVERSE"), -0.1),
            neuron("hidden", "hidden-4", Some("IF"), 0.0),
            neuron("output", "output-0", Some("IDENTITY"), 1.0),
            neuron("output", "output-1", Some("IDENTITY"), 0.0),
        ],
        vec![
            edge("input-0", "hidden-3", -0.3),
            synapse("input-0", "hidden-4", -0.3, Some("condition")),
            edge("hidden-3", "hidden-3b", 0.3),
            synapse("hidden-3", "hidden-4", 0.3, Some("positive")),
            edge("hidden-3", "output-0", 0.6),
            synapse("hidden-3b", "hidden-4", 0.3, Some("negative")),
            edge("hidden-4", "output-1", 0.7),
            edge("input-2", "output-1", 0.8),
            edge("first-one", "output-1", 0.8),
            edge("first-one", "output-0", 0.9),
        ],
    );
    let removal = remove_saturated_hidden_3(&before);
    let first = removal
        .creature
        .neurons
        .iter()
        .position(|n| n.uuid == "first-one")
        .unwrap();
    assert_eq!(removal.creature.neurons[first + 1].uuid, "unity-constant-0");
    assert_eq!(removal.creature.neurons[first + 1].neuron_type, "constant");
    assert_same_outputs_on_grid(&before, &removal.creature);
}

/// `hidden-0` (IDENTITY, bias 0) feeds `output-0` at `outward_weight` with the
/// `condition` role. `output-0` carries `squash` and bias 0.25, and `input-0` also
/// feeds it at 0.5. Once `hidden-0` is gone `output-0` still has two inbound
/// synapses, so the bias fold takes no reconnect draw.
fn aggregate_target_fixture(squash: &str, outward_weight: f64) -> CreatureExport {
    creature(
        2,
        vec![
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some(squash), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-0", "output-0", 0.5),
            edge("input-1", "output-0", 1.0),
            synapse("hidden-0", "output-0", outward_weight, Some("condition")),
        ],
    )
}

#[test]
fn a_minimum_or_hypotv2_target_takes_the_constant_branch() {
    // The constant's edge into output-0 has weight 2.0 * 1.5 = 3.0. Both factors
    // are exact in f64, so the product is exactly 3.0, and the edge keeps the
    // outward `condition` role.
    for squash in ["MINIMUM", "HYPOTv2"] {
        let mints = Cell::new(0_u64);
        let removal = remove_neuron(
            &aggregate_target_fixture(squash, 2.0),
            "hidden-0",
            1.5,
            &[],
            || {
                mints.set(mints.get() + 1);
                fresh_uuid()
            },
            never_draw,
        )
        .unwrap_or_else(|error| panic!("{squash}: refused with {error}"))
        .unwrap_or_else(|| panic!("{squash}: the removal was not applied"));

        assert_eq!(
            mints.get(),
            1,
            "{squash}: the constant branch mints one constant"
        );
        assert_eq!(
            removal.unity_constant,
            Some(UnitySlot {
                uuid: "unity-constant-0".to_string(),
                slot: 0,
            }),
            "{squash}: unity constant"
        );
        assert!(
            removal
                .creature
                .neurons
                .iter()
                .all(|n| n.uuid != "hidden-0"),
            "{squash}: hidden-0 is removed"
        );
        let constant_edges: Vec<&SynapseExport> = removal
            .creature
            .synapses
            .iter()
            .filter(|s| s.from_uuid == "unity-constant-0")
            .collect();
        assert_eq!(constant_edges.len(), 1, "{squash}: one constant edge");
        let constant_edge = constant_edges[0];
        assert_eq!(constant_edge.to_uuid, "output-0", "{squash}: target");
        assert_eq!(
            constant_edge.weight.to_bits(),
            3.0_f64.to_bits(),
            "{squash}: weight 2.0 * 1.5"
        );
        assert_eq!(
            constant_edge.synapse_type.as_deref(),
            Some("condition"),
            "{squash}: role preserved"
        );
    }
}

#[test]
fn an_identity_target_on_the_same_fixture_takes_the_bias_fold() {
    // Control: with IDENTITY the target needs no constant. The fold adds
    // weight * activation = 2.0 * 1.5 = 3.0 to the bias 0.25, giving 3.25.
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &aggregate_target_fixture("IDENTITY", 2.0),
        "hidden-0",
        1.5,
        &[],
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .unwrap()
    .unwrap();

    assert_eq!(mints.get(), 0, "the bias fold mints no constant");
    assert_eq!(removal.unity_constant, None);
    assert!(
        removal
            .creature
            .neurons
            .iter()
            .all(|n| n.uuid != "unity-constant-0"),
        "no constant neuron is added"
    );
    assert!(
        removal
            .creature
            .synapses
            .iter()
            .all(|s| s.from_uuid != "unity-constant-0"),
        "no constant synapse is added"
    );
    let output = removal
        .creature
        .neurons
        .iter()
        .find(|n| n.uuid == "output-0")
        .unwrap();
    assert_eq!(
        output.bias.to_bits(),
        3.25_f64.to_bits(),
        "0.25 + 2.0 * 1.5"
    );
}

#[test]
fn a_non_finite_activation_through_the_constant_branch_is_refused_naming_the_target() {
    // The constant's weight into output-0 is 2.0 * activation. NaN gives NaN,
    // +inf gives +inf and -inf gives -inf, and none of those is finite. The
    // refusal names output-0 and takes no UUID.
    let fixture = aggregate_target_fixture("IF", 2.0);
    for activation in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mints = Cell::new(0_u64);
        let refused = remove_neuron(
            &fixture,
            "hidden-0",
            activation,
            &[],
            || {
                mints.set(mints.get() + 1);
                fresh_uuid()
            },
            never_draw,
        );
        assert_eq!(
            refused,
            Err(NeuronRemovalError::NonFiniteConstantWeight {
                target: "output-0".to_string(),
            }),
            "activation {activation}"
        );
        assert_eq!(
            mints.get(),
            0,
            "activation {activation}: a refused constant takes no UUID"
        );
    }

    // Control: a finite activation on the same fixture is accepted.
    let accepted = remove_neuron(&fixture, "hidden-0", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(
        accepted.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
}

#[test]
fn a_zero_outward_weight_with_an_infinite_activation_is_refused() {
    // The constant's weight is 0.0 * activation. IEEE 754 makes zero times an
    // infinity NaN, not zero, so for +inf and -inf the weight is not finite and
    // the branch refuses, even though the synapse carries no signal.
    let fixture = aggregate_target_fixture("IF", 0.0);
    for activation in [f64::INFINITY, f64::NEG_INFINITY] {
        let mints = Cell::new(0_u64);
        let refused = remove_neuron(
            &fixture,
            "hidden-0",
            activation,
            &[],
            || {
                mints.set(mints.get() + 1);
                fresh_uuid()
            },
            never_draw,
        );
        assert_eq!(
            refused,
            Err(NeuronRemovalError::NonFiniteConstantWeight {
                target: "output-0".to_string(),
            }),
            "activation {activation}"
        );
        assert_eq!(
            mints.get(),
            0,
            "activation {activation}: a refused constant takes no UUID"
        );
    }

    // Control: a finite activation gives 0.0 * 1.5 = 0.0, which is finite, so the
    // constant is accepted with a +0.0 edge weight.
    let accepted = remove_neuron(&fixture, "hidden-0", 1.5, &[], fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    let constant_edge = accepted
        .creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == "unity-constant-0")
        .expect("the accepted constant feeds output-0");
    assert_eq!(constant_edge.to_uuid, "output-0");
    assert_eq!(constant_edge.weight.to_bits(), 0.0_f64.to_bits());
}

#[test]
fn a_second_tagged_slot_zero_holder_is_merged_into_the_first_as_captured() {
    let record = golden_record("unity_reuse_second_slot_zero_holder");
    assert_eq!(record["result"], true);
    assert_eq!(record["minted"], 1, "the temporary constant survives");
    let creature = record_creature(&record["input"]);
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &creature,
        "hidden-0",
        1.5,
        &record_slots(&record),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    )
    .expect("the merge is accepted")
    .expect("the removal is applied");
    assert_eq!(mints.get(), 1);
    assert_matches_capture(&record, &removal);

    // The derivation: `k0b`'s edge into `hidden-1` (0.25) is re-sourced onto `k0`,
    // `k0b` is removed, `k0`'s bias becomes 1, and the new constant takes a fresh
    // slot 0 feeding the IF condition at 3 * 1.5 = 4.5.
    assert!(!removal.creature.neurons.iter().any(|n| n.uuid == "k0b"));
    assert_eq!(
        removal.creature.neurons[0].bias.to_bits(),
        1.0_f64.to_bits()
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0", "hidden-1", 0.25))
    );
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    // The capture leaves both constants tagged slot 0.
    let tagged_zero: Vec<&str> = record["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["unitySlot"] == "0")
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    assert_eq!(tagged_zero, ["k0", "unity-constant-0"]);
}

/// Replays a merge capture: the port, handed the capture's name for the survivor,
/// matches the capture's `after` bit for bit and reports the survivor's slot.
fn replay_merge_capture(name: &str, survivor: &str, slot: u8) -> NeuronRemoval {
    let record = golden_record(name);
    assert_eq!(record["result"], true, "{name}");
    let creature = record_creature(&record["input"]);
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &creature,
        record["remove"].as_str().unwrap(),
        record["activation"].as_f64().unwrap(),
        &record_slots(&record),
        || {
            mints.set(mints.get() + 1);
            survivor.to_string()
        },
        never_draw,
    )
    .expect("the merge is accepted")
    .expect("the removal is applied");
    assert_eq!(mints.get(), 1, "{name}: the port mints the survivor only");
    assert_matches_capture(&record, &removal);
    let tagged = record["after"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["uuid"] == survivor)
        .unwrap();
    assert_eq!(tagged["unitySlot"], slot.to_string(), "{name}");
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: survivor.to_string(),
            slot,
        }),
        "{name}"
    );
    removal
}

#[test]
fn a_third_slot_zero_holder_is_left_as_it_is_as_captured() {
    // The dedupe loop merges `k0b` into `k0` and stops, so `k0c` keeps its bias
    // 0.3 and its edge, and the new constant lands after `k0` and `k0c`.
    let removal = replay_merge_capture("unity_reuse_third_slot_zero_holder", "unity-constant-0", 0);
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids[..3], ["k0", "k0c", "unity-constant-0"]);
    assert_eq!(
        removal.creature.neurons[1].bias.to_bits(),
        0.3_f64.to_bits()
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0", "hidden-1", 0.25))
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0c", "hidden-1", 0.125))
    );
}

#[test]
fn a_second_tagged_slot_one_holder_is_merged_into_the_first_as_captured() {
    // Slot 0 is reused, so the slot-1 call runs. Its dedupe loop merges `k1b` into
    // `k1`, and the temporary slot-1 constant survives. Both `k0` and `k1` are set
    // to bias 1, and the survivor feeds the IF condition at 3 * 1.5 = 4.5.
    let removal = replay_merge_capture("unity_reuse_second_slot_one_holder", "unity-constant-1", 1);
    assert!(!removal.creature.neurons.iter().any(|n| n.uuid == "k1b"));
    for holder in &removal.creature.neurons[..2] {
        assert_eq!(holder.bias.to_bits(), 1.0_f64.to_bits(), "{}", holder.uuid);
    }
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k1", "hidden-1", 0.25))
    );
    // TypeScript mints the temporary slot-0 constant first, then the survivor.
    assert_eq!(
        golden_record("unity_reuse_second_slot_one_holder")["minted"],
        2
    );
}

#[test]
fn a_second_holder_matched_by_its_legacy_id_alone_is_left_as_it_is() {
    // TypeScript's dedupe loop matches the slot tag only, so it skips `k0b`, which
    // holds slot 0 by id -1000 alone. The single-holder reuse then runs: `k0` is
    // set to bias 1, the survivor takes slot 1, and `k0b` keeps its bias -0.2 and
    // its edge.
    let record = golden_record("unity_reuse_second_slot_zero_holder");
    let mut fixture = record_creature(&record["input"]);
    fixture.neurons[1].id = Some(-1000);
    let only_k0 = [UnitySlot {
        uuid: "k0".to_string(),
        slot: 0,
    }];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &only_k0, fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(removal.unity_constant, survivor_in_slot_one());
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(
        uuids,
        ["k0", "k0b", "unity-constant-0", "hidden-1", "output-0"]
    );
    assert_eq!(
        removal.creature.neurons[0].bias.to_bits(),
        1.0_f64.to_bits()
    );
    assert_eq!(
        removal.creature.neurons[1].bias.to_bits(),
        (-0.2_f64).to_bits()
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0b", "hidden-1", 0.25))
    );
    assert!(removal.creature.synapses.contains(&synapse(
        "unity-constant-0",
        "output-0",
        4.5,
        Some("condition")
    )));
}

#[test]
fn a_holder_behind_the_removed_neuron_is_never_handed_over_by_typescripts_loader() {
    // The capture lists `k0` behind `hidden-0`, but `Creature.fromJSON` puts the
    // constants first, so TypeScript runs the sound reuse: `k0` leads, its bias
    // becomes 1, and the survivor takes slot 1.
    let record = golden_record("unity_reuse_holder_behind");
    let listed: Vec<&str> = record["input"]["neurons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["uuid"].as_str().unwrap())
        .collect();
    assert_eq!(listed[..2], ["hidden-0", "k0"]);
    assert_eq!(record["after"]["neurons"][0]["uuid"], "k0");
    assert_eq!(record["after"]["neurons"][1]["unitySlot"], "1");

    // The port reads the order it is handed. As listed, `k0` does not lead, so the
    // removal is refused naming it.
    let as_listed = record_creature(&record["input"]);
    let slots = record_slots(&record);
    assert_eq!(
        remove_neuron(&as_listed, "hidden-0", 1.5, &slots, fresh_uuid, never_draw),
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0".to_string(),
        })
    );

    // In the order TypeScript's loader gives, the port replays the capture. It is
    // handed the capture's name for the survivor, since TypeScript mints twice.
    assert_eq!(record["minted"], 2);
    let mut loaded = as_listed.clone();
    loaded.neurons.swap(0, 1);
    let loaded = in_typescript_order(loaded);
    let removal = remove_neuron(
        &loaded,
        "hidden-0",
        1.5,
        &slots,
        || "unity-constant-1".to_string(),
        never_draw,
    )
    .unwrap()
    .unwrap();
    assert_matches_capture(&record, &removal);
    assert_eq!(removal.unity_constant.map(|s| s.slot), Some(1));
}

#[test]
fn a_second_holder_with_an_inward_synapse_is_refused() {
    // Removing `k0b` would also drop its inward synapse and could cascade into its
    // feeder, which the merge does not port.
    let record = golden_record("unity_reuse_second_slot_zero_holder");
    let mut fixture = record_creature(&record["input"]);
    fixture.synapses.push(edge("input-1", "k0b", 0.5));
    let fixture = in_typescript_order(fixture);
    assert_eq!(
        remove_neuron(
            &fixture,
            "hidden-0",
            1.5,
            &record_slots(&record),
            fresh_uuid,
            never_draw
        ),
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0b".to_string(),
        })
    );
}

#[test]
fn a_merged_survivor_lands_right_after_the_remaining_leading_constants() {
    // `hidden-a` sits between the constants and the removed `hidden-0`. TypeScript
    // inserts the temporary constant after `k0` and `k0b`, then removes `k0b`, so
    // the survivor ends up directly after `k0`, ahead of `hidden-a`.
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("constant", "k0b", None, -0.2),
            neuron("hidden", "hidden-a", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-a", 1.0),
            edge("hidden-a", "hidden-0", 1.0),
            edge("hidden-a", "output-0", 1.0),
            edge("k0b", "hidden-a", 0.25),
            synapse("k0", "output-0", 0.5, Some("negative")),
            synapse("hidden-0", "output-0", 3.0, Some("condition")),
        ],
    );
    let slots = [
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        },
        UnitySlot {
            uuid: "k0b".to_string(),
            slot: 0,
        },
    ];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(uuids, ["k0", "unity-constant-0", "hidden-a", "output-0"]);
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "unity-constant-0".to_string(),
            slot: 0,
        })
    );
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k0", "hidden-a", 0.25))
    );
}

#[test]
fn a_tagged_holder_behind_the_insertion_point_is_not_merged() {
    // TypeScript's dedupe loop meets `k0`, then the temporary constant at the
    // insertion point, and stops there, so `k0z` behind it is never reached. The
    // single-holder reuse runs, and `k0z` keeps its bias and its edge.
    let fixture = creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("constant", "k0z", None, -0.2),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
            synapse("k0z", "output-0", 0.25, Some("negative")),
        ],
    );
    let slots = [
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        },
        UnitySlot {
            uuid: "k0z".to_string(),
            slot: 0,
        },
    ];
    let removal = remove_neuron(&fixture, "hidden-0", 1.5, &slots, fresh_uuid, never_draw)
        .unwrap()
        .unwrap();
    assert_eq!(removal.unity_constant, survivor_in_slot_one());
    let k0z = removal
        .creature
        .neurons
        .iter()
        .find(|n| n.uuid == "k0z")
        .unwrap();
    assert_eq!(k0z.bias.to_bits(), (-0.2_f64).to_bits());
    assert!(removal.creature.synapses.contains(&synapse(
        "k0z",
        "output-0",
        0.25,
        Some("negative")
    )));
}

/// Two leading constants hold slot 0 by tag, the second feeding `hidden-1`, and the
/// IF `output-0` is fed by `hidden-0`, so removing a constant that feeds the IF
/// reaches the constant branch. `k0b` is the second holder; `k0` the first.
fn two_slot_zero_holders_fixture() -> CreatureExport {
    in_typescript_order(creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("constant", "k0b", None, 0.3),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
            edge("k0", "output-0", 0.25),
            synapse("k0b", "output-0", 0.5, Some("positive")),
        ],
    ))
}

fn slot_zero_held_by_k0_and_k0b() -> [UnitySlot; 2] {
    [
        UnitySlot {
            uuid: "k0".to_string(),
            slot: 0,
        },
        UnitySlot {
            uuid: "k0b".to_string(),
            slot: 0,
        },
    ]
}

#[test]
fn removing_either_of_two_slot_zero_holders_is_refused_and_mints_nothing() {
    // The merge re-sources and removes `k0b`, so a removal of `k0b` or of `k0`
    // itself would be answered with a creature no capture backs. Both are refused
    // before any change, naming the removed constant.
    let fixture = two_slot_zero_holders_fixture();
    for removed in ["k0", "k0b"] {
        let mints = Cell::new(0_u64);
        let result = remove_neuron(
            &fixture,
            removed,
            1.5,
            &slot_zero_held_by_k0_and_k0b(),
            || {
                mints.set(mints.get() + 1);
                fresh_uuid()
            },
            never_draw,
        );
        assert_eq!(
            result,
            Err(NeuronRemovalError::UnitySlotReuseNotPorted {
                constant: removed.to_string(),
            }),
            "{removed}"
        );
        assert_eq!(mints.get(), 0, "{removed}: no UUID is minted");
    }
}

/// `k0` holds slot 0, `k1` slot 1, and `k2` and `k2b` both slot 2, all ahead of
/// `hidden-0`. Each of the two slot-2 holders feeds `output-0`; `k2b_role` is the
/// role of `k2b`'s edge, so the caller picks whether it duplicates `k2`'s.
fn two_slot_two_holders_fixture(k2b_role: Option<&str>) -> CreatureExport {
    in_typescript_order(creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            neuron("constant", "k1", None, -0.2),
            neuron("constant", "k2", None, 0.4),
            neuron("constant", "k2b", None, 0.6),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("hidden-0", "output-0", 2.0),
            edge("input-1", "output-0", 1.0),
            edge("k0", "output-0", 0.25),
            edge("k1", "output-0", 0.5),
            edge("k2", "output-0", 0.75),
            synapse("k2b", "output-0", 0.125, k2b_role),
        ],
    ))
}

fn two_slot_two_holders_slots() -> Vec<UnitySlot> {
    [("k0", 0), ("k1", 1), ("k2", 2), ("k2b", 2)]
        .into_iter()
        .map(|(uuid, slot)| UnitySlot {
            uuid: uuid.to_string(),
            slot,
        })
        .collect()
}

#[test]
fn two_tagged_slot_two_holders_are_merged_as_captured() {
    // `k2b`'s edge takes a role `k2` does not use, so merging it into `k2` makes
    // no duplicate. TypeScript's `createConstantOne(creature, 2)` merges it, and
    // `removeNeuron` succeeds with the survivor in slot 2.
    let fixture = two_slot_two_holders_fixture(Some("negative"));
    let mints = Cell::new(0_u64);
    let removal = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &two_slot_two_holders_slots(),
        || {
            mints.set(mints.get() + 1);
            "survivor".to_string()
        },
        never_draw,
    )
    .expect("the merge is accepted")
    .expect("the removal is applied");
    assert_eq!(mints.get(), 1);
    assert!(!removal.creature.neurons.iter().any(|n| n.uuid == "k2b"));
    assert_eq!(
        removal.unity_constant,
        Some(UnitySlot {
            uuid: "survivor".to_string(),
            slot: 2,
        })
    );
    // `k2b`'s edge is re-sourced onto `k2`, and every holder is reset to bias 1.
    assert!(removal.creature.synapses.contains(&synapse(
        "k2",
        "output-0",
        0.125,
        Some("negative")
    )));
    for holder in ["k0", "k1", "k2"] {
        let bias = removal
            .creature
            .neurons
            .iter()
            .find(|n| n.uuid == holder)
            .unwrap()
            .bias;
        assert_eq!(bias.to_bits(), 1.0_f64.to_bits(), "{holder}");
    }
}

#[test]
fn a_second_slot_two_holder_with_a_duplicate_synapse_is_refused_and_named() {
    // `k2` and `k2b` both feed `output-0` in the standard role, so merging `k2b`
    // into `k2` would duplicate that synapse. The control above, whose roles
    // differ, is accepted.
    let fixture = two_slot_two_holders_fixture(None);
    let mints = Cell::new(0_u64);
    let result = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &two_slot_two_holders_slots(),
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        result,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k2b".to_string(),
        })
    );
    assert_eq!(mints.get(), 0);
}

#[test]
fn a_second_tagged_slot_two_holder_is_merged_into_the_first_as_captured() {
    // Slots 0 and 1 are reused, so the slot-2 call runs. Its dedupe loop merges
    // `k2b` into `k2`, and the temporary slot-2 constant survives. `k0`, `k1` and
    // `k2` are set to bias 1, `k2b`'s edge into `hidden-1` (0.125) is re-sourced
    // onto `k2`, and the survivor feeds the IF condition at 3 * 1.5 = 4.5.
    let removal = replay_merge_capture("unity_reuse_second_slot_two_holder", "unity-constant-2", 2);
    let uuids: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(
        uuids,
        ["k0", "k1", "k2", "unity-constant-2", "hidden-1", "output-0"]
    );
    for holder in &removal.creature.neurons[..3] {
        assert_eq!(holder.bias.to_bits(), 1.0_f64.to_bits(), "{}", holder.uuid);
    }
    assert!(
        removal
            .creature
            .synapses
            .contains(&edge("k2", "hidden-1", 0.125))
    );
    assert!(removal.creature.synapses.contains(&synapse(
        "unity-constant-2",
        "output-0",
        4.5,
        Some("condition")
    )));
    // TypeScript mints the temporary constant for slot 0, slot 1 and slot 2; the
    // survivor is the third (one slot-1 call per outward synapse, and this
    // capture has one).
    assert_eq!(
        golden_record("unity_reuse_second_slot_two_holder")["minted"],
        3
    );
}

#[test]
fn one_constant_holding_slot_one_by_id_and_slot_two_by_tag_is_refused_before_a_slot_two_merge() {
    // `k1` holds slot 1 by its legacy id -1001 and slot 2 by tag, and `k2` holds
    // slot 2 by tag as well. The slot-2 dedupe loop would merge `k2` into `k1`,
    // but `createConstantOne(creature, 2)` would then re-tag the slot-1 holder,
    // a shape the frozen TypeScript answers with `false`. It is refused, as one
    // constant holding slots 0 and 1 is, and nothing is minted.
    let mut k1 = neuron("constant", "k1", None, -0.2);
    k1.id = Some(-1001);
    let fixture = in_typescript_order(creature(
        2,
        vec![
            neuron("constant", "k0", None, 0.7),
            k1,
            neuron("constant", "k2", None, 0.4),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-1", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-0", "hidden-1", 1.0),
            edge("k0", "hidden-1", 0.5),
            edge("k1", "hidden-1", 0.25),
            synapse("k2", "output-0", 0.125, Some("negative")),
            synapse("hidden-0", "output-0", 3.0, Some("condition")),
            synapse("hidden-1", "output-0", 2.0, Some("positive")),
        ],
    ));
    let slots: Vec<UnitySlot> = [("k0", 0), ("k1", 2), ("k2", 2)]
        .into_iter()
        .map(|(uuid, slot)| UnitySlot {
            uuid: uuid.to_string(),
            slot,
        })
        .collect();
    let mints = Cell::new(0_u64);
    let result = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots,
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        result,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k1".to_string(),
        })
    );
    assert_eq!(mints.get(), 0);
}

#[test]
fn one_constant_holding_slot_zero_by_id_and_slot_two_by_tag_is_refused_before_a_slot_two_merge() {
    // `k0` holds slot 0 by its legacy id -1000 and slot 2 by tag, `k1` holds slot 1
    // and `k2` holds slot 2 by tag. The slot-2 dedupe loop would merge `k2` into
    // `k0`, but `createConstantOne(creature, 0)` has already overwritten `k0`'s
    // tag with "0", so TypeScript finds `k2` alone and answers `false`. It is
    // refused, as one constant holding slots 1 and 2 is, and nothing is minted.
    let mut k0 = neuron("constant", "k0", None, 0.7);
    k0.id = Some(-1000);
    let fixture = in_typescript_order(creature(
        2,
        vec![
            k0,
            neuron("constant", "k1", None, -0.2),
            neuron("constant", "k2", None, 0.4),
            neuron("hidden", "hidden-0", Some("IDENTITY"), 0.0),
            neuron("hidden", "hidden-1", Some("IDENTITY"), 0.0),
            neuron("output", "output-0", Some("IF"), 0.25),
        ],
        vec![
            edge("input-0", "hidden-0", 1.0),
            edge("input-0", "hidden-1", 1.0),
            edge("k0", "hidden-1", 0.5),
            edge("k1", "hidden-1", 0.25),
            synapse("k2", "output-0", 0.125, Some("negative")),
            synapse("hidden-0", "output-0", 3.0, Some("condition")),
            synapse("hidden-1", "output-0", 2.0, Some("positive")),
        ],
    ));
    let slots: Vec<UnitySlot> = [("k0", 2), ("k1", 1), ("k2", 2)]
        .into_iter()
        .map(|(uuid, slot)| UnitySlot {
            uuid: uuid.to_string(),
            slot,
        })
        .collect();
    let mints = Cell::new(0_u64);
    let result = remove_neuron(
        &fixture,
        "hidden-0",
        1.5,
        &slots,
        || {
            mints.set(mints.get() + 1);
            fresh_uuid()
        },
        never_draw,
    );
    assert_eq!(
        result,
        Err(NeuronRemovalError::UnitySlotReuseNotPorted {
            constant: "k0".to_string(),
        })
    );
    assert_eq!(mints.get(), 0);
}
