/** Capture the unmodified legacy implementation; never regenerate from core. */
const [checkout, destination] = Deno.args;
if (!checkout || !destination) {
  throw new Error(
    "Usage: capture_legacy_compaction.ts <NEAT-AI checkout> <output.json>",
  );
}
const revision = "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab";
const head = await new Deno.Command("git", {
  args: ["-C", checkout, "rev-parse", "HEAD"],
}).output();
if (
  !head.success || new TextDecoder().decode(head.stdout).trim() !== revision
) {
  throw new Error(`Legacy checkout must be at ${revision}`);
}
const diff = await new Deno.Command("git", {
  args: ["-C", checkout, "status", "--porcelain", "--untracked-files=no"],
}).output();
if (!diff.success || diff.stdout.length) {
  throw new Error("Legacy checkout must have no tracked modifications");
}
const root = new URL(`file://${await Deno.realPath(checkout)}/`);
const { Creature } = await import(new URL("src/Creature.ts", root).href);
const { compactCreature } = await import(
  new URL("src/compact/CompactCreature.ts", root).href
);
const probes = [[0, 0, 0], [1, 1, 1], [-1, -1, -1], [0.25, -0.75, 0.5], [
  -2,
  1,
  -0.5,
]];
const { calculate } = await import(
  new URL("src/architecture/Score.ts", root).href
);
const { initWasmForTests } = await import(
  new URL("test/_initWasm.ts", root).href
);
await initWasmForTests();
const captures: unknown[] = [];
function record(
  name: string,
  json: unknown,
  test: string,
  feedbackLoop: boolean,
  activationTolerance: number,
  fixture?: string,
) {
  const creature = Creature.fromJSON(json);
  if (creature.input > probes[0].length) {
    throw new Error(`Missing probe dimensions for ${name}`);
  }
  const before = creature.exportJSON();
  const beforeScore = calculate(creature, 0, 0.0001);
  const after = compactCreature(creature, feedbackLoop);
  const caseProbes = probes.map((row) => row.slice(0, creature.input));
  captures.push({
    name,
    revision,
    source: "src/compact/CompactCreature.ts",
    test,
    fixture,
    feedbackLoop,
    activationTolerance,
    growthCost: 0.0001,
    error: 0,
    beforeScore,
    afterScore: calculate(after ?? creature, 0, 0.0001),
    before,
    after: after?.exportJSON() ?? null,
    probes: caseProbes,
    outputs: caseProbes.map((probe) =>
      Array.from(creature.activate(new Float32Array(probe), false))
    ),
  });
}
// First case is the original COMPLEMENT regression's input, unchanged.
// The other cases vary only squash to cover fan-in refusal, aggregate refusal and no-op.
for (
  const [name, hiddenSquash, outputSquash] of [
    ["complement_bypass", "COMPLEMENT", "IDENTITY"],
    ["identity_fanin_noop", "IDENTITY", "IDENTITY"],
    ["aggregate_guard", "COMPLEMENT", "MAXIMUM"],
    ["nonlinear_noop", "TANH", "TANH"],
  ]
) {
  record(
    name,
    {
      input: 2,
      output: 1,
      neurons: [
        { uuid: "hidden-0", type: "hidden", squash: hiddenSquash, bias: 0.25 },
        { uuid: "output-0", type: "output", squash: outputSquash, bias: 0.1 },
      ],
      synapses: [
        { fromUUID: "input-0", toUUID: "hidden-0", weight: 0.5 },
        { fromUUID: "input-1", toUUID: "hidden-0", weight: -2 },
        { fromUUID: "hidden-0", toUUID: "output-0", weight: 1.5 },
        { fromUUID: "input-1", toUUID: "output-0", weight: 0.25 },
      ],
    },
    "test/compact/CompactCreatureComplementBypass.ts",
    true,
    1e-5,
  );
}
// Both inputs are copied from the original transitive-fold regression.
for (const varying of [false, true]) {
  record(
    varying ? "constant_with_varying_input" : "constant_transitive_fold",
    {
      semanticVersion: "4.0.0",
      forwardOnly: true,
      input: 1,
      output: 1,
      neurons: [
        { type: "constant", uuid: "const-c", bias: varying ? 0.4 : 0.5 },
        { type: "hidden", uuid: "hidden-h", squash: "LOGISTIC", bias: 0.2 },
        { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.1 },
      ],
      synapses: [
        { fromUUID: "const-c", toUUID: "hidden-h", weight: 0.5 },
        ...(varying
          ? [{ fromUUID: "input-0", toUUID: "hidden-h", weight: 0.7 }]
          : []),
        { fromUUID: "hidden-h", toUUID: "output-0", weight: 0.5 },
        ...(!varying
          ? [{ fromUUID: "input-0", toUUID: "output-0", weight: 0.6 }]
          : []),
      ],
    },
    "test/compact/CompactCreatureConstantTransitiveFold.ts",
    false,
    1e-6,
  );
}

