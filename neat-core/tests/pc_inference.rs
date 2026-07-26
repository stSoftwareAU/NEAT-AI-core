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

// ---------------------------------------------------------------------------
// Issue #389 — differential equivalence of the scratch-buffer settling loop.
//
// The optimisation reuses working buffers across steps (and across samples in
// `infer_batch`) instead of allocating two `Vec`s per settling step. These
// tests pin the numerics to a standalone reference that reproduces the
// pre-change algorithm — fresh `Vec`s, per-edge `compute_pre_activation` — and
// assert `infer` is *bit-identical* to it over randomised multi-fan-in
// topologies, supervised and unsupervised, on both the converged-early and
// steps-exhausted paths. Any numeric drift (a mishoisted derivative, or stale
// scratch leaking between `infer_batch` samples) fails here.
// ---------------------------------------------------------------------------

use neat_core::derivative::apply_derivative;
use neat_core::squash::apply_squash;

/// Deterministic SplitMix64 PRNG so the randomised topologies are reproducible
/// without an `rand` dependency.
struct DiffRng {
    state: u64,
}

impl DiffRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[-1.0, 1.0)`.
    fn signed(&mut self) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        unit * 2.0 - 1.0
    }

    /// Uniform integer in `[0, bound)`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// A layered topology with multi-fan-in hidden neurons feeding downstream
/// neurons — so a target neuron reachable from several hidden sources exercises
/// the Gauss-Seidel aliasing the derivative recompute depends on.
fn random_topology(seed: u64) -> (usize, usize, Vec<PcNeuron>, Vec<PcConnection>) {
    const SQUASHES: [SquashType; 6] = [
        SquashType::Tanh,
        SquashType::Relu,
        SquashType::Logistic,
        SquashType::Softsign,
        SquashType::Swish,
        SquashType::Identity,
    ];

    let mut rng = DiffRng::new(seed);
    let num_inputs = 2 + rng.below(3); // 2..=4
    let hidden_layers = 1 + rng.below(3); // 1..=3
    let num_outputs = 1 + rng.below(2); // 1..=2

    let mut neurons: Vec<PcNeuron> = Vec::new();
    let mut connections: Vec<PcConnection> = Vec::new();
    let mut prev: Vec<usize> = (0..num_inputs).collect();
    let mut next_full = num_inputs;

    for _ in 0..hidden_layers {
        let size = 2 + rng.below(3); // 2..=4 per layer
        let mut this: Vec<usize> = Vec::with_capacity(size);
        for _ in 0..size {
            let conn_start = connections.len();
            let max_k = prev.len();
            // Prefer >= 2 sources so downstream targets get multiple hidden
            // inbound edges (the aliasing case).
            let k = if max_k <= 2 {
                max_k
            } else {
                2 + rng.below(max_k - 1)
            };
            for _ in 0..k {
                let src = prev[rng.below(prev.len())];
                connections.push(PcConnection {
                    from: src,
                    weight: rng.signed(),
                });
            }
            neurons.push(PcNeuron {
                bias: rng.signed() * 0.2,
                squash_type: SQUASHES[rng.below(SQUASHES.len())],
                is_hidden: true,
                conn_start,
                conn_count: connections.len() - conn_start,
            });
            this.push(next_full);
            next_full += 1;
        }
        prev = this;
    }

    for _ in 0..num_outputs {
        let conn_start = connections.len();
        let max_k = prev.len();
        let k = if max_k <= 1 {
            max_k
        } else {
            1 + rng.below(max_k)
        };
        for _ in 0..k {
            let src = prev[rng.below(prev.len())];
            connections.push(PcConnection {
                from: src,
                weight: rng.signed(),
            });
        }
        neurons.push(PcNeuron {
            bias: rng.signed() * 0.2,
            squash_type: SquashType::Identity,
            is_hidden: false,
            conn_start,
            conn_count: connections.len() - conn_start,
        });
    }

    (num_inputs, num_outputs, neurons, connections)
}

fn ref_prediction(
    neurons: &[PcNeuron],
    connections: &[PcConnection],
    rel: usize,
    latents: &[f32],
) -> f32 {
    let neuron = &neurons[rel];
    let mut sum = neuron.bias;
    for ci in neuron.conn_start..neuron.conn_start + neuron.conn_count {
        let c = &connections[ci];
        sum += c.weight * latents[c.from];
    }
    apply_squash(neuron.squash_type, sum)
}

fn ref_pre(neurons: &[PcNeuron], connections: &[PcConnection], rel: usize, latents: &[f32]) -> f32 {
    let neuron = &neurons[rel];
    let mut sum = neuron.bias;
    for ci in neuron.conn_start..neuron.conn_start + neuron.conn_count {
        let c = &connections[ci];
        sum += c.weight * latents[c.from];
    }
    sum
}

/// Standalone reproduction of the pre-change settling algorithm, allocating
/// fresh buffers each step and recomputing every pre-activation per edge.
#[allow(clippy::too_many_arguments)]
fn reference_infer(
    num_inputs: usize,
    num_outputs: usize,
    neurons: &[PcNeuron],
    connections: &[PcConnection],
    inference_steps: u32,
    inference_rate: f32,
    energy_threshold: f32,
    input: &[f32],
    targets: Option<&[f32]>,
) -> PcInferenceResult {
    let num_neurons = num_inputs + neurons.len();

    // Outward map (full source index -> list of (target full index, weight)).
    let mut outward: Vec<Vec<(usize, f32)>> = vec![Vec::new(); num_neurons];
    for (ni, neuron) in neurons.iter().enumerate() {
        let actual = num_inputs + ni;
        for ci in neuron.conn_start..neuron.conn_start + neuron.conn_count {
            let c = &connections[ci];
            outward[c.from].push((actual, c.weight));
        }
    }
    let hidden: Vec<usize> = neurons
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_hidden)
        .map(|(i, _)| num_inputs + i)
        .collect();

    let compute_errors = |latents: &[f32]| -> (Vec<f32>, Vec<f32>) {
        let n = neurons.len();
        let mut p = vec![0.0f32; n];
        let mut e = vec![0.0f32; n];
        for i in 0..n {
            let pred = ref_prediction(neurons, connections, i, latents);
            p[i] = pred;
            e[i] = latents[num_inputs + i] - pred;
        }
        (p, e)
    };
    let compute_energy = |errors: &[f32]| -> f32 {
        let mut s = 0.0f32;
        for &e in errors {
            s += e * e;
        }
        0.5 * s
    };

    let mut latents = vec![0.0f32; num_neurons];
    let input_len = input.len().min(num_inputs);
    latents[..input_len].copy_from_slice(&input[..input_len]);
    for i in 0..neurons.len() {
        latents[num_inputs + i] = ref_prediction(neurons, connections, i, &latents);
    }
    if let Some(tgt) = targets {
        let output_start = num_neurons - num_outputs;
        for j in 0..num_outputs.min(tgt.len()) {
            latents[output_start + j] = tgt[j];
        }
    }

    let mut energy_history = Vec::with_capacity(inference_steps as usize + 1);
    let mut converged = false;
    let mut steps_used = 0u32;

    let (mut predictions, mut errors) = compute_errors(&latents);
    let mut energy = compute_energy(&errors);
    energy_history.push(energy);

    for t in 0..inference_steps {
        steps_used = t + 1;
        if energy <= energy_threshold {
            converged = true;
            break;
        }
        for &hidden_idx in &hidden {
            let hidden_rel = hidden_idx - num_inputs;
            let mut gradient = errors[hidden_rel];
            for &(to, weight) in &outward[hidden_idx] {
                let target_rel = to - num_inputs;
                let target_error = errors[target_rel];
                let target_squash = neurons[target_rel].squash_type;
                let pre = ref_pre(neurons, connections, target_rel, &latents);
                let derivative = apply_derivative(target_squash, pre);
                gradient -= weight * target_error * derivative;
            }
            latents[hidden_idx] -= inference_rate * gradient;
        }
        latents[..input_len].copy_from_slice(&input[..input_len]);
        if let Some(tgt) = targets {
            let output_start = num_neurons - num_outputs;
            for j in 0..num_outputs.min(tgt.len()) {
                latents[output_start + j] = tgt[j];
            }
        }
        let (np, ne) = compute_errors(&latents);
        predictions = np;
        errors = ne;
        energy = compute_energy(&errors);
        energy_history.push(energy);
    }
    if !converged && energy <= energy_threshold {
        converged = true;
    }

    PcInferenceResult {
        latents,
        predictions,
        errors,
        final_energy: energy,
        energy_history,
        steps_used,
        converged,
    }
}

fn assert_bit_identical(label: &str, a: &PcInferenceResult, b: &PcInferenceResult) {
    assert_eq!(a.latents, b.latents, "{label}: latents differ");
    assert_eq!(a.predictions, b.predictions, "{label}: predictions differ");
    assert_eq!(a.errors, b.errors, "{label}: errors differ");
    assert_eq!(
        a.final_energy.to_bits(),
        b.final_energy.to_bits(),
        "{label}: final_energy differ ({} vs {})",
        a.final_energy,
        b.final_energy
    );
    assert_eq!(
        a.energy_history, b.energy_history,
        "{label}: energy_history differ"
    );
    assert_eq!(a.steps_used, b.steps_used, "{label}: steps_used differ");
    assert_eq!(a.converged, b.converged, "{label}: converged differ");
}

#[test]
fn infer_scratch_buffers_match_baseline() {
    let rate = 0.05f32;
    // (threshold, label): 0.0 forces the steps-exhausted path; a huge threshold
    // forces convergence on the first check (converged-early path).
    let regimes = [(0.0f32, "steps-exhausted"), (1e9f32, "converged-early")];

    for seed in 0..40u64 {
        let (num_inputs, num_outputs, neurons, connections) = random_topology(seed);
        let mut rng = DiffRng::new(seed ^ 0xDEAD_BEEF);
        let input: Vec<f32> = (0..num_inputs).map(|_| rng.signed()).collect();
        let targets: Vec<f32> = (0..num_outputs).map(|_| rng.signed()).collect();
        let steps = 30u32;

        for &(threshold, regime) in &regimes {
            for (sup_label, tgt) in [("unsupervised", None), ("supervised", Some(&targets[..]))] {
                let engine = PredictiveCodingEngine::new_from_parts(
                    num_inputs,
                    num_outputs,
                    neurons.clone(),
                    connections.clone(),
                    steps,
                    rate,
                    threshold,
                );
                let got = engine.infer(&input, tgt);
                let want = reference_infer(
                    num_inputs,
                    num_outputs,
                    &neurons,
                    &connections,
                    steps,
                    rate,
                    threshold,
                    &input,
                    tgt,
                );
                assert_bit_identical(
                    &format!("seed {seed} / {regime} / {sup_label}"),
                    &got,
                    &want,
                );
            }
        }
    }
}

#[test]
fn infer_batch_matches_per_sample_bit_identical() {
    // Reusing one scratch across samples must be byte-identical to independent
    // `infer` calls — a stale-buffer leak between samples would fail here.
    let (num_inputs, num_outputs, neurons, connections) = random_topology(777);
    let engine = PredictiveCodingEngine::new_from_parts(
        num_inputs,
        num_outputs,
        neurons,
        connections,
        25,
        0.05,
        0.0,
    );

    let mut rng = DiffRng::new(0x0BAD_F00D);
    // Deliberately vary input length, including one shorter than num_inputs, so
    // the latent-reset path (stale trailing inputs) is exercised across reuse.
    let samples: Vec<Vec<f32>> = (0..6)
        .map(|k| {
            let len = if k == 3 { num_inputs - 1 } else { num_inputs };
            (0..len).map(|_| rng.signed()).collect()
        })
        .collect();
    let refs: Vec<&[f32]> = samples.iter().map(|v| v.as_slice()).collect();

    let batch = engine.infer_batch(&refs, None);
    assert_eq!(batch.len(), samples.len());
    for (i, sample) in samples.iter().enumerate() {
        let single = engine.infer(sample, None);
        assert_bit_identical(&format!("batch sample {i}"), &batch[i], &single);
    }
}
