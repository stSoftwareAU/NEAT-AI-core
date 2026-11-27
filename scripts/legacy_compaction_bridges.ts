/** Original bridge ordering fixtures; the frozen helper owns all rewrites. */
type CaptureGraph = {
  semanticVersion: string;
  forwardOnly?: boolean;
  input: number;
  output: number;
  neurons: Record<string, unknown>[];
  synapses: Record<string, unknown>[];
};

// Constructors and row builders copied from ParallelBridgeMergeOrdering.ts.
function inputRows(inputs: number, rows: number): Float32Array[] {
  const data: Float32Array[] = [];
  for (let r = 0; r < rows; r++) {
    const row = new Float32Array(inputs);
    for (let i = 0; i < inputs; i++) {
      row[i] = Math.sin((i + 1) * 0.7 + r * 1.31);
    }
    data.push(row);
  }
  return data;
}

function dyadicRows(inputs: number, rows: number): Float32Array[] {
  const data: Float32Array[] = [];
  for (let r = 0; r < rows; r++) {
    const row = new Float32Array(inputs);
    for (let i = 0; i < inputs; i++) {
      row[i] = ((i + r) % 5) / 4 - 0.5;
    }
    data.push(row);
  }
  return data;
}

function lateSourceBridges(): CaptureGraph {
  return {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "bridge-early", squash: "IDENTITY", bias: 0.1 },
      { type: "hidden", uuid: "source-late", squash: "LOGISTIC", bias: -0.4 },
      { type: "hidden", uuid: "bridge-late", squash: "IDENTITY", bias: 0.2 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "bridge-early", weight: 0.5 },
      { fromUUID: "bridge-early", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "source-late", weight: 0.9 },
      { fromUUID: "source-late", toUUID: "bridge-late", weight: 0.3 },
      { fromUUID: "bridge-late", toUUID: "output-0", weight: 1.5 },
    ],
  };
}

function bridgesIntoAggregate(
  squash: string,
  typed: boolean,
  count: number,
): CaptureGraph {
  const json: CaptureGraph = {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: count,
    output: 1,
    neurons: [],
    synapses: [],
  };

  const types = ["condition", "positive", "negative"] as const;
  for (let i = 0; i < count; i++) {
    json.neurons.push({
      type: "hidden",
      uuid: `bridge-${i}`,
      squash: "IDENTITY",
      // Exactly representable in float32 so the merge is bit-for-bit exact.
      bias: (i % 5) / 8,
    });
    json.synapses.push({
      fromUUID: `input-${i}`,
      toUUID: `bridge-${i}`,
      weight: 0.5 + (i % 3) / 4,
    });
    json.synapses.push({
      fromUUID: `bridge-${i}`,
      toUUID: "aggregate",
      weight: 0.25 * ((i % 4) + 1),
      type: typed ? types[i % types.length] : undefined,
    });
  }

  json.neurons.push({ type: "hidden", uuid: "aggregate", squash, bias: 0.5 });
  json.neurons.push({
    type: "output",
    uuid: "output-0",
    squash: "IDENTITY",
    bias: 0,
  });
  json.synapses.push({
    fromUUID: "aggregate",
    toUUID: "output-0",
    weight: 1,
  });
  return json;
}

const backward: CaptureGraph = {
  semanticVersion: "4.0.0",
  input: 1,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "bridge-a", squash: "IDENTITY", bias: 0.1 },
    { type: "hidden", uuid: "target", squash: "IDENTITY", bias: 0 },
    { type: "hidden", uuid: "back-source", squash: "LOGISTIC", bias: 0 },
    { type: "hidden", uuid: "bridge-b", squash: "IDENTITY", bias: 0.2 },
    { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromUUID: "input-0", toUUID: "bridge-a", weight: 0.5 },
    { fromUUID: "bridge-a", toUUID: "target", weight: 2.0 },
    { fromUUID: "target", toUUID: "back-source", weight: 0.7 },
    { fromUUID: "back-source", toUUID: "bridge-b", weight: 0.3 },
    { fromUUID: "bridge-b", toUUID: "target", weight: 1.5 },
    { fromUUID: "target", toUUID: "output-0", weight: 1.0 },
  ],
};

export async function captureLegacyBridges(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { exportJSONUnchecked } = await import(
    new URL("src/creature/CreatureSerialization.ts", root).href
  );
  const { mergeParallelBridges } = await import(
    new URL("src/compact/ParallelBridgeMerge.ts", root).href
  );
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();
  const captures: unknown[] = [];
  for (
    const [name, input, load] of [
      ["late_source", lateSourceBridges(), true],
      ["backward_refusal", backward, false],
      ["maximum_refusal", bridgesIntoAggregate("MAXIMUM", false, 6), false],
      ["if_refusal", bridgesIntoAggregate("IF", true, 12), true],
    ] as const
  ) {
    // Loading adds IDs to its argument. Keep the original raw input intact.
    const original = load
      ? Creature.fromJSON(structuredClone(input), false, "#3809")
      : undefined;
    const working = original
      ? exportJSONUnchecked(original)
      : structuredClone(input);
    const before = structuredClone(working);
    const result = mergeParallelBridges(working);
    // Retain the exact helper output before loading can add/change fields.
    const after = structuredClone(working);
    const reloaded = load
      ? Creature.fromJSON(working, false, "#3809")
      : undefined;
    const rows = original
      ? (name === "if_refusal"
        ? dyadicRows(input.input, 25)
        : inputRows(input.input, 25))
      : undefined;
    const beforeOutputs: number[][] = [];
    const afterOutputs: number[][] = [];
    if (rows) {
      for (const row of rows) {
        beforeOutputs.push(Array.from(original.activate(row, false)));
        afterOutputs.push(Array.from(reloaded.activate(row, false)));
      }
    }
    captures.push({
      name,
      revision,
      test: "test/compact/ParallelBridgeMergeOrdering.ts",
      source: "src/compact/ParallelBridgeMerge.ts",
      preparation: load ? "Creature.fromJSON; exportJSONUnchecked" : "none",
      input,
      before,
      result,
      after,
      reloaded: reloaded ? exportJSONUnchecked(reloaded) : null,
      probes: rows?.map((row) => Array.from(row)) ?? null,
      activationTolerance: load ? (name === "if_refusal" ? 0 : 1e-6) : null,
      beforeOutputs: rows ? beforeOutputs : null,
      afterOutputs: rows ? afterOutputs : null,
    });
  }
  return captures;
}
