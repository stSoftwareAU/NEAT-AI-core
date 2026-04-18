//! SIMD weighted-sum tests (moved from `src/simd.rs`).

use neat_core::network::SynapseData;
use neat_core::simd::{
    weighted_sum_no_bias_simd, weighted_sum_of_squares_simd, weighted_sum_of_squares_v2_simd,
    weighted_sum_simd, weighted_sum_simd_4records, weighted_sum_simd_8records,
};

/// Helper to create test synapse data
fn make_synapse(from_index: u32, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
        _padding: [0; 3],
    }
}

/// Helper to compute expected weighted sum via naive scalar loop
fn naive_weighted_sum(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let mut sum = bias;
    for synapse in synapses.iter().take(end).skip(start) {
        sum += activations[synapse.from_index as usize] * synapse.weight;
    }
    sum
}

#[test]
fn test_weighted_sum_empty() {
    let synapses: Vec<SynapseData> = vec![];
    let activations: Vec<f32> = vec![1.0, 2.0, 3.0];
    let result = weighted_sum_simd(&synapses, &activations, 0, 0, 0.5);
    assert!(
        (result - 0.5).abs() < 1e-6,
        "Empty synapse range should return bias"
    );
}

#[test]
fn test_weighted_sum_single_synapse() {
    let synapses = vec![make_synapse(1, 0.5)];
    let activations = vec![0.0, 2.0, 0.0];
    let result = weighted_sum_simd(&synapses, &activations, 0, 1, 1.0);
    // Expected: 1.0 + 2.0 * 0.5 = 2.0
    assert!(
        (result - 2.0).abs() < 1e-6,
        "Single synapse result mismatch: {result}"
    );
}

