//! Creature JSON parsing and compilation tests (moved from `src/creature.rs`).

use neat_core::squash::apply_squash;
use neat_core::{
    SquashType, SynapseType, compile_creature, parse_creature_json, parse_squash_name,
    parse_synapse_type,
};

#[test]
fn test_parse_squash_names() {
    // Standard activations
    assert_eq!(parse_squash_name("IDENTITY").unwrap(), SquashType::Identity);
    assert_eq!(parse_squash_name("ReLU").unwrap(), SquashType::Relu);
    assert_eq!(parse_squash_name("RELU").unwrap(), SquashType::Relu);
    assert_eq!(parse_squash_name("TANH").unwrap(), SquashType::Tanh);
    assert_eq!(parse_squash_name("LOGISTIC").unwrap(), SquashType::Logistic);
    assert_eq!(parse_squash_name("SELU").unwrap(), SquashType::Selu);
    assert_eq!(parse_squash_name("ELU").unwrap(), SquashType::Elu);
    assert_eq!(
        parse_squash_name("LeakyReLU").unwrap(),
        SquashType::LeakyRelu
    );
    assert_eq!(parse_squash_name("ReLU6").unwrap(), SquashType::Relu6);
    assert_eq!(
        parse_squash_name("HARD_TANH").unwrap(),
        SquashType::HardTanh
    );
    assert_eq!(parse_squash_name("SOFTSIGN").unwrap(), SquashType::Softsign);
    assert_eq!(parse_squash_name("Softplus").unwrap(), SquashType::Softplus);
    assert_eq!(parse_squash_name("Swish").unwrap(), SquashType::Swish);
    assert_eq!(parse_squash_name("Mish").unwrap(), SquashType::Mish);
    assert_eq!(parse_squash_name("GELU").unwrap(), SquashType::Gelu);
    assert_eq!(parse_squash_name("SINE").unwrap(), SquashType::Sine);
    assert_eq!(parse_squash_name("Cosine").unwrap(), SquashType::Cosine);
    assert_eq!(parse_squash_name("TAN").unwrap(), SquashType::Tan);
    assert_eq!(parse_squash_name("ArcTan").unwrap(), SquashType::ArcTan);
    assert_eq!(parse_squash_name("GAUSSIAN").unwrap(), SquashType::Gaussian);
    assert_eq!(
        parse_squash_name("BENT_IDENTITY").unwrap(),
        SquashType::BentIdentity
    );
    assert_eq!(
        parse_squash_name("BIPOLAR_SIGMOID").unwrap(),
        SquashType::BipolarSigmoid
    );
    assert_eq!(parse_squash_name("BIPOLAR").unwrap(), SquashType::Bipolar);
    assert_eq!(parse_squash_name("STEP").unwrap(), SquashType::Step);
    assert_eq!(
        parse_squash_name("COMPLEMENT").unwrap(),
        SquashType::Complement
    );
    assert_eq!(parse_squash_name("ABSOLUTE").unwrap(), SquashType::Absolute);
    assert_eq!(parse_squash_name("SQUARE").unwrap(), SquashType::Square);
    assert_eq!(parse_squash_name("Cube").unwrap(), SquashType::Cube);
    assert_eq!(parse_squash_name("SQRT").unwrap(), SquashType::Sqrt);
    assert_eq!(
        parse_squash_name("StdInverse").unwrap(),
        SquashType::StdInverse
    );
    assert_eq!(
        parse_squash_name("Exponential").unwrap(),
        SquashType::Exponential
    );
    assert_eq!(
        parse_squash_name("LogSigmoid").unwrap(),
        SquashType::LogSigmoid
    );
    assert_eq!(parse_squash_name("ISRU").unwrap(), SquashType::Isru);

    // Aggregate functions
    assert_eq!(parse_squash_name("MINIMUM").unwrap(), SquashType::Minimum);
    assert_eq!(parse_squash_name("MAXIMUM").unwrap(), SquashType::Maximum);
    assert_eq!(parse_squash_name("IF").unwrap(), SquashType::If);

    // Deprecated
    assert_eq!(parse_squash_name("HYPOT").unwrap(), SquashType::Hypotenuse);
    assert_eq!(
        parse_squash_name("HYPOTv2").unwrap(),
        SquashType::HypotenuseV2
    );
    assert_eq!(parse_squash_name("MEAN").unwrap(), SquashType::Mean);

    // Aliases
    assert_eq!(parse_squash_name("CLIPPED").unwrap(), SquashType::HardTanh);
    assert_eq!(
        parse_squash_name("INVERSE").unwrap(),
        SquashType::Complement
    );
    assert_eq!(parse_squash_name("SINUSOID").unwrap(), SquashType::Sine);
}

