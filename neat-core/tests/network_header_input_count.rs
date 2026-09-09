//! Header input-count contract for `CompiledNetwork::new` (Issue #601).
//!
//! `new` decodes `num_neurons` and `num_inputs` from the 8-byte header of a
//! host-supplied buffer and then computes `num_neurons - num_inputs`. That is a
//! `usize` subtraction, so a header declaring more inputs than nodes wrapped to
//! a value near `usize::MAX` under the release profile's Cargo-default
//! `overflow-checks = false` and aborted the WASM module inside
//! `Vec::with_capacity`. The loader must refuse such a header with a typed
//! error instead.

use neat_core::network::MAX_NODE_COUNT;
use neat_core::{CompiledNetwork, NetworkError};

/// Build the 8-byte little-endian header `new` reads before anything else.
fn header(num_neurons: u32, num_inputs: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&num_neurons.to_le_bytes());
    bytes.extend_from_slice(&num_inputs.to_le_bytes());
    bytes
}

/// One malformed header the loader must refuse, with the case name it is
/// reported and cited under.
struct HeaderCase {
    /// Case name, reported by
    /// [`every_header_declaring_more_inputs_than_neurons_is_refused`].
    name: &'static str,
    /// Declared node count in the header.
    num_neurons: u32,
    /// Declared input count in the header; larger than `num_neurons` in every
    /// case here, which is exactly what used to underflow.
    num_inputs: u32,
}

/// The attacker-reachable shapes of `num_inputs > num_neurons`: the minimal
/// payload from the report, the smallest off-by-one, and the top of the range a
/// `u32` header field can carry.
const UNDERFLOWING_HEADERS: &[HeaderCase] = &[
    HeaderCase {
        name: "rejects_one_input_declared_against_zero_neurons",
        num_neurons: 0,
        num_inputs: 1,
    },
    HeaderCase {
        name: "rejects_one_more_input_than_neurons",
        num_neurons: 8,
        num_inputs: 9,
    },
    HeaderCase {
        name: "rejects_an_input_count_at_the_top_of_the_u32_range",
        num_neurons: MAX_NODE_COUNT as u32,
        num_inputs: u32::MAX,
    },
];

#[test]
fn every_header_declaring_more_inputs_than_neurons_is_refused() {
    for case in UNDERFLOWING_HEADERS {
        let result = CompiledNetwork::new(&header(case.num_neurons, case.num_inputs));
        match result {
            Err(NetworkError::InvalidInputCount {
                num_inputs,
                num_neurons,
            }) => {
                assert_eq!(
                    (num_inputs, num_neurons),
                    (case.num_inputs as usize, case.num_neurons as usize),
                    "{}: the refusal must name both declared counts",
                    case.name
                );
            }
            Err(other) => panic!("{}: expected InvalidInputCount, got {other:?}", case.name),
            Ok(_) => panic!("{}: a malformed header must not load", case.name),
        }
    }
}

#[test]
fn a_header_declaring_every_node_an_input_still_loads() {
    // The accepting edge of the same comparison: `num_inputs == num_neurons` is
    // an all-input network with no non-input neurons, so it loads from the
    // header alone. The guard therefore refuses only the underflowing case
    // rather than tightening what a valid buffer may declare.
    let net = CompiledNetwork::new(&header(3, 3)).expect("an all-input network must load");
    assert_eq!(net.num_neurons(), 3);
    assert_eq!(net.num_inputs(), 3);
}
