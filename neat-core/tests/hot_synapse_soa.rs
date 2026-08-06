//! Issue #533 — the struct-of-arrays hot view of the synapse array.
//!
//! `CompiledNetwork::hot_weights` / `hot_from` mirror `synapses[i].weight` and
//! `synapses[i].from_index` in the same order, so the record-interleaved gather
//! streams 6 B per synapse instead of `SynapseData`'s 8 B. The view is
//! redundant, and redundancy is a correctness hazard, so these tests pin it as
//! a **public invariant** across every construction path:
//!
//! - the binary deserialiser `CompiledNetwork::new`,
//! - `compile_creature` (JSON), and
//! - `Clone` (directory-mode scoring clones one network per worker).
//!
//! They are "what" tests — they build real networks through the public entry
//! points and assert on observable state and on scored numbers, not on how the
//! gather is wired. The final test pins the fail-loud guard: a network whose
//! hot view has drifted must panic in a debug build rather than silently
//! scoring wrong numbers.

use neat_core::loss::mse_sum_batch_packed;
use neat_core::{
    CompiledNetwork, SynapseData, compile_creature, hot_synapse_soa, parse_creature_json,
};

/// Assert the SoA view mirrors `synapses` element-for-element. Weights are
/// compared by **bit pattern**: the hot view is a copy, not a recomputation, so
/// anything short of bit equality is drift.
fn assert_hot_view_mirrors_synapses(net: &CompiledNetwork, context: &str) {
    assert_eq!(
        net.hot_weights.len(),
        net.synapses.len(),
        "{context}: hot_weights must hold one entry per synapse"
    );
    assert_eq!(
        net.hot_from.len(),
        net.synapses.len(),
        "{context}: hot_from must hold one entry per synapse"
    );
    for (i, s) in net.synapses.iter().enumerate() {
        assert_eq!(
            net.hot_weights[i].to_bits(),
            s.weight.to_bits(),
            "{context}: hot_weights[{i}] ({}) is not synapses[{i}].weight ({})",
            net.hot_weights[i],
            s.weight
        );
        assert_eq!(
            net.hot_from[i], s.from_index,
            "{context}: hot_from[{i}] is not synapses[{i}].from_index"
        );
    }
}

/// A creature with two hidden neurons and one output over `input` inputs, and
/// deliberately **asymmetric** weights so a reordered or duplicated hot entry
/// cannot coincide with the correct one.
const CREATURE_JSON: &str = r#"{
    "input": 3,
    "output": 1,
    "neurons": [
        {"type": "hidden", "uuid": "hidden-0", "bias": 0.11, "squash": "TANH"},
        {"type": "hidden", "uuid": "hidden-1", "bias": -0.07, "squash": "TANH"},
        {"type": "output", "uuid": "output-0", "bias": 0.02, "squash": "TANH"}
    ],
    "synapses": [
        {"fromUUID": "input-0", "toUUID": "hidden-0", "weight": 0.31},
        {"fromUUID": "input-1", "toUUID": "hidden-0", "weight": -0.62},
        {"fromUUID": "input-2", "toUUID": "hidden-0", "weight": 0.17},
        {"fromUUID": "input-0", "toUUID": "hidden-1", "weight": -0.24},
        {"fromUUID": "input-2", "toUUID": "hidden-1", "weight": 0.53},
        {"fromUUID": "hidden-0", "toUUID": "output-0", "weight": 0.72},
        {"fromUUID": "hidden-1", "toUUID": "output-0", "weight": -0.41}
    ]
}"#;

/// Serialise the same topology in the compiled binary format
/// (header `[num_neurons: u32, num_inputs: u32]`, then per non-input neuron a
/// 12-byte header `[bias: f64, squash: u8, is_constant: u8, num_synapses: u16]`
/// followed by 12-byte synapse records `[from: u16, type: u8, pad, weight: f64]`).
fn serialise_binary_network() -> Vec<u8> {
    /// `(bias, squash_type, [(from_index, weight)…])` for one non-input neuron.
    type NeuronSpec = (f64, u8, Vec<(u16, f64)>);

    // 3 inputs + hidden-0, hidden-1, output-0.
    const TANH: u8 = 7;
    let neurons: [NeuronSpec; 3] = [
        (0.11, TANH, vec![(0, 0.31), (1, -0.62), (2, 0.17)]),
        (-0.07, TANH, vec![(0, -0.24), (2, 0.53)]),
        (0.02, TANH, vec![(3, 0.72), (4, -0.41)]),
    ];

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&6u32.to_le_bytes()); // num_neurons
    bytes.extend_from_slice(&3u32.to_le_bytes()); // num_inputs
    for (bias, squash, synapses) in &neurons {
        bytes.extend_from_slice(&bias.to_le_bytes());
        bytes.push(*squash);
        bytes.push(0); // is_constant
        bytes.extend_from_slice(&(synapses.len() as u16).to_le_bytes());
        for (from, weight) in synapses {
            bytes.extend_from_slice(&from.to_le_bytes());
            bytes.push(0); // synapse_type
            bytes.push(0); // padding
            bytes.extend_from_slice(&weight.to_le_bytes());
        }
    }
    bytes
}

/// Packed `[inputs…, target]` records, distinct per record so a lane mix-up in
/// the interleaved gather cannot hide.
fn packed_records(num_records: usize, num_inputs: usize) -> Vec<f32> {
    (0..num_records)
        .flat_map(|r| {
            let mut row: Vec<f32> = (0..num_inputs)
                .map(|i| (((r * num_inputs + i) as f32) * 0.037).sin())
                .collect();
            row.push(((r as f32) * 0.11).cos() * 0.5);
            row
        })
        .collect()
}

