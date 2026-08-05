//! Parity and safety coverage for the Issue #510 experimental unchecked
//! aggregate kernels.
//!
//! The whole file is gated on the prototype feature: with the feature off the
//! module does not exist and there is nothing to compare.
#![cfg(feature = "experimental-aggregate-unchecked")]

use neat_core::aggregate_experiment::{
    aggregate_forward_safe, aggregate_forward_safe_indexed, aggregate_forward_unchecked,
    aggregate_traced_safe, aggregate_traced_safe_span, aggregate_traced_unchecked,
};
use neat_core::{CompiledNetwork, NeuronData, SquashType, SynapseData, apply_limit_range};

/// Small deterministic PRNG so the fixtures are reproducible.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn next_signed(&mut self) -> f32 {
        (self.next_u64() % 2001) as f32 / 1000.0 - 1.0
    }

    fn next_below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }
}

const AGGREGATES: [SquashType; 3] = [SquashType::Minimum, SquashType::Maximum, SquashType::If];

/// An all-aggregate feedforward network: every non-input neuron is Minimum,
/// Maximum or If, fan-in varies (including zero, to cover the empty-span
/// branch) and synapse types cover Condition/Positive/Negative/Standard.
fn build_aggregate_network(seed: u64, num_inputs: usize, num_non_inputs: usize) -> CompiledNetwork {
    let mut rng = Lcg::new(seed);
    let num_neurons = num_inputs + num_non_inputs;
    let mut neurons = Vec::with_capacity(num_non_inputs);
    let mut synapses: Vec<SynapseData> = Vec::new();

    for n in 0..num_non_inputs {
        let global_idx = num_inputs + n;
        // 0..=5 so some neurons have an empty span.
        let fan_in = rng.next_below(6);
        let start_synapse = synapses.len() as u32;
        for k in 0..fan_in {
            synapses.push(SynapseData {
                weight: rng.next_signed() * 1.5,
                from_index: rng.next_below(global_idx) as u16,
                // 0 Standard, 1 Condition, 2 Positive, 3 Negative (cycled so the
                // If arm sees every branch).
                synapse_type: (k % 4) as u8,
            });
        }
        neurons.push(NeuronData {
            bias: rng.next_signed() * 0.5,
            start_synapse,
            num_synapses: fan_in as u16,
            squash_type: AGGREGATES[n % AGGREGATES.len()] as u8,
            is_constant: false,
        });
    }

    let estimated_trace_size = (num_non_inputs / 10).max(1) * 2 + 1;
    CompiledNetwork {
        num_neurons,
        num_inputs,
        neurons,
        synapses,
        activations: vec![0.0; num_neurons],
        hint_values_buffer: vec![0.0; num_non_inputs],
        trace_data_buffer: Vec::with_capacity(estimated_trace_size),
        batch_activations: std::array::from_fn(|_| vec![0.0; num_neurons]),
        batch_hints: std::array::from_fn(|_| vec![0.0; num_non_inputs]),
        batch_traces: std::array::from_fn(|_| Vec::with_capacity(estimated_trace_size)),
    }
}

/// Reference forward pass built from the *checked* kernel only — the control
/// the prototype must reproduce bit-for-bit.
fn reference_forward(net: &CompiledNetwork, input: &[f32]) -> Vec<f32> {
    let mut act = vec![0.0f32; net.num_neurons];
    let in_len = input.len().min(net.num_inputs);
    act[..in_len].copy_from_slice(&input[..in_len]);

    for (neuron_idx, neuron) in net.neurons.iter().enumerate() {
        let squash = SquashType::from(neuron.squash_type);
        let value = aggregate_forward_safe(&net.synapses, &act, neuron, squash)
            .expect("fixture is all-aggregate");
        act[net.num_inputs + neuron_idx] = apply_limit_range(squash, value);
    }
    act
}