#[test]
fn test_weighted_sum_three_synapses() {
    // Exercises the scalar fallback path (count < 4)
    let synapses = vec![
        make_synapse(0, 1.0),
        make_synapse(1, 2.0),
        make_synapse(2, 3.0),
    ];
    let activations = vec![1.0, 2.0, 3.0];
    let result = weighted_sum_simd(&synapses, &activations, 0, 3, 0.0);
    // Expected: 1*1 + 2*2 + 3*3 = 1 + 4 + 9 = 14
    let expected = naive_weighted_sum(&synapses, &activations, 0, 3, 0.0);
    assert!(
        (result - expected).abs() < 1e-5,
        "3 synapses: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_exactly_four() {
    // Exercises the SIMD path with no remainder
    let synapses = vec![
        make_synapse(0, 0.5),
        make_synapse(1, -0.5),
        make_synapse(2, 1.0),
        make_synapse(3, -1.0),
    ];
    let activations = vec![2.0, 2.0, 3.0, 3.0];
    let result = weighted_sum_simd(&synapses, &activations, 0, 4, 0.1);
    let expected = naive_weighted_sum(&synapses, &activations, 0, 4, 0.1);
    assert!(
        (result - expected).abs() < 1e-5,
        "4 synapses: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_five_synapses() {
    // Exercises the SIMD path with 1 remainder
    let synapses: Vec<SynapseData> = (0..5)
        .map(|i| make_synapse(i, (i as f32 + 1.0) * 0.1))
        .collect();
    let activations: Vec<f32> = (0..5).map(|i| i as f32 * 0.5).collect();
    let result = weighted_sum_simd(&synapses, &activations, 0, 5, -0.3);
    let expected = naive_weighted_sum(&synapses, &activations, 0, 5, -0.3);
    assert!(
        (result - expected).abs() < 1e-4,
        "5 synapses: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_eight_synapses() {
    // Exercises the dual-accumulator path (exactly 8 = one chunk of 8)
    let synapses: Vec<SynapseData> = (0..8)
        .map(|i| make_synapse(i, (i as f32 - 3.5) * 0.2))
        .collect();
    let activations: Vec<f32> = (0..8).map(|i| (i as f32).sin()).collect();
    let result = weighted_sum_simd(&synapses, &activations, 0, 8, 0.0);
    let expected = naive_weighted_sum(&synapses, &activations, 0, 8, 0.0);
    assert!(
        (result - expected).abs() < 1e-4,
        "8 synapses: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_large() {
    // Test with many synapses to exercise both dual-accumulator and remainder
    let n = 25; // Typical average synapses per neuron in production
    let synapses: Vec<SynapseData> = (0..n)
        .map(|i| make_synapse(i % 10, ((i as f32) * 0.7).sin()))
        .collect();
    let activations: Vec<f32> = (0..10).map(|i| (i as f32 * 0.3).cos()).collect();
    let result = weighted_sum_simd(&synapses, &activations, 0, n as usize, 0.5);
    let expected = naive_weighted_sum(&synapses, &activations, 0, n as usize, 0.5);
    assert!(
        (result - expected).abs() < 1e-3,
        "25 synapses: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_with_offset() {
    // Test partial range (start > 0)
    let synapses: Vec<SynapseData> = (0..10)
        .map(|i| make_synapse(i % 5, i as f32 * 0.1))
        .collect();
    let activations: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let result = weighted_sum_simd(&synapses, &activations, 3, 9, 1.0);
    let expected = naive_weighted_sum(&synapses, &activations, 3, 9, 1.0);
    assert!(
        (result - expected).abs() < 1e-4,
        "Offset range: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_negative_weights_and_activations() {
    let synapses = vec![
        make_synapse(0, -1.5),
        make_synapse(1, 2.5),
        make_synapse(2, -0.5),
        make_synapse(3, 1.5),
    ];
    let activations = vec![-1.0, -2.0, 3.0, -4.0];
    let result = weighted_sum_simd(&synapses, &activations, 0, 4, 0.0);
    let expected = naive_weighted_sum(&synapses, &activations, 0, 4, 0.0);
    assert!(
        (result - expected).abs() < 1e-5,
        "Negative values: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_of_squares_empty() {
    let synapses: Vec<SynapseData> = vec![];
    let activations: Vec<f32> = vec![1.0];
    let result = weighted_sum_of_squares_simd(&synapses, &activations, 0, 0);
    assert!((result).abs() < 1e-6, "Empty should return 0");
}

#[test]
fn test_weighted_sum_of_squares_basic() {
    let synapses = vec![make_synapse(0, 2.0), make_synapse(1, 3.0)];
    let activations = vec![1.0, 2.0];
    let result = weighted_sum_of_squares_simd(&synapses, &activations, 0, 2);
    // (1*2)^2 + (2*3)^2 = 4 + 36 = 40
    assert!(
        (result - 40.0).abs() < 1e-4,
        "Sum of squares: got {result}, expected 40.0"
    );
}

#[test]
fn test_weighted_sum_of_squares_simd_path() {
    // 5 synapses: exercises SIMD (4) + scalar remainder (1)
    let synapses: Vec<SynapseData> = (0..5)
        .map(|i| make_synapse(i, (i as f32 + 1.0) * 0.5))
        .collect();
    let activations: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let result = weighted_sum_of_squares_simd(&synapses, &activations, 0, 5);

    let mut expected = 0.0f32;
    for i in 0..5 {
        let val = activations[i] * synapses[i].weight;
        expected += val * val;
    }
    assert!(
        (result - expected).abs() < 1e-3,
        "Sum of squares SIMD: got {result}, expected {expected}"
    );
}

#[test]
fn test_weighted_sum_no_bias_empty() {
    let synapses: Vec<SynapseData> = vec![];
    let activations: Vec<f32> = vec![1.0];
    let result = weighted_sum_no_bias_simd(&synapses, &activations, 0, 0);
    assert!((result).abs() < 1e-6, "Empty should return 0");
}

#[test]
fn test_weighted_sum_no_bias_basic() {
    let synapses = vec![
        make_synapse(0, 1.0),
        make_synapse(1, 2.0),
        make_synapse(2, 3.0),
        make_synapse(3, 4.0),
    ];
    let activations = vec![1.0, 1.0, 1.0, 1.0];
    let result = weighted_sum_no_bias_simd(&synapses, &activations, 0, 4);
    // 1*1 + 1*2 + 1*3 + 1*4 = 10
    assert!(
        (result - 10.0).abs() < 1e-5,
        "No bias sum: got {result}, expected 10.0"
    );
}

#[test]
fn test_weighted_sum_of_squares_v2_basic() {
    let synapses = vec![make_synapse(0, 1.0), make_synapse(1, 2.0)];
    let activations = vec![1.0, 2.0];
    let bias = 0.5;
    let result = weighted_sum_of_squares_v2_simd(&synapses, &activations, 0, 2, bias);
    // (0.5 + 1*1)^2 + (0.5 + 2*2)^2 = 1.5^2 + 4.5^2 = 2.25 + 20.25 = 22.5
    assert!(
        (result - 22.5).abs() < 1e-4,
        "Sum of squares V2: got {result}, expected 22.5"
    );
}

#[test]
fn test_weighted_sum_of_squares_v2_simd_path() {
    let synapses: Vec<SynapseData> = (0..5).map(|i| make_synapse(i, 1.0)).collect();
    let activations: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let bias = -1.0;
    let result = weighted_sum_of_squares_v2_simd(&synapses, &activations, 0, 5, bias);

    let mut expected = 0.0f32;
    for i in 0..5 {
        let val = bias + activations[i] * synapses[i].weight;
        expected += val * val;
    }
    assert!(
        (result - expected).abs() < 1e-3,
        "Sum of squares V2 SIMD: got {result}, expected {expected}"
    );
}

#[test]
fn test_4records_basic() {
    let synapses = vec![make_synapse(0, 1.0), make_synapse(1, 2.0)];
    let act0 = vec![1.0, 2.0];
    let act1 = vec![3.0, 4.0];
    let act2 = vec![5.0, 6.0];
    let act3 = vec![7.0, 8.0];
    let (r0, r1, r2, r3) =
        weighted_sum_simd_4records(&synapses, &act0, &act1, &act2, &act3, 0, 2, 0.5);

    // Record 0: 0.5 + 1*1 + 2*2 = 5.5
    assert!(
        (r0 - 5.5).abs() < 1e-5,
        "4records r0: got {r0}, expected 5.5"
    );
    // Record 1: 0.5 + 3*1 + 4*2 = 11.5
    assert!(
        (r1 - 11.5).abs() < 1e-5,
        "4records r1: got {r1}, expected 11.5"
    );
    // Record 2: 0.5 + 5*1 + 6*2 = 17.5
    assert!(
        (r2 - 17.5).abs() < 1e-5,
        "4records r2: got {r2}, expected 17.5"
    );
    // Record 3: 0.5 + 7*1 + 8*2 = 23.5
    assert!(
        (r3 - 23.5).abs() < 1e-5,
        "4records r3: got {r3}, expected 23.5"
    );
}

#[test]
fn test_4records_empty() {
    let synapses: Vec<SynapseData> = vec![];
    let act = vec![1.0];
    let (r0, r1, r2, r3) = weighted_sum_simd_4records(&synapses, &act, &act, &act, &act, 0, 0, 2.0);
    assert!((r0 - 2.0).abs() < 1e-6);
    assert!((r1 - 2.0).abs() < 1e-6);
    assert!((r2 - 2.0).abs() < 1e-6);
    assert!((r3 - 2.0).abs() < 1e-6);
}

#[test]
fn test_8records_basic() {
    let synapses = vec![make_synapse(0, 2.0)];
    let make_act = |v: f32| vec![v];
    let a0 = make_act(1.0);
    let a1 = make_act(2.0);
    let a2 = make_act(3.0);
    let a3 = make_act(4.0);
    let a4 = make_act(5.0);
    let a5 = make_act(6.0);
    let a6 = make_act(7.0);
    let a7 = make_act(8.0);
    let (r0, r1, r2, r3, r4, r5, r6, r7) =
        weighted_sum_simd_8records(&synapses, &a0, &a1, &a2, &a3, &a4, &a5, &a6, &a7, 0, 1, 0.0);
    // Each: bias(0) + activation * weight(2)
    assert!((r0 - 2.0).abs() < 1e-5);
    assert!((r1 - 4.0).abs() < 1e-5);
    assert!((r2 - 6.0).abs() < 1e-5);
    assert!((r3 - 8.0).abs() < 1e-5);
    assert!((r4 - 10.0).abs() < 1e-5);
    assert!((r5 - 12.0).abs() < 1e-5);
    assert!((r6 - 14.0).abs() < 1e-5);
    assert!((r7 - 16.0).abs() < 1e-5);
}

#[test]
fn test_8records_empty() {
    let synapses: Vec<SynapseData> = vec![];
    let act = vec![1.0];
    let (r0, r1, r2, r3, r4, r5, r6, r7) = weighted_sum_simd_8records(
        &synapses, &act, &act, &act, &act, &act, &act, &act, &act, 0, 0, 3.0,
    );
    for r in [r0, r1, r2, r3, r4, r5, r6, r7] {
        assert!((r - 3.0).abs() < 1e-6);
    }
}
