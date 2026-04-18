//! Predictive-coding inference tests (moved from `src/pc_inference.rs`).

use neat_core::pc_inference::{PcConnection, PcInferenceResult, PcNeuron, PredictiveCodingEngine};
use neat_core::squash::SquashType;

/// Helper: builds a simple network for testing.
/// 2 inputs → 1 hidden (Tanh, bias=0) → 1 output (Identity, bias=0).
fn make_simple_network(
    inference_steps: u32,
    inference_rate: f32,
    energy_threshold: f32,
) -> PredictiveCodingEngine {
    // Hidden neuron (index 2): connects from inputs 0 and 1
    // Output neuron (index 3): connects from hidden 2
    let neurons = vec![
        PcNeuron {
            bias: 0.0,
            squash_type: SquashType::Tanh,
            is_hidden: true,
            conn_start: 0,
            conn_count: 2,
        },
        PcNeuron {
            bias: 0.0,
            squash_type: SquashType::Identity,
            is_hidden: false, // output
            conn_start: 2,
            conn_count: 1,
        },
    ];

    let connections = vec![
        PcConnection {
            from: 0,
            weight: 0.5,
        },
        PcConnection {
            from: 1,
            weight: 0.3,
        },
        PcConnection {
            from: 2,
            weight: 1.0,
        },
    ];

    PredictiveCodingEngine::new_from_parts(
        2, // num_inputs
        1, // num_outputs
        neurons,
        connections,
        inference_steps,
        inference_rate,
        energy_threshold,
    )
}

/// Helper: network with 2 inputs → 2 hidden → 1 output.
fn make_deeper_network(
    inference_steps: u32,
    inference_rate: f32,
    energy_threshold: f32,
) -> PredictiveCodingEngine {
    // Hidden0 (idx 2): from input0 w=0.5, input1 w=-0.3
    // Hidden1 (idx 3): from input0 w=-0.4, input1 w=0.6
    // Output (idx 4): from hidden0 w=1.0, hidden1 w=-0.5
    let neurons = vec![
        PcNeuron {
            bias: 0.1,
            squash_type: SquashType::Relu,
            is_hidden: true,
            conn_start: 0,
            conn_count: 2,
        },
        PcNeuron {
            bias: -0.1,
            squash_type: SquashType::Relu,
            is_hidden: true,
            conn_start: 2,
            conn_count: 2,
        },
        PcNeuron {
            bias: 0.0,
            squash_type: SquashType::Identity,
            is_hidden: false,
            conn_start: 4,
            conn_count: 2,
        },
    ];

    let connections = vec![
        PcConnection {
            from: 0,
            weight: 0.5,
        },
        PcConnection {
            from: 1,
            weight: -0.3,
        },
        PcConnection {
            from: 0,
            weight: -0.4,
        },
        PcConnection {
            from: 1,
            weight: 0.6,
        },
        PcConnection {
            from: 2,
            weight: 1.0,
        },
        PcConnection {
            from: 3,
            weight: -0.5,
        },
    ];

    PredictiveCodingEngine::new_from_parts(
        2,
        1,
        neurons,
        connections,
        inference_steps,
        inference_rate,
        energy_threshold,
    )
}

#[test]
fn test_deterministic_inference() {
    // Same input must produce exactly the same output.
    let engine = make_simple_network(50, 0.05, 1e-6);
    let input = [1.0f32, 0.5];

    let result1 = engine.infer(&input, None);
    let result2 = engine.infer(&input, None);

    assert_eq!(
        result1.latents, result2.latents,
        "Latents must be deterministic"
    );
    assert_eq!(
        result1.final_energy, result2.final_energy,
        "Energy must be deterministic"
    );
    assert_eq!(
        result1.steps_used, result2.steps_used,
        "Steps must be deterministic"
    );
    assert_eq!(
        result1.converged, result2.converged,
        "Convergence must be deterministic"
    );
}

#[test]
fn test_input_clamping() {
    // Input neurons must remain clamped after inference.
    let engine = make_simple_network(50, 0.05, 1e-6);
    let input = [2.0f32, -1.5];

    let result = engine.infer(&input, None);

    assert_eq!(result.latents[0], 2.0, "Input 0 must remain clamped");
    assert_eq!(result.latents[1], -1.5, "Input 1 must remain clamped");
}

#[test]
fn test_target_clamping() {
    // Output neurons must be clamped to targets when provided.
    let engine = make_simple_network(50, 0.05, 1e-6);
    let input = [1.0f32, 0.5];
    let targets = [0.8f32];

    let result = engine.infer(&input, Some(&targets));

    // Output neuron is the last one (index 3)
    let output_idx = engine.num_neurons - 1;
    assert!(
        (result.latents[output_idx] - 0.8).abs() < 1e-6,
        "Output must be clamped to target, got {}",
        result.latents[output_idx]
    );
}

#[test]
fn test_energy_non_negative() {
    // Energy must always be non-negative.
    let engine = make_simple_network(50, 0.05, 1e-6);
    let input = [1.0f32, 0.5];

    let result = engine.infer(&input, None);

    assert!(result.final_energy >= 0.0, "Energy must be non-negative");
    for &e in &result.energy_history {
        assert!(e >= 0.0, "Energy at each step must be non-negative");
    }
}