#[test]
fn test_parse_squash_name_unknown() {
    assert!(parse_squash_name("NONEXISTENT").is_err());
}

#[test]
fn test_parse_synapse_types() {
    assert_eq!(parse_synapse_type(None), SynapseType::Standard);
    assert_eq!(parse_synapse_type(Some("positive")), SynapseType::Positive);
    assert_eq!(parse_synapse_type(Some("negative")), SynapseType::Negative);
    assert_eq!(
        parse_synapse_type(Some("condition")),
        SynapseType::Condition
    );
    // Unknown types default to Standard
    assert_eq!(parse_synapse_type(Some("unknown")), SynapseType::Standard);
}

#[test]
fn test_parse_minimal_creature_json() {
    let json = r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 0.5},
                {"fromUUID": "input-1", "toUUID": "output-0", "weight": -0.3}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    assert_eq!(creature.input, 2);
    assert_eq!(creature.output, 1);
    assert_eq!(creature.neurons.len(), 1);
    assert_eq!(creature.synapses.len(), 2);
}

#[test]
fn test_compile_minimal_creature() {
    let json = r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 0.5},
                {"fromUUID": "input-1", "toUUID": "output-0", "weight": -0.3}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    assert_eq!(network.num_neurons, 3);
    assert_eq!(network.num_inputs, 2);
    assert_eq!(network.neurons.len(), 1);
    assert_eq!(network.synapses.len(), 2);

    // Activate and verify: output = identity(0.5 * 1.0 + (-0.3) * 0.5 + 0.1) = 0.45
    let output = network.activate(&[1.0, 0.5], 1);
    let expected = 0.5 * 1.0 + (-0.3) * 0.5 + 0.1;
    assert!((output[0] - expected as f32).abs() < 1e-5);
}

#[test]
fn test_compile_creature_with_hidden_neurons() {
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "hidden", "uuid": "hidden-0", "bias": 0.0, "squash": "TANH"},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "hidden-0", "weight": 1.0},
                {"fromUUID": "hidden-0", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    let input_val: f32 = 0.5;
    let output = network.activate(&[input_val], 1);
    let expected = apply_squash(SquashType::Tanh, input_val);
    assert!((output[0] - expected).abs() < 1e-5);
}

#[test]
fn test_compile_creature_with_constant_neuron() {
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "constant", "uuid": "const-0", "bias": 0.75},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "const-0", "toUUID": "output-0", "weight": 2.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // Constant neuron has activation = bias = 0.75
    // Output = identity(0.75 * 2.0 + 0.0) = 1.5
    let output = network.activate(&[999.0], 1);
    assert!((output[0] - 1.5).abs() < 1e-5);
}

#[test]
fn test_compile_creature_no_hidden_neurons() {
    let json = r#"{
            "input": 2,
            "output": 2,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"},
                {"type": "output", "uuid": "output-1", "bias": -0.2, "squash": "LOGISTIC"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0},
                {"fromUUID": "input-1", "toUUID": "output-1", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    let output = network.activate(&[0.5, 0.0], 2);
    // output-0 = identity(0.5 * 1.0 + 0.1) = 0.6
    assert!((output[0] - 0.6).abs() < 1e-5);
    // output-1 = logistic(0.0 * 1.0 + (-0.2)) = logistic(-0.2)
    let expected_logistic = apply_squash(SquashType::Logistic, -0.2);
    assert!((output[1] - expected_logistic).abs() < 1e-5);
}

#[test]
fn test_compile_creature_with_if_neuron() {
    let json = r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "hidden", "uuid": "if-node", "bias": 0.0, "squash": "IF"},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "if-node", "weight": 1.0, "type": "condition"},
                {"fromUUID": "input-1", "toUUID": "if-node", "weight": 1.0, "type": "positive"},
                {"fromUUID": "input-1", "toUUID": "if-node", "weight": -1.0, "type": "negative"},
                {"fromUUID": "if-node", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // Condition input-0 = 1.0 > 0, so positive branch: input-1 * 1.0 = 3.0
    let output_positive = network.activate(&[1.0, 3.0], 1);
    assert!((output_positive[0] - 3.0).abs() < 1e-5);

    // Condition input-0 = -1.0 <= 0, so negative branch: input-1 * -1.0 = -3.0
    network.reset_state();
    let output_negative = network.activate(&[-1.0, 3.0], 1);
    assert!((output_negative[0] - (-3.0)).abs() < 1e-5);
}

