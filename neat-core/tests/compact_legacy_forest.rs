//! Public forest bridge observations (Issue #776).
//!
//! The 3.4 MB creature stays in the legacy checkout. This file checks the
//! counts and refusals the original tests assert, not a native bridge rewrite.

use serde_json::Value;

fn load() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_forest.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn forest_bridge_summary_records_the_refusal_and_forward_contracts() {
    let record = load();
    assert_eq!(
        record["revision"],
        "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab"
    );
    assert_eq!(record["fixture"], "test/data/grq-23-forests-constants.json");
    assert_eq!(
        record["fixtureSha256"],
        "36dcda9e779c785ecc9d50bbf53e859170bb8671a539d0e11a205f4c63cabb0a"
    );
    assert_eq!(record["fixtureBytes"], 3_516_246);
    let input = record["input"].as_u64().unwrap();
    let declared_neurons = record["declaredNeurons"].as_u64().unwrap();
    let declared_synapses = record["declaredSynapses"].as_u64().unwrap();
    // The published file's own header: 2 511 inputs, 2 538 exported neurons,
    // 24 100 synapses. A live `Creature` also lists the inputs.
    assert_eq!(input, 2511);
    assert_eq!(declared_neurons, 2538);
    assert_eq!(declared_synapses, 24_100);
    assert!(
        record.get("neurons").is_none(),
        "the creature body stays out of core"
    );
    assert!(record.get("synapses").is_none());

    let cases = record["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);

    let ordering = &cases[0];
    assert_eq!(ordering["name"], "ordering_declines_if_bridges");
    assert_eq!(ordering["preparation"], "mergeRedundantConstants");
    assert_eq!(ordering["removedNeurons"], 0);
    assert_eq!(ordering["neuronsBefore"], ordering["neuronsAfter"]);
    assert_eq!(ordering["synapsesBefore"], declared_synapses);
    assert_eq!(ordering["synapsesAfterHelper"], ordering["synapsesBefore"]);
    assert_eq!(ordering["synapsesAfterReload"], ordering["synapsesBefore"]);
    // Constant merge runs before the bridge call, so the export is smaller
    // than the file's neuron list. The bridge pass then removes nothing.
    let neurons_before = ordering["neuronsBefore"].as_u64().unwrap();
    assert!(neurons_before < declared_neurons);
    assert_eq!(neurons_before, 2263);
    assert_eq!(ordering["probeRows"], 25);
    assert_eq!(ordering["probe"], "sine");
    assert_eq!(ordering["worstOutputDelta"], 0);

    let exact = &cases[1];
    assert_eq!(exact["name"], "exactness_fixpoint_stays_forward");
    assert_eq!(exact["preparation"], "mergeToFixpoint");
    assert_eq!(exact["outputCheck"], "unseeded");
    assert_eq!(exact["backwardSynapses"], 0);
    assert_eq!(exact["removedNeurons"], 0);
    let neurons_before = exact["neuronsBefore"].as_u64().unwrap();
    let neurons_after = exact["neuronsAfter"].as_u64().unwrap();
    let synapses_before = exact["synapsesBefore"].as_u64().unwrap();
    let synapses_after = exact["synapsesAfter"].as_u64().unwrap();
    assert_eq!(neurons_before, input + declared_neurons);
    assert_eq!(synapses_before, declared_synapses);
    assert_eq!(
        synapses_after,
        synapses_before - (neurons_before - neurons_after)
    );
}