#[test]
fn test_energy_convergence_with_targets() {
    // With targets, energy should generally decrease (or at least not increase
    // dramatically) as inference settles.
    let engine = make_simple_network(100, 0.05, 1e-10);
    let input = [1.0f32, 0.5];
    let targets = [0.5f32];

    let result = engine.infer(&input, Some(&targets));

    // Check that the final energy is less than or equal to the initial energy.
    let initial_energy = result.energy_history[0];
    assert!(
        result.final_energy <= initial_energy + 1e-5,
        "Final energy {} should not exceed initial energy {} significantly",
        result.final_energy,
        initial_energy
    );
}

#[test]
fn test_early_termination() {
    // With a high threshold, inference should stop early.
    let engine = make_simple_network(1000, 0.05, 1e6); // Very high threshold
    let input = [1.0f32, 0.5];

    let result = engine.infer(&input, None);

    // Should converge immediately since threshold is very high.
    assert!(result.converged, "Should converge with high threshold");
    // Initial energy is computed before any update, and convergence checked
    // at the start of each iteration, so steps_used should be 1.
    assert!(
        result.steps_used <= 1,
        "Should stop early, got {} steps",
        result.steps_used
    );
}

#[test]
fn test_zero_steps() {
    // With 0 inference steps, should just return initial state.
    let engine = make_simple_network(0, 0.05, 1e-6);
    let input = [1.0f32, 0.5];

    let result = engine.infer(&input, None);

    assert_eq!(result.steps_used, 0, "Should use 0 steps");
    // Energy history should have only the initial energy.
    assert_eq!(
        result.energy_history.len(),
        1,
        "Should have initial energy only"
    );
}

#[test]
fn test_prediction_error_computation() {
    // For a network with no hidden neurons (just input→output with identity),
    // the prediction should match the weighted sum + bias.
    let neurons = vec![PcNeuron {
        bias: 0.5,
        squash_type: SquashType::Identity,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    }];
    let connections = vec![PcConnection {
        from: 0,
        weight: 2.0,
    }];

    let engine = PredictiveCodingEngine::new_from_parts(1, 1, neurons, connections, 0, 0.05, 1e-6);

    let input = [3.0f32];
    let result = engine.infer(&input, None);

    // prediction = identity(2.0 * 3.0 + 0.5) = 6.5
    // latent was initialised to prediction, so error = 0
    assert!(
        (result.predictions[0] - 6.5).abs() < 1e-5,
        "Prediction should be 6.5, got {}",
        result.predictions[0]
    );
    assert!(
        result.errors[0].abs() < 1e-5,
        "Error should be ~0 for initialised latent, got {}",
        result.errors[0]
    );
}

#[test]
fn test_batch_matches_sequential() {
    // Batch inference must produce identical results to sequential inference.
    let engine = make_deeper_network(20, 0.05, 1e-6);

    let input0 = [1.0f32, 0.5];
    let input1 = [-1.0f32, 2.0];
    let input2 = [0.0f32, 0.0];
    let input3 = [3.0f32, -2.0];

    let seq_results: Vec<PcInferenceResult> = vec![
        engine.infer(&input0, None),
        engine.infer(&input1, None),
        engine.infer(&input2, None),
        engine.infer(&input3, None),
    ];

    let inputs: Vec<&[f32]> = vec![&input0, &input1, &input2, &input3];
    let batch_results = engine.infer_batch(&inputs, None);

    assert_eq!(batch_results.len(), 4, "Batch should return 4 results");

    for (i, (seq, batch)) in seq_results.iter().zip(batch_results.iter()).enumerate() {
        assert_eq!(
            seq.latents.len(),
            batch.latents.len(),
            "Record {i}: latent length mismatch"
        );
        for (j, (s, b)) in seq.latents.iter().zip(batch.latents.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, latent {j}: seq={s}, batch={b}"
            );
        }
        assert!(
            (seq.final_energy - batch.final_energy).abs() < 1e-5,
            "Record {i}: energy mismatch seq={}, batch={}",
            seq.final_energy,
            batch.final_energy
        );
        assert_eq!(
            seq.steps_used, batch.steps_used,
            "Record {i}: steps mismatch"
        );
    }
}

#[test]
fn test_deeper_network_settles() {
    // A deeper network should still settle and produce reasonable results.
    let engine = make_deeper_network(100, 0.05, 1e-6);
    let input = [1.0f32, 2.0];

    let result = engine.infer(&input, None);

    // Input neurons must remain clamped.
    assert_eq!(result.latents[0], 1.0);
    assert_eq!(result.latents[1], 2.0);

    // Energy must be non-negative.
    assert!(result.final_energy >= 0.0);

    // All predictions and errors should be finite.
    for &p in &result.predictions {
        assert!(p.is_finite(), "Prediction must be finite, got {p}");
    }
    for &e in &result.errors {
        assert!(e.is_finite(), "Error must be finite, got {e}");
    }
}