#[test]
fn test_compile_creature_all_squash_types() {
    // Test that all squash function names can be parsed and used in a creature
    let squash_names = [
        "IDENTITY",
        "ReLU",
        "ReLU6",
        "LeakyReLU",
        "SELU",
        "ELU",
        "LOGISTIC",
        "TANH",
        "HARD_TANH",
        "SOFTSIGN",
        "Softplus",
        "Swish",
        "Mish",
        "GELU",
        "SINE",
        "Cosine",
        "TAN",
        "ArcTan",
        "GAUSSIAN",
        "BENT_IDENTITY",
        "BIPOLAR_SIGMOID",
        "BIPOLAR",
        "STEP",
        "COMPLEMENT",
        "ABSOLUTE",
        "SQUARE",
        "Cube",
        "SQRT",
        "StdInverse",
        "Exponential",
        "LogSigmoid",
        "ISRU",
    ];

    for name in &squash_names {
        let json = format!(
            r#"{{
                    "input": 1,
                    "output": 1,
                    "neurons": [
                        {{"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "{name}"}}
                    ],
                    "synapses": [
                        {{"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}}
                    ]
                }}"#
        );

        let creature = parse_creature_json(&json).unwrap();
        let mut network = compile_creature(&creature).unwrap();
        let output = network.activate(&[0.5], 1);

        // Verify output matches direct squash application
        let squash_type = parse_squash_name(name).unwrap();
        let expected = apply_squash(squash_type, 0.5);
        assert!(
            (output[0] - expected).abs() < 1e-5,
            "Squash {name}: got {}, expected {}",
            output[0],
            expected,
        );
    }
}

#[test]
fn test_compile_creature_synapse_types() {
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0, "type": "positive"}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let network = compile_creature(&creature).unwrap();
    assert_eq!(
        network.synapses[0].synapse_type,
        SynapseType::Positive as u8
    );
}

#[test]
fn test_compile_creature_default_squash() {
    // When squash is omitted, it should default to IDENTITY
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.5}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // IDENTITY(1.0 * 1.0 + 0.5) = 1.5
    let output = network.activate(&[1.0], 1);
    assert!((output[0] - 1.5).abs() < 1e-5);
}

#[test]
fn test_compile_creature_disconnected_neuron() {
    // A hidden neuron with no incoming synapses should just output its bias
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "hidden", "uuid": "disconnected", "bias": 0.42, "squash": "IDENTITY"},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "disconnected", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // disconnected neuron: identity(0 + 0.42) = 0.42, output: identity(0.42 * 1.0) = 0.42
    let output = network.activate(&[999.0], 1);
    assert!((output[0] - 0.42).abs() < 1e-5);
}

#[test]
fn test_compile_creature_output_count_mismatch() {
    let json = r#"{
            "input": 1,
            "output": 2,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": []
        }"#;

    let creature = parse_creature_json(json).unwrap();
    // Issue #115: compile_creature now returns a typed CreatureError; assert on
    // the Display message (preserved) rather than a bare String.
    let err = compile_creature(&creature).err().expect("should fail");
    assert!(err.to_string().contains("Expected 2 output neurons"));
}

#[test]
fn test_compile_creature_unknown_source_uuid() {
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "nonexistent", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    // Issue #115: typed CreatureError; assert on the preserved Display message.
    let err = compile_creature(&creature).err().expect("should fail");
    assert!(err.to_string().contains("Unknown source neuron UUID"));
}

#[test]
fn test_compile_creature_with_semantic_version() {
    let json = r#"{
            "input": 1,
            "output": 1,
            "semanticVersion": "1.0.0",
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": []
        }"#;

    let creature = parse_creature_json(json).unwrap();
    assert_eq!(creature.semantic_version.as_deref(), Some("1.0.0"));
}

#[test]
fn test_compile_creature_with_aggregate_minimum() {
    let json = r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "hidden", "uuid": "min-node", "bias": 0.0, "squash": "MINIMUM"},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "min-node", "weight": 1.0},
                {"fromUUID": "input-1", "toUUID": "min-node", "weight": 1.0},
                {"fromUUID": "min-node", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // MINIMUM(3.0 * 1.0, 1.0 * 1.0) + 0.0 = 1.0
    let output = network.activate(&[3.0, 1.0], 1);
    assert!((output[0] - 1.0).abs() < 1e-5);
}

