//! Predictive-coding learning / gradient tests (moved from `src/pc_learning.rs`).

use neat_core::pc_inference::{PcConnection, PcNeuron, PredictiveCodingEngine};
use neat_core::squash::SquashType;

/// Helper: simple network for gradient testing.
/// 1 input → 1 output (Identity, bias=0.5, weight=2.0).
fn make_gradient_network() -> PredictiveCodingEngine {
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

    PredictiveCodingEngine::new_from_parts(1, 1, neurons, connections, 0, 0.05, 1e-6)
}

#[test]
fn test_gradient_identity_squash() {
    // Identity: f'(x) = 1, so gradient = η · 1 · ε · x_source
    let engine = make_gradient_network();
    let latents = [3.0f32, 7.0]; // input=3.0, output_latent=7.0
    // prediction = identity(2.0 * 3.0 + 0.5) = 6.5
    // error = 7.0 - 6.5 = 0.5
    let errors = [0.5f32];
    let learning_rate = 0.01;

    let result = engine.compute_gradients(&latents, &errors, learning_rate);

    // Bias delta = 0.01 * 1.0 * 0.5 = 0.005
    assert!(
        (result.bias_deltas[0] - 0.005).abs() < 1e-6,
        "Bias delta should be 0.005, got {}",
        result.bias_deltas[0]
    );

    // Weight delta = 0.01 * 1.0 * 0.5 * 3.0 = 0.015
    assert_eq!(result.weight_deltas.len(), 1);
    let (_, _, delta) = result.weight_deltas[0];
    assert!(
        (delta - 0.015).abs() < 1e-6,
        "Weight delta should be 0.015, got {delta}"
    );
}

#[test]
fn test_gradient_zero_error() {
    // When error is zero, all gradients should be zero.
    let engine = make_gradient_network();
    let latents = [3.0f32, 6.5]; // prediction = 6.5, latent = 6.5 → error = 0
    let errors = [0.0f32];

    let result = engine.compute_gradients(&latents, &errors, 0.01);

    assert!(
        result.bias_deltas[0].abs() < 1e-7,
        "Bias delta should be 0 when error is 0"
    );
    for (_, _, delta) in &result.weight_deltas {
        assert!(
            delta.abs() < 1e-7,
            "Weight delta should be 0 when error is 0"
        );
    }
}

#[test]
fn test_gradient_tanh_squash() {
    // Tanh: f'(0) = 1, f'(1) ≈ 0.4200
    let neurons = vec![PcNeuron {
        bias: 0.0,
        squash_type: SquashType::Tanh,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    }];
    let connections = vec![PcConnection {
        from: 0,
        weight: 1.0,
    }];
    let engine = PredictiveCodingEngine::new_from_parts(1, 1, neurons, connections, 0, 0.05, 1e-6);

    // pre_activation = 1.0 * 1.0 + 0.0 = 1.0
    // f'(1.0) = 1 - tanh²(1.0) ≈ 0.4200
    let latents = [1.0f32, 0.5];
    let errors = [1.0f32]; // arbitrary error
    let learning_rate = 1.0; // use 1.0 to simplify checking

    let result = engine.compute_gradients(&latents, &errors, learning_rate);

    let tanh_1 = 1.0f32.tanh();
    let expected_derivative = 1.0 - tanh_1 * tanh_1;

    // Bias delta = 1.0 * f'(1.0) * 1.0 = f'(1.0)
    assert!(
        (result.bias_deltas[0] - expected_derivative).abs() < 1e-4,
        "Bias delta should be ~{expected_derivative}, got {}",
        result.bias_deltas[0]
    );

    // Weight delta = 1.0 * f'(1.0) * 1.0 * 1.0 (source_latent=1.0) = f'(1.0)
    let (_, _, delta) = result.weight_deltas[0];
    assert!(
        (delta - expected_derivative).abs() < 1e-4,
        "Weight delta should be ~{expected_derivative}, got {delta}"
    );
}

#[test]
fn test_gradient_relu_squash() {
    // ReLU: f'(x) = 1 for x > 0, 0 for x <= 0
    let neurons = vec![PcNeuron {
        bias: 0.0,
        squash_type: SquashType::Relu,
        is_hidden: false,
        conn_start: 0,
        conn_count: 1,
    }];
    let connections = vec![PcConnection {
        from: 0,
        weight: 1.0,
    }];
    let engine = PredictiveCodingEngine::new_from_parts(1, 1, neurons, connections, 0, 0.05, 1e-6);

    // Positive pre-activation: f'(2.0) = 1
    let latents_pos = [2.0f32, 1.0];
    let errors = [0.5f32];
    let result_pos = engine.compute_gradients(&latents_pos, &errors, 0.1);

    // delta = 0.1 * 1.0 * 0.5 * 2.0 = 0.1
    let (_, _, delta_pos) = result_pos.weight_deltas[0];
    assert!(
        (delta_pos - 0.1).abs() < 1e-6,
        "Positive ReLU weight delta should be 0.1, got {delta_pos}"
    );

    // Negative pre-activation: f'(-1.0) = 0 → all gradients zero
    let latents_neg = [-1.0f32, 0.0];
    let result_neg = engine.compute_gradients(&latents_neg, &errors, 0.1);

    let (_, _, delta_neg) = result_neg.weight_deltas[0];
    assert!(
        delta_neg.abs() < 1e-7,
        "Negative ReLU weight delta should be 0, got {delta_neg}"
    );
}

#[test]
fn test_gradient_multiple_connections() {
    // 2 inputs → 1 hidden → 1 output
    let neurons = vec![
        PcNeuron {
            bias: 0.1,
            squash_type: SquashType::Identity,
            is_hidden: true,
            conn_start: 0,
            conn_count: 2,
        },
        PcNeuron {
            bias: 0.0,
            squash_type: SquashType::Identity,
            is_hidden: false,
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

    let engine = PredictiveCodingEngine::new_from_parts(2, 1, neurons, connections, 0, 0.05, 1e-6);

    let latents = [1.0f32, 2.0, 0.8, 1.0];
    let errors = [0.1f32, 0.2]; // errors for hidden and output
    let learning_rate = 0.01;

    let result = engine.compute_gradients(&latents, &errors, learning_rate);

    // Should have 2 bias deltas (hidden + output)
    assert_eq!(result.bias_deltas.len(), 2);

    // Should have 3 weight deltas (2 for hidden, 1 for output)
    assert_eq!(result.weight_deltas.len(), 3);

    // All deltas should be finite
    for &bd in &result.bias_deltas {
        assert!(bd.is_finite(), "Bias delta should be finite");
    }
    for (_, _, delta) in &result.weight_deltas {
        assert!(delta.is_finite(), "Weight delta should be finite");
    }
}