#[test]
fn unchecked_forward_kernel_is_bit_identical_to_the_checked_reference() {
    let net = build_aggregate_network(0x51_0A, 8, 48);
    let mut rng = Lcg::new(0x51_0B);
    let input: Vec<f32> = (0..net.num_inputs).map(|_| rng.next_signed()).collect();
    let act = reference_forward(&net, &input);

    for neuron in &net.neurons {
        let squash = SquashType::from(neuron.squash_type);
        let safe = aggregate_forward_safe(&net.synapses, &act, neuron, squash)
            .expect("fixture is all-aggregate");
        // SAFETY: every `from_index` is drawn below the neuron's own global
        // index, which is < `act.len()`.
        let prototype = unsafe { aggregate_forward_unchecked(&net.synapses, &act, neuron, squash) }
            .expect("fixture is all-aggregate");
        assert_eq!(
            safe.to_bits(),
            prototype.to_bits(),
            "forward parity failed for {squash:?}"
        );
    }
}

#[test]
fn unchecked_traced_kernel_reproduces_activation_and_trace_info() {
    let net = build_aggregate_network(0x51_0C, 8, 48);
    let mut rng = Lcg::new(0x51_0D);
    let input: Vec<f32> = (0..net.num_inputs).map(|_| rng.next_signed()).collect();
    let act = reference_forward(&net, &input);

    for neuron in &net.neurons {
        let squash = SquashType::from(neuron.squash_type);
        let (safe_act, safe_trace) = aggregate_traced_safe(&net.synapses, &act, neuron, squash)
            .expect("fixture is all-aggregate");
        // SAFETY: as above — every `from_index` is in range for `act`.
        let (proto_act, proto_trace) =
            unsafe { aggregate_traced_unchecked(&net.synapses, &act, neuron, squash) }
                .expect("fixture is all-aggregate");
        assert_eq!(
            safe_act.to_bits(),
            proto_act.to_bits(),
            "traced activation parity failed for {squash:?}"
        );
        assert_eq!(
            safe_trace.to_bits(),
            proto_trace.to_bits(),
            "trace info parity failed for {squash:?}"
        );
    }
}

#[test]
fn attribution_controls_are_bit_identical_to_the_shipped_reference() {
    // The two safe attribution arms only change how the synapse span is walked,
    // so they must agree with the reference bit-for-bit — otherwise a measured
    // delta between them would be a semantic change, not an iteration-style one.
    let net = build_aggregate_network(0x51_12, 8, 48);
    let mut rng = Lcg::new(0x51_13);
    let input: Vec<f32> = (0..net.num_inputs).map(|_| rng.next_signed()).collect();
    let act = reference_forward(&net, &input);

    for neuron in &net.neurons {
        let squash = SquashType::from(neuron.squash_type);
        let reference =
            aggregate_forward_safe(&net.synapses, &act, neuron, squash).expect("aggregate");
        let indexed =
            aggregate_forward_safe_indexed(&net.synapses, &act, neuron, squash).expect("aggregate");
        assert_eq!(
            reference.to_bits(),
            indexed.to_bits(),
            "indexed forward control diverged for {squash:?}"
        );

        let (traced, trace_info) =
            aggregate_traced_safe(&net.synapses, &act, neuron, squash).expect("aggregate");
        let (span, span_info) =
            aggregate_traced_safe_span(&net.synapses, &act, neuron, squash).expect("aggregate");
        assert_eq!(
            traced.to_bits(),
            span.to_bits(),
            "span traced control diverged for {squash:?}"
        );
        assert_eq!(
            trace_info.to_bits(),
            span_info.to_bits(),
            "span traced control trace info diverged for {squash:?}"
        );
    }
}

