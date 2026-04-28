//! Issue #2463 — native-testable codec for the byte-packed
//! `propagate_topological` ABI shared with NEAT-AI's
//! `src/propagate/WasmTopologicalBackprop.ts`.
//!
//! The TS encoder is the canonical contract (Issue #1954). Earlier WASM
//! exports lifted in PR #36 hard-coded a `HEADER_BYTES = 40` /
//! `NEURON_RECORD_BYTES = 20` layout and silently zeroed `adjusted_bias`,
//! which shifted all subsequent reads by 4 bytes per neuron and one extra
//! 4-byte slot at the start of the buffer. With the buffer mis-aligned,
//! `synapse.from` / `synapse.to` were read from junk, every downstream
//! bounds check on `reverse_topo_order` panicked, and the release build
//! emitted `RuntimeError: unreachable` inside `propagate_topological`.
//!
//! This module contains the canonical decoder/encoder pair and the
//! constants that mirror the TS contract exactly. It is **not** gated to
//! `wasm32` so the same logic can be unit-tested natively.

use crate::topological_backprop::{
    NeuronInput, PropagateInput, PropagateOutcome, PropagateOutput, SynapseInput,
};

/// Header size in bytes (mirrors `HEADER_SIZE` in `WasmTopologicalBackprop.ts`).
///
/// Layout: 6 × `u32` (24 bytes) + `f64 plank_constant` (8 bytes) +
/// `u8 normalise_gradients` + 3 padding bytes = 36.
pub const HEADER_BYTES: usize = 36;

/// Per-neuron record size in bytes (mirrors `NEURON_STRIDE` in TS).
///
/// Layout: 4 × `u8` (squash_type, neuron_type, propagate_needed,
/// update_needed) + 5 × `f32` (hint_value, range_low, range_high,
/// adjusted_activation, **adjusted_bias**) = 24.
pub const NEURON_RECORD_BYTES: usize = 24;

/// Per-synapse record size in bytes (mirrors `SYNAPSE_STRIDE` in TS).
///
/// Layout: 2 × `u32` + 2 × `f32` + `u8` + 3 padding bytes = 20.
pub const SYNAPSE_RECORD_BYTES: usize = 20;

/// Inward-map stride per neuron: `u32 start, u32 count` = 8 bytes.
pub const INWARD_MAP_STRIDE: usize = 8;

/// Per-neuron output slot count (f64s) — see `wasm_exports.rs` encode loop.
pub const PER_NEURON_OUT_F64S: usize = 7;

/// Per-synapse output slot count (f64s).
pub const PER_SYNAPSE_OUT_F64S: usize = 7;

/// Owned, decoded form of the byte-packed buffer. The caller borrows the
/// fields to construct a [`PropagateInput`] for `propagate_topological_loop`.
#[derive(Debug, Clone)]
pub struct DecodedPropagate {
    pub neurons: Vec<NeuronInput>,
    pub synapses: Vec<SynapseInput>,
    pub inward_starts: Vec<u32>,
    pub inward_counts: Vec<u32>,
    pub inward_indices: Vec<u32>,
    pub reverse_topo_order: Vec<u32>,
    pub expected: Vec<f32>,
    pub input_count: u32,
    pub output_count: u32,
    pub plank_constant: f32,
    pub normalise_gradients: bool,
}

impl DecodedPropagate {
    /// Borrow the decoded buffers as a [`PropagateInput`] view.
    pub fn as_input(&self) -> PropagateInput<'_> {
        PropagateInput {
            neurons: &self.neurons,
            synapses: &self.synapses,
            inward_starts: &self.inward_starts,
            inward_counts: &self.inward_counts,
            inward_synapse_indices: &self.inward_indices,
            reverse_topo_order: &self.reverse_topo_order,
            expected: &self.expected,
            input_count: self.input_count,
            output_count: self.output_count,
            plank_constant: self.plank_constant,
            normalise_gradients: self.normalise_gradients,
        }
    }
}

/// Reasons a buffer cannot be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Buffer shorter than the fixed header.
    HeaderTooShort,
    /// Buffer truncated before the variable-length sections finish.
    BufferTruncated,
}

#[inline]
fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

#[inline]
fn read_f32_le(buf: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

#[inline]
fn read_f64_le(buf: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
    ])
}

