//! Original tag-preservation observations, not native rewrite parity (#776).
use serde_json::{Value, json};

#[test]
fn tag_captures_preserve_all_original_neuron_and_synapse_metadata() {
    let cases: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/compact_legacy_tags.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let names = [
        "complement_new",
        "complement_existing",
        "identity_chain",
        "orphan_constant",
        "bridge_identity_neurons",
        "bridge_complement_neurons",
        "bridge_synapses",
        "complement_output",
    ];
    assert_eq!(cases.len(), names.len());
    for name in names {
        let c = cases.iter().find(|c| c["name"] == name).expect(name);
        assert_eq!(c["revision"], "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab");
        assert_eq!(c["test"], "test/compact/CompactTagPreservation.ts");
        let input = &c["input"];
        let after = &c["after"];
        assert_eq!(after["input"], input["input"]);
        assert_eq!(after["output"], input["output"]);
        let mut output = input["neurons"].as_array().unwrap().last().unwrap().clone();
        let (neurons, edges) = match name {
            "complement_new" | "complement_existing" | "complement_output" => {
                assert_eq!(c["operation"], "compactCreature");
                assert_eq!(c["result"], "candidate");
                // COMPLEMENT(0.5*x) * 0.8 = 0.8 - 0.4*x.
                output["bias"] = json!(0.8);
                let mut edge = json!({"fromUUID":"input-0","toUUID":"output-0","weight":-0.4});
                match name {
                    "complement_new" => {
                        edge["tags"] = json!([{"name":"source","value":"training-data"},{"name":"layer","value":"output-link"}])
                    }
                    "complement_existing" => {
                        edge["weight"] = json!(0.3_f64 - 0.8 * 0.5);
                        edge["tags"] = json!([{"name":"path","value":"direct"},{"name":"path","value":"complement"}]);
                    }
                    "complement_output" => assert_eq!(after["neurons"][0]["id"], -1),
                    _ => unreachable!(),
                }
                (json!([output]), json!([edge]))
            }
            "identity_chain" => {
                assert_eq!(c["operation"], "compactCreature");
                assert_eq!(c["result"], "candidate");
                (
                    json!([input["neurons"][0], output]),
                    json!([
                        {"fromUUID":"input-0","toUUID":"hidden-0","weight":1},
                        {"fromUUID":"hidden-0","toUUID":"output-0","weight":1,"tags":[{"name":"chain","value":"bridge"},{"name":"layer","value":"to-output"}]}
                    ]),
                )
            }
            "orphan_constant" => {
                assert_eq!(c["operation"], "cleanupOrphanedNeurons");
                assert_eq!(c["result"], json!({"removed":0,"converted":1}));
                let mut constant = input["neurons"][0].clone();
                constant["type"] = json!("constant");
                constant.as_object_mut().unwrap().remove("squash");
                (json!([constant, output]), input["synapses"].clone())
            }
            "bridge_identity_neurons" | "bridge_complement_neurons" | "bridge_synapses" => {
                assert_eq!(c["operation"], "mergeParallelBridges");
                assert_eq!(c["result"], json!({"removedNeurons":1}));
                let complement = name == "bridge_complement_neurons";
                let mut kept = input["neurons"][1].clone();
                kept["squash"] = json!("IDENTITY");
                // Two unit outbound weights: COMPLEMENT contributes 1 + 1;
                // IDENTITY contributes zero. Both inbound signs flip for COMPLEMENT.
                kept["bias"] = json!(if complement { 2 } else { 0 });
                if name != "bridge_synapses" {
                    kept["tags"] = json!([
                        input["neurons"][0]["tags"][0],
                        input["neurons"][1]["tags"][0]
                    ]);
                }
                let mut edges = json!([
                    {"fromUUID":"input-0","toUUID":"bridge-B","weight":if complement {-0.5} else {0.5}},
                    {"fromUUID":"input-1","toUUID":"bridge-B","weight":if complement {-0.3} else {0.3}},
                    {"fromUUID":"bridge-B","toUUID":"output-0","weight":1}
                ]);
                if name == "bridge_synapses" {
                    edges[0]["tags"] = input["synapses"][0]["tags"].clone();
                    edges[1]["tags"] = input["synapses"][2]["tags"].clone();
                }
                let id = after["neurons"][0]["id"].as_i64().unwrap();
                for edge in after["synapses"].as_array().unwrap() {
                    if edge["fromUUID"] == "bridge-B" {
                        assert_eq!(edge["fromId"], id);
                        assert_eq!(edge["toId"], -1);
                    } else {
                        assert_eq!(edge["toId"], id);
                        assert_eq!(
                            edge["fromId"],
                            if edge["fromUUID"] == "input-0" { 0 } else { 1 }
                        );
                    }
                }
                (json!([kept, output]), edges)
            }
            _ => unreachable!(),
        };
        assert_eq!(
            canonical(&after["neurons"]),
            canonical(&neurons),
            "{name} neurons"
        );
        assert_eq!(
            canonical(&after["synapses"]),
            canonical(&edges),
            "{name} synapses"
        );
        if c["operation"] == "compactCreature" {
            assert_eq!(c["preparation"], "Creature.fromJSON");
            assert_eq!(c["feedbackLoop"], false);
        } else {
            assert_eq!(c["preparation"], "none");
            assert_eq!(c["input"], c["before"]);
            assert!(c["feedbackLoop"].is_null());
        }
        assert_eq!(
            c["outputNormalization"],
            if name == "complement_output" {
                "normaliseCreatureExport"
            } else {
                "none"
            }
        );
    }
}

// Tag and edge ordering is not asserted by these originals. Preserve multiplicity
// while comparing complete records, excluding only added integer identity handles.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Array(items) => {
            let mut items: Vec<_> = items.iter().map(canonical).collect();
            items.sort_by_key(Value::to_string);
            Value::Array(items)
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "id" | "fromId" | "toId"))
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}