#[test]
fn test_compile_creature_with_aggregate_maximum() {
    let json = r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "hidden", "uuid": "max-node", "bias": 0.0, "squash": "MAXIMUM"},
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "max-node", "weight": 1.0},
                {"fromUUID": "input-1", "toUUID": "max-node", "weight": 1.0},
                {"fromUUID": "max-node", "toUUID": "output-0", "weight": 1.0}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // MAXIMUM(3.0 * 1.0, 1.0 * 1.0) + 0.0 = 3.0
    let output = network.activate(&[3.0, 1.0], 1);
    assert!((output[0] - 3.0).abs() < 1e-5);
}

#[test]
fn test_parse_real_creature_json() {
    // Test with the real .a.json test data from the repository
    let json = r#"{
            "semanticVersion": "1.0.0",
            "neurons": [
                {"type": "hidden", "uuid": "a527d5a1-5cc8-4d1f-904b-b56c359cba71", "bias": 0.1, "squash": "LOGISTIC"},
                {"type": "hidden", "uuid": "dbc2b405-0277-4539-a3d6-2f943f79b752", "bias": -0.2, "squash": "LOGISTIC"},
                {"type": "hidden", "uuid": "0836f694-0cb2-4969-b29f-f8f787944f02", "bias": 0.3, "squash": "IF"},
                {"type": "output", "uuid": "output-0", "bias": -0.4, "squash": "LOGISTIC"},
                {"type": "output", "uuid": "output-1", "bias": 0.5, "squash": "LOGISTIC"}
            ],
            "synapses": [
                {"weight": 0.1, "fromUUID": "input-0", "toUUID": "a527d5a1-5cc8-4d1f-904b-b56c359cba71"},
                {"weight": -0.2, "fromUUID": "input-1", "toUUID": "dbc2b405-0277-4539-a3d6-2f943f79b752"},
                {"weight": 0.15, "fromUUID": "input-1", "toUUID": "0836f694-0cb2-4969-b29f-f8f787944f02", "type": "condition"},
                {"weight": 0.3, "fromUUID": "a527d5a1-5cc8-4d1f-904b-b56c359cba71", "toUUID": "0836f694-0cb2-4969-b29f-f8f787944f02", "type": "positive"},
                {"weight": -0.4, "fromUUID": "dbc2b405-0277-4539-a3d6-2f943f79b752", "toUUID": "0836f694-0cb2-4969-b29f-f8f787944f02", "type": "negative"},
                {"weight": -0.5, "fromUUID": "0836f694-0cb2-4969-b29f-f8f787944f02", "toUUID": "output-0"},
                {"weight": 0.6, "fromUUID": "0836f694-0cb2-4969-b29f-f8f787944f02", "toUUID": "output-1"}
            ],
            "input": 2,
            "output": 2
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    assert_eq!(network.num_neurons, 7);
    assert_eq!(network.num_inputs, 2);
    assert_eq!(network.neurons.len(), 5);
    assert_eq!(network.synapses.len(), 7);

    // Verify the network activates without error
    let output = network.activate(&[0.5, -0.3], 2);
    assert_eq!(output.len(), 2);
    // Outputs should be in valid logistic range [0, 1]
    assert!(output[0] >= 0.0 && output[0] <= 1.0);
    assert!(output[1] >= 0.0 && output[1] <= 1.0);
}

#[test]
fn test_compile_creature_large_network() {
    // Test with a larger network (many inputs, hidden layers)
    let mut neurons_json = String::new();
    let mut synapses_json = String::new();

    // 10 hidden neurons
    for i in 0..10 {
        if !neurons_json.is_empty() {
            neurons_json.push_str(", ");
        }
        neurons_json.push_str(&format!(
            r#"{{"type": "hidden", "uuid": "h-{i}", "bias": 0.01, "squash": "TANH"}}"#
        ));
    }
    // 2 output neurons
    for i in 0..2 {
        neurons_json.push_str(&format!(
            r#", {{"type": "output", "uuid": "o-{i}", "bias": 0.0, "squash": "LOGISTIC"}}"#
        ));
    }

    // Connect 5 inputs to hidden neurons
    for input_idx in 0..5 {
        for hidden_idx in 0..10 {
            if !synapses_json.is_empty() {
                synapses_json.push_str(", ");
            }
            synapses_json.push_str(&format!(
                r#"{{"fromUUID": "input-{input_idx}", "toUUID": "h-{hidden_idx}", "weight": 0.1}}"#
            ));
        }
    }
    // Connect hidden to outputs
    for hidden_idx in 0..10 {
        for output_idx in 0..2 {
            synapses_json.push_str(&format!(
                r#", {{"fromUUID": "h-{hidden_idx}", "toUUID": "o-{output_idx}", "weight": 0.05}}"#
            ));
        }
    }

    let json = format!(
        r#"{{
                "input": 5,
                "output": 2,
                "neurons": [{neurons_json}],
                "synapses": [{synapses_json}]
            }}"#
    );

    let creature = parse_creature_json(&json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    assert_eq!(network.num_neurons, 17); // 5 inputs + 10 hidden + 2 outputs
    assert_eq!(network.num_inputs, 5);
    assert_eq!(network.neurons.len(), 12);
    assert_eq!(network.synapses.len(), 70); // 5*10 + 10*2

    let output = network.activate(&[0.1, 0.2, 0.3, 0.4, 0.5], 2);
    assert_eq!(output.len(), 2);
    // Logistic output should be in [0, 1]
    assert!(output[0] >= 0.0 && output[0] <= 1.0);
    assert!(output[1] >= 0.0 && output[1] <= 1.0);
}

