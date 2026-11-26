/** Original raw-helper inputs; deliberately bypass Creature.fromJSON (#776). */
export async function captureLegacyHelpers(root: URL, revision: string) {
  const { pruneZeroWeightSynapses, mergeDuplicateSynapses } = await import(
    new URL("src/compact/CompactUtils.ts", root).href
  );
  const { removeBackwardSynapses } = await import(
    new URL("src/compact/RemoveBackwardSynapses.ts", root).href
  );
  const { normaliseCreatureExport } = await import(
    new URL("src/architecture/NormaliseCreatureExport.ts", root).href
  );
  const { pruneOrphanMemeticReferences } = await import(
    new URL("src/compact/MemeticCleanup.ts", root).href
  );
  const helpers = {
    pruneZeroWeightSynapses,
    mergeDuplicateSynapses,
    removeBackwardSynapses,
    pruneOrphanMemeticReferences,
  };
  const tagA = { name: "tag", value: "a" };
  const tagB = { name: "tag", value: "b" };
  // Copied unchanged from the original tests in the pinned checkout.
  const cases = [
    {
      name: "typed_zero",
      test: "test/compact/ZeroWeightSynapsePruningTyped.ts",
      helper: "pruneZeroWeightSynapses",
      prepare: false,
      json: {
        input: 1,
        output: 2,
        neurons: [
          { type: "hidden", uuid: "if-0", squash: "IF", bias: 0 },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.1 },
          { type: "output", uuid: "output-1", squash: "IDENTITY", bias: -0.2 },
        ],
        synapses: [
          // When an output has other inbound connections, a zero-weight, untyped
          // synapse should be pruned.
          { fromUUID: "input-0", toId: -1, weight: 0 },
          { fromUUID: "input-0", toId: -1, weight: 0.3 },

          // Structural safety: when an output would otherwise have zero inbound
          // connections, preserve the last inbound synapse (even if its weight is 0).
          { fromUUID: "input-0", toId: -2, weight: 0 },

          // IF requires typed inbound connections; these must never be pruned.
          {
            fromUUID: "input-0",
            toUUID: "if-0",
            weight: 0,
            type: "condition",
          },
          {
            fromUUID: "input-0",
            toUUID: "if-0",
            weight: 0.2,
            type: "positive",
          },
          {
            fromUUID: "input-0",
            toUUID: "if-0",
            weight: -0.2,
            type: "negative",
          },

          // Extra safety: even untyped zero-weight edges to IF targets are preserved.
          { fromUUID: "input-0", toUUID: "if-0", weight: 0 },
        ],
        memetic: { generation: 0, score: 0, biases: {}, weights: {} },
      },
    },
    {
      name: "duplicate_sum",
      test: "test/compact/MergeDuplicateSynapses.ts",
      helper: "mergeDuplicateSynapses",
      prepare: false,
      json: {
        input: 1,
        output: 1,
        neurons: [
          { type: "hidden", uuid: "hidden-0", squash: "IDENTITY", bias: 0 },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.1 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "output-0", weight: 0.5 },
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: -0.2,
            tags: [tagB],
          },
          {
            fromUUID: "input-0",
            toUUID: "hidden-0",
            weight: 0.1,
            type: "condition",
            tags: [tagA],
          },
          {
            fromUUID: "input-0",
            toUUID: "hidden-0",
            weight: 0.2,
            type: "condition",
            tags: [tagB],
          },
          {
            fromUUID: "input-0",
            toUUID: "hidden-0",
            weight: 0.3,
            type: "positive",
          },
        ],
        memetic: { generation: 0, score: 0, biases: {}, weights: {} },
      },
    },
    {
      name: "distinct_if_roles",
      test: "test/compact/MergeDuplicateSynapses.ts",
      helper: "mergeDuplicateSynapses",
      prepare: false,
      json: {
        input: 1,
        output: 1,
        neurons: [
          { type: "output", uuid: "output-0", squash: "IF", bias: 0 },
        ],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: 0.3,
            type: "condition",
          },
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: 0.5,
            type: "positive",
          },
        ],
      },
    },
    {
      name: "backward_repair",
      test: "test/compact/RemoveBackwardSynapses.ts",
      helper: "removeBackwardSynapses",
      prepare: true,
      json: {
        neurons: [
          { uuid: "hidden-0", type: "hidden", squash: "IDENTITY", bias: 0 },
          { uuid: "hidden-1", type: "hidden", squash: "IDENTITY", bias: 0 },
          { uuid: "output-0", type: "output", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "hidden-0", weight: 1 },
          // Forward synapse (hidden-0 → hidden-1) — must survive.
          { fromUUID: "hidden-0", toUUID: "hidden-1", weight: 0.5 },
          // Backward synapse (hidden-1 → hidden-0) — must be removed.
          { fromUUID: "hidden-1", toUUID: "hidden-0", weight: 0.25 },
          { fromUUID: "hidden-1", toUUID: "output-0", weight: 1 },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "forward_noop",
      test: "test/compact/RemoveBackwardSynapses.ts",
      helper: "removeBackwardSynapses",
      prepare: true,
      json: {
        neurons: [
          { uuid: "hidden-0", type: "hidden", squash: "IDENTITY", bias: 0 },
          { uuid: "output-0", type: "output", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "hidden-0", weight: 1 },
          { fromUUID: "hidden-0", toUUID: "output-0", weight: 1 },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "duplicate_tags",
      test: "test/compact/MergeDuplicateSynapsesTagDedup.ts",
      helper: "mergeDuplicateSynapses",
      prepare: false,
      json: {
        input: 1,
        output: 1,
        neurons: [{
          type: "output",
          uuid: "output-0",
          squash: "IDENTITY",
          bias: 0,
        }],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: 0.25,
            tags: [{ name: "meta", value: "same" }],
          },
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: 0.75,
            tags: [{ name: "meta", value: "same" }],
          },
        ],
      },
    },
    {
      name: "memetic_count",
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      helper: "pruneOrphanMemeticReferences",
      prepare: false,
      json: {
        neurons: [{ id: 0 }, { id: 1 }, { id: 2 }],
        synapses: [{ from: 0, to: 1 }, { from: 1, to: 2 }],
        memetic: {
          generation: 1,
          score: 0,
          biases: { 0: 0.1, 1: 0.2, 99: 0.5 },
          weights: {
            0: [{ toId: 1, weight: 0.3 }, { toId: 2, weight: 0.4 }],
            1: [{ toId: 2, weight: 0.5 }],
            42: [{ toId: 1, weight: 0.6 }, { toId: 2, weight: 0.7 }],
          },
          ancestry: [{
            generation: 0,
            score: 0,
            biases: {},
            weights: {
              0: [{ toId: 1, weight: 0.1 }, { toId: 9, weight: 0.2 }],
            },
          }],
        },
      },
    },
    {
      name: "memetic_clean",
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      helper: "pruneOrphanMemeticReferences",
      prepare: false,
      json: {
        neurons: [{ id: 0 }, { id: 1 }],
        synapses: [{ from: 0, to: 1 }],
        memetic: {
          generation: 1,
          score: 0,
          biases: { 0: 0.1, 1: 0.2 },
          weights: { 0: [{ toId: 1, weight: 0.3 }] },
        },
      },
    },
    {
      name: "memetic_array_weights",
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      helper: "pruneOrphanMemeticReferences",
      prepare: false,
      json: {
        neurons: [{ id: 10 }, { id: 11 }, { id: 12 }],
        synapses: [{ from: 0, to: 1 }, { from: 1, to: 2 }],
        memetic: {
          generation: 1,
          score: 0,
          biases: { 10: 0.1, 99: 0.5 },
          weights: [
            { fromUUID: "input-0", toUUID: "input-1", weight: 0.3 },
            { fromUUID: "input-1", toUUID: "output-0", weight: 0.4 },
            { fromUUID: "output-0", toUUID: "input-0", weight: 0.5 },
          ],
        },
      },
    },
    {
      name: "memetic_absent",
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      helper: "pruneOrphanMemeticReferences",
      prepare: false,
      json: {
        neurons: [{ id: 0 }, { id: 1 }],
        synapses: [{ from: 0, to: 1 }],
      },
    },
    {
      name: "memetic_no_weights",
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      helper: "pruneOrphanMemeticReferences",
      prepare: false,
      json: {
        neurons: [{ id: 0 }, { id: 1 }],
        synapses: [{ from: 0, to: 1 }],
        memetic: {
          generation: 1,
          score: 0,
          biases: { 0: 0.1, 99: 0.5 },
          ancestry: [{ generation: 0, score: 0, biases: { 0: 0.1, 98: 0.2 } }],
        },
      },
    },
  ] as const;
  return cases.map(({ name, test, helper, prepare, json }) => {
    const working = structuredClone(json);
    // Only the backward-edge tests explicitly normalise before invoking the helper.
    if (prepare) normaliseCreatureExport(working);
    const before = structuredClone(working);
    const result = helpers[helper](working);
    return {
      name,
      revision,
      test,
      helper,
      preparation: prepare ? "normaliseCreatureExport" : "none",
      input: json,
      before,
      result,
      after: working,
    };
  });
}
