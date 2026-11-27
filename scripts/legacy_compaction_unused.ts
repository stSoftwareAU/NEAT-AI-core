/**
 * Capture the frozen `compactUnused` (src/compact/CompactUnused.ts) for the
 * unused-choice baseline. The frozen functions run unchanged; this module only
 * supplies inputs, pins the two sources of nondeterminism (`crypto.randomUUID`
 * and the global RNG that drives the shuffle and the removal draws), and records
 * what they did. No policy is reimplemented here.
 */

type Rng = {
  readonly seeded: boolean;
  readonly seed: number | null;
  random(): number;
  randomInt(min: number, max: number): number;
  choice<T>(array: readonly T[]): T;
};

type Tag = { name: string; value: string };

type Attempt = { traced: unknown; plankConstant: number };

/** Structural shapes of the frozen creature and trace JSON that this capture reads. */
type TraceState = {
  count: number;
  totalActivation: number;
  maximumActivation: number;
  minimumActivation: number;
};
type GraphNeuron = {
  type: string;
  uuid?: string;
  squash?: string;
  bias: number;
  trace?: TraceState;
  tags?: unknown[];
};
type GraphSynapse = {
  fromUUID?: string;
  toUUID?: string;
  weight: number;
  type?: string;
};
type Graph = { neurons: GraphNeuron[]; synapses: GraphSynapse[] };
/** A live creature: neurons after the inputs, synapses by runtime index. */
type CreatureInstance = {
  input: number;
  neurons: GraphNeuron[];
  synapses: { from: number; to: number; weight: number; type?: string }[];
  traceJSON(): Graph;
};
type CreatureModule = { fromJSON(json: unknown): CreatureInstance };

type CaseSpec = {
  name: string;
  test: string;
  preparation: string;
  seed: number;
  /** Retry (and expect compaction) until a pass compacts, up to `maxAttempts`. */
  retry: boolean;
  /** Whether the frozen compactUnused must (true) or must not (false) compact. */
  expectCompacted: boolean;
  maxAttempts: number;
  /** Builds the per-case state from the recording RNG, then yields one attempt per call. */
  prepare: (rng: Rng) => (attempt: number) => Attempt;
};

/** Records every draw from the wrapped generator, so draw slices can be taken later. */
function recordingRng(
  inner: { random(): number; seeded: boolean; seed: number | null },
  draws: number[],
): Rng {
  const rng: Rng = {
    seeded: inner.seeded,
    seed: inner.seed,
    random(): number {
      const value = inner.random();
      draws.push(value);
      return value;
    },
    randomInt(min: number, max: number): number {
      return Math.floor(rng.random() * (max - min + 1)) + min;
    },
    choice<T>(array: readonly T[]): T {
      if (array.length === 0) {
        throw new Error("Cannot choose from an empty array");
      }
      return array[rng.randomInt(0, array.length - 1)];
    },
  };
  return rng;
}

/** Fail loud when a value cannot survive a JSON round trip unchanged. */
function assertJsonSafe(value: unknown, path: string): void {
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error(`${path} is ${value}; a JSON snapshot cannot carry it`);
    }
    return;
  }
  if (value !== null && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      assertJsonSafe(child, `${path}.${key}`);
    }
  }
}

/** Non-finite numbers become strings, as in the unity capture. */
function numberOrString(value: unknown): number | string {
  if (typeof value !== "number") {
    throw new Error(`expected a number, got ${typeof value}`);
  }
  return Number.isFinite(value) ? value : String(value);
}

/** Pinned tag-free projection used for both aligned and runtime graphs. */
function projectNeuron(n: {
  type: string;
  uuid?: string;
  squash?: string;
  bias: number;
}) {
  return {
    type: n.type,
    uuid: n.uuid ?? null,
    squash: n.squash ?? null,
    bias: numberOrString(n.bias),
  };
}

function projectSynapse(s: {
  fromUUID?: string;
  toUUID?: string;
  weight: number;
  type?: string;
}) {
  return {
    fromUUID: s.fromUUID ?? null,
    toUUID: s.toUUID ?? null,
    weight: numberOrString(s.weight),
    type: s.type ?? null,
  };
}

function projectTrace(trace: TraceState | undefined) {
  if (!trace) return null;
  return {
    count: trace.count,
    totalActivation: numberOrString(trace.totalActivation),
    maximumActivation: numberOrString(trace.maximumActivation),
    minimumActivation: numberOrString(trace.minimumActivation),
  };
}

/** Input neurons are addressed as input-N, as in the unity capture. */
function endpointUUID(
  creature: { input: number; neurons: GraphNeuron[] },
  index: number,
): string | undefined {
  return index < creature.input
    ? `input-${index}`
    : creature.neurons[index].uuid;
}

