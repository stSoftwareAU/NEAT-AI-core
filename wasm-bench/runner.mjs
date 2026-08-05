// Issue #509 — interleaved A/B driver for the wasm gather4 benchmark harness.
//
// Instantiates the safe control and the `unchecked-gather4` prototype in the
// SAME Node process and alternates between them sample by sample, flipping the
// order every sample so run-order bias cancels. Both modules therefore see the
// same machine conditions — the only way to get a usable signal on a loaded
// host.
//
// Usage:
//   node runner.mjs <control.wasm> <unchecked.wasm> <samples> <shape> <records>
//
// Emits CSV rows on stdout: variant,bench,sample,nanos,checksum_bits.

import { readFile } from "node:fs/promises";

const [controlPath, uncheckedPath, samplesArg, shapeArg, recordsArg, sessionArg] = process.argv
  .slice(2);
if (!controlPath || !uncheckedPath) {
  console.error(
    "usage: node runner.mjs <control.wasm> <unchecked.wasm> [samples] [shape] [records]",
  );
  process.exit(2);
}

const SAMPLES = Number(samplesArg ?? 15);
const SHAPE = Number(shapeArg ?? 5); // NETWORKS index; 5 = production_exact
const RECORDS = Number(recordsArg ?? 4096);
const SESSION = Number(sessionArg ?? 0);

// The module carries no real imports (no WASI, no wasm-bindgen glue is
// reachable), but stub anything the linker left behind rather than failing
// instantiation — and fail loud if a stub is ever actually called.
async function instantiate(path) {
  const module = await WebAssembly.compile(await readFile(path));
  const imports = {};
  for (const { module: m, name } of WebAssembly.Module.imports(module)) {
    imports[m] ??= {};
    imports[m][name] = () => {
      throw new Error(`harness called unexpected import ${m}.${name}`);
    };
  }
  const instance = await WebAssembly.instantiate(module, imports);
  return instance.exports;
}

const variants = {
  control: await instantiate(controlPath),
  unchecked: await instantiate(uncheckedPath),
};

const topology = {};
for (const [label, exports] of Object.entries(variants)) {
  const synapses = exports.setup(SHAPE, RECORDS);
  topology[label] = {
    synapses,
    neurons: exports.neuron_count(),
    inputs: exports.input_count(),
    records: exports.record_count(),
  };
}
console.error("fixture:", JSON.stringify(topology.control));

const BENCHES = ["kernel", "activate", "score"];
const call = (exports, bench) => {
  if (bench === "kernel") {
    exports.seed_activations();
    const t = process.hrtime.bigint();
    const checksum = exports.bench_kernel();
    return [process.hrtime.bigint() - t, checksum];
  }
  const t = process.hrtime.bigint();
  const checksum = bench === "activate" ? exports.bench_activate() : exports.bench_score();
  return [process.hrtime.bigint() - t, checksum];
};

const bits = (f) => {
  const buf = new DataView(new ArrayBuffer(8));
  buf.setFloat64(0, f);
  return buf.getBigUint64(0).toString(16).padStart(16, "0");
};

// Warm-up: V8 tiers wasm up in the background (Liftoff → TurboFan), so the
// first samples would otherwise measure the baseline compiler.
for (let w = 0; w < 3; w++) {
  for (const bench of BENCHES) {
    for (const label of Object.keys(variants)) call(variants[label], bench);
  }
}

const rows = [];
for (let sample = 0; sample < SAMPLES; sample++) {
  // Flip the order every sample so neither variant is systematically first.
  const order = sample % 2 === 0 ? ["control", "unchecked"] : ["unchecked", "control"];
  for (const bench of BENCHES) {
    for (const label of order) {
      const [nanos, checksum] = call(variants[label], bench);
      rows.push(`${label},${bench},${sample},${nanos},${bits(checksum)},${SESSION}`);
    }
  }
}
console.log(rows.join("\n"));