/// Compute the total expected buffer size for the given counts.
fn expected_size(
    neuron_count: usize,
    synapse_count: usize,
    total_inward: usize,
    order_length: usize,
    output_count: usize,
) -> usize {
    HEADER_BYTES
        + neuron_count * NEURON_RECORD_BYTES
        + synapse_count * SYNAPSE_RECORD_BYTES
        + neuron_count * INWARD_MAP_STRIDE
        + total_inward * 4
        + order_length * 4
        + output_count * 4
}

/// Decode the byte-packed buffer into owned vectors. Returns
/// [`DecodeError`] when the buffer is too short to satisfy the declared
/// counts — callers should treat this as the "no data" path (the WASM
/// shim in earlier code returned an empty `Vec` for the same condition).
pub fn decode_propagate_buffer(data: &[u8]) -> Result<DecodedPropagate, DecodeError> {
    if data.len() < HEADER_BYTES {
        return Err(DecodeError::HeaderTooShort);
    }

    // Header.
    let neuron_count = read_u32_le(data, 0) as usize;
    let input_count = read_u32_le(data, 4);
    let output_count = read_u32_le(data, 8);
    let synapse_count = read_u32_le(data, 12) as usize;
    let order_length = read_u32_le(data, 16) as usize;
    let total_inward_entries = read_u32_le(data, 20) as usize;
    let plank_constant = read_f64_le(data, 24) as f32;
    let normalise_gradients = data[32] != 0;

    let needed = expected_size(
        neuron_count,
        synapse_count,
        total_inward_entries,
        order_length,
        output_count as usize,
    );
    if data.len() < needed {
        return Err(DecodeError::BufferTruncated);
    }

    let mut offset = HEADER_BYTES;

    // Per-neuron records.
    let mut neurons: Vec<NeuronInput> = Vec::with_capacity(neuron_count);
    for i in 0..neuron_count {
        let base = offset + i * NEURON_RECORD_BYTES;
        neurons.push(NeuronInput {
            squash_type: data[base],
            neuron_type: data[base + 1],
            propagate_needed: data[base + 2] != 0,
            update_needed: data[base + 3] != 0,
            hint_value: read_f32_le(data, base + 4),
            range_low: read_f32_le(data, base + 8),
            range_high: read_f32_le(data, base + 12),
            adjusted_activation: read_f32_le(data, base + 16),
            adjusted_bias: read_f32_le(data, base + 20),
        });
    }
    offset += neuron_count * NEURON_RECORD_BYTES;

    // Per-synapse records.
    let mut synapses: Vec<SynapseInput> = Vec::with_capacity(synapse_count);
    for i in 0..synapse_count {
        let base = offset + i * SYNAPSE_RECORD_BYTES;
        synapses.push(SynapseInput {
            from: read_u32_le(data, base),
            to: read_u32_le(data, base + 4),
            original_weight: read_f32_le(data, base + 8),
            adjusted_weight: read_f32_le(data, base + 12),
            is_self_loop: data[base + 16] != 0,
        });
    }
    offset += synapse_count * SYNAPSE_RECORD_BYTES;

    // Inward mapping.
    let mut inward_starts = Vec::with_capacity(neuron_count);
    let mut inward_counts = Vec::with_capacity(neuron_count);
    for i in 0..neuron_count {
        let base = offset + i * INWARD_MAP_STRIDE;
        inward_starts.push(read_u32_le(data, base));
        inward_counts.push(read_u32_le(data, base + 4));
    }
    offset += neuron_count * INWARD_MAP_STRIDE;

    // Inward indices.
    let mut inward_indices = Vec::with_capacity(total_inward_entries);
    for i in 0..total_inward_entries {
        inward_indices.push(read_u32_le(data, offset + i * 4));
    }
    offset += total_inward_entries * 4;

    // Reverse topological order.
    let mut reverse_topo_order = Vec::with_capacity(order_length);
    for i in 0..order_length {
        reverse_topo_order.push(read_u32_le(data, offset + i * 4));
    }
    offset += order_length * 4;

    // Expected outputs.
    let mut expected = Vec::with_capacity(output_count as usize);
    for i in 0..output_count as usize {
        expected.push(read_f32_le(data, offset + i * 4));
    }

    Ok(DecodedPropagate {
        neurons,
        synapses,
        inward_starts,
        inward_counts,
        inward_indices,
        reverse_topo_order,
        expected,
        input_count,
        output_count,
        plank_constant,
        normalise_gradients,
    })
}