/**
 * Runs the frozen compactUnused once on `traced` and records everything a
 * native replay needs. `draws` is the recording RNG's log; it is cleared right
 * before the call, so every entry in it afterwards was drawn by compactUnused.
 */
function compactOnce(
  Creature: CreatureModule,
  compactUnused: (
    traced: unknown,
    plankConstant: number,
  ) => CreatureInstance | undefined,
  exportJSONWithRuntimeIds: (creature: CreatureInstance) => Graph,
  CreatureUtil: { shuffle: (array: Int32Array, length?: number) => void },
  newbornGraceRemaining: (neuron: unknown) => number,
  getTag: (target: unknown, name: string) => string | undefined,
  draws: number[],
  traced: unknown,
  plankConstant: number,
) {
  assertJsonSafe(traced, "traced");
  const recorded = JSON.parse(JSON.stringify(traced));

  // The same algorithm the frozen code runs before its own shuffle: the aligned
  // trace is what compactUnused iterates, and the runtime graph is the one it
  // rewrites. Both are built from clones so the input stays untouched.
  const aligned = Creature.fromJSON(JSON.parse(JSON.stringify(traced)))
    .traceJSON();
  const start = Creature.fromJSON(JSON.parse(JSON.stringify(traced)));
  const clean = Creature.fromJSON(exportJSONWithRuntimeIds(start));
  const runtimeGraph = exportJSONWithRuntimeIds(clean);

  const alignedNeurons = aligned.neurons.map((n) => ({
    ...projectNeuron(n),
    trace: projectTrace(n.trace),
  }));
  const alignedSynapses = aligned.synapses.map(projectSynapse);
  const graceRemaining = aligned.neurons.map((n: unknown) =>
    newbornGraceRemaining(n)
  );

  const runtimeNeurons = runtimeGraph.neurons.map(projectNeuron);
  const runtimeSynapses = runtimeGraph.synapses.map(projectSynapse);
  const runtimeMatchesAligned = JSON.stringify([
    alignedNeurons.map((
      n: { type: string; uuid: string | null },
    ) => [n.type, n.uuid]),
    alignedSynapses,
  ]) === JSON.stringify([
    runtimeNeurons.map((
      n: { type: string; uuid: string | null },
    ) => [n.type, n.uuid]),
    runtimeSynapses,
  ]);

  // Pin the UUID of any constant minted, and capture the shuffle and the
  // removal draws that follow it.
  const originalRandomUUID = crypto.randomUUID.bind(crypto);
  const originalShuffle = CreatureUtil.shuffle;
  let minted = 0;
  let shuffleCalls = 0;
  let shuffled: number[] = [];
  let afterShuffle = -1;
  crypto.randomUUID = () =>
    `unity-constant-${minted++}` as `${string}-${string}-${string}-${string}-${string}`;
  CreatureUtil.shuffle = (array: Int32Array, length?: number) => {
    shuffleCalls++;
    originalShuffle.call(CreatureUtil, array, length);
    shuffled = Array.from(array);
    afterShuffle = draws.length;
  };
  draws.length = 0;
  let compacted: CreatureInstance | undefined;
  try {
    compacted = compactUnused(traced, plankConstant);
  } finally {
    crypto.randomUUID = originalRandomUUID;
    CreatureUtil.shuffle = originalShuffle;
  }
  if (shuffleCalls !== 1 || afterShuffle < 0) {
    throw new Error(
      `compactUnused must shuffle exactly once through CreatureUtil.shuffle, saw ${shuffleCalls}`,
    );
  }
  const removalDraws = draws.slice(afterShuffle);

  const result = compacted === undefined ? null : {
    unusedTag: getTag(compacted, "unused") ?? null,
    after: {
      neurons: compacted.neurons.slice(compacted.input).map((n) => ({
        type: n.type,
        uuid: n.uuid,
        squash: n.squash ?? null,
        bias: numberOrString(n.bias),
        tags: n.tags ?? [],
      })),
      synapses: compacted.synapses.map((s) => ({
        fromUUID: endpointUUID(compacted, s.from),
        toUUID: endpointUUID(compacted, s.to),
        weight: numberOrString(s.weight),
        type: s.type ?? null,
      })),
    },
  };

  return {
    compacted: compacted !== undefined,
    capture: {
      plankConstant,
      traced: recorded,
      aligned: { neurons: alignedNeurons, synapses: alignedSynapses },
      runtimeGraphMatchesAligned: runtimeMatchesAligned,
      runtimeGraph: runtimeMatchesAligned
        ? null
        : { neurons: runtimeNeurons, synapses: runtimeSynapses },
      graceRemaining,
      shuffled,
      removalDraws,
      minted,
      result,
      chosenUUID: result === null ? null : result.unusedTag,
    },
  };
}