// Load the original typed-IF fixtures directly from the pinned legacy checkout.
for (
  const [name, fixture] of [
    ["if_positive", "test/data/if-collapse-positive.json"],
    ["if_negative", "test/data/if-collapse-negative.json"],
    ["if_varying", "test/data/if-collapse-varying-condition.json"],
    ["if_zero", "test/data/if-collapse-negative.json"],
  ]
) {
  const json = JSON.parse(await Deno.readTextFile(new URL(fixture, root)));
  if (name === "if_zero") {
    // Synthetic boundary variation: a zero selector still takes the negative arm.
    for (const synapse of json.synapses) {
      if (synapse.type === "condition") synapse.weight = 0;
    }
  }
  record(
    name,
    json,
    "test/compact/CompactCreatureIfCollapse.ts",
    false,
    1e-6,
    fixture,
  );
}

// Original inputs copied unchanged from each regression, in source test order.
// Parallel tests assert graph arithmetic at 1e-12, but have no activation bound;
// 1e-6 is an additional native activation replay bound, not a relaxed source bound.
record(
  "zero_weight_orphan",
  {
    neurons: [
      { type: "hidden", uuid: "hidden-unused", squash: "TANH", bias: 0.2 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.1 },
    ],
    synapses: [
      // This path is live and determines outputs.
      { fromUUID: "input-0", toUUID: "output-0", weight: 1.25 },

      // These synapses are behaviour-preserving dead weight:
      // hidden-unused influences output only via a zero-weight edge.
      { fromUUID: "input-0", toUUID: "hidden-unused", weight: -0.5 },
      { fromUUID: "hidden-unused", toUUID: "output-0", weight: 0 },
    ],
    input: 1,
    output: 1,
  },
  "test/compact/ZeroWeightSynapsePruning.ts",
  false,
  1e-7,
);
record(
  "parallel_two",
  {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_weights",
  {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0.1 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: 0.2 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_bias",
  {
    input: 3,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0.5 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: -0.3 },
      { type: "hidden", uuid: "h2", squash: "IDENTITY", bias: 0.1 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.2 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 1.0 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 1.0 },
      { fromUUID: "h1", toUUID: "output-0", weight: 3.0 },
      { fromUUID: "input-2", toUUID: "h2", weight: 1.0 },
      { fromUUID: "h2", toUUID: "output-0", weight: 0.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_duplicate_sources",
  {
    input: 1,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-0", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_nonlinear",
  {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "LOGISTIC", bias: 0 },
      { type: "hidden", uuid: "h1", squash: "LOGISTIC", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_fanin",
  {
    input: 3,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "input-1", toUUID: "h0", weight: 0.3 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-2", toUUID: "h1", weight: 0.4 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_forward_only",
  {
    input: 2,
    output: 1,
    forwardOnly: true,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 0.5 },
      { fromUUID: "h0", toUUID: "output-0", weight: 2.0 },
      { fromUUID: "input-1", toUUID: "h1", weight: 0.3 },
      { fromUUID: "h1", toUUID: "output-0", weight: 1.5 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);
record(
  "parallel_three",
  {
    input: 3,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "h0", squash: "IDENTITY", bias: 0.1 },
      { type: "hidden", uuid: "h1", squash: "IDENTITY", bias: -0.2 },
      { type: "hidden", uuid: "h2", squash: "IDENTITY", bias: 0.3 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "h0", weight: 1.0 },
      { fromUUID: "h0", toUUID: "output-0", weight: 0.5 },
      { fromUUID: "input-1", toUUID: "h1", weight: 2.0 },
      { fromUUID: "h1", toUUID: "output-0", weight: 0.8 },
      { fromUUID: "input-2", toUUID: "h2", weight: -1.0 },
      { fromUUID: "h2", toUUID: "output-0", weight: 1.2 },
    ],
  },
  "test/compact/ParallelIdentityBridgeMerge.ts",
  false,
  1e-6,
);

await Deno.writeTextFile(destination, JSON.stringify(captures, null, 2) + "\n");