#[test]
fn test_energy_history_length() {
    // Energy history should have initial + one entry per completed step.
    let engine = make_simple_network(10, 0.05, 1e-10); // Low threshold so it won't converge early
    let input = [1.0f32, 0.5];
    let targets = [0.5f32];

    let result = engine.infer(&input, Some(&targets));

    // history starts with 1 (initial), then adds 1 per step
    // If converged, history length = steps_used + 1 (might be less than 10+1)
    // If not converged, history length = inference_steps + 1
    assert!(
        result.energy_history.len() >= 2,
        "Energy history should have at least 2 entries"
    );
    assert!(
        result.energy_history.len() <= 11,
        "Energy history should have at most 11 entries (10 steps + initial)"
    );
}

#[test]
fn test_various_squash_functions() {
    // Test with different squash functions.
    let squash_types = [
        SquashType::Identity,
        SquashType::Relu,
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::LeakyRelu,
    ];

    for &squash in &squash_types {
        let neurons = vec![
            PcNeuron {
                bias: 0.1,
                squash_type: squash,
                is_hidden: true,
                conn_start: 0,
                conn_count: 1,
            },
            PcNeuron {
                bias: 0.0,
                squash_type: SquashType::Identity,
                is_hidden: false,
                conn_start: 1,
                conn_count: 1,
            },
        ];
        let connections = vec![
            PcConnection {
                from: 0,
                weight: 1.0,
            },
            PcConnection {
                from: 2,
                weight: 1.0,
            },
        ];

        let engine =
            PredictiveCodingEngine::new_from_parts(2, 1, neurons, connections, 20, 0.05, 1e-6);

        let result = engine.infer(&[1.0, 0.5], None);

        assert!(
            result.final_energy.is_finite(),
            "Energy must be finite for {:?}",
            squash
        );
        for &l in &result.latents {
            assert!(l.is_finite(), "Latent must be finite for {:?}", squash);
        }
    }
}

#[test]
fn test_no_hidden_neurons() {
    // Network with only input→output (no hidden neurons to update).
    let neurons = vec![PcNeuron {
        bias: 0.0,
        squash_type: SquashType::Identity,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    }];
    let connections = vec![PcConnection {
        from: 0,
        weight: 1.0,
    }];

    let engine = PredictiveCodingEngine::new_from_parts(1, 1, neurons, connections, 50, 0.05, 1e-6);

    let result = engine.infer(&[2.0], None);

    // With no hidden neurons, the latent for the output should be the prediction.
    // prediction = identity(1.0 * 2.0 + 0.0) = 2.0, error = 0.
    assert!(
        (result.latents[1] - 2.0).abs() < 1e-5,
        "Output latent should be 2.0, got {}",
        result.latents[1]
    );
    assert!(
        result.final_energy < 1e-5,
        "Energy should be ~0 with no hidden neurons, got {}",
        result.final_energy
    );
}

#[test]
fn test_wasm_serialisation_roundtrip() {
    // Test that the WASM binary serialisation format works correctly.
    let mut data: Vec<u8> = Vec::new();

    // Header: num_inputs=2, num_outputs=1, num_neurons=4, steps=10, rate=0.05, threshold=1e-6
    data.extend_from_slice(&2u32.to_le_bytes()); // num_inputs
    data.extend_from_slice(&1u32.to_le_bytes()); // num_outputs
    data.extend_from_slice(&4u32.to_le_bytes()); // num_neurons_total
    data.extend_from_slice(&10u32.to_le_bytes()); // inference_steps
    data.extend_from_slice(&0.05f32.to_le_bytes()); // inference_rate
    data.extend_from_slice(&1e-6f32.to_le_bytes()); // energy_threshold

    // Neuron 0 (hidden, Tanh): bias=0.0, squash=7(Tanh), is_hidden=1, 2 connections
    data.extend_from_slice(&0.0f32.to_le_bytes());
    data.push(7); // Tanh
    data.push(1); // is_hidden
    data.extend_from_slice(&2u16.to_le_bytes());
    // Connection from input 0, weight 0.5
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0.5f32.to_le_bytes());
    // Connection from input 1, weight 0.3
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0.3f32.to_le_bytes());

    // Neuron 1 (output, Identity): bias=0.0, squash=0(Identity), is_hidden=0, 1 connection
    data.extend_from_slice(&0.0f32.to_le_bytes());
    data.push(0); // Identity
    data.push(0); // is_hidden = false (output)
    data.extend_from_slice(&1u16.to_le_bytes());
    // Connection from hidden 0 (full index 2), weight 1.0
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&1.0f32.to_le_bytes());

    let engine = PredictiveCodingEngine::new(&data).expect("Should parse successfully");

    assert_eq!(engine.num_inputs, 2);
    assert_eq!(engine.num_outputs, 1);
    assert_eq!(engine.num_neurons, 4);

    // Run inference to verify it works.
    let result = engine.infer(&[1.0, 0.5], None);
    assert!(result.final_energy.is_finite());
    assert_eq!(result.latents.len(), 4);
}
