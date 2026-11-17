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

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use neat_core::{compile_creature, parse_creature_json};

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
