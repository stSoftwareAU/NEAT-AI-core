//! Short-record input semantics for the single-record entry points (Issue #519).
//!
//! One rule covers every record narrower than the network's input arity: the
//! input slots the record does not cover read as **zero**, never as whatever a
//! previous call left in the reused activation buffer. `load_record`
//! (`batch_scoring.rs`, Issue #445) has always obeyed it; the three
//! single-record entry points — [`CompiledNetwork::activate`],
//! [`CompiledNetwork::activate_into`] and
//! [`CompiledNetwork::activate_and_trace`] — did not, so the same record scored
//! differently depending on which path it took.
//!
//! These are "what" tests: they drive the public entry points with real
//! networks and assert on returned numbers. Two observable properties pin the
//! rule — a warmed network must score a narrow record identically to a freshly
//! loaded one (statelessness), and a narrow record must score identically
//! through the single-record path and the batched loader (parity).

use neat_core::{CompiledNetwork, NeuronData, SynapseData};

const IDENTITY: u8 = 0;
const TANH: u8 = 7;
const MEAN: u8 = 37;

/// Network input arity. Records narrower than this leave uncovered slots.
const NUM_INPUTS: usize = 4;

/// Width of the short record: covers half the input slots.
const SHORT_WIDTH: usize = 2;

/// A full-width record whose tail values are large enough that leaking them
/// into a later call moves the outputs well outside `TOL`.
const FULL_RECORD: [f32; NUM_INPUTS] = [0.4, -0.7, 3.5, -4.25];

/// The short record — the same leading values, so only the uncovered tail can
/// explain any difference in the outputs.
const SHORT_RECORD: [f32; SHORT_WIDTH] = [0.4, -0.7];

/// The short record written out at full width with the uncovered slots zeroed:
/// what a stateless narrow call must be equivalent to.
const SHORT_PADDED: [f32; NUM_INPUTS] = [0.4, -0.7, 0.0, 0.0];

/// Absolute tolerance against the batched kernels, which re-associate the
/// weighted sums in `f32` (same tolerance as `batch_record_skeleton.rs`).
const TOL: f32 = 1e-5;

fn network(neurons: Vec<NeuronData>, synapses: Vec<SynapseData>) -> CompiledNetwork {
    let num_non_inputs = neurons.len();
    let num_neurons = NUM_INPUTS + num_non_inputs;
    CompiledNetwork {
        num_neurons,
        num_inputs: NUM_INPUTS,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::new(),
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
        batch_traces: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
    }
}

/// One hidden neuron reading all four inputs, one output neuron reading it.
/// `hidden_squash` picks the arm under test: a standard weighted sum, or an
/// aggregate that takes the exact single-record kernel.
fn build_network(hidden_squash: u8) -> CompiledNetwork {
    let mut synapses = Vec::new();
    for i in 0..NUM_INPUTS {
        synapses.push(SynapseData {
            weight: 0.3 + 0.11 * (i as f32),
            from_index: i as u16,
            synapse_type: 0,
        });
    }
    let hidden = NeuronData {
        bias: 0.05,
        start_synapse: 0,
        num_synapses: NUM_INPUTS as u16,
        squash_type: hidden_squash,
        is_constant: false,
    };

    let output_start = synapses.len() as u32;
    synapses.push(SynapseData {
        weight: 0.9,
        from_index: NUM_INPUTS as u16,
        synapse_type: 0,
    });
    let output = NeuronData {
        bias: -0.02,
        start_synapse: output_start,
        num_synapses: 1,
        squash_type: IDENTITY,
        is_constant: false,
    };

    network(vec![hidden, output], synapses)
}

/// Every hidden-squash arm the single-record entry points must obey the rule
/// for: a standard weighted sum, and an aggregate that cannot be lane-vectorised.
const HIDDEN_SQUASHES: [(&str, u8); 3] = [
    ("identity", IDENTITY),
    ("tanh", TANH),
    ("mean (aggregate)", MEAN),
];

fn assert_close(label: &str, actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "{label}: output length");
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() <= TOL,
            "{label}: output {i} was {a}, expected {e}"
        );
    }
}

/// The batched loader's answer for the short record — the reference both the
/// single-record path and the zero-padded record must agree with.
fn batched_short_outputs(squash: u8) -> Vec<f32> {
    build_network(squash).score_records_flat(&SHORT_RECORD, SHORT_WIDTH, 1)
}

