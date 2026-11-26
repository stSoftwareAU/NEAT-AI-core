/** Public synthetic TS/WASM baseline; no native-compactor performance claim. */
import { cpus, totalmem } from "node:os";
import {
  LEGACY_COMPACTION_REVISION,
  legacyCompactionSource,
} from "./legacy_compaction_source.ts";

const [checkout, destination, count = "3"] = Deno.args;
const repetitions = Number(count);
if (
  !checkout || !destination || !Number.isInteger(repetitions) || repetitions < 3
) {
  throw new Error(
    "Usage: benchmark_legacy_compaction.ts <frozen NEAT-AI checkout> <report.json> [repetitions >= 3]",
  );
}
const root = await legacyCompactionSource(checkout);
const bundleCheck = await new Deno.Command("bash", {
  args: ["build.sh", "--verify-only"],
  cwd: await Deno.realPath(checkout),
}).output();
if (!bundleCheck.success) {
  throw new Error(
    `Legacy WASM verification failed: ${
      new TextDecoder().decode(bundleCheck.stderr)
    }`,
  );
}
const legacyManifest = JSON.parse(
  await Deno.readTextFile(new URL("deno.json", root)),
);
const { Creature } = await import(new URL("src/Creature.ts", root).href);
const { productionScaleCreature } = await import(
  new URL("bench/_productionScaleCreature.ts", root).href
);
const { compactCreatureVariants } = await import(
  new URL("src/compact/CompactCreature.ts", root).href
);
const { calculate } = await import(
  new URL("src/architecture/Score.ts", root).href
);
const { createSeededRng, setRandomNumberGenerator } = await import(
  new URL("src/utils/RandomNumberGenerator.ts", root).href
);
const { initWasmForTests } = await import(
  new URL("test/_initWasm.ts", root).href
);
await initWasmForTests();
const seed = 1910;
const growthCost = 0.0001;
const probes = Array.from(
  { length: 32 },
  (_, r) =>
    Array.from({ length: 8 }, (_, c) => ((r * 7 + c * 11) % 31 - 15) / 15),
);
// Public synthetic targets, independent of either candidate's activation.
const targets = probes.map((_, r) =>
  Array.from({ length: 4 }, (_, c) => ((r * 3 + c * 5) % 17 - 8) / 8)
);
function timed<T>(run: () => T): { value: T; ms: number } {
  const start = performance.now();
  const value = run();
  return { value, ms: performance.now() - start };
}
async function hash(text: string): Promise<string> {
  const bytes = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(text),
  );
  return Array.from(
    new Uint8Array(bytes),
    (x) => x.toString(16).padStart(2, "0"),
  ).join("");
}
setRandomNumberGenerator(createSeededRng(seed));
const generated = timed(() => productionScaleCreature(seed));
const template = generated.value;
const inputText = JSON.stringify(template.exportJSON());
const inputHash = await hash(inputText);
const rows = [];
let reference = "";
for (let iteration = 0; iteration <= repetitions; iteration++) {
  setRandomNumberGenerator(createSeededRng(seed));
  const parsed = timed(() => JSON.parse(inputText));
  const loaded = timed(() => Creature.fromJSON(parsed.value));
  const creature = loaded.value;
  const exported = timed(() => creature.exportJSON());
  const encoded = timed(() => JSON.stringify(exported.value));
  if (encoded.value !== inputText) {
    throw new Error("Input load/export changed the frozen graph");
  }
  const memoryBefore = Deno.memoryUsage();
  const compacted = timed(() => compactCreatureVariants(creature, false));
  const memoryAfter = Deno.memoryUsage();
  if (JSON.stringify(creature.exportJSON()) !== inputText) {
    throw new Error("Compaction changed its input graph");
  }
  const graphs = [];
  for (
    const [name, graph] of [
      ["original", creature],
      ["safe", compacted.value.safe],
      ["aggressive", compacted.value.aggressive],
    ] as const
  ) {
    if (!graph) {
      graphs.push({ name, present: false });
      continue;
    }
    const wire = JSON.stringify(graph.exportJSON());
    // First pass may include lazy graph setup; process/WASM startup is excluded.
    const activate = () =>
      probes.map((p) =>
        Array.from(graph.activate(new Float32Array(p), false)) as number[]
      );
    const cold = timed(activate);
    const warm = timed(activate);
    if (JSON.stringify(cold.value) !== JSON.stringify(warm.value)) {
      throw new Error(`Non-repeatable forward-only activation: ${name}`);
    }
    let squaredError = 0;
    for (let r = 0; r < targets.length; r++) {
      if (warm.value[r].length !== targets[r].length) {
        throw new Error("Output arity mismatch");
      }
      for (let c = 0; c < targets[r].length; c++) {
        const value = warm.value[r][c];
        if (!Number.isFinite(value)) {
          throw new Error(`Non-finite ${name} output`);
        }
        squaredError += (value - targets[r][c]) ** 2;
      }
    }
    const mse = squaredError / (targets.length * targets[0].length);
    const score = timed(() => calculate(graph, mse, growthCost));
    const zeroErrorScore = calculate(graph, 0, growthCost);
    if (![mse, score.value, zeroErrorScore].every(Number.isFinite)) {
      throw new Error(`Non-finite ${name} score`);
    }
    graphs.push({
      name,
      present: true,
      neuronsIncludingInputs: graph.neurons.length,
      synapses: graph.synapses.length,
      wireBytes: new TextEncoder().encode(wire).length,
      sha256: await hash(wire),
      outputs: warm.value,
      mse,
      score: score.value,
      zeroErrorScore,
      activationFirstPassMs: cold.ms,
      activationSecondPassMs: warm.ms,
      scoreMs: score.ms,
    });
  }
  const fingerprint = JSON.stringify(graphs.map((g) => ({
    name: g.name,
    present: g.present,
    sha256: g.sha256,
    outputs: g.outputs,
    mse: g.mse,
    score: g.score,
    zeroErrorScore: g.zeroErrorScore,
  })));
  if (iteration === 0) reference = fingerprint;
  else if (fingerprint !== reference) {
    throw new Error("Candidate or scoring changed between repetitions");
  }
  rows.push({
    iteration,
    warmup: iteration === 0,
    parseMs: parsed.ms,
    loadMs: loaded.ms,
    exportMs: exported.ms,
    stringifyMs: encoded.ms,
    compactVariantsMs: compacted.ms,
    memoryBefore,
    memoryAfter,
    graphs,
  });
  console.error(
    `Completed ${
      iteration === 0 ? "warmup" : `sample ${iteration}/${repetitions}`
    }`,
  );
}
const report = {
  schema: 1,
  measuredAt: new Date().toISOString(),
  wasmBundleVerified: true,
  declaredNeatCore: legacyManifest.neatCore,
  revision: LEGACY_COMPACTION_REVISION,
  seed,
  repetitions,
  warmups: 1,
  runtime:
    "legacy TypeScript compaction with its vendored WASM activation/score primitives",
  environment: {
    deno: Deno.version,
    os: Deno.build.os,
    arch: Deno.build.arch,
    cpuModel: cpus()[0]?.model,
    logicalCpus: cpus().length,
    hostMemoryBytes: totalmem(),
  },
  generator: "bench/_productionScaleCreature.ts:productionScaleCreature",
  generatorMs: generated.ms,
  inputSha256: inputHash,
  inputWireBytes: new TextEncoder().encode(inputText).length,
  entryPoint:
    "compactCreatureVariants(creature, false); mcmcTemperature omitted",
  growthCost,
  probes,
  targets,
  memoryCaveat:
    "Process snapshots, not allocations or peak memory; GC and prior samples affect them.",
  scoreCaveat:
    "32 public synthetic rows; not representative data, acceptance, or external scorer parity.",
  rows,
};
await Deno.writeTextFile(destination, JSON.stringify(report, null, 2) + "\n");
