//! Issue #602 — the `propagate_topological` boundary refuses a header whose
//! declared counts no buffer could satisfy.
//!
//! The five counts are read straight out of an untrusted buffer, so their
//! total size can exceed what a 32-bit `usize` expresses. Summed at the
//! shipped wasm32 width that total wrapped below the buffer's real length,
//! the truncation gate agreed, and the per-record loops indexed off the end
//! of the buffer — a panic that aborts the whole module. These tests pin the
//! observable contract at the public boundary: every such header is refused
//! with `BufferTruncated`, whatever the host's pointer width.

use neat_core::propagate_codec::{
    DecodeError, HEADER_BYTES, INWARD_MAP_STRIDE, NEURON_RECORD_BYTES, SYNAPSE_RECORD_BYTES,
    decode_propagate_buffer,
};

/// A header carrying the six counts, `plank_constant` and the
/// `normalise_gradients` flag — exactly `HEADER_BYTES` long, so it holds none
/// of the sections the counts below declare.
fn header_only(
    neuron_count: u32,
    output_count: u32,
    synapse_count: u32,
    order_length: u32,
    total_inward: u32,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    for field in [
        neuron_count,
        0, // input_count
        output_count,
        synapse_count,
        order_length,
        total_inward,
    ] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(&1e-7f64.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 4]); // normalise_gradients + padding
    assert_eq!(bytes.len(), HEADER_BYTES);
    bytes
}

/// `2^27 × (NEURON_RECORD_BYTES + INWARD_MAP_STRIDE)` is exactly `2^32`, so a
/// 32-bit sum of it comes back as the bare header size — the length of the
/// buffer supplied here.
#[test]
fn a_neuron_count_whose_size_wraps_a_32_bit_usize_is_refused() {
    assert_eq!(NEURON_RECORD_BYTES + INWARD_MAP_STRIDE, 32);
    let buffer = header_only(1 << 27, 0, 0, 0, 0);

    assert_eq!(
        decode_propagate_buffer(&buffer).err(),
        Some(DecodeError::BufferTruncated)
    );
}

/// `u32::MAX × SYNAPSE_RECORD_BYTES` wraps a 32-bit sum *below* the header
/// size, so the pre-fix gate was satisfied by a buffer of any length at all.
#[test]
fn a_synapse_count_no_buffer_could_hold_is_refused() {
    assert_eq!(SYNAPSE_RECORD_BYTES, 20);
    let buffer = header_only(0, 0, u32::MAX, 0, 0);

    assert_eq!(
        decode_propagate_buffer(&buffer).err(),
        Some(DecodeError::BufferTruncated)
    );
}

/// The same wraparound reached through the two flat `u32` arrays and the
/// expected-output section, none of which the neuron and synapse cases cover.
#[test]
fn flat_section_counts_no_buffer_could_hold_are_refused() {
    for (output_count, order_length, total_inward) in
        [(u32::MAX, 0, 0), (0, u32::MAX, 0), (0, 0, u32::MAX)]
    {
        let buffer = header_only(0, output_count, 0, order_length, total_inward);

        assert_eq!(
            decode_propagate_buffer(&buffer).err(),
            Some(DecodeError::BufferTruncated),
            "counts ({output_count}, {order_length}, {total_inward}) must be refused"
        );
    }
}
