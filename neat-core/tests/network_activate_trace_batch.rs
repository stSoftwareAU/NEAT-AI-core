//! Compiled network activation / trace batch tests (moved from `src/network.rs`).
//!
//! Issue #484 — this file is the sole home of the `activate_and_trace_batch_4way`
//! parity tests; the duplicate copy in `src/network.rs` `mod tests` is gone. New
//! batch-parity cases belong here, exercised through the public API, so the trace
//! header layout only ever has to be updated in one place.

use neat_core::{CompiledNetwork, NeuronData, SynapseData};

/// Records per `activate_and_trace_batch_4way` call — also the header length.
const BATCH_RECORDS: usize = 4;

/// Every fixture below has a single output neuron.
const NUM_OUTPUTS: usize = 1;

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
        // Issue #155 - 4-way batch scratch buffers
        batch_activations: [
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
            vec![0.0; num_neurons],
        ],
        batch_hints: [
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
            vec![0.0; num_non_inputs],
        ],
        batch_traces: [
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
            Vec::with_capacity(estimated_trace_size),
        ],
        // NEAT-AI-scorer#531 — fused MSE interleaved scratch.
        mse_inter: vec![0.0; num_neurons * 8],
    }
}

fn make_synapse(from_index: u16, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
    }
}

fn make_synapse_typed(from_index: u16, weight: f32, synapse_type: u8) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type,
    }
}

/// Split the packed `activate_and_trace_batch_4way` output into its records.
///
/// The first [`BATCH_RECORDS`] values are the per-record lengths; the records
/// follow back to back. Each `start` is accumulated from the preceding lengths
/// as the header is walked, so a record is always sliced with **its own**
/// length. The final offset must land exactly on the end of the buffer — a
/// header that under- or over-covers the payload is a failure, not a silent
/// truncation.
fn split_batch_records(batch_result: &[f32]) -> Vec<&[f32]> {
    assert!(
        batch_result.len() >= BATCH_RECORDS,
        "batch result is shorter than its {BATCH_RECORDS}-value length header"
    );

    let mut records = Vec::with_capacity(BATCH_RECORDS);
    let mut start = BATCH_RECORDS;
    for (i, &len_value) in batch_result[..BATCH_RECORDS].iter().enumerate() {
        let len = len_value as usize;
        assert!(
            start + len <= batch_result.len(),
            "record {i}: header length {len} overruns the batch buffer"
        );
        records.push(&batch_result[start..start + len]);
        start += len;
    }
    assert_eq!(
        start,
        batch_result.len(),
        "header lengths must cover the whole batch buffer exactly"
    );
    records
}

