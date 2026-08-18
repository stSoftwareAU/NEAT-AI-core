//! Issue #544 — the per-sample propagate surface an out-of-crate host reaches.
//!
//! NEAT-AI-Backpropagation owns the `trainDir` epoch loop and calls into this
//! crate one sample at a time, either through the typed
//! `propagate_topological_loop` or through the byte-packed ABI
//! (`propagate_codec`). Both are exercised from **outside** the crate here, so
//! narrowing either to `pub(crate)`, or moving them out of `neat-core`, fails
//! this file at compile time — the "do keep" half of the ownership fence in
//! AGENTS.md. The in-crate unit tests cover decoder offsets and error paths;
//! this file covers the boundary and the sentinel contract the host decodes.

use neat_core::propagate_codec::{
    HEADER_BYTES, NEURON_RECORD_BYTES, PER_NEURON_OUT_F64S, PER_SYNAPSE_OUT_F64S,
    SYNAPSE_RECORD_BYTES, decode_propagate_buffer, encode_propagate_output,
};
use neat_core::topological_backprop::{
    NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, NeuronInput, PropagateInput, PropagateOutcome,
    PropagateOutput, SynapseInput, propagate_topological_loop,
};

const PLANK: f32 = 1.0e-7;
/// Both inputs sit at this activation, so every inward link is a positive one.
const INPUT_ACTIVATION: f32 = 0.5;
/// The identity output neuron's own activation before propagation.
const OUTPUT_ACTIVATION: f32 = 0.5;

fn neuron(neuron_type: u8, adjusted_activation: f32) -> NeuronInput {
    NeuronInput {
        squash_type: 0, // Identity — target activation passes through unsquashed.
        neuron_type,
        propagate_needed: true,
        update_needed: true,
        hint_value: 0.0,
        range_low: -1.0e6,
        range_high: 1.0e6,
        adjusted_activation,
        adjusted_bias: 0.0,
    }
}

/// 2 input neurons (0, 1) → 1 identity output neuron (2), one unit-weight
/// synapse each. Returns the pieces a [`PropagateInput`] borrows.
fn fixture() -> (Vec<NeuronInput>, Vec<SynapseInput>) {
    let neurons = vec![
        neuron(NEURON_TYPE_INPUT, INPUT_ACTIVATION),
        neuron(NEURON_TYPE_INPUT, INPUT_ACTIVATION),
        neuron(NEURON_TYPE_OUTPUT, OUTPUT_ACTIVATION),
    ];
    let synapses = vec![
        SynapseInput {
            from: 0,
            to: 2,
            original_weight: 1.0,
            adjusted_weight: 1.0,
            is_self_loop: false,
        },
        SynapseInput {
            from: 1,
            to: 2,
            original_weight: 1.0,
            adjusted_weight: 1.0,
            is_self_loop: false,
        },
    ];
    (neurons, synapses)
}

fn run(neurons: &[NeuronInput], synapses: &[SynapseInput], expected: &[f32]) -> PropagateOutput {
    let inward_starts = [0u32, 0, 0];
    let inward_counts = [0u32, 0, 2];
    let inward_indices = [0u32, 1];
    let order = [2u32];
    let input = PropagateInput {
        neurons,
        synapses,
        inward_starts: &inward_starts,
        inward_counts: &inward_counts,
        inward_synapse_indices: &inward_indices,
        reverse_topo_order: &order,
        expected,
        input_count: 2,
        output_count: 1,
        plank_constant: PLANK,
        normalise_gradients: false,
    };
    let output = propagate_topological_loop(&input);
    assert_eq!(output.neurons.len(), neurons.len());
    assert_eq!(output.synapses.len(), synapses.len());
    output
}

#[test]
fn typed_propagate_loop_drives_an_identity_output_to_its_expected_value() {
    let (neurons, synapses) = fixture();
    // expected − activation = 1.0 − 0.5 = 0.5, an error well above PLANK.
    let output = run(&neurons, &synapses, &[1.0]);

    // One inward link per input neuron, each accumulated exactly once with the
    // source activation on the positive side (INPUT_ACTIVATION > 0).
    for (i, delta) in output.synapses.iter().enumerate() {
        assert_eq!(delta.count, 1.0, "synapse {i} accumulation count");
        assert_eq!(
            delta.total_positive_activation, INPUT_ACTIVATION,
            "synapse {i} positive activation"
        );
        assert_eq!(
            delta.total_negative_activation, 0.0,
            "synapse {i} negative activation"
        );
        assert_eq!(delta.count_positive, 1.0, "synapse {i} positive count");
        assert_eq!(delta.count_negative, 0.0, "synapse {i} negative count");
    }

    match output.neurons[2] {
        PropagateOutcome::Standard(s) => {
            assert_eq!(
                s.total_error_absolute_delta, 0.5,
                "|expected − adjusted_activation|"
            );
            // Identity squash: the clamped target activation (0.5 + 0.5) is the
            // new cached activation, unchanged by unsquash.
            assert_eq!(s.cached_activation, 1.0, "identity target activation");
            assert!(!s.no_change, "update_needed neuron is not flagged noChange");
            assert_eq!(s.bias_count_delta, 1, "one bias accumulation");
            assert_eq!(
                s.trace_activation,
                Some(1.0),
                "trace activation is recorded for an update_needed neuron"
            );
        }
        other => panic!("expected a Standard outcome, got {other:?}"),
    }
}