#[test]
fn test_compile_creature_single_output_no_synapses() {
    // Edge case: output neuron with no incoming connections
    let json = r#"{
            "input": 1,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.7, "squash": "IDENTITY"}
            ],
            "synapses": []
        }"#;

    let creature = parse_creature_json(json).unwrap();
    let mut network = compile_creature(&creature).unwrap();

    // Output = identity(0 + 0.7) = 0.7
    let output = network.activate(&[1.0], 1);
    assert!((output[0] - 0.7).abs() < 1e-5);
}

#[test]
fn test_parse_creature_json_ignores_extra_fields() {
    // The JSON may contain extra fields like forwardOnly, memetic, tags etc.
    let json = r#"{
            "input": 1,
            "output": 1,
            "forwardOnly": true,
            "tags": [{"name": "test"}],
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY", "frozen": true}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0, "frozen": true}
            ]
        }"#;

    let creature = parse_creature_json(json).unwrap();
    assert_eq!(creature.input, 1);
    let network = compile_creature(&creature).unwrap();
    assert_eq!(network.num_neurons, 2);
}

#[test]
fn test_compile_creature_deprecated_squash() {
    // Test deprecated aggregate functions
    for (name, expected_squash) in [
        ("HYPOT", SquashType::Hypotenuse),
        ("HYPOTv2", SquashType::HypotenuseV2),
        ("MEAN", SquashType::Mean),
    ] {
        let json = format!(
            r#"{{
                    "input": 2,
                    "output": 1,
                    "neurons": [
                        {{"type": "hidden", "uuid": "agg", "bias": 0.0, "squash": "{name}"}},
                        {{"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}}
                    ],
                    "synapses": [
                        {{"fromUUID": "input-0", "toUUID": "agg", "weight": 1.0}},
                        {{"fromUUID": "input-1", "toUUID": "agg", "weight": 1.0}},
                        {{"fromUUID": "agg", "toUUID": "output-0", "weight": 1.0}}
                    ]
                }}"#
        );

        let creature = parse_creature_json(&json).unwrap();
        let network = compile_creature(&creature).unwrap();
        assert_eq!(
            SquashType::from(network.neurons[0].squash_type),
            expected_squash,
            "Failed for deprecated squash: {name}"
        );
    }
}

/// Issue #177 - `SynapseData::from_index` is a u16, so a creature whose node count
/// exceeds `MAX_NODE_COUNT` must be rejected at compile time rather than silently
/// truncating source indices.
#[test]
fn compile_creature_rejects_too_many_nodes() {
    use neat_core::network::MAX_NODE_COUNT;
    use neat_core::{CreatureError, CreatureExport, NeuronExport};

    // One node over the limit, all hidden so the output count stays 0.
    let neurons = (0..=MAX_NODE_COUNT)
        .map(|i| NeuronExport {
            neuron_type: "hidden".to_string(),
            uuid: format!("n{i}"),
            bias: 0.0,
            squash: None,
        })
        .collect();

    let creature = CreatureExport {
        input: 0,
        output: 0,
        neurons,
        synapses: Vec::new(),
        semantic_version: None,
        forward_only: false,
    };

    match compile_creature(&creature) {
        Err(CreatureError::TooManyNodes { count }) => assert_eq!(count, MAX_NODE_COUNT + 1),
        Err(other) => panic!("expected TooManyNodes, got {other:?}"),
        Ok(_) => panic!("expected TooManyNodes error, creature compiled"),
    }
}
