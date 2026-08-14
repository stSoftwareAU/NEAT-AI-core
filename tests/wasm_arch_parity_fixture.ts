// Committed numeric-parity fixture for the wasm32 / wasm64 bundles — Issue #541.
//
// The fixture is *code*, not a checked-in binary: the network buffer and the
// record batch are assembled here from named constants, so a reviewer can see
// exactly which kernels the parity check exercises and a diff shows a changed
// weight instead of a changed blob.
//
// Coverage is deliberate. Widening `cfg(target_arch = "wasm32")` to the wasm
// family means the SIMD kernels in `simd.rs` now compile for a second address
// size, so the fixture has to reach them:
//
//   * synapse spans of 4, 8 and 12 — the 4-wide gather, the dual-accumulator
//     8-chunk walk, and a 0..3 remainder that must continue (not restart) the
//     running accumulator;
//   * both inline-squash tiers: Identity/ReLU/Logistic/Tanh (branched inline)
//     and Gelu (deferred to `apply_squash`);
//   * the aggregate squashes that bypass the lane-vectorised path entirely —
//     Minimum, Maximum, Hypotenuse and Mean;
//   * a mixed-sign input distribution, so a lane or stride slip cannot cancel.

/** Squash-type ids, mirroring `SquashType::from_u8` in `neat-core/src/squash.rs`. */
export const SQUASH = {
  IDENTITY: 0,
  RELU: 1,
  LOGISTIC: 6,
  TANH: 7,
  GELU: 13,
  MINIMUM: 32,
  MAXIMUM: 33,
  HYPOTENUSE: 35,
  MEAN: 37,
} as const;

/** Inputs consumed by the fixture network. */
export const NUM_INPUTS = 12;

/** Outputs produced by the fixture network. */
export const NUM_OUTPUTS = 3;

/** Records in the packed batch built by {@link buildParityRecords}. */
export const NUM_RECORDS = 11;

interface Synapse {
  from: number;
  weight: number;
}

interface Neuron {
  bias: number;
  squash: number;
  synapses: Synapse[];
}

/**
 * Deterministic pseudo-random weight in [-1, 1). A tiny LCG keeps the fixture
 * reproducible across runtimes without committing a table of magic numbers.
 */
function lcgWeight(seed: number): number {
  const next = (seed * 1103515245 + 12345) % 2147483648;
  return (next / 2147483648) * 2 - 1;
}

function span(from: number, count: number, seed: number): Synapse[] {
  return Array.from({ length: count }, (_, i) => ({
    from: from + i,
    weight: lcgWeight(seed + i * 7919),
  }));
}

/** The fixture topology: 12 inputs, 6 hidden neurons, 3 outputs. */
function fixtureNeurons(): Neuron[] {
  const hidden: Neuron[] = [
    // 12 synapses: two full 4-lane chunks plus a third — the dual-accumulator walk.
    { bias: 0.25, squash: SQUASH.RELU, synapses: span(0, 12, 1) },
    // 8 synapses: exactly two chunks, no remainder.
    { bias: -0.5, squash: SQUASH.TANH, synapses: span(0, 8, 2) },
    // 7 synapses: one chunk plus a 3-wide remainder that must continue the sum.
    { bias: 0.125, squash: SQUASH.LOGISTIC, synapses: span(2, 7, 3) },
    // 5 synapses under an aggregate squash — bypasses the lane-vectorised path.
    { bias: 0.0, squash: SQUASH.MINIMUM, synapses: span(0, 5, 4) },
    { bias: 0.0, squash: SQUASH.MAXIMUM, synapses: span(4, 6, 5) },
    // Hypotenuse reads the sum-of-squares kernel; Mean reads the no-bias kernel.
    { bias: 0.75, squash: SQUASH.HYPOTENUSE, synapses: span(1, 9, 6) },
  ];
  const hiddenBase = NUM_INPUTS;
  const outputs: Neuron[] = [
    {
      bias: 0.05,
      squash: SQUASH.IDENTITY,
      synapses: span(hiddenBase, hidden.length, 7),
    },
    {
      bias: -0.2,
      squash: SQUASH.GELU,
      synapses: span(hiddenBase, hidden.length, 8),
    },
    {
      bias: 0.4,
      squash: SQUASH.MEAN,
      synapses: span(hiddenBase, hidden.length, 9),
    },
  ];
  return [...hidden, ...outputs];
}