#[test]
fn typed_propagate_loop_reports_no_change_when_the_output_already_matches() {
    let (neurons, synapses) = fixture();
    // expected == adjusted_activation ⇒ error 0.0, below PLANK.
    let output = run(&neurons, &synapses, &[OUTPUT_ACTIVATION]);

    match output.neurons[2] {
        PropagateOutcome::NoChange { cached_activation } => {
            assert_eq!(cached_activation, OUTPUT_ACTIVATION);
        }
        other => panic!("expected NoChange, got {other:?}"),
    }
    // The loop bails before accumulation, so no weight statistics move.
    for (i, delta) in output.synapses.iter().enumerate() {
        assert_eq!(delta.count, 0.0, "synapse {i} accumulation count");
        assert_eq!(
            delta.total_positive_activation, 0.0,
            "synapse {i} positive activation"
        );
    }
}

/// Mirror of the TypeScript encoder in NEAT-AI's `WasmTopologicalBackprop.ts`
/// — the host side of the packed ABI. Built here rather than imported so the
/// buffer this test decodes is written independently of the decoder.
struct Packer {
    bytes: Vec<u8>,
}

impl Packer {
    /// Pack the [`fixture`] network with `expected` as the target output.
    fn fixture_buffer(expected: f32) -> Vec<u8> {
        let mut p = Packer { bytes: Vec::new() };
        // Header: neuron/input/output/synapse counts, order length, inward total.
        p.u32(3);
        p.u32(2);
        p.u32(1);
        p.u32(2);
        p.u32(1);
        p.u32(2);
        p.f64(PLANK as f64);
        p.u8(0); // normalise_gradients
        p.u8(0);
        p.u8(0);
        p.u8(0);
        assert_eq!(p.bytes.len(), HEADER_BYTES);

        for (kind, activation) in [
            (NEURON_TYPE_INPUT, INPUT_ACTIVATION),
            (NEURON_TYPE_INPUT, INPUT_ACTIVATION),
            (NEURON_TYPE_OUTPUT, OUTPUT_ACTIVATION),
        ] {
            p.u8(0); // squash_type: Identity
            p.u8(kind);
            p.u8(1); // propagate_needed
            p.u8(1); // update_needed
            p.f32(0.0); // hint_value
            p.f32(-1.0e6); // range_low
            p.f32(1.0e6); // range_high
            p.f32(activation);
            p.f32(0.0); // adjusted_bias
        }
        assert_eq!(p.bytes.len(), HEADER_BYTES + 3 * NEURON_RECORD_BYTES);

        for from in [0u32, 1] {
            p.u32(from);
            p.u32(2); // to
            p.f32(1.0); // original_weight
            p.f32(1.0); // adjusted_weight
            p.u8(0); // is_self_loop
            p.u8(0);
            p.u8(0);
            p.u8(0);
        }
        assert_eq!(
            p.bytes.len(),
            HEADER_BYTES + 3 * NEURON_RECORD_BYTES + 2 * SYNAPSE_RECORD_BYTES
        );

        for (start, count) in [(0u32, 0u32), (0, 0), (0, 2)] {
            p.u32(start);
            p.u32(count);
        }
        p.u32(0); // inward indices
        p.u32(1);
        p.u32(2); // reverse topological order: the output neuron only
        p.f32(expected);
        p.bytes
    }

    fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
}

#[test]
fn packed_abi_round_trip_encodes_the_standard_outcome_slots() {
    let buffer = Packer::fixture_buffer(1.0);
    let decoded = decode_propagate_buffer(&buffer).expect("decode packed buffer");
    let packed = encode_propagate_output(&propagate_topological_loop(&decoded.as_input()));

    assert_eq!(
        packed.len(),
        3 * PER_NEURON_OUT_F64S + 2 * PER_SYNAPSE_OUT_F64S,
        "3 neuron slots then 2 synapse slots"
    );

    // Input neurons are never propagated through: every slot is NaN.
    for slot in &packed[..2 * PER_NEURON_OUT_F64S] {
        assert!(slot.is_nan(), "input-neuron slots encode as NaN");
    }

    // Output neuron (index 2) — same values the typed loop reports above, in
    // the documented slot order.
    let out = &packed[2 * PER_NEURON_OUT_F64S..3 * PER_NEURON_OUT_F64S];
    assert_eq!(out[0], 0.5, "total_error_absolute delta");
    assert_eq!(out[1], 1.0, "cached activation");
    assert_eq!(out[2], 0.0, "noChange flag clear");
    assert_eq!(out[3], 1.0, "bias count delta");
    assert_eq!(out[6], 1.0, "trace activation");

    // Synapse section: count then the positive/negative activation pair.
    for i in 0..2 {
        let base = 3 * PER_NEURON_OUT_F64S + i * PER_SYNAPSE_OUT_F64S;
        assert_eq!(packed[base], 1.0, "synapse {i} count");
        assert_eq!(
            packed[base + 1],
            INPUT_ACTIVATION as f64,
            "synapse {i} positive activation"
        );
        assert_eq!(packed[base + 2], 0.0, "synapse {i} negative activation");
    }
}

#[test]
fn packed_abi_round_trip_keeps_the_no_change_sentinel() {
    let buffer = Packer::fixture_buffer(OUTPUT_ACTIVATION);
    let decoded = decode_propagate_buffer(&buffer).expect("decode packed buffer");
    let packed = encode_propagate_output(&propagate_topological_loop(&decoded.as_input()));

    // The TS↔host contract: slot 1 of a neuron carries −Infinity to select the
    // noChange path, with the cached activation stashed in the last slot.
    let out = &packed[2 * PER_NEURON_OUT_F64S..3 * PER_NEURON_OUT_F64S];
    assert_eq!(out[1], f64::NEG_INFINITY, "noChange sentinel");
    assert_eq!(out[6], OUTPUT_ACTIVATION as f64, "cached activation");
}
