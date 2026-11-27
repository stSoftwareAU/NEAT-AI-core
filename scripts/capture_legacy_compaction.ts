import {
  LEGACY_COMPACTION_REVISION as revision,
  legacyCompactionSource,
} from "./legacy_compaction_source.ts";

/** Capture the unmodified legacy implementation; never regenerate from core. */
const [checkout, destination, mode = "creature"] = Deno.args;
if (!checkout || !destination) {
  throw new Error(
    "Usage: capture_legacy_compaction.ts <NEAT-AI checkout> <output.json> [creature|helpers|variants|bridges|metadata|tags|constants|behaviour]",
  );
}
const root = await legacyCompactionSource(checkout);
if (mode === "behaviour") {
  const { captureLegacyBehaviour } = await import(
    "./legacy_compaction_behaviour.ts"
  );
  const captures = await captureLegacyBehaviour(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "constants") {
  const { captureLegacyConstants } = await import(
    "./legacy_compaction_constants.ts"
  );
  const captures = await captureLegacyConstants(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "tags") {
  const { captureLegacyTags } = await import("./legacy_compaction_tags.ts");
  const captures = await captureLegacyTags(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "bridges") {
  const { captureLegacyBridges } = await import(
    "./legacy_compaction_bridges.ts"
  );
  const captures = await captureLegacyBridges(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "variants") {
  const { captureLegacyVariants } = await import(
    "./legacy_compaction_variants.ts"
  );
  const captures = await captureLegacyVariants(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "metadata") {
  const { captureLegacyMetadata } = await import(
    "./legacy_compaction_metadata.ts"
  );
  const captures = await captureLegacyMetadata(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode === "helpers") {
  const { captureLegacyHelpers } = await import(
    "./legacy_compaction_helpers.ts"
  );
  const captures = await captureLegacyHelpers(root, revision);
  await Deno.writeTextFile(
    destination,
    JSON.stringify(captures, null, 2) + "\n",
  );
  Deno.exit(0);
}
if (mode !== "creature") throw new Error(`Unknown capture mode: ${mode}`);
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

// Original published fixture inputs, loaded without edits.
for (
  const [name, fixture] of [
    ["constant_fold_full", "test/data/constant-fold-full-removal.json"],
    ["constant_fold_partial_if", "test/data/constant-fold-partial-if.json"],
  ]
) {
  record(
    name,
    JSON.parse(await Deno.readTextFile(new URL(fixture, root))),
    "test/compact/CompactCreatureConstantFold.ts",
    false,
    1e-6,
    fixture,
  );
}

// Original makeConstantForest constructor from the frozen test, unchanged.
function makeConstantForest(count: number): unknown {
  const neurons: Record<string, unknown>[] = [];
  const synapses: Record<string, unknown>[] = [];

  for (let i = 0; i < count; i++) {
    neurons.push({
      type: "constant",
      uuid: `const-pos-${i}`,
      bias: 0.1 * (i + 1),
    });
    neurons.push({
      type: "constant",
      uuid: `const-neg-${i}`,
      bias: -0.2 * (i + 1),
    });
  }

  for (let i = 0; i < count; i++) {
    neurons.push({ type: "hidden", uuid: `if-${i}`, squash: "IF", bias: 0 });
    synapses.push({
      fromUUID: "input-0",
      toUUID: `if-${i}`,
      weight: 0.3 + i * 0.1,
      type: "condition",
    });
    synapses.push({
      fromUUID: `const-pos-${i}`,
      toUUID: `if-${i}`,
      weight: 0.5,
      type: "positive",
    });
    synapses.push({
      fromUUID: `const-neg-${i}`,
      toUUID: `if-${i}`,
      weight: 0.7,
      type: "negative",
    });
    synapses.push({
      fromUUID: `if-${i}`,
      toUUID: "output-0",
      weight: 1 / count,
    });
  }

  neurons.push({
    type: "output",
    uuid: "output-0",
    squash: "IDENTITY",
    bias: 0,
  });
  synapses.push({ fromUUID: "input-1", toUUID: "output-0", weight: 0.25 });

  return {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: 2,
    output: 1,
    neurons,
    synapses,
  };
}

record(
  "constant_disconnected",
  {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: 1,
    output: 1,
    neurons: [
      { type: "constant", uuid: "unused-const", bias: 1 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "output-0", weight: 1 },
    ],
  },
  "test/compact/CompactCreatureDisconnectedConstant.ts",
  false,
  1e-6,
);

record(
  "constant_merge_forest",
  makeConstantForest(12),
  "test/compact/CompactCreatureConstantMerge.ts",
  false,
  1e-6,
);

record(
  "constant_merge_if_roles",
  {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: 2,
    output: 1,
    neurons: [
      { type: "constant", uuid: "const-cond", bias: 0.5 },
      { type: "constant", uuid: "const-pos", bias: -0.25 },
      { type: "constant", uuid: "const-neg", bias: 2 },
      { type: "hidden", uuid: "if-neuron", squash: "IF", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      {
        fromUUID: "input-0",
        toUUID: "if-neuron",
        weight: 1,
        type: "condition",
      },
      {
        fromUUID: "const-cond",
        toUUID: "if-neuron",
        weight: 0.4,
        type: "condition",
      },
      {
        fromUUID: "input-1",
        toUUID: "if-neuron",
        weight: 0.7,
        type: "positive",
      },
      {
        fromUUID: "const-pos",
        toUUID: "if-neuron",
        weight: 0.6,
        type: "positive",
      },
      {
        fromUUID: "const-neg",
        toUUID: "if-neuron",
        weight: 0.3,
        type: "negative",
      },
      { fromUUID: "if-neuron", toUUID: "output-0", weight: 1 },
    ],
  },
  "test/compact/CompactCreatureConstantMerge.ts",
  false,
  1e-6,
);

record(
  "constant_merge_surplus",
  {
    semanticVersion: "4.0.0",
    forwardOnly: true,
    input: 1,
    output: 1,
    neurons: [
      { type: "constant", uuid: "const-a", bias: 0.5 },
      { type: "constant", uuid: "const-b", bias: -2 },
      { type: "constant", uuid: "const-c", bias: 3 },
      { type: "constant", uuid: "const-d", bias: 0.25 },
      { type: "hidden", uuid: "max-neuron", squash: "MAXIMUM", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "max-neuron", weight: 1 },
      { fromUUID: "const-a", toUUID: "max-neuron", weight: 0.6 },
      { fromUUID: "const-b", toUUID: "max-neuron", weight: 0.7 },
      { fromUUID: "const-c", toUUID: "max-neuron", weight: 0.8 },
      { fromUUID: "const-d", toUUID: "max-neuron", weight: 0.9 },
      { fromUUID: "max-neuron", toUUID: "output-0", weight: 1 },
    ],
  },
  "test/compact/CompactCreatureConstantMerge.ts",
  false,
  1e-6,
);

await Deno.writeTextFile(destination, JSON.stringify(captures, null, 2) + "\n");
