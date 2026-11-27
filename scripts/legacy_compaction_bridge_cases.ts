/**
 * Remaining small parallel-bridge fixtures. The frozen helper and compactor
 * own every rewrite; this module only supplies the original inputs.
 */
type CaptureGraph = {
  semanticVersion?: string;
  forwardOnly?: boolean;
  input: number;
  output: number;
  neurons: Record<string, unknown>[];
  synapses: Record<string, unknown>[];
};

/** Repeated two-bridge literal from ParallelBridgeMerge.ts and the corner cases. */
function idPair(
  squash: string,
  bias0: number,
  bias1: number,
): CaptureGraph {
  return {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", id: 5000, squash, bias: bias0 },
      { type: "hidden", uuid: "h1", id: 5001, squash, bias: bias1 },
      { type: "output", uuid: "output-0", id: -1, squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromId: 0, toId: 5000, weight: 0.5 },
      { fromId: 5000, toId: -1, weight: 2.0 },
      { fromId: 1, toId: 5001, weight: 0.3 },
      { fromId: 5001, toId: -1, weight: 1.5 },
    ],
  };
}

function uuidPair(bias0: number, bias1: number): CaptureGraph {
  return {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: bias0 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: bias1 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  };
}

function typedExtra(role: string): CaptureGraph {
  return {
    input: 3,
    output: 1,
    neurons: [
      { type: "hidden", id: 5000, squash: "IDENTITY", bias: 0 },
      { type: "hidden", id: 5001, squash: "IDENTITY", bias: 0 },
      { type: "output", id: -1, squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromId: 0, toId: 5000, weight: 0.5 },
      { fromId: 5000, toId: -1, weight: 2.0 },
      { fromId: 2, toId: 5000, weight: 0.3, type: role },
      { fromId: 1, toId: 5001, weight: 0.4 },
      { fromId: 5001, toId: -1, weight: 1.5 },
    ],
  };
}

const complementCompact: CaptureGraph = {
  input: 2,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "h0", squash: "COMPLEMENT", bias: 0.1 },
    { type: "hidden", uuid: "h1", squash: "COMPLEMENT", bias: 0.2 },
    { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromUUID: "input-0", toUUID: "h0", weight: 0.4 },
    { fromUUID: "h0", toUUID: "output-0", weight: 1.0 },
    { fromUUID: "input-1", toUUID: "h1", weight: 0.6 },
    { fromUUID: "h1", toUUID: "output-0", weight: 1.0 },
  ],
};

const complementThree: CaptureGraph = {
  input: 3,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "h0", id: 5000, squash: "COMPLEMENT", bias: 0.5 },
    { type: "hidden", uuid: "h1", id: 5001, squash: "COMPLEMENT", bias: -0.3 },
    { type: "hidden", uuid: "h2", id: 5002, squash: "COMPLEMENT", bias: 0.3 },
    { type: "output", uuid: "output-0", id: -1, squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromId: 0, toId: 5000, weight: 1.0 },
    { fromId: 5000, toId: -1, weight: 0.5 },
    { fromId: 1, toId: 5001, weight: 2.0 },
    { fromId: 5001, toId: -1, weight: 0.8 },
    { fromId: 2, toId: 5002, weight: -1.0 },
    { fromId: 5002, toId: -1, weight: 1.2 },
  ],
};

const keptSource: CaptureGraph = {
  input: 2,
  output: 1,
  neurons: [
    { type: "hidden", id: 5002, squash: "IDENTITY", bias: 0.1 },
    { type: "hidden", id: 5000, squash: "IDENTITY", bias: 0 },
    { type: "hidden", id: 5001, squash: "IDENTITY", bias: 0 },
    { type: "output", id: -1, squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromId: 0, toId: 5002, weight: 1.0 },
    { fromId: 5002, toId: 5000, weight: 0.5 },
    { fromId: 5000, toId: -1, weight: 2.0 },
    { fromId: 1, toId: 5001, weight: 0.3 },
    { fromId: 5001, toId: -1, weight: 1.5 },
    { fromId: 5002, toId: -1, weight: 0.1 },
  ],
};

