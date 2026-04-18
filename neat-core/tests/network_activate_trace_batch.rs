//! Compiled network activation / trace batch tests (moved from `src/network.rs`).

use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Helper to build a CompiledNetwork directly for testing
fn make_network(
    num_inputs: usize,
    neurons: Vec<NeuronData>,
    synapses: Vec<SynapseData>,
) -> CompiledNetwork {
    let num_neurons = num_inputs + neurons.len();
    let num_non_inputs = neurons.len();
    let estimated_trace_size = (num_non_inputs / 10).max(1) * 2 + 1;
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::with_capacity(estimated_trace_size),
    }
}

fn make_synapse(from_index: u32, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
        _padding: [0; 3],
    }
}

fn make_synapse_typed(from_index: u32, weight: f32, synapse_type: u8) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type,
        _padding: [0; 3],
    }
}

/// Test that batch 4-way matches single-record activate_and_trace for standard squash (ReLU)
#[test]
fn test_batch_4way_matches_single_relu() {
    // Network: 2 inputs, 1 hidden (ReLU), 1 output (Identity)
    let synapses = vec![
        make_synapse(0, 0.5),  // hidden <- input0
        make_synapse(1, -0.3), // hidden <- input1
        make_synapse(2, 1.0),  // output <- hidden
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.1,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: 1, // ReLU
            is_constant: false,
        },
        NeuronData {
            bias: -0.2,
            start_synapse: 2,
            num_synapses: 1,
            squash_type: 0, // Identity
            is_constant: false,
        },
    ];

    let inputs: [&[f32]; 4] = [&[1.0, 2.0], &[0.5, -1.0], &[-2.0, 3.0], &[0.0, 0.0]];

    // Run single-record activate_and_trace for each
    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    // Run batch 4-way
    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed_input: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed_input, 2, 1);

    // Parse batch result - first 4 values are record lengths
    let len0 = batch_result[0] as usize;
    let len1 = batch_result[1] as usize;
    let len2 = batch_result[2] as usize;
    let len3 = batch_result[3] as usize;

    let start0 = 4;
    let start1 = start0 + len0;
    let start2 = start1 + len1;
    let start3 = start2 + len2;

    let batch_records = [
        &batch_result[start0..start0 + len0],
        &batch_result[start1..start1 + len1],
        &batch_result[start2..start2 + len2],
        &batch_result[start3..start3 + len3],
    ];

    // Compare each batch record with its single-record counterpart
    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(
            single.len(),
            batch.len(),
            "Record {i}: length mismatch (single={}, batch={})",
            single.len(),
            batch.len()
        );
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with TANH and LOGISTIC squash functions
#[test]
fn test_batch_4way_matches_single_tanh_logistic() {
    let synapses = vec![
        make_synapse(0, 1.0),
        make_synapse(1, 0.5),
        make_synapse(2, -0.7),
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.0,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: 7, // TANH
            is_constant: false,
        },
        NeuronData {
            bias: 0.5,
            start_synapse: 2,
            num_synapses: 1,
            squash_type: 6, // LOGISTIC
            is_constant: false,
        },
    ];

    let inputs: [&[f32]; 4] = [&[1.0, 0.5], &[-1.0, 2.0], &[0.3, -0.3], &[2.0, -1.0]];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 2, 1);

    let len0 = batch_result[0] as usize;
    let len1 = batch_result[1] as usize;
    let len2 = batch_result[2] as usize;

    let start0 = 4;
    let start1 = start0 + len0;
    let start2 = start1 + len1;
    let start3 = start2 + len2;

    let batch_records = [
        &batch_result[start0..start0 + len0],
        &batch_result[start1..start1 + len1],
        &batch_result[start2..start2 + len0],
        &batch_result[start3..start3 + batch_result[3] as usize],
    ];

    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(single.len(), batch.len(), "Record {i}: length mismatch");
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with MINIMUM aggregate function
#[test]
fn test_batch_4way_minimum_aggregate() {
    // 2 inputs -> 1 MINIMUM neuron (output)
    let synapses = vec![make_synapse(0, 1.0), make_synapse(1, 1.0)];
    let neurons = vec![NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 2,
        squash_type: 32, // MINIMUM
        is_constant: false,
    }];

    let inputs: [&[f32]; 4] = [
        &[3.0, 1.0],  // min = 1.0
        &[-1.0, 2.0], // min = -1.0
        &[5.0, 5.0],  // min = 5.0
        &[0.0, -3.0], // min = -3.0
    ];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 2, 1);

    let len0 = batch_result[0] as usize;
    let len1 = batch_result[1] as usize;
    let len2 = batch_result[2] as usize;

    let start0 = 4;
    let start1 = start0 + len0;
    let start2 = start1 + len1;
    let start3 = start2 + len2;

    let batch_records = [
        &batch_result[start0..start0 + len0],
        &batch_result[start1..start1 + len1],
        &batch_result[start2..start2 + len2],
        &batch_result[start3..start3 + batch_result[3] as usize],
    ];

    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(single.len(), batch.len(), "Record {i}: length mismatch");
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with MAXIMUM aggregate function
#[test]
fn test_batch_4way_maximum_aggregate() {
    let synapses = vec![make_synapse(0, 1.0), make_synapse(1, 1.0)];
    let neurons = vec![NeuronData {
        bias: 0.5,
        start_synapse: 0,
        num_synapses: 2,
        squash_type: 33, // MAXIMUM
        is_constant: false,
    }];

    let inputs: [&[f32]; 4] = [&[3.0, 1.0], &[-1.0, 2.0], &[5.0, 5.0], &[0.0, -3.0]];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 2, 1);

    let len0 = batch_result[0] as usize;
    let len1 = batch_result[1] as usize;
    let len2 = batch_result[2] as usize;

    let start0 = 4;
    let start1 = start0 + len0;
    let start2 = start1 + len1;
    let start3 = start2 + len2;

    let batch_records = [
        &batch_result[start0..start0 + len0],
        &batch_result[start1..start1 + len1],
        &batch_result[start2..start2 + len2],
        &batch_result[start3..start3 + batch_result[3] as usize],
    ];

    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(single.len(), batch.len(), "Record {i}: length mismatch");
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with IF aggregate function
#[test]
fn test_batch_4way_if_aggregate() {
    // 3 inputs -> 1 IF neuron
    // synapse0: condition, synapse1: positive, synapse2: negative
    let synapses = vec![
        make_synapse_typed(0, 1.0, 1), // condition
        make_synapse_typed(1, 1.0, 3), // positive
        make_synapse_typed(2, 1.0, 2), // negative
    ];
    let neurons = vec![NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 3,
        squash_type: 34, // IF
        is_constant: false,
    }];

    let inputs: [&[f32]; 4] = [
        &[1.0, 5.0, 10.0],  // condition>0 -> positive=5.0
        &[-1.0, 5.0, 10.0], // condition<=0 -> negative=10.0
        &[0.5, 3.0, 7.0],   // condition>0 -> positive=3.0
        &[-2.0, 3.0, 7.0],  // condition<=0 -> negative=7.0
    ];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(3, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(3, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 3, 1);

    let len0 = batch_result[0] as usize;
    let len1 = batch_result[1] as usize;
    let len2 = batch_result[2] as usize;

    let start0 = 4;
    let start1 = start0 + len0;
    let start2 = start1 + len1;
    let start3 = start2 + len2;

    let batch_records = [
        &batch_result[start0..start0 + len0],
        &batch_result[start1..start1 + len1],
        &batch_result[start2..start2 + len2],
        &batch_result[start3..start3 + batch_result[3] as usize],
    ];

    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(single.len(), batch.len(), "Record {i}: length mismatch");
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with constant neurons
#[test]
fn test_batch_4way_constant_neuron() {
    let synapses = vec![
        make_synapse(2, 1.0), // output <- constant
    ];
    let neurons = vec![
        NeuronData {
            bias: 42.0,
            start_synapse: 0,
            num_synapses: 0,
            squash_type: 0,
            is_constant: true,
        },
        NeuronData {
            bias: 0.0,
            start_synapse: 0,
            num_synapses: 1,
            squash_type: 0, // Identity
            is_constant: false,
        },
    ];

    let inputs: [&[f32]; 4] = [&[1.0, 2.0], &[3.0, 4.0], &[5.0, 6.0], &[7.0, 8.0]];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 2, 1);

    // All 4 records should produce the same output (constant 42.0 passed through identity)
    for r in 0..4 {
        let start = 4 + (0..r).map(|i| batch_result[i] as usize).sum::<usize>();
        let len = batch_result[r] as usize;
        let batch_record = &batch_result[start..start + len];
        let single = &single_results[r];

        assert_eq!(
            single.len(),
            batch_record.len(),
            "Record {r}: length mismatch"
        );
        for (j, (s, b)) in single.iter().zip(batch_record.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {r}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// Test batch 4-way with a deeper network (multiple layers)
#[test]
fn test_batch_4way_multi_layer() {
    // 2 inputs -> 2 hidden (ReLU) -> 1 output (Identity)
    let synapses = vec![
        // Hidden 0 (idx 2): from input 0 and 1
        make_synapse(0, 0.5),
        make_synapse(1, 0.3),
        // Hidden 1 (idx 3): from input 0 and 1
        make_synapse(0, -0.4),
        make_synapse(1, 0.6),
        // Output (idx 4): from hidden 0 and hidden 1
        make_synapse(2, 1.0),
        make_synapse(3, -0.5),
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.1,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: 1, // ReLU
            is_constant: false,
        },
        NeuronData {
            bias: -0.1,
            start_synapse: 2,
            num_synapses: 2,
            squash_type: 1, // ReLU
            is_constant: false,
        },
        NeuronData {
            bias: 0.0,
            start_synapse: 4,
            num_synapses: 2,
            squash_type: 0, // Identity
            is_constant: false,
        },
    ];

    let inputs: [&[f32]; 4] = [&[1.0, 2.0], &[-1.0, 0.5], &[3.0, -2.0], &[0.0, 0.0]];

    let mut single_results = Vec::new();
    for input in &inputs {
        let mut net = make_network(2, neurons.clone(), synapses.clone());
        let result = net.activate_and_trace(input, 1);
        single_results.push(result);
    }

    let net = make_network(2, neurons.clone(), synapses.clone());
    let packed: Vec<f32> = inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, 2, 1);

    for r in 0..4 {
        let start = 4 + (0..r).map(|i| batch_result[i] as usize).sum::<usize>();
        let len = batch_result[r] as usize;
        let batch_record = &batch_result[start..start + len];
        let single = &single_results[r];

        assert_eq!(
            single.len(),
            batch_record.len(),
            "Record {r}: length mismatch"
        );
        for (j, (s, b)) in single.iter().zip(batch_record.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "Record {r}, element {j}: single={s}, batch={b}"
            );
        }
    }
}