#[test]
fn activate_into_scores_a_narrow_record_without_the_previous_calls_inputs() {
    for (label, squash) in HIDDEN_SQUASHES {
        let mut net = build_network(squash);
        let mut warm = [0.0f32; 1];
        net.activate_into(&FULL_RECORD, &mut warm);

        let mut after_full = [0.0f32; 1];
        net.activate_into(&SHORT_RECORD, &mut after_full);

        let mut fresh_out = [0.0f32; 1];
        build_network(squash).activate_into(&SHORT_RECORD, &mut fresh_out);

        assert_close(
            &format!("activate_into/{label}: warmed vs fresh"),
            &after_full,
            &fresh_out,
        );
    }
}

#[test]
fn activate_scores_a_narrow_record_without_the_previous_calls_inputs() {
    for (label, squash) in HIDDEN_SQUASHES {
        let mut net = build_network(squash);
        net.activate(&FULL_RECORD, 1);
        let after_full = net.activate(&SHORT_RECORD, 1);

        let fresh = build_network(squash).activate(&SHORT_RECORD, 1);

        assert_close(
            &format!("activate/{label}: warmed vs fresh"),
            &after_full,
            &fresh,
        );
    }
}

#[test]
fn activate_and_trace_scores_a_narrow_record_without_the_previous_calls_inputs() {
    for (label, squash) in HIDDEN_SQUASHES {
        let mut net = build_network(squash);
        net.activate_and_trace(&FULL_RECORD, 1);
        let after_full = net.activate_and_trace(&SHORT_RECORD, 1);

        let fresh = build_network(squash).activate_and_trace(&SHORT_RECORD, 1);

        assert_close(
            &format!("activate_and_trace/{label}: warmed vs fresh"),
            &after_full,
            &fresh,
        );
    }
}

#[test]
fn a_narrow_record_matches_its_zero_padded_full_width_form() {
    for (label, squash) in HIDDEN_SQUASHES {
        let mut narrow = build_network(squash);
        narrow.activate_into(&FULL_RECORD, &mut [0.0f32; 1]);
        let mut narrow_out = [0.0f32; 1];
        narrow.activate_into(&SHORT_RECORD, &mut narrow_out);

        let mut padded = build_network(squash);
        padded.activate_into(&FULL_RECORD, &mut [0.0f32; 1]);
        let mut padded_out = [0.0f32; 1];
        padded.activate_into(&SHORT_PADDED, &mut padded_out);

        assert_close(
            &format!("{label}: narrow vs zero-padded"),
            &narrow_out,
            &padded_out,
        );
    }
}

#[test]
fn single_record_entry_points_match_the_batched_loader_for_a_narrow_record() {
    for (label, squash) in HIDDEN_SQUASHES {
        let batched = batched_short_outputs(squash);

        let mut net = build_network(squash);
        net.activate_into(&FULL_RECORD, &mut [0.0f32; 1]);

        let mut into_out = [0.0f32; 1];
        net.activate_into(&SHORT_RECORD, &mut into_out);
        assert_close(
            &format!("activate_into/{label} vs batched loader"),
            &into_out,
            &batched,
        );

        net.activate_into(&FULL_RECORD, &mut [0.0f32; 1]);
        let activate_out = net.activate(&SHORT_RECORD, 1);
        assert_close(
            &format!("activate/{label} vs batched loader"),
            &activate_out,
            &batched,
        );

        net.activate_into(&FULL_RECORD, &mut [0.0f32; 1]);
        let traced = net.activate_and_trace(&SHORT_RECORD, 1);
        assert_close(
            &format!("activate_and_trace/{label} vs batched loader"),
            &traced[..1],
            &batched,
        );
    }
}

#[test]
fn a_full_width_record_is_unaffected_by_the_zero_fill() {
    for (label, squash) in HIDDEN_SQUASHES {
        let mut net = build_network(squash);
        let mut first = [0.0f32; 1];
        net.activate_into(&FULL_RECORD, &mut first);

        net.activate_into(&SHORT_RECORD, &mut [0.0f32; 1]);
        let mut again = [0.0f32; 1];
        net.activate_into(&FULL_RECORD, &mut again);

        assert_close(
            &format!("{label}: full-width record repeatable"),
            &again,
            &first,
        );
    }
}