/// Encode a [`PropagateOutput`] using the TS↔WASM sentinel contract.
///
/// Section 1: `neuron_count × 7` f64s with sentinel encoding for
/// Skipped/NoChange/Special/Standard outcomes.
/// Section 2: `synapse_count × 7` f64s with the synapse delta accumulators.
pub fn encode_propagate_output(output: &PropagateOutput) -> Vec<f64> {
    let mut packed = Vec::with_capacity(
        output.neurons.len() * PER_NEURON_OUT_F64S + output.synapses.len() * PER_SYNAPSE_OUT_F64S,
    );

    for outcome in &output.neurons {
        match outcome {
            PropagateOutcome::Skipped => {
                for _ in 0..PER_NEURON_OUT_F64S {
                    packed.push(f64::NAN);
                }
            }
            PropagateOutcome::NoChange { cached_activation } => {
                packed.push(0.0); // total_error_absolute_delta
                packed.push(f64::NEG_INFINITY); // sentinel: TS noChange path
                packed.push(0.0);
                packed.push(0.0);
                packed.push(0.0);
                packed.push(0.0);
                packed.push(*cached_activation as f64);
            }
            PropagateOutcome::Special { target_activation } => {
                packed.push(0.0);
                packed.push(f64::INFINITY); // sentinel: TS custom propagate
                packed.push(0.0);
                packed.push(0.0);
                packed.push(0.0);
                packed.push(0.0);
                packed.push(*target_activation as f64);
            }
            PropagateOutcome::Standard(s) => {
                packed.push(s.total_error_absolute_delta as f64);
                packed.push(s.cached_activation as f64);
                packed.push(if s.no_change { 1.0 } else { 0.0 });
                packed.push(s.bias_count_delta as f64);
                packed.push(s.total_bias_delta as f64);
                packed.push(s.total_adjusted_bias_delta as f64);
                packed.push(s.trace_activation.map(|v| v as f64).unwrap_or(f64::NAN));
            }
        }
    }

    for syn in &output.synapses {
        packed.push(syn.count as f64);
        packed.push(syn.total_positive_activation as f64);
        packed.push(syn.total_negative_activation as f64);
        packed.push(syn.count_positive as f64);
        packed.push(syn.count_negative as f64);
        packed.push(syn.total_positive_adjusted_value as f64);
        packed.push(syn.total_negative_adjusted_value as f64);
    }

    packed
}