/// One batch-parity fixture: a network plus the four input records to run.
struct ParityCase {
    name: &'static str,
    num_inputs: usize,
    neurons: Vec<NeuronData>,
    synapses: Vec<SynapseData>,
    inputs: [&'static [f32]; BATCH_RECORDS],
}

/// The parity contract: every record of the 4-way batch must equal the
/// single-record `activate_and_trace` result for the same inputs.
fn assert_batch_matches_single(case: &ParityCase) {
    let name = case.name;

    // Run single-record activate_and_trace for each input, on a fresh network.
    let single_results: Vec<Vec<f32>> = case
        .inputs
        .iter()
        .map(|input| {
            let mut net =
                make_network(case.num_inputs, case.neurons.clone(), case.synapses.clone());
            net.activate_and_trace(input, NUM_OUTPUTS)
        })
        .collect();

    // Run the same inputs as one packed 4-way batch.
    let mut net = make_network(case.num_inputs, case.neurons.clone(), case.synapses.clone());
    let packed: Vec<f32> = case.inputs.iter().flat_map(|i| i.iter().copied()).collect();
    let batch_result = net.activate_and_trace_batch_4way(&packed, case.num_inputs, NUM_OUTPUTS);

    let batch_records = split_batch_records(&batch_result);

    for (i, (single, batch)) in single_results.iter().zip(batch_records.iter()).enumerate() {
        assert_eq!(
            single.len(),
            batch.len(),
            "{name}, record {i}: length mismatch (single={}, batch={})",
            single.len(),
            batch.len()
        );
        for (j, (s, b)) in single.iter().zip(batch.iter()).enumerate() {
            assert!(
                (s - b).abs() < 1e-5,
                "{name}, record {i}, element {j}: single={s}, batch={b}"
            );
        }
    }
}

/// The fixtures the parity contract is checked against — one per activation
/// path the 4-way batch has to get right.
fn parity_cases() -> Vec<ParityCase> {
    vec![
        // Standard squash: 2 inputs, 1 hidden (ReLU), 1 output (Identity).
        ParityCase {
            name: "relu_hidden_identity_output",
            num_inputs: 2,
            synapses: vec![
                make_synapse(0, 0.5),  // hidden <- input0
                make_synapse(1, -0.3), // hidden <- input1
                make_synapse(2, 1.0),  // output <- hidden
            ],
            neurons: vec![
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
            ],
            inputs: [&[1.0, 2.0], &[0.5, -1.0], &[-2.0, 3.0], &[0.0, 0.0]],
        },
        // TANH hidden into a LOGISTIC output.
        ParityCase {
            name: "tanh_hidden_logistic_output",
            num_inputs: 2,
            synapses: vec![
                make_synapse(0, 1.0),
                make_synapse(1, 0.5),
                make_synapse(2, -0.7),
            ],
            neurons: vec![
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
            ],
            inputs: [&[1.0, 0.5], &[-1.0, 2.0], &[0.3, -0.3], &[2.0, -1.0]],
        },
        // MINIMUM aggregate: 2 inputs -> 1 MINIMUM neuron (output).
        ParityCase {
            name: "minimum_aggregate",
            num_inputs: 2,
            synapses: vec![make_synapse(0, 1.0), make_synapse(1, 1.0)],
            neurons: vec![NeuronData {
                bias: 0.0,
                start_synapse: 0,
                num_synapses: 2,
                squash_type: 32, // MINIMUM
                is_constant: false,
            }],
            inputs: [
                &[3.0, 1.0],  // min = 1.0
                &[-1.0, 2.0], // min = -1.0
                &[5.0, 5.0],  // min = 5.0
                &[0.0, -3.0], // min = -3.0
            ],
        },
        // MAXIMUM aggregate, with a non-zero bias.
        ParityCase {
            name: "maximum_aggregate",
            num_inputs: 2,
            synapses: vec![make_synapse(0, 1.0), make_synapse(1, 1.0)],
            neurons: vec![NeuronData {
                bias: 0.5,
                start_synapse: 0,
                num_synapses: 2,
                squash_type: 33, // MAXIMUM
                is_constant: false,
            }],
            inputs: [&[3.0, 1.0], &[-1.0, 2.0], &[5.0, 5.0], &[0.0, -3.0]],
        },
        // IF aggregate: 3 inputs -> 1 IF neuron, both branches exercised.
        ParityCase {
            name: "if_aggregate",
            num_inputs: 3,
            synapses: vec![
                make_synapse_typed(0, 1.0, 1), // condition
                make_synapse_typed(1, 1.0, 3), // positive
                make_synapse_typed(2, 1.0, 2), // negative
            ],
            neurons: vec![NeuronData {
                bias: 0.0,
                start_synapse: 0,
                num_synapses: 3,
                squash_type: 34, // IF
                is_constant: false,
            }],
            inputs: [
                &[1.0, 5.0, 10.0],  // condition>0 -> positive=5.0
                &[-1.0, 5.0, 10.0], // condition<=0 -> negative=10.0
                &[0.5, 3.0, 7.0],   // condition>0 -> positive=3.0
                &[-2.0, 3.0, 7.0],  // condition<=0 -> negative=7.0
            ],
        },
        // Constant neuron feeding an Identity output.
        ParityCase {
            name: "constant_neuron",
            num_inputs: 2,
            synapses: vec![
                make_synapse(2, 1.0), // output <- constant
            ],
            neurons: vec![
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
            ],
            inputs: [&[1.0, 2.0], &[3.0, 4.0], &[5.0, 6.0], &[7.0, 8.0]],
        },
        // Deeper network: 2 inputs -> 2 hidden (ReLU) -> 1 output (Identity).
        ParityCase {
            name: "multi_layer",
            num_inputs: 2,
            synapses: vec![
                // Hidden 0 (idx 2): from input 0 and 1
                make_synapse(0, 0.5),
                make_synapse(1, 0.3),
                // Hidden 1 (idx 3): from input 0 and 1
                make_synapse(0, -0.4),
                make_synapse(1, 0.6),
                // Output (idx 4): from hidden 0 and hidden 1
                make_synapse(2, 1.0),
                make_synapse(3, -0.5),
            ],
            neurons: vec![
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
            ],
            inputs: [&[1.0, 2.0], &[-1.0, 0.5], &[3.0, -2.0], &[0.0, 0.0]],
        },
    ]
}

/// Every record of the 4-way batch matches the single-record trace, across all
/// squash kinds, constant neurons and network depths in the fixture table.
#[test]
fn batch_4way_matches_single_record_for_every_fixture() {
    for case in parity_cases() {
        assert_batch_matches_single(&case);
    }
}

/// Issue #476 - regression: records of **differing** lengths are each sliced
/// with their own length. The hand-copied variants sliced record 2 with record
/// 0's length, which only went unnoticed because every record in those fixtures
/// happened to be the same size.
#[test]
fn split_batch_records_uses_each_records_own_length() {
    // Header [1, 2, 3, 4] then 10 payload values, all lengths distinct.
    let batch_result: Vec<f32> = vec![
        1.0, 2.0, 3.0, 4.0,  // header
        10.0, // record 0
        20.0, 21.0, // record 1
        30.0, 31.0, 32.0, // record 2
        40.0, 41.0, 42.0, 43.0, // record 3
    ];

    let records = split_batch_records(&batch_result);

    assert_eq!(records.len(), BATCH_RECORDS);
    assert_eq!(records[0], &[10.0]);
    assert_eq!(records[1], &[20.0, 21.0]);
    assert_eq!(records[2], &[30.0, 31.0, 32.0]);
    assert_eq!(records[3], &[40.0, 41.0, 42.0, 43.0]);
}

/// A header whose lengths do not account for the whole payload is a broken
/// oracle and must fail loudly rather than silently comparing a prefix.
#[test]
#[should_panic(expected = "cover the whole batch buffer")]
fn split_batch_records_rejects_header_that_undercovers_the_buffer() {
    // Header claims 1+1+1+1 = 4 values but 5 follow.
    let batch_result: Vec<f32> = vec![1.0, 1.0, 1.0, 1.0, 10.0, 20.0, 30.0, 40.0, 50.0];
    let _ = split_batch_records(&batch_result);
}

/// A header claiming more data than the buffer holds must fail loudly rather
/// than panicking with an opaque slice-index message.
#[test]
#[should_panic(expected = "overruns the batch buffer")]
fn split_batch_records_rejects_header_that_overruns_the_buffer() {
    // Header claims 1+1+1+9 = 12 values but only 4 follow.
    let batch_result: Vec<f32> = vec![1.0, 1.0, 1.0, 9.0, 10.0, 20.0, 30.0, 40.0];
    let _ = split_batch_records(&batch_result);
}

/// Issue #155 - Regression test for buffer reuse in `activate_and_trace_batch_4way`.
///
/// The method now reuses preallocated scratch buffers across calls instead of
/// allocating 12 fresh vectors each time. This verifies the reused buffers are
/// correctly reset between calls: invoking the method twice on the same network
/// (with different inputs interleaved) must produce byte-identical output to a
/// fresh network for each call, proving no state leaks between invocations.
#[test]
fn test_batch_4way_buffer_reuse_no_state_leak() {
    // Network exercising standard, aggregate and trace-producing squashes so the
    // hint and trace buffers are non-trivially populated each call.
    let synapses = vec![
        make_synapse(0, 0.7),          // hidden(MAX) <- input0
        make_synapse(1, -0.4),         // hidden(MAX) <- input1
        make_synapse_typed(2, 1.0, 0), // output(TANH) <- hidden
    ];
    let neurons = vec![
        NeuronData {
            bias: 0.05,
            start_synapse: 0,
            num_synapses: 2,
            squash_type: 33, // MAXIMUM (aggregate -> writes trace data)
            is_constant: false,
        },
        NeuronData {
            bias: -0.1,
            start_synapse: 2,
            num_synapses: 1,
            squash_type: 7, // TANH
            is_constant: false,
        },
    ];

    let batch_a: [&[f32]; 4] = [&[1.0, 2.0], &[0.5, -1.0], &[-2.0, 3.0], &[0.4, 0.1]];
    let batch_b: [&[f32]; 4] = [&[-1.5, 0.2], &[3.0, 1.0], &[0.0, -0.5], &[2.0, -2.0]];

    let packed_a: Vec<f32> = batch_a.iter().flat_map(|i| i.iter().copied()).collect();
    let packed_b: Vec<f32> = batch_b.iter().flat_map(|i| i.iter().copied()).collect();

    // Reference outputs from fresh networks (no buffer reuse possible).
    let mut ref_net_a = make_network(2, neurons.clone(), synapses.clone());
    let expected_a = ref_net_a.activate_and_trace_batch_4way(&packed_a, 2, 1);
    let mut ref_net_b = make_network(2, neurons.clone(), synapses.clone());
    let expected_b = ref_net_b.activate_and_trace_batch_4way(&packed_b, 2, 1);

    // Reused network: two consecutive calls on the SAME instance.
    let mut net = make_network(2, neurons.clone(), synapses.clone());
    let got_a = net.activate_and_trace_batch_4way(&packed_a, 2, 1);
    let got_b = net.activate_and_trace_batch_4way(&packed_b, 2, 1);

    assert_eq!(
        got_a, expected_a,
        "First reused call must match fresh-network output"
    );
    assert_eq!(
        got_b, expected_b,
        "Second reused call must match fresh-network output (no state leak)"
    );

    // A third call repeating the first inputs must again equal the first output.
    let got_a_again = net.activate_and_trace_batch_4way(&packed_a, 2, 1);
    assert_eq!(
        got_a_again, expected_a,
        "Repeating inputs after other calls must reproduce output"
    );
}
