// Tests for the wasm32/wasm64 numeric-parity fixture and comparator — Issue #541.
//
// The end-to-end parity gate needs two *built* bundles, so it runs in CI
// (`wasm-bundle.yml`) rather than here. What these tests pin is the part that
// can silently go vacuous: a fixture that stops reaching the SIMD kernels, or a
// comparator that reports agreement it never checked.
//
// Run: deno test tests/wasm_arch_parity_test.ts

import { assert, assertEquals } from "jsr:@std/assert@1";
import { compare, type Observations } from "../scripts/check_wasm_arch_parity.ts";
import {
  bitPatterns,
  buildParityInput,
  buildParityNetwork,
  buildParityRecords,
  NUM_INPUTS,
  NUM_OUTPUTS,
  NUM_RECORDS,
  scalarBitPattern,
  SQUASH,
} from "./wasm_arch_parity_fixture.ts";

/** Decode the fixture buffer back into the shape `CompiledNetwork::new` reads. */
function decodeNetwork(bytes: Uint8Array) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const numNeurons = view.getUint32(0, true);
  const numInputs = view.getUint32(4, true);
  let o = 8;
  const neurons: { bias: number; squash: number; synapses: { from: number; weight: number }[] }[] =
    [];
  for (let n = numInputs; n < numNeurons; n++) {
    const bias = view.getFloat64(o, true);
    const squash = view.getUint8(o + 8);
    const count = view.getUint16(o + 10, true);
    o += 12;
    const synapses = [];
    for (let s = 0; s < count; s++) {
      synapses.push({ from: view.getUint16(o, true), weight: view.getFloat64(o + 4, true) });
      o += 12;
    }
    neurons.push({ bias, squash, synapses });
  }
  return { numNeurons, numInputs, neurons, consumed: o };
}

Deno.test("the fixture network serialises to exactly the bytes it declares", () => {
  const bytes = buildParityNetwork();
  const net = decodeNetwork(bytes);
  assertEquals(net.numInputs, NUM_INPUTS);
  assertEquals(net.numNeurons, NUM_INPUTS + net.neurons.length);
  // No trailing slack: a mis-sized buffer would leave `CompiledNetwork::new`
  // parsing padding as a synapse.
  assertEquals(net.consumed, bytes.length);
});

Deno.test("every source index in the fixture is in range for the unchecked gather", () => {
  const net = decodeNetwork(buildParityNetwork());
  for (const neuron of net.neurons) {
    for (const s of neuron.synapses) {
      assert(
        s.from < net.numNeurons,
        `from_index ${s.from} must be < num_neurons ${net.numNeurons}`,
      );
    }
  }
});

Deno.test("the fixture reaches the full chunk-walk: 8-chunk, remainder and below-threshold spans", () => {
  const counts = decodeNetwork(buildParityNetwork()).neurons.map((n) => n.synapses.length);
  assert(counts.some((c) => c >= 8), `no span reaches the 8-chunk walk: ${counts}`);
  assert(counts.some((c) => c % 4 !== 0), `no span leaves a 0..3 remainder: ${counts}`);
  assert(counts.some((c) => c >= 4 && c < 8), `no span sits in the single-chunk tier: ${counts}`);
});

Deno.test("the fixture exercises both inline-squash tiers and the aggregate set", () => {
  const squashes = new Set(decodeNetwork(buildParityNetwork()).neurons.map((n) => n.squash));
  for (const inline of [SQUASH.IDENTITY, SQUASH.RELU, SQUASH.LOGISTIC, SQUASH.TANH]) {
    assert(squashes.has(inline), `inline squash ${inline} missing from the fixture`);
  }
  assert(squashes.has(SQUASH.GELU), "no squash outside the inline set");
  for (const aggregate of [SQUASH.MINIMUM, SQUASH.MAXIMUM, SQUASH.HYPOTENUSE, SQUASH.MEAN]) {
    assert(squashes.has(aggregate), `aggregate squash ${aggregate} missing from the fixture`);
  }
});