// ---------------------------------------------------------------------------
// Tests — exercise the codec natively. These tests build buffers using the
// exact byte layout produced by `WasmTopologicalBackprop.ts` and verify that
// the decoder reads the matching fields, including `adjusted_bias`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topological_backprop::{
        NEURON_TYPE_HIDDEN, NEURON_TYPE_INPUT, NEURON_TYPE_OUTPUT, propagate_topological_loop,
    };

    /// Mirror of the TS encoder — used to build deterministic test buffers.
    struct Builder {
        bytes: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Self { bytes: Vec::new() }
        }
        fn push_u8(&mut self, v: u8) {
            self.bytes.push(v);
        }
        fn push_u32(&mut self, v: u32) {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
        fn push_f32(&mut self, v: f32) {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
        fn push_f64(&mut self, v: f64) {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
    }

    #[test]
    fn header_and_neuron_record_constants_match_ts_contract() {
        // Mirrors HEADER_SIZE / NEURON_STRIDE / SYNAPSE_STRIDE in
        // WasmTopologicalBackprop.ts. If any of these change, the TS side
        // must change in lock-step.
        assert_eq!(HEADER_BYTES, 36);
        assert_eq!(NEURON_RECORD_BYTES, 24);
        assert_eq!(SYNAPSE_RECORD_BYTES, 20);
        assert_eq!(INWARD_MAP_STRIDE, 8);
    }

    #[test]
    fn header_too_short_returns_error() {
        let buf = vec![0u8; HEADER_BYTES - 1];
        match decode_propagate_buffer(&buf) {
            Err(DecodeError::HeaderTooShort) => {}
            other => panic!("expected HeaderTooShort, got {:?}", other.err()),
        }
    }

    #[test]
    fn buffer_truncated_returns_error() {
        // Declare 1 neuron / 0 synapses but provide only a header.
        let mut b = Builder::new();
        b.push_u32(1); // neuron_count
        b.push_u32(0); // input_count
        b.push_u32(0); // output_count
        b.push_u32(0); // synapse_count
        b.push_u32(0); // order_length
        b.push_u32(0); // total_inward_entries
        b.push_f64(1e-7); // plank_constant
        b.push_u8(0); // normalise_gradients
        b.push_u8(0);
        b.push_u8(0);
        b.push_u8(0);
        assert_eq!(b.bytes.len(), HEADER_BYTES);

        match decode_propagate_buffer(&b.bytes) {
            Err(DecodeError::BufferTruncated) => {}
            other => panic!("expected BufferTruncated, got {:?}", other.err()),
        }
    }

    /// Build a minimal but well-formed buffer for a 2-input → 1-output
    /// network and verify every neuron field round-trips, including
    /// `adjusted_bias` — the field that was silently zeroed in the lifted
    /// PR #36 decoder.
    #[test]
    fn decoder_round_trips_neuron_adjusted_bias() {
        // 2 input neurons + 1 output neuron, 2 synapses, no hidden.
        let neuron_count: u32 = 3;
        let input_count: u32 = 2;
        let output_count: u32 = 1;
        let synapse_count: u32 = 2;
        let order_length: u32 = 1; // only the output neuron
        let total_inward: u32 = 2; // both synapses feed the output

        let mut b = Builder::new();
        // Header.
        b.push_u32(neuron_count);
        b.push_u32(input_count);
        b.push_u32(output_count);
        b.push_u32(synapse_count);
        b.push_u32(order_length);
        b.push_u32(total_inward);
        b.push_f64(1.0e-7);
        b.push_u8(1); // normalise_gradients = true
        b.push_u8(0);
        b.push_u8(0);
        b.push_u8(0);

        // Per-neuron records — distinct adjusted_bias per neuron so a wrong
        // stride will produce a wrong value.
        let neuron_biases: [f32; 3] = [0.125f32, 0.25f32, 0.5f32];
        for (i, &bias) in neuron_biases.iter().enumerate() {
            b.push_u8(0); // squash_type
            b.push_u8(if i < 2 {
                NEURON_TYPE_INPUT
            } else {
                NEURON_TYPE_OUTPUT
            });
            b.push_u8(1); // propagate_needed
            b.push_u8(1); // update_needed
            b.push_f32(0.0); // hint_value
            b.push_f32(-1.0e6); // range_low
            b.push_f32(1.0e6); // range_high
            b.push_f32(0.5 + i as f32 * 0.1); // adjusted_activation
            b.push_f32(bias); // adjusted_bias — the field the bug ignored
        }

        // Per-synapse records.
        for (i, (from, to)) in [(0u32, 2u32), (1u32, 2u32)].iter().enumerate() {
            b.push_u32(*from);
            b.push_u32(*to);
            b.push_f32(0.5 + i as f32 * 0.1); // original_weight
            b.push_f32(0.5 + i as f32 * 0.1); // adjusted_weight
            b.push_u8(0); // is_self_loop
            b.push_u8(0); // padding
            b.push_u8(0);
            b.push_u8(0);
        }

        // Inward mapping (start, count) per neuron.
        for &(start, count) in &[(0u32, 0u32), (0u32, 0u32), (0u32, 2u32)] {
            b.push_u32(start);
            b.push_u32(count);
        }

        // Inward indices.
        b.push_u32(0);
        b.push_u32(1);

        // Reverse topological order.
        b.push_u32(2);

        // Expected outputs.
        b.push_f32(1.0);

        let decoded = decode_propagate_buffer(&b.bytes).expect("decode");

        // Header round-trip.
        assert_eq!(decoded.neurons.len(), neuron_count as usize);
        assert_eq!(decoded.synapses.len(), synapse_count as usize);
        assert_eq!(decoded.input_count, input_count);
        assert_eq!(decoded.output_count, output_count);
        assert!(decoded.normalise_gradients);
        assert!((decoded.plank_constant - 1.0e-7).abs() < 1e-12);

        // Per-neuron adjusted_bias must be the value the TS side wrote.
        for (i, expected_bias) in neuron_biases.iter().enumerate() {
            assert_eq!(
                decoded.neurons[i].adjusted_bias, *expected_bias,
                "neuron {} adjusted_bias mismatch",
                i
            );
        }

        // Synapse from/to must align — pre-fix this would be junk because
        // the reader started 4 bytes too late and 12 bytes per neuron too
        // close together.
        assert_eq!(decoded.synapses[0].from, 0);
        assert_eq!(decoded.synapses[0].to, 2);
        assert_eq!(decoded.synapses[1].from, 1);
        assert_eq!(decoded.synapses[1].to, 2);

        // Reverse topo order must round-trip exactly — the trap manifested
        // as a bounds check failure on this slice when the buffer alignment
        // was off.
        assert_eq!(decoded.reverse_topo_order, vec![2u32]);
        assert_eq!(decoded.inward_indices, vec![0u32, 1u32]);
        assert_eq!(decoded.expected, vec![1.0f32]);
    }

    /// Issue #2463 regression: prior to the fix, the decoder produced
    /// out-of-range neuron indices in `reverse_topo_order` because the
    /// header was treated as 40 bytes and each neuron record as 20 bytes.
    /// A `propagate_topological_loop` call against the resulting input
    /// would index past `neurons.len()` and panic — compiled to
    /// `unreachable` in release WASM. This test feeds the canonical TS
    /// layout end-to-end and confirms the loop completes without panic
    /// and returns the expected neuron count.
    #[test]
    fn end_to_end_decoder_drives_propagate_loop_without_trap() {
        // Same shape as docs/evidence/2461-repro.ts: small forward-only
        // network with hidden + output neurons.
        let neuron_count: u32 = 4; // 2 inputs, 1 hidden, 1 output
        let input_count: u32 = 2;
        let output_count: u32 = 1;
        let synapse_count: u32 = 3;
        let order_length: u32 = 2;
        let total_inward: u32 = 3;

        let mut b = Builder::new();
        b.push_u32(neuron_count);
        b.push_u32(input_count);
        b.push_u32(output_count);
        b.push_u32(synapse_count);
        b.push_u32(order_length);
        b.push_u32(total_inward);
        b.push_f64(1.0e-7);
        b.push_u8(0); // normalise_gradients
        b.push_u8(0);
        b.push_u8(0);
        b.push_u8(0);

        // Neurons: 0,1 = input; 2 = hidden; 3 = output.
        let neurons = [
            (NEURON_TYPE_INPUT, 0.6f32, 0.0f32),
            (NEURON_TYPE_INPUT, 0.4, 0.0),
            (NEURON_TYPE_HIDDEN, 0.5, 0.1),
            (NEURON_TYPE_OUTPUT, 0.5, 0.2),
        ];
        for (kind, act, bias) in neurons.iter() {
            b.push_u8(0); // identity squash
            b.push_u8(*kind);
            b.push_u8(1); // propagate_needed
            b.push_u8(1); // update_needed
            b.push_f32(0.0); // hint
            b.push_f32(-1.0e6);
            b.push_f32(1.0e6);
            b.push_f32(*act);
            b.push_f32(*bias);
        }

        // Synapses: 0→2, 1→2, 2→3.
        for &(from, to) in &[(0u32, 2u32), (1u32, 2u32), (2u32, 3u32)] {
            b.push_u32(from);
            b.push_u32(to);
            b.push_f32(0.5);
            b.push_f32(0.5);
            b.push_u8(0);
            b.push_u8(0);
            b.push_u8(0);
            b.push_u8(0);
        }

        // Inward mapping: inputs have 0, hidden has 2, output has 1.
        for &(start, count) in &[(0u32, 0u32), (0u32, 0u32), (0u32, 2u32), (2u32, 1u32)] {
            b.push_u32(start);
            b.push_u32(count);
        }
        // Inward indices: synapses 0,1 → hidden; synapse 2 → output.
        for i in 0u32..3 {
            b.push_u32(i);
        }
        // Reverse topo order: output first, then hidden.
        b.push_u32(3);
        b.push_u32(2);
        // Expected output.
        b.push_f32(1.0);

        let decoded = decode_propagate_buffer(&b.bytes).expect("decode");
        let input = decoded.as_input();

        // This call would have panicked / trapped pre-fix because the
        // offsets would have shifted reverse_topo_order off the end of
        // the buffer and into junk neuron indices ≥ neuron_count.
        let output = propagate_topological_loop(&input);
        assert_eq!(output.neurons.len(), neuron_count as usize);
        assert_eq!(output.synapses.len(), synapse_count as usize);

        // Encoding must produce the documented stride.
        let packed = encode_propagate_output(&output);
        assert_eq!(
            packed.len(),
            neuron_count as usize * PER_NEURON_OUT_F64S
                + synapse_count as usize * PER_SYNAPSE_OUT_F64S
        );
    }

    #[test]
    fn empty_buffer_returns_header_too_short() {
        match decode_propagate_buffer(&[]) {
            Err(DecodeError::HeaderTooShort) => {}
            other => panic!("expected HeaderTooShort, got {:?}", other.err()),
        }
    }
}