const duplicateSources: CaptureGraph = {
  input: 1,
  output: 1,
  neurons: [
    { type: "hidden", id: 5000, squash: "IDENTITY", bias: 0 },
    { type: "hidden", id: 5001, squash: "IDENTITY", bias: 0 },
    { type: "output", id: -1, squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromId: 0, toId: 5000, weight: 0.5 },
    { fromId: 5000, toId: -1, weight: 2.0 },
    { fromId: 0, toId: 5001, weight: 0.3 },
    { fromId: 5001, toId: -1, weight: 1.5 },
  ],
};

const complementTyped: CaptureGraph = {
  input: 3,
  output: 1,
  neurons: [
    { type: "hidden", id: 5000, squash: "COMPLEMENT", bias: 0 },
    { type: "hidden", id: 5001, squash: "COMPLEMENT", bias: 0 },
    { type: "output", id: -1, squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromId: 0, toId: 5000, weight: 0.5 },
    { fromId: 5000, toId: -1, weight: 2.0 },
    { fromId: 2, toId: 5000, weight: 0.1, type: "negative" },
    { fromId: 1, toId: 5001, weight: 0.3 },
    { fromId: 5001, toId: -1, weight: 1.5 },
  ],
};

const ifTarget: CaptureGraph = {
  forwardOnly: true,
  input: 3,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "bridge-a", squash: "IDENTITY", bias: 0.25 },
    { type: "hidden", uuid: "bridge-b", squash: "IDENTITY", bias: -0.5 },
    { type: "output", uuid: "output-0", squash: "IF", bias: 0 },
  ],
  synapses: [
    { fromUUID: "input-0", toUUID: "bridge-a", weight: 0.5 },
    { fromUUID: "bridge-a", toUUID: "output-0", weight: 2, type: "condition" },
    { fromUUID: "input-1", toUUID: "bridge-b", weight: 0.3 },
    {
      fromUUID: "bridge-b",
      toUUID: "output-0",
      weight: 1.5,
      type: "condition",
    },
    { fromUUID: "input-2", toUUID: "output-0", weight: 1, type: "positive" },
    { fromUUID: "input-2", toUUID: "output-0", weight: -1, type: "negative" },
  ],
};

const maximumTarget: CaptureGraph = {
  forwardOnly: true,
  input: 2,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "bridge-a", squash: "IDENTITY", bias: 0 },
    { type: "hidden", uuid: "bridge-b", squash: "IDENTITY", bias: 0 },
    { type: "output", uuid: "output-0", squash: "MAXIMUM", bias: 0 },
  ],
  synapses: [
    { fromUUID: "input-0", toUUID: "bridge-a", weight: 0.5 },
    { fromUUID: "bridge-a", toUUID: "output-0", weight: 2 },
    { fromUUID: "input-1", toUUID: "bridge-b", weight: 0.3 },
    { fromUUID: "bridge-b", toUUID: "output-0", weight: 1.5 },
  ],
};

const keepLast: CaptureGraph = {
  forwardOnly: true,
  input: 2,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "bridge-early", squash: "IDENTITY", bias: 0.25 },
    { type: "hidden", uuid: "late-source", squash: "LOGISTIC", bias: 0.1 },
    { type: "hidden", uuid: "bridge-late", squash: "IDENTITY", bias: -0.5 },
    { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.75 },
  ],
  synapses: [
    { fromUUID: "input-0", toUUID: "bridge-early", weight: 0.5 },
    { fromUUID: "bridge-early", toUUID: "output-0", weight: 2 },
    { fromUUID: "input-1", toUUID: "late-source", weight: 0.8 },
    { fromUUID: "late-source", toUUID: "bridge-late", weight: 0.3 },
    { fromUUID: "bridge-late", toUUID: "output-0", weight: 1.5 },
  ],
};