Deno.test("fixture weights are non-degenerate and mixed-sign", () => {
  const weights = decodeNetwork(buildParityNetwork()).neurons.flatMap((n) =>
    n.synapses.map((s) => s.weight)
  );
  assert(weights.some((w) => w > 0), "no positive weight");
  assert(weights.some((w) => w < 0), "no negative weight");
  // A constant-weight fixture would let a lane swap cancel; require spread.
  assert(new Set(weights).size > weights.length / 2, "weights are too repetitive to catch a slip");
});

Deno.test("packed records carry the declared stride and record count", () => {
  const records = buildParityRecords();
  const stride = NUM_INPUTS + NUM_OUTPUTS;
  assertEquals(records.length, NUM_RECORDS * stride);
  // Records above 8 with an odd count means the 8-group, the 4-group and the
  // scalar tail all run in one call.
  assert(NUM_RECORDS > 8 && NUM_RECORDS % 8 !== 0, `NUM_RECORDS ${NUM_RECORDS} skips a tier`);
  for (let r = 0; r < NUM_RECORDS; r++) {
    const inputs = records.subarray(r * stride, r * stride + NUM_INPUTS);
    assertEquals([...inputs], [...buildParityInput(r)]);
  }
});

Deno.test("each record has a distinct input vector", () => {
  const seen = new Set(
    Array.from({ length: NUM_RECORDS }, (_, r) => bitPatterns(buildParityInput(r)).join(",")),
  );
  assertEquals(seen.size, NUM_RECORDS);
});

Deno.test("bitPatterns separates values a tolerance comparison would merge", () => {
  // +0 vs -0 compare equal with ===; their bit patterns do not.
  assertEquals(bitPatterns(new Float32Array([0])), ["00000000"]);
  assertEquals(bitPatterns(new Float32Array([-0])), ["80000000"]);
  // Adjacent floats one ulp apart must not collapse.
  const one = bitPatterns(new Float32Array([1]))[0];
  const nextUp = bitPatterns(new Float32Array([1 + 2 ** -23]))[0];
  assert(one !== nextUp, "one-ulp neighbours must differ");
});

Deno.test("scalarBitPattern distinguishes a last-ulp f64 difference", () => {
  assert(scalarBitPattern(0.1) !== scalarBitPattern(0.1 + Number.EPSILON / 8));
  assertEquals(scalarBitPattern(1), "3ff0000000000000");
});

Deno.test("compare reports nothing when both arches agree", () => {
  const left: Observations = new Map([["activate[0]", ["3f800000", "bf000000"]]]);
  const right: Observations = new Map([["activate[0]", ["3f800000", "bf000000"]]]);
  assertEquals(compare(left, right), []);
});

Deno.test("compare names the observation and index of a single differing bit pattern", () => {
  const left: Observations = new Map([["activate[3]", ["3f800000", "bf000000"]]]);
  const right: Observations = new Map([["activate[3]", ["3f800000", "bf000001"]]]);
  const failures = compare(left, right);
  assertEquals(failures.length, 1);
  assert(failures[0].includes("activate[3][1]"), failures[0]);
  assert(failures[0].includes("bf000000") && failures[0].includes("bf000001"), failures[0]);
});

Deno.test("compare fails when an observation exists on only one arch", () => {
  const left: Observations = new Map([["mse_sum_batch_packed", ["3ff0000000000000"]]]);
  const failures = compare(left, new Map());
  assertEquals(failures.length, 1);
  assert(failures[0].includes("mse_sum_batch_packed"), failures[0]);
});

Deno.test("compare fails on a length mismatch instead of comparing the shared prefix", () => {
  const left: Observations = new Map([["activate[0]", ["3f800000", "bf000000"]]]);
  const right: Observations = new Map([["activate[0]", ["3f800000"]]]);
  const failures = compare(left, right);
  assertEquals(failures.length, 1);
  assert(failures[0].includes("length 2 vs 1"), failures[0]);
});