#[test]
fn empty_span_yields_the_bias_on_both_kernels() {
    let act = [0.5f32, -0.25, 1.0];
    for squash in AGGREGATES {
        let neuron = NeuronData {
            bias: 0.75,
            start_synapse: 0,
            num_synapses: 0,
            squash_type: squash as u8,
            is_constant: false,
        };
        let safe = aggregate_forward_safe(&[], &act, &neuron, squash).expect("aggregate");
        // SAFETY: an empty span dereferences nothing.
        let prototype = unsafe { aggregate_forward_unchecked(&[], &act, &neuron, squash) }
            .expect("aggregate");
        assert_eq!(safe.to_bits(), 0.75f32.to_bits());
        assert_eq!(prototype.to_bits(), safe.to_bits());

        let (traced_safe, trace_info) =
            aggregate_traced_safe(&[], &act, &neuron, squash).expect("aggregate");
        // SAFETY: as above.
        let (traced_proto, proto_info) =
            unsafe { aggregate_traced_unchecked(&[], &act, &neuron, squash) }.expect("aggregate");
        assert_eq!(traced_safe.to_bits(), traced_proto.to_bits());
        assert_eq!(trace_info.to_bits(), proto_info.to_bits());
    }
}

#[test]
fn ties_keep_the_first_winning_synapse_on_both_kernels() {
    // Three synapses whose weighted values are all exactly 1.0: the winning
    // local index must stay 0 on both kernels (strict `<` / `>` comparison).
    let act = [1.0f32, 1.0, 1.0];
    let synapses: Vec<SynapseData> = (0..3)
        .map(|i| SynapseData {
            weight: 1.0,
            from_index: i,
            synapse_type: 0,
        })
        .collect();

    for squash in [SquashType::Minimum, SquashType::Maximum] {
        let neuron = NeuronData {
            bias: 0.0,
            start_synapse: 0,
            num_synapses: 3,
            squash_type: squash as u8,
            is_constant: false,
        };
        let (_, safe_idx) = aggregate_traced_safe(&synapses, &act, &neuron, squash).expect("agg");
        // SAFETY: `from_index` 0..3 is in range for a 3-element buffer.
        let (_, proto_idx) =
            unsafe { aggregate_traced_unchecked(&synapses, &act, &neuron, squash) }.expect("agg");
        assert_eq!(safe_idx, 0.0, "{squash:?} tie should keep the first index");
        assert_eq!(proto_idx, safe_idx);
    }
}

#[test]
fn non_prototyped_squashes_are_not_claimed_by_either_kernel() {
    let act = [0.5f32];
    let neuron = NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 0,
        squash_type: SquashType::Mean as u8,
        is_constant: false,
    };
    for squash in [
        SquashType::Mean,
        SquashType::Hypotenuse,
        SquashType::HypotenuseV2,
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Identity,
    ] {
        assert!(aggregate_forward_safe(&[], &act, &neuron, squash).is_none());
        // SAFETY: returns before touching the buffer for non-prototyped types.
        assert!(unsafe { aggregate_forward_unchecked(&[], &act, &neuron, squash) }.is_none());
        assert!(aggregate_traced_safe(&[], &act, &neuron, squash).is_none());
        // SAFETY: as above.
        assert!(unsafe { aggregate_traced_unchecked(&[], &act, &neuron, squash) }.is_none());
    }
}

#[test]
#[should_panic(expected = "range end index")]
fn malformed_synapse_span_still_panics_instead_of_reading_out_of_bounds() {
    // `start_synapse..end_synapse` runs past the synapse array: the prototype
    // must fall back to the checked reference and panic, never read unchecked.
    let act = [0.5f32, 0.25];
    let synapses = [SynapseData {
        weight: 1.0,
        from_index: 0,
        synapse_type: 0,
    }];
    let neuron = NeuronData {
        bias: 0.0,
        start_synapse: 0,
        num_synapses: 9,
        squash_type: SquashType::Maximum as u8,
        is_constant: false,
    };
    // SAFETY: every `from_index` present in `synapses` is in range for `act`;
    // the span itself is deliberately malformed, which the kernel validates.
    let _ = unsafe {
        aggregate_forward_unchecked(&synapses, &act, &neuron, SquashType::Maximum)
    };
}