#[test]
fn compile_creature_builds_a_hot_view_matching_every_synapse() {
    let creature = parse_creature_json(CREATURE_JSON).expect("creature must parse");
    let net = compile_creature(&creature).expect("creature must compile");

    assert_eq!(net.synapses.len(), 7, "fixture must carry all 7 synapses");
    assert_hot_view_mirrors_synapses(&net, "compile_creature");

    // Non-vacuous: the hot view must carry the fixture's actual asymmetric
    // weights and sources in declaration order, not zeros or a sorted copy.
    assert_eq!(net.hot_from, vec![0u16, 1, 2, 0, 2, 3, 4]);
    let expected: Vec<f32> = vec![0.31, -0.62, 0.17, -0.24, 0.53, 0.72, -0.41]
        .into_iter()
        .map(|w: f64| w as f32)
        .collect();
    assert_eq!(net.hot_weights, expected);
}

#[test]
fn binary_deserialiser_builds_a_hot_view_matching_every_synapse() {
    let net = CompiledNetwork::new(&serialise_binary_network()).expect("network must load");

    assert_eq!(net.synapses.len(), 7, "fixture must carry all 7 synapses");
    assert_hot_view_mirrors_synapses(&net, "CompiledNetwork::new");
    assert_eq!(net.hot_from, vec![0u16, 1, 2, 0, 2, 3, 4]);
}

#[test]
fn cloning_a_network_preserves_the_hot_view() {
    // Directory-mode scoring clones one `CompiledNetwork` per worker, so the
    // hot view has to survive `Clone` intact.
    let net = CompiledNetwork::new(&serialise_binary_network()).expect("network must load");
    let clone = net.clone();
    assert_hot_view_mirrors_synapses(&clone, "clone");
    assert_eq!(clone.hot_weights, net.hot_weights);
    assert_eq!(clone.hot_from, net.hot_from);
}

#[test]
fn both_construction_paths_agree_on_the_hot_view() {
    // The JSON and binary routes describe the same topology, so their hot views
    // must be identical — this is what stops one path drifting from the other.
    let creature = parse_creature_json(CREATURE_JSON).expect("creature must parse");
    let from_json = compile_creature(&creature).expect("creature must compile");
    let from_binary = CompiledNetwork::new(&serialise_binary_network()).expect("must load");

    assert_eq!(from_json.hot_from, from_binary.hot_from);
    assert_eq!(from_json.hot_weights, from_binary.hot_weights);
}

#[test]
fn hot_synapse_soa_preserves_order_and_handles_an_empty_array() {
    // Edge case: no synapses at all yields two empty arrays, not a panic.
    let (weights, from) = hot_synapse_soa(&[]);
    assert!(weights.is_empty() && from.is_empty());

    // Repeated sources and a repeated weight must survive in position order —
    // a de-duplicating or sorting implementation would fail here.
    let synapses = vec![
        SynapseData {
            weight: -1.5,
            from_index: 9,
            synapse_type: 0,
        },
        SynapseData {
            weight: 0.25,
            from_index: 9,
            synapse_type: 1,
        },
        SynapseData {
            weight: -1.5,
            from_index: 2,
            synapse_type: 2,
        },
    ];
    let (weights, from) = hot_synapse_soa(&synapses);
    assert_eq!(weights, vec![-1.5f32, 0.25, -1.5]);
    assert_eq!(from, vec![9u16, 9, 2]);
}

#[test]
fn interleaved_scoring_matches_the_single_record_reference() {
    // The gather now reads the hot view, so the batched fused-MSE result must
    // still equal an independent oracle: each record scored on its own through
    // the scalar `activate` forward pass, which never touches `hot_weights` or
    // `hot_from`. Record counts straddle the scalar, 4-way, 8-record and
    // 32-record interleaved tiers.
    const TOL: f64 = 1e-3;
    let creature = parse_creature_json(CREATURE_JSON).expect("creature must parse");
    let num_inputs = 3usize;

    for num_records in [1usize, 5, 8, 33, 70] {
        let mut net = compile_creature(&creature).expect("creature must compile");
        let records = packed_records(num_records, num_inputs);
        let batched = mse_sum_batch_packed(&mut net, &records, num_inputs, 1, true);

        let mut reference = 0.0f64;
        let mut oracle = compile_creature(&creature).expect("creature must compile");
        for r in 0..num_records {
            let base = r * (num_inputs + 1);
            let out = oracle.activate(&records[base..base + num_inputs], 1);
            let diff = (records[base + num_inputs] - out[0]) as f64;
            reference += diff * diff;
        }

        assert!(
            (batched - reference).abs() < TOL,
            "n={num_records}: interleaved MSE {batched} vs single-record reference {reference}"
        );
    }
}

/// Fail-loud guard (debug builds): a drifted hot view must panic rather than
/// score silently-wrong numbers. Only meaningful where `debug_assert!` is
/// compiled in, which is how the test suite runs.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "Issue #533")]
fn a_drifted_hot_view_fails_loud_instead_of_scoring_wrong_numbers() {
    let creature = parse_creature_json(CREATURE_JSON).expect("creature must parse");
    let mut net = compile_creature(&creature).expect("creature must compile");

    // Simulate the redundancy hazard the issue calls out: a caller mutates a
    // weight and forgets to rebuild the hot view.
    net.synapses[0].weight = 12.5;

    let records = packed_records(8, 3);
    let _ = mse_sum_batch_packed(&mut net, &records, 3, 1, true);
}