/**
 * Serialise the fixture into the `CompiledNetwork::new` wire format:
 * `u32 num_neurons`, `u32 num_inputs`, then per non-input neuron
 * `f64 bias`, `u8 squash`, `u8 is_constant`, `u16 num_synapses`, and per
 * synapse `u16 from_index`, `u8 synapse_type`, `u8 padding`, `f64 weight`.
 */
export function buildParityNetwork(): Uint8Array<ArrayBuffer> {
  const neurons = fixtureNeurons();
  const numNeurons = NUM_INPUTS + neurons.length;
  const size = 8 +
    neurons.reduce((acc, n) => acc + 12 + n.synapses.length * 12, 0);
  const buffer = new ArrayBuffer(size);
  const view = new DataView(buffer);
  let o = 0;
  view.setUint32(o, numNeurons, true);
  o += 4;
  view.setUint32(o, NUM_INPUTS, true);
  o += 4;
  for (const neuron of neurons) {
    view.setFloat64(o, neuron.bias, true);
    o += 8;
    view.setUint8(o++, neuron.squash);
    view.setUint8(o++, 0); // is_constant
    view.setUint16(o, neuron.synapses.length, true);
    o += 2;
    for (const s of neuron.synapses) {
      view.setUint16(o, s.from, true);
      o += 2;
      view.setUint8(o++, 0); // synapse_type CONDITION/NORMAL
      view.setUint8(o++, 0); // padding
      view.setFloat64(o, s.weight, true);
      o += 8;
    }
  }
  return new Uint8Array(buffer);
}

/** Input values for record `index`, spanning both signs and a zero. */
export function buildParityInput(index: number): Float32Array {
  const input = new Float32Array(NUM_INPUTS);
  for (let i = 0; i < NUM_INPUTS; i++) {
    // Alternating signs with a per-record offset: a lane swap changes the sum.
    input[i] = ((i % 2 === 0) ? 1 : -1) * ((i + 1) * 0.125 + index * 0.0625);
  }
  input[index % NUM_INPUTS] = 0; // one exact zero, rotated per record
  return input;
}

/**
 * The packed `[inputs…, targets…]` batch the `*_sum_batch_packed` loss entry
 * points scan. Stride is `NUM_INPUTS + NUM_OUTPUTS`; {@link NUM_RECORDS} is odd
 * and above 8 so the 8-record group, the 4-record group and the scalar tail all
 * run in one call.
 */
export function buildParityRecords(): Float32Array {
  const stride = NUM_INPUTS + NUM_OUTPUTS;
  const records = new Float32Array(NUM_RECORDS * stride);
  for (let r = 0; r < NUM_RECORDS; r++) {
    records.set(buildParityInput(r), r * stride);
    for (let t = 0; t < NUM_OUTPUTS; t++) {
      records[r * stride + NUM_INPUTS + t] = ((t % 2 === 0) ? 0.75 : -0.25) + r * 0.03125;
    }
  }
  return records;
}

/** The raw IEEE-754 bit patterns behind a float array, as hex words. */
export function bitPatterns(values: Float32Array | Float64Array): string[] {
  const view = new DataView(values.buffer, values.byteOffset, values.byteLength);
  const out: string[] = [];
  if (values instanceof Float32Array) {
    for (let i = 0; i < values.length; i++) {
      out.push(view.getUint32(i * 4, true).toString(16).padStart(8, "0"));
    }
  } else {
    for (let i = 0; i < values.length; i++) {
      out.push(view.getBigUint64(i * 8, true).toString(16).padStart(16, "0"));
    }
  }
  return out;
}

/** A single f64 scalar's bit pattern, for the loss-sum comparisons. */
export function scalarBitPattern(value: number): string {
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, value, true);
  return view.getBigUint64(0, true).toString(16).padStart(16, "0");
}