#[test]
fn out_of_range_source_index_is_still_rejected_at_load_time() {
    // The unchecked gather's precondition is upheld by `CompiledNetwork::new`.
    // Header: num_neurons = 2, num_inputs = 1; one Maximum neuron whose single
    // synapse points at node 7, which does not exist.
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    // Neuron header: f64 bias, u8 squash_type, u8 is_constant, u16 num_synapses.
    buf.extend_from_slice(&0.0f64.to_le_bytes());
    buf.push(SquashType::Maximum as u8);
    buf.push(0);
    buf.extend_from_slice(&1u16.to_le_bytes());
    // Synapse record: u16 from_index, u8 synapse_type, u8 padding, f64 weight.
    buf.extend_from_slice(&7u16.to_le_bytes());
    buf.push(0);
    buf.push(0);
    buf.extend_from_slice(&1.0f64.to_le_bytes());

    let Err(err) = CompiledNetwork::new(&buf) else {
        panic!("out-of-range from_index must be rejected at load time");
    };
    assert!(
        format!("{err:?}").contains("InvalidSynapseIndex"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn activate_matches_the_checked_reference_on_an_aggregate_network() {
    let mut net = build_aggregate_network(0x51_0E, 8, 48);
    let mut rng = Lcg::new(0x51_0F);
    let input: Vec<f32> = (0..net.num_inputs).map(|_| rng.next_signed()).collect();

    let expected = reference_forward(&net, &input);
    let outputs = net.activate(&input, 4);

    let output_start = net.num_neurons - 4;
    for (i, value) in outputs.iter().enumerate() {
        assert_eq!(
            value.to_bits(),
            expected[output_start + i].to_bits(),
            "activate output {i} diverged from the checked reference"
        );
    }
    // Whole-buffer parity, not just the outputs.
    for (idx, (got, want)) in net.activations.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got.to_bits(), want.to_bits(), "activation {idx} diverged");
    }
}

#[test]
fn activate_and_trace_matches_the_checked_reference_including_trace_data() {
    let mut net = build_aggregate_network(0x51_10, 8, 48);
    let mut rng = Lcg::new(0x51_11);
    let input: Vec<f32> = (0..net.num_inputs).map(|_| rng.next_signed()).collect();
    let num_outputs = 4;
    let num_non_inputs = net.num_neurons - net.num_inputs;

    // Reference: checked kernels only, reproducing activate_and_trace's layout.
    let mut act = vec![0.0f32; net.num_neurons];
    act[..net.num_inputs].copy_from_slice(&input);
    let mut expected_hints = vec![0.0f32; num_non_inputs];
    let mut expected_trace: Vec<f32> = Vec::new();
    for (neuron_idx, neuron) in net.neurons.iter().enumerate() {
        let squash = SquashType::from(neuron.squash_type);
        let (value, trace_info) = aggregate_traced_safe(&net.synapses, &act, neuron, squash)
            .expect("fixture is all-aggregate");
        let limited = apply_limit_range(squash, value);
        act[net.num_inputs + neuron_idx] = limited;
        expected_hints[neuron_idx] = limited;
        expected_trace.push(neuron_idx as f32);
        expected_trace.push(trace_info);
    }
    expected_trace.push(-1.0);

    let result = net.activate_and_trace(&input, num_outputs);

    let output_start = net.num_neurons - num_outputs;
    for i in 0..num_outputs {
        assert_eq!(result[i].to_bits(), act[output_start + i].to_bits());
    }
    for i in 0..num_non_inputs {
        assert_eq!(
            result[num_outputs + i].to_bits(),
            act[net.num_inputs + i].to_bits(),
            "traced activation {i} diverged"
        );
        assert_eq!(
            result[num_outputs + num_non_inputs + i].to_bits(),
            expected_hints[i].to_bits(),
            "hint value {i} diverged"
        );
    }
    let trace = &result[num_outputs + 2 * num_non_inputs..];
    assert_eq!(trace.len(), expected_trace.len(), "trace length diverged");
    for (i, (got, want)) in trace.iter().zip(expected_trace.iter()).enumerate() {
        assert_eq!(got.to_bits(), want.to_bits(), "trace entry {i} diverged");
    }
}