export async function captureLegacyUnused(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { compactUnused } = await import(
    new URL("src/compact/CompactUnused.ts", root).href
  );
  const { exportJSONWithRuntimeIds } = await import(
    new URL("src/architecture/PopulateRuntimeIdsFromCreature.ts", root).href
  );
  const { CreatureUtil } = await import(
    new URL("src/architecture/CreatureUtils.ts", root).href
  );
  const { newbornGraceRemaining } = await import(
    new URL("src/architecture/NewbornGrace.ts", root).href
  );
  const { createBackPropagationConfig } = await import(
    new URL("src/propagate/BackPropagation.ts", root).href
  );
  const { SparseConfig } = await import(
    new URL("src/propagate/sparse/SparseConfig.ts", root).href
  );
  const {
    createSeededRng,
    getRandomNumberGenerator,
    setRandomNumberGenerator,
  } = await import(new URL("src/utils/RandomNumberGenerator.ts", root).href);
  const { getTag } = await import("@stsoftware/tags/mod");
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();

  /** Runs one case: installs the recording RNG, retries as the original test does, restores the RNG. */
  function captureCase(spec: CaseSpec) {
    const draws: number[] = [];
    const rng = recordingRng(createSeededRng(spec.seed), draws);
    const previous = getRandomNumberGenerator();
    setRandomNumberGenerator(rng);
    try {
      const attemptOf = spec.prepare(rng);
      const limit = spec.retry ? spec.maxAttempts : 1;
      for (let attempts = 1; attempts <= limit; attempts++) {
        const attempt = attemptOf(attempts);
        const outcome = compactOnce(
          Creature,
          compactUnused,
          exportJSONWithRuntimeIds,
          CreatureUtil,
          newbornGraceRemaining,
          getTag,
          draws,
          attempt.traced,
          attempt.plankConstant,
        );
        if (outcome.compacted !== spec.expectCompacted) {
          if (spec.retry && !outcome.compacted) continue;
          throw new Error(
            `${spec.name}: expected compacted=${spec.expectCompacted} on attempt ${attempts}, got ${outcome.compacted}`,
          );
        }
        return {
          name: spec.name,
          revision,
          test: spec.test,
          source: "src/compact/CompactUnused.ts",
          operation: "compactUnused",
          preparation: spec.preparation,
          seed: spec.seed,
          attempts,
          ...outcome.capture,
        };
      }
      throw new Error(
        `${spec.name}: no attempt compacted within ${limit} attempts`,
      );
    } finally {
      setRandomNumberGenerator(previous);
    }
  }

  // ── NewbornGrace.ts: hand-written traces, no data ──────────────────────────
  const PLANK = 1e-7;
  const activeTrace = () => ({
    count: 10,
    totalBias: 0,
    totalAdjustedBias: 0,
    hintValue: 0,
    maximumActivation: 1,
    minimumActivation: 0,
    totalActivation: 5,
  });
  // Copied from test/compact/NewbornGrace.ts tracedCreature(); the quiet neuron
  // carries the grace tag, the loud one never does unless a case sets it.
  const tracedCreature = (quietTags?: Tag[]) => ({
    neurons: [
      {
        type: "hidden",
        uuid: "hidden-quiet",
        squash: "IDENTITY",
        bias: 0,
        tags: quietTags,
        trace: activeTrace(),
      },
      {
        type: "hidden",
        uuid: "hidden-loud",
        squash: "IDENTITY",
        bias: 0,
        trace: activeTrace(),
      },
      {
        type: "output",
        uuid: "output-0",
        squash: "IDENTITY",
        bias: 0,
        trace: activeTrace(),
      },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "hidden-quiet", weight: 1 },
      { fromUUID: "input-1", toUUID: "hidden-loud", weight: 1 },
      { fromUUID: "hidden-quiet", toUUID: "output-0", weight: 1e-6 },
      { fromUUID: "hidden-loud", toUUID: "output-0", weight: 0.9 },
      { fromUUID: "input-0", toUUID: "output-0", weight: 0.5 },
    ],
    input: 2,
    output: 1,
  });
  const newbornCase = (
    name: string,
    seed: number,
    preparation: string,
    traced: () => unknown,
    expectCompacted: boolean,
  ): CaseSpec => ({
    name,
    test: "test/compact/NewbornGrace.ts",
    preparation,
    seed,
    retry: false,
    expectCompacted,
    maxAttempts: 1,
    prepare: () => () => ({ traced: traced(), plankConstant: PLANK }),
  });
  const newbornPrep =
    "Hand-written tracedCreature() copied from test/compact/NewbornGrace.ts; no data is traced. The global RNG is a seeded generator so the shuffle is reproducible.";

  const newborn: CaseSpec[] = [
    newbornCase(
      "newborn_no_grace",
      778001,
      `${newbornPrep} No grace tag on any neuron.`,
      () => tracedCreature(),
      true,
    ),
    newbornCase(
      "newborn_grace_1",
      778002,
      `${newbornPrep} Quiet neuron tagged newborn-grace = "1".`,
      () => tracedCreature([{ name: "newborn-grace", value: "1" }]),
      true,
    ),
    newbornCase(
      "newborn_grace_2",
      778003,
      `${newbornPrep} Quiet neuron tagged newborn-grace = "2".`,
      () => tracedCreature([{ name: "newborn-grace", value: "2" }]),
      true,
    ),
    newbornCase(
      "newborn_grace_0_spent",
      778004,
      `${newbornPrep} Quiet neuron tagged newborn-grace = "0", the second pass of the original two-pass test.`,
      () => tracedCreature([{ name: "newborn-grace", value: "0" }]),
      true,
    ),
    newbornCase(
      "newborn_nothing_to_compact",
      778005,
      `${newbornPrep} Both hidden neurons tagged newborn-grace = "2", so no candidate is left; the original asserts undefined.`,
      () => {
        const trace = tracedCreature([{ name: "newborn-grace", value: "2" }]);
        trace.neurons[1].tags = [{ name: "newborn-grace", value: "2" }];
        return trace;
      },
      false,
    ),
  ];

  // ── UnusedClipped.ts: CLIPPED hidden neuron, 1000 seeded rows ──────────────
  const unusedClipped: CaseSpec = {
    name: "unused_clipped",
    test: "test/compact/UnusedClipped.ts",
    preparation:
      "Creature fixture copied from test/compact/UnusedClipped.ts. The original Math.random() data (1000 rows x 3) is drawn from a seeded generator (seed 778011) instead; the attempt loop, the activateAndTrace/propagate trace and the 1e-6 trace-matches-activation check are kept. The .trace/ files are not written. Retries until compacted, up to 240 attempts.",
    seed: 778011,
    retry: true,
    expectCompacted: true,
    maxAttempts: 240,
    prepare: (rng) => {
      const creature = Creature.fromJSON({
        neurons: [
          { type: "hidden", uuid: "hidden-3", squash: "CLIPPED", bias: 2 },
          { type: "output", squash: "IDENTITY", uuid: "output-0", bias: 1 },
          { type: "output", squash: "IDENTITY", uuid: "output-1", bias: 0 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "hidden-3", weight: -0.3 },
          { fromUUID: "input-1", toUUID: "hidden-3", weight: 0.3 },
          { fromUUID: "hidden-3", toUUID: "output-0", weight: 0.6 },
          { fromUUID: "hidden-3", toUUID: "output-1", weight: 0.7 },
          { fromUUID: "input-2", toUUID: "output-1", weight: 0.8 },
        ],
        input: 3,
        output: 2,
      });
      creature.validate();
      const data: number[][] = [];
      for (let i = 1000; i--;) {
        data.push([rng.random(), rng.random(), rng.random()]);
      }
      const outputs: Float32Array[] = new Array(data.length);
      for (let i = data.length; i--;) {
        outputs[i] = creature.activate(new Float32Array(data[i]), false);
      }
      return (attempt) => {
        if (attempt > 1) creature.clearState();
        const config = createBackPropagationConfig();
        const sparseConfig = new SparseConfig(
          exportJSONWithRuntimeIds(creature),
          config,
        );
        for (let i = data.length; i--;) {
          const actual = creature.activateAndTrace(
            new Float32Array(data[i]),
            false,
            sparseConfig,
          );
          creature.propagate(
            new Float32Array(outputs[i]),
            config,
            sparseConfig,
          );
          for (const k of [0, 1]) {
            if (Math.abs(actual[k] - outputs[i][k]) > 0.000_001) {
              throw new Error(
                `trace ${k} row ${i} differs: actual ${actual[k]}, expected ${
                  outputs[i][k]
                }`,
              );
            }
          }
        }
        return {
          traced: creature.traceJSON(),
          plankConstant: config.plankConstant,
        };
      };
    },
  };

  // ── Cascade.ts: Cosine feeding CLIPPED, 1000 seeded rows, creature rebuilt per attempt ──
  const cascade: CaseSpec = {
    name: "cascade_cosine_clipped",
    test: "test/compact/Cascade.ts",
    preparation:
      "Creature fixture (Cosine -> CLIPPED cascade) copied from test/compact/Cascade.ts and rebuilt each attempt. The original Math.random() data (1000 rows x 3) is drawn once from a seeded generator (seed 778012); the attempt loop and the trace-matches-activation check are kept. The .trace/ files are not written. Retries until compacted, up to 120 attempts.",
    seed: 778012,
    retry: true,
    expectCompacted: true,
    maxAttempts: 120,
    prepare: (rng) => {
      const data: number[][] = [];
      for (let i = 1000; i--;) {
        data.push([rng.random(), rng.random(), rng.random()]);
      }
      return () => {
        const creature = Creature.fromJSON({
          neurons: [
            { type: "hidden", uuid: "hidden-3", squash: "Cosine", bias: 2 },
            { type: "hidden", uuid: "hidden-4", squash: "CLIPPED", bias: 2 },
            { type: "output", squash: "IDENTITY", uuid: "output-0", bias: 1 },
            { type: "output", squash: "IDENTITY", uuid: "output-1", bias: 0 },
          ],
          synapses: [
            { fromUUID: "input-0", toUUID: "hidden-3", weight: -0.3 },
            { fromUUID: "input-1", toUUID: "hidden-3", weight: 0.3 },
            { fromUUID: "hidden-3", toUUID: "hidden-4", weight: -0.5 },
            { fromUUID: "hidden-4", toUUID: "output-0", weight: 0.6 },
            { fromUUID: "hidden-4", toUUID: "output-1", weight: 0.7 },
            { fromUUID: "input-2", toUUID: "output-1", weight: 0.8 },
          ],
          input: 3,
          output: 2,
        });
        creature.validate();
        const outputs: Float32Array[] = new Array(data.length);
        for (let i = data.length; i--;) {
          outputs[i] = creature.activate(new Float32Array(data[i]), false);
        }
        const config = createBackPropagationConfig();
        const sparseConfig = new SparseConfig(
          exportJSONWithRuntimeIds(creature),
          config,
        );
        for (let i = data.length; i--;) {
          const actual = creature.activateAndTrace(
            new Float32Array(data[i]),
            false,
            sparseConfig,
          );
          creature.propagate(outputs[i], config, sparseConfig);
          for (const k of [0, 1]) {
            if (Math.abs(actual[k] - outputs[i][k]) > 0.000_001) {
              throw new Error(
                `trace ${k} row ${i} differs: actual ${actual[k]}, expected ${
                  outputs[i][k]
                }`,
              );
            }
          }
        }
        return {
          traced: creature.traceJSON(),
          plankConstant: config.plankConstant,
        };
      };
    },
  };

  // ── CompactUnusedOrphanCleanup.ts: two tests, 100 seeded rows per attempt ──
  const orphanJson = (
    neurons: { type: string; uuid: string; squash: string; bias: number }[],
    synapses: { fromUUID: string; toUUID: string; weight: number }[],
  ) => ({ neurons, synapses, input: 1, output: 1 });
  const orphanAttempt = (
    rng: Rng,
    json: unknown,
  ): (attempt: number) => Attempt =>
  (_attempt: number) => {
    const fresh = Creature.fromJSON(json);
    const inputCount: number = fresh.input;
    const sampleCount = 100;
    const data: number[][] = [];
    for (let i = sampleCount; i--;) {
      const row: number[] = [];
      for (let j = inputCount; j--;) {
        row.push(rng.random());
      }
      data.push(row);
    }
    const config = createBackPropagationConfig();
    const sparseConfig = new SparseConfig(
      exportJSONWithRuntimeIds(fresh),
      config,
    );
    const outputs: Float32Array[] = new Array(data.length);
    for (let i = data.length; i--;) {
      outputs[i] = fresh.activate(new Float32Array(data[i]), false);
    }
    for (let i = data.length; i--;) {
      fresh.activateAndTrace(new Float32Array(data[i]), false, sparseConfig);
      fresh.propagate(outputs[i], config, sparseConfig);
    }
    return { traced: fresh.traceJSON(), plankConstant: config.plankConstant };
  };
  const orphanFeeder = orphanJson(
    [
      { type: "hidden", uuid: "hidden-A", squash: "IDENTITY", bias: 0.5 },
      { type: "hidden", uuid: "hidden-B", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    [
      { fromUUID: "input-0", toUUID: "hidden-A", weight: 2.0 },
      { fromUUID: "hidden-A", toUUID: "hidden-B", weight: 1.0 },
      { fromUUID: "hidden-B", toUUID: "output-0", weight: 0.001 },
      { fromUUID: "input-0", toUUID: "output-0", weight: 1.0 },
    ],
  );
  const orphanCascade = orphanJson(
    [
      { type: "hidden", uuid: "hidden-A", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "hidden-B", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "hidden-C", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    [
      { fromUUID: "input-0", toUUID: "hidden-A", weight: 2.0 },
      { fromUUID: "hidden-A", toUUID: "hidden-B", weight: 1.0 },
      { fromUUID: "hidden-B", toUUID: "hidden-C", weight: 1.0 },
      { fromUUID: "hidden-C", toUUID: "output-0", weight: 0.001 },
      { fromUUID: "input-0", toUUID: "output-0", weight: 1.0 },
    ],
  );
  const orphanCases: CaseSpec[] = [
    {
      name: "orphan_cleanup_feeder",
      test: "test/compact/CompactUnusedOrphanCleanup.ts",
      preparation:
        "Feeder fixture (input -> A -> B -> output, plus input -> output) copied from the first test of test/compact/CompactUnusedOrphanCleanup.ts. Each attempt builds a fresh creature and traces 100 rows drawn from a seeded generator (seed 778013) instead of Math.random(). Retries until compacted, up to 20 attempts (the original runs all 20 and checks each compacted one).",
      seed: 778013,
      retry: true,
      expectCompacted: true,
      maxAttempts: 20,
      prepare: (rng) => orphanAttempt(rng, orphanFeeder),
    },
    {
      name: "orphan_cleanup_cascade",
      test: "test/compact/CompactUnusedOrphanCleanup.ts",
      preparation:
        "Chain fixture (input -> A -> B -> C -> output, plus input -> output) copied from the second test of test/compact/CompactUnusedOrphanCleanup.ts. Each attempt builds a fresh creature and traces 100 rows drawn from a seeded generator (seed 778014) instead of Math.random(). Retries until compacted, up to 20 attempts (the original runs all 20 and checks each compacted one).",
      seed: 778014,
      retry: true,
      expectCompacted: true,
      maxAttempts: 20,
      prepare: (rng) => orphanAttempt(rng, orphanCascade),
    },
  ];

  // ── FixIF.ts, CompactConstant.ts, CompactKeepOrder.ts: IF and constant neurons ──
  type Trainable = {
    validate(): void;
    activate(input: Float32Array, flag: boolean): Float32Array;
    activateAndTrace(
      input: Float32Array,
      flag: boolean,
      sparse: unknown,
    ): Float32Array;
    propagate(output: Float32Array, config: unknown, sparse: unknown): unknown;
    traceJSON(): unknown;
  };

  /** Rows as the frozen tests draw them: `Math.random() * scale - shift`, from the seeded generator. */
  function signedRows(
    rng: Rng,
    count: number,
    scale: number,
    shift: number,
  ): number[][] {
    const data: number[][] = [];
    for (let i = count; i--;) {
      data.push([
        rng.random() * scale - shift,
        rng.random() * scale - shift,
        rng.random() * scale - shift,
      ]);
    }
    return data;
  }

  /** Fails loud when an output strays from the frozen tests' 1e-6 bound. */
  function assertOutputs(
    actual: ArrayLike<number>,
    expected: Float32Array,
    label: string,
  ) {
    for (const k of [0, 1]) {
      if (Math.abs(actual[k] - expected[k]) > 0.000_001) {
        throw new Error(
          `${label}: output ${k} is ${actual[k]}, expected ${expected[k]}`,
        );
      }
    }
  }

  /** The frozen tests' trace pass: each row is traced and propagated, and its traced outputs checked. */
  function tracePass(
    creature: Trainable,
    data: number[][],
    outputs: Float32Array[],
    config: unknown,
    sparseConfig: unknown,
  ) {
    for (let i = data.length; i--;) {
      const actual = creature.activateAndTrace(
        new Float32Array(data[i]),
        false,
        sparseConfig,
      );
      creature.propagate(new Float32Array(outputs[i]), config, sparseConfig);
      assertOutputs(actual, outputs[i], `trace row ${i}`);
    }
  }

  /** The frozen tests' activation check: every row must reproduce the original outputs. */
  function checkActivations(
    creature: Trainable,
    data: number[][],
    outputs: Float32Array[],
  ) {
    for (let i = data.length; i--;) {
      assertOutputs(
        creature.activate(new Float32Array(data[i]), false),
        outputs[i],
        `activation row ${i}`,
      );
    }
  }

  /** The frozen tests' first half: build the creature, record the activations, then trace every row. */
  function buildAndTrace(
    rng: Rng,
    json: () => unknown,
    count: number,
    scale: number,
    shift: number,
  ) {
    const creature: Trainable = Creature.fromJSON(json());
    creature.validate();
    const data = signedRows(rng, count, scale, shift);
    const outputs: Float32Array[] = new Array(data.length);
    for (let i = data.length; i--;) {
      outputs[i] = creature.activate(new Float32Array(data[i]), false);
    }
    const config = createBackPropagationConfig();
    const sparseConfig = new SparseConfig(
      exportJSONWithRuntimeIds(creature),
      config,
    );
    tracePass(creature, data, outputs, config, sparseConfig);
    return { creature, data, outputs, config, sparseConfig };
  }

  /**
   * The test's own first compaction, run unrecorded inside a chained case so the
   * recorded second call sees the creature the test sees. Any constant it mints
   * is pinned as `first-constant-N`, so it cannot collide with the recorded
   * call's `unity-constant-N`.
   */
  function firstCompaction(traced: unknown, plankConstant: number): Trainable {
    const originalRandomUUID = crypto.randomUUID.bind(crypto);
    let minted = 0;
    crypto.randomUUID = () =>
      `first-constant-${minted++}` as `${string}-${string}-${string}-${string}-${string}`;
    let compacted: Trainable | undefined;
    try {
      compacted = compactUnused(traced, plankConstant);
    } finally {
      crypto.randomUUID = originalRandomUUID;
    }
    if (compacted === undefined) {
      throw new Error("the first compaction must return a creature");
    }
    return compacted;
  }

  const fixIfJson = () => ({
    neurons: [
      { type: "hidden", uuid: "hidden-3", squash: "CLIPPED", bias: 2 },
      { type: "hidden", uuid: "hidden-4", squash: "IF", bias: 0 },
      { type: "output", squash: "IDENTITY", uuid: "output-0", bias: 1 },
      { type: "output", squash: "IDENTITY", uuid: "output-1", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "hidden-3", weight: -0.3 },
      {
        fromUUID: "input-0",
        toUUID: "hidden-4",
        weight: -0.3,
        type: "condition",
      },
      { fromUUID: "input-1", toUUID: "hidden-3", weight: 0.3 },
      {
        fromUUID: "input-1",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "positive",
      },
      { fromUUID: "hidden-3", toUUID: "output-0", weight: 0.6 },
      {
        fromUUID: "hidden-3",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "negative",
      },
      { fromUUID: "hidden-4", toUUID: "output-1", weight: 0.7 },
      { fromUUID: "input-2", toUUID: "output-1", weight: 0.8 },
    ],
    input: 3,
    output: 2,
  });

  const constantJson = () => ({
    neurons: [
      { type: "hidden", uuid: "hidden-3", squash: "CLIPPED", bias: 2.5 },
      { type: "hidden", uuid: "hidden-3b", squash: "INVERSE", bias: -0.1 },
      { type: "hidden", uuid: "hidden-4", squash: "IF", bias: 0 },
      { type: "output", squash: "IDENTITY", uuid: "output-0", bias: 1 },
      { type: "output", squash: "IDENTITY", uuid: "output-1", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "hidden-3", weight: -0.3 },
      {
        fromUUID: "input-0",
        toUUID: "hidden-4",
        weight: -0.3,
        type: "condition",
      },
      { fromUUID: "hidden-3", toUUID: "hidden-3b", weight: 0.3 },
      {
        fromUUID: "hidden-3",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "positive",
      },
      { fromUUID: "hidden-3", toUUID: "output-0", weight: 0.6 },
      {
        fromUUID: "hidden-3b",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "negative",
      },
      { fromUUID: "hidden-4", toUUID: "output-1", weight: 0.7 },
      { fromUUID: "input-2", toUUID: "output-1", weight: 0.8 },
    ],
    input: 3,
    output: 2,
  });

  const keepOrderJson = () => ({
    neurons: [
      { type: "constant", uuid: "first-one", bias: 1 },
      { type: "hidden", uuid: "hidden-3", squash: "CLIPPED", bias: 2.5 },
      { type: "hidden", uuid: "hidden-3b", squash: "INVERSE", bias: -0.1 },
      { type: "hidden", uuid: "hidden-4", squash: "IF", bias: 0 },
      { type: "output", squash: "IDENTITY", uuid: "output-0", bias: 1 },
      { type: "output", squash: "IDENTITY", uuid: "output-1", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "hidden-3", weight: -0.3 },
      {
        fromUUID: "input-0",
        toUUID: "hidden-4",
        weight: -0.3,
        type: "condition",
      },
      { fromUUID: "hidden-3", toUUID: "hidden-3b", weight: 0.3 },
      {
        fromUUID: "hidden-3",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "positive",
      },
      { fromUUID: "hidden-3", toUUID: "output-0", weight: 0.6 },
      {
        fromUUID: "hidden-3b",
        toUUID: "hidden-4",
        weight: 0.3,
        type: "negative",
      },
      { fromUUID: "hidden-4", toUUID: "output-1", weight: 0.7 },
      { fromUUID: "input-2", toUUID: "output-1", weight: 0.8 },
      { fromUUID: "first-one", toUUID: "output-1", weight: 0.8 },
      { fromUUID: "first-one", toUUID: "output-0", weight: 0.9 },
    ],
    input: 3,
    output: 2,
  });

  const ifPrep =
    "Creature fixture copied from the test named in the test field; the original Math.random() data is drawn from a seeded generator, and the .trace/ and .test/ files are not written.";
  const ifCases: CaseSpec[] = [
    {
      name: "fix_if_compact",
      test: "test/compact/FixIF.ts",
      preparation:
        `${ifPrep} The 1000 rows x 3 are Math.random() * 2 - 1 in the original (seed 778021). The single compactUnused call is recorded.`,
      seed: 778021,
      retry: false,
      expectCompacted: true,
      maxAttempts: 1,
      prepare: (rng) => () => {
        const run = buildAndTrace(rng, fixIfJson, 1000, 2, 1);
        return {
          traced: run.creature.traceJSON(),
          plankConstant: run.config.plankConstant,
        };
      },
    },
    {
      name: "compact_constant_first",
      test: "test/compact/CompactConstant.ts",
      preparation:
        `${ifPrep} The 1000 rows x 3 are Math.random() * 2 - 1 in the original (seed 778031). The first compactUnused call of the test is recorded.`,
      seed: 778031,
      retry: false,
      expectCompacted: true,
      maxAttempts: 1,
      prepare: (rng) => () => {
        const run = buildAndTrace(rng, constantJson, 1000, 2, 1);
        return {
          traced: run.creature.traceJSON(),
          plankConstant: run.config.plankConstant,
        };
      },
    },
    {
      name: "compact_constant_second",
      test: "test/compact/CompactConstant.ts",
      preparation:
        `${ifPrep} The second compactUnused call of the test, on the creature the test builds from its first compaction (seed 778031, the same data run). The first call runs unrecorded with pinned constant UUIDs; the test's activation and trace passes then run on the compacted creature, and its traceJSON is recorded.`,
      seed: 778031,
      retry: false,
      expectCompacted: true,
      maxAttempts: 1,
      prepare: (rng) => () => {
        const run = buildAndTrace(rng, constantJson, 1000, 2, 1);
        const compacted = firstCompaction(
          run.creature.traceJSON(),
          run.config.plankConstant,
        );
        checkActivations(compacted, run.data, run.outputs);
        for (let i = run.data.length; i--;) {
          const sparseConfig = new SparseConfig(
            exportJSONWithRuntimeIds(compacted),
            run.config,
          );
          const actual = compacted.activateAndTrace(
            new Float32Array(run.data[i]),
            false,
            sparseConfig,
          );
          compacted.propagate(
            new Float32Array(run.outputs[i]),
            run.config,
            sparseConfig,
          );
          assertOutputs(actual, run.outputs[i], `compacted trace row ${i}`);
        }
        return {
          traced: compacted.traceJSON(),
          plankConstant: run.config.plankConstant,
        };
      },
    },
    {
      name: "compact_keep_order_first",
      test: "test/compact/CompactKeepOrder.ts",
      preparation:
        `${ifPrep} The 10 000 rows x 3 are Math.random() * 3 - 1.5 in the original (seed 778041). The first compactUnused call of the test is recorded.`,
      seed: 778041,
      retry: false,
      expectCompacted: true,
      maxAttempts: 1,
      prepare: (rng) => () => {
        const run = buildAndTrace(rng, keepOrderJson, 10_000, 3, 1.5);
        return {
          traced: run.creature.traceJSON(),
          plankConstant: run.config.plankConstant,
        };
      },
    },
    {
      name: "compact_keep_order_second",
      test: "test/compact/CompactKeepOrder.ts",
      preparation:
        `${ifPrep} The second compactUnused call of the test, on the creature the test builds from its first compaction (seed 778041, the same data run). The first call runs unrecorded with pinned constant UUIDs; the test's activation and trace passes, which trace the compacted creature with the original sparse config, then run, and its traceJSON is recorded.`,
      seed: 778041,
      retry: false,
      expectCompacted: true,
      maxAttempts: 1,
      prepare: (rng) => () => {
        const run = buildAndTrace(rng, keepOrderJson, 10_000, 3, 1.5);
        const compacted = firstCompaction(
          run.creature.traceJSON(),
          run.config.plankConstant,
        );
        compacted.validate();
        checkActivations(compacted, run.data, run.outputs);
        tracePass(
          compacted,
          run.data,
          run.outputs,
          run.config,
          run.sparseConfig,
        );
        return {
          traced: compacted.traceJSON(),
          plankConstant: run.config.plankConstant,
        };
      },
    },
  ];

  const captures: unknown[] = [];
  for (
    const spec of [
      ...newborn,
      unusedClipped,
      cascade,
      ...orphanCases,
      ...ifCases,
    ]
  ) {
    captures.push(captureCase(spec));
  }
  return captures;
}