const backwardSource: CaptureGraph = {
  input: 3,
  output: 1,
  neurons: [
    { type: "hidden", uuid: "back-fed", squash: "IDENTITY", bias: 0 },
    { type: "hidden", uuid: "later", squash: "LOGISTIC", bias: 0 },
    { type: "hidden", uuid: "bridge-b", squash: "IDENTITY", bias: 0.2 },
    { type: "hidden", uuid: "bridge-c", squash: "IDENTITY", bias: -0.3 },
    { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
  ],
  synapses: [
    { fromUUID: "later", toUUID: "back-fed", weight: 0.4 },
    { fromUUID: "back-fed", toUUID: "output-0", weight: 1.1 },
    { fromUUID: "input-0", toUUID: "later", weight: 0.7 },
    { fromUUID: "input-1", toUUID: "bridge-b", weight: 0.5 },
    { fromUUID: "bridge-b", toUUID: "output-0", weight: 2 },
    { fromUUID: "input-2", toUUID: "bridge-c", weight: 0.3 },
    { fromUUID: "bridge-c", toUUID: "output-0", weight: 1.5 },
  ],
};

const MERGE = "src/compact/ParallelBridgeMerge.ts";
const COMPACT = "src/compact/CompactCreature.ts";

export async function captureLegacyBridgeCases(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { exportJSONUnchecked } = await import(
    new URL("src/creature/CreatureSerialization.ts", root).href
  );
  const { compactCreature } = await import(
    new URL("src/compact/CompactCreature.ts", root).href
  );
  const { mergeParallelBridges } = await import(
    new URL("src/compact/ParallelBridgeMerge.ts", root).href
  );
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();

  const captures: unknown[] = [];

  const helperCases: Array<[string, string, CaptureGraph]> = [
    [
      "complement_pair",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("COMPLEMENT", 0, 0),
    ],
    [
      "complement_weights",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("COMPLEMENT", 0.1, 0.2),
    ],
    [
      "mixed_squash",
      "test/compact/ParallelBridgeMerge.ts",
      {
        input: 2,
        output: 1,
        neurons: [
          { type: "hidden", uuid: "h0", id: 5000, squash: "IDENTITY", bias: 0 },
          {
            type: "hidden",
            uuid: "h1",
            id: 5001,
            squash: "COMPLEMENT",
            bias: 0,
          },
          {
            type: "output",
            uuid: "output-0",
            id: -1,
            squash: "IDENTITY",
            bias: 0,
          },
        ],
        synapses: [
          { fromId: 0, toId: 5000, weight: 0.5 },
          { fromId: 5000, toId: -1, weight: 2.0 },
          { fromId: 1, toId: 5001, weight: 0.3 },
          { fromId: 5001, toId: -1, weight: 1.5 },
        ],
      },
    ],
    [
      "logistic_refusal",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("LOGISTIC", 0, 0),
    ],
    [
      "tanh_refusal",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("TANH", 0, 0),
    ],
    [
      "relu_refusal",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("RELU", 0, 0),
    ],
    [
      "absolute_refusal",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("ABSOLUTE", 0, 0),
    ],
    [
      "maximum_neuron_refusal",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("MAXIMUM", 0, 0),
    ],
    [
      "complement_three",
      "test/compact/ParallelBridgeMerge.ts",
      complementThree,
    ],
    [
      "identity_still_merges",
      "test/compact/ParallelBridgeMerge.ts",
      idPair("IDENTITY", 0.1, 0.2),
    ],
    [
      "uuid_only",
      "test/compact/ParallelIdentityBridgeUnification.ts",
      uuidPair(0.1, 0.2),
    ],
    [
      "typed_condition",
      "test/compact/ParallelMergeCornerCases.ts",
      typedExtra("condition"),
    ],
    [
      "typed_positive",
      "test/compact/ParallelMergeCornerCases.ts",
      typedExtra("positive"),
    ],
    ["kept_source", "test/compact/ParallelMergeCornerCases.ts", keptSource],
    [
      "duplicate_sources",
      "test/compact/ParallelMergeCornerCases.ts",
      duplicateSources,
    ],
    [
      "post_prune_shape",
      "test/compact/ParallelMergeCornerCases.ts",
      idPair("IDENTITY", 0, 0),
    ],
    [
      "complement_typed",
      "test/compact/ParallelMergeCornerCases.ts",
      complementTyped,
    ],
    [
      "identity_integrity",
      "test/compact/ParallelMergeCornerCases.ts",
      idPair("IDENTITY", 0.1, 0.2),
    ],
    [
      "complement_integrity",
      "test/compact/ParallelMergeCornerCases.ts",
      idPair("COMPLEMENT", 0, 0),
    ],
    [
      "if_target_declined",
      "test/compact/ParallelBridgeMergeExactness.ts",
      ifTarget,
    ],
    [
      "maximum_target_declined",
      "test/compact/ParallelBridgeMergeExactness.ts",
      maximumTarget,
    ],
    [
      "backward_source_left",
      "test/compact/ParallelBridgeMergeExactness.ts",
      backwardSource,
    ],
  ];

  for (const [name, test, graph] of helperCases) {
    const input = structuredClone(graph);
    const working = structuredClone(graph);
    const before = structuredClone(working);
    const result = mergeParallelBridges(working);
    captures.push({
      name,
      revision,
      test,
      source: MERGE,
      operation: "mergeParallelBridges",
      preparation: "none",
      input,
      before,
      result,
      after: structuredClone(working),
      reloaded: null,
      probes: null,
      activationTolerance: null,
      beforeOutputs: null,
      afterOutputs: null,
    });
  }

  // Original load/export sequence. Unseeded Math.random probes stay in the
  // source test; they are not a reproducible corpus.
  {
    const input = structuredClone(keepLast);
    const loaded = Creature.fromJSON(structuredClone(input), false);
    const working = exportJSONUnchecked(loaded);
    const before = structuredClone(working);
    const result = mergeParallelBridges(working);
    const after = structuredClone(working);
    const reloaded = Creature.fromJSON(
      structuredClone(after),
      false,
      "issue-3809",
    );
    captures.push({
      name: "keep_last_bridge",
      revision,
      test: "test/compact/ParallelBridgeMergeExactness.ts",
      source: MERGE,
      operation: "mergeParallelBridges",
      preparation: "Creature.fromJSON; exportJSONUnchecked",
      input,
      before,
      result,
      after,
      reloaded: exportJSONUnchecked(reloaded),
      probes: null,
      activationTolerance: null,
      beforeOutputs: null,
      afterOutputs: null,
    });
  }

  const identityProbes = [
    [0.25, -0.75],
    [1, 1],
    [-2, 0.5],
  ];
  {
    const input = uuidPair(0.1, 0.2);
    const original = Creature.fromJSON(structuredClone(input));
    const beforeOutputs = identityProbes.map((sample) => [
      original.activate(new Float32Array(sample))[0],
    ]);
    const before = original.exportJSON();
    const compacted = compactCreature(original, false);
    if (!compacted) throw new Error("identity compact returned no candidate");
    compacted.validate();
    const afterOutputs = identityProbes.map((sample) => [
      compacted.activate(new Float32Array(sample))[0],
    ]);
    captures.push({
      name: "identity_compact",
      revision,
      test: "test/compact/ParallelIdentityBridgeUnification.ts",
      source: COMPACT,
      operation: "compactCreature",
      preparation: "Creature.fromJSON",
      feedbackLoop: false,
      input,
      before,
      result: { candidate: true },
      after: compacted.exportJSON(),
      reloaded: null,
      probes: identityProbes,
      activationTolerance: 1e-6,
      beforeOutputs,
      afterOutputs,
    });
  }

  {
    const input = structuredClone(complementCompact);
    const original = Creature.fromJSON(structuredClone(input));
    const before = original.exportJSON();
    const compacted = compactCreature(original, false);
    if (!compacted) throw new Error("complement compact returned no candidate");
    compacted.validate();
    captures.push({
      name: "complement_compact",
      revision,
      test: "test/compact/ParallelBridgeMerge.ts",
      source: COMPACT,
      operation: "compactCreature",
      preparation: "Creature.fromJSON",
      feedbackLoop: false,
      input,
      before,
      result: { candidate: true },
      after: compacted.exportJSON(),
      reloaded: null,
      probes: null,
      activationTolerance: null,
      beforeOutputs: null,
      afterOutputs: null,
    });
  }

  return captures;
}
