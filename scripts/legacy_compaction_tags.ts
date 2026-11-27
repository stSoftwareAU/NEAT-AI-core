/** Original tag-preservation inputs, with their original preparation boundaries. */
export async function captureLegacyTags(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { compactCreature } = await import(
    new URL("src/compact/CompactCreature.ts", root).href
  );
  const { cleanupOrphanedNeurons } = await import(
    new URL("src/compact/OrphanedNeuronCleanup.ts", root).href
  );
  const { mergeParallelBridges } = await import(
    new URL("src/compact/ParallelBridgeMerge.ts", root).href
  );
  const { normaliseCreatureExport } = await import(
    new URL("src/architecture/NormaliseCreatureExport.ts", root).href
  );
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();
  // Literal inputs from all eight tests in the frozen CompactTagPreservation.ts.
  const cases = [
    {
      name: "complement_new",
      testName:
        "compact: COMPLEMENT bypass preserves synapse tags on new synapses",
      operation: "compactCreature",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "complement-0",
            squash: "COMPLEMENT",
            bias: 0,
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "complement-0",
            weight: 0.5,
            tags: [{ name: "source", value: "training-data" }],
          },
          {
            fromUUID: "complement-0",
            toUUID: "output-0",
            weight: 0.8,
            tags: [{ name: "layer", value: "output-link" }],
          },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "complement_existing",
      testName:
        "compact: COMPLEMENT bypass merges tags when adding to existing synapse",
      operation: "compactCreature",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "complement-0",
            squash: "COMPLEMENT",
            bias: 0,
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "complement-0",
            weight: 0.5,
            tags: [{ name: "path", value: "complement" }],
          },
          {
            fromUUID: "complement-0",
            toUUID: "output-0",
            weight: 0.8,
          },
          {
            fromUUID: "input-0",
            toUUID: "output-0",
            weight: 0.3,
            tags: [{ name: "path", value: "direct" }],
          },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "identity_chain",
      testName: "compact: IDENTITY chain merge preserves synapse tags",
      operation: "compactCreature",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "hidden-0",
            squash: "IDENTITY",
            bias: 0,
          },
          {
            type: "hidden",
            uuid: "hidden-1",
            squash: "IDENTITY",
            bias: 0,
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "hidden-0",
            weight: 1.0,
          },
          {
            fromUUID: "hidden-0",
            toUUID: "hidden-1",
            weight: 1.0,
            tags: [{ name: "chain", value: "bridge" }],
          },
          {
            fromUUID: "hidden-1",
            toUUID: "output-0",
            weight: 1.0,
            tags: [{ name: "layer", value: "to-output" }],
          },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "orphan_constant",
      testName:
        "compact: orphaned neuron conversion to constant preserves neuron tags",
      operation: "cleanupOrphanedNeurons",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "orphan-0",
            squash: "IDENTITY",
            bias: 0.5,
            tags: [{ name: "purpose", value: "bias-source" }],
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          // orphan-0 has outward but no inward connections -> will be converted to constant
          { fromUUID: "orphan-0", toUUID: "output-0", weight: 1.0 },
          { fromUUID: "input-0", toUUID: "output-0", weight: 0.5 },
        ],
        input: 1,
        output: 1,
      },
    },
    {
      name: "bridge_identity_neurons",
      testName:
        "compact: parallel IDENTITY bridge merge preserves neuron tags on kept neuron",
      operation: "mergeParallelBridges",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "bridge-A",
            squash: "IDENTITY",
            bias: 0,
            tags: [{ name: "role", value: "primary" }],
          },
          {
            type: "hidden",
            uuid: "bridge-B",
            squash: "IDENTITY",
            bias: 0,
            tags: [{ name: "role", value: "secondary" }],
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "bridge-A", weight: 0.5 },
          { fromUUID: "bridge-A", toUUID: "output-0", weight: 1.0 },
          { fromUUID: "input-1", toUUID: "bridge-B", weight: 0.3 },
          { fromUUID: "bridge-B", toUUID: "output-0", weight: 1.0 },
        ],
        input: 2,
        output: 1,
      },
    },
    {
      name: "bridge_complement_neurons",
      testName:
        "compact: parallel bridge merge preserves neuron tags on kept neuron",
      operation: "mergeParallelBridges",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "bridge-A",
            squash: "COMPLEMENT",
            bias: 0,
            tags: [{ name: "variant", value: "alpha" }],
          },
          {
            type: "hidden",
            uuid: "bridge-B",
            squash: "COMPLEMENT",
            bias: 0,
            tags: [{ name: "variant", value: "beta" }],
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "bridge-A", weight: 0.5 },
          { fromUUID: "bridge-A", toUUID: "output-0", weight: 1.0 },
          { fromUUID: "input-1", toUUID: "bridge-B", weight: 0.3 },
          { fromUUID: "bridge-B", toUUID: "output-0", weight: 1.0 },
        ],
        input: 2,
        output: 1,
      },
    },
    {
      name: "bridge_synapses",
      testName:
        "compact: parallel IDENTITY bridge merge preserves synapse tags",
      operation: "mergeParallelBridges",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "bridge-A",
            squash: "IDENTITY",
            bias: 0,
          },
          {
            type: "hidden",
            uuid: "bridge-B",
            squash: "IDENTITY",
            bias: 0,
          },
          { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
        ],
        synapses: [
          {
            fromUUID: "input-0",
            toUUID: "bridge-A",
            weight: 0.5,
            tags: [{ name: "channel", value: "A" }],
          },
          { fromUUID: "bridge-A", toUUID: "output-0", weight: 1.0 },
          {
            fromUUID: "input-1",
            toUUID: "bridge-B",
            weight: 0.3,
            tags: [{ name: "channel", value: "B" }],
          },
          { fromUUID: "bridge-B", toUUID: "output-0", weight: 1.0 },
        ],
        input: 2,
        output: 1,
      },
    },
    {
      name: "complement_output",
      testName:
        "compact: COMPLEMENT bypass preserves neuron tags on remaining neurons",
      operation: "compactCreature",
      input: {
        neurons: [
          {
            type: "hidden",
            uuid: "complement-0",
            squash: "COMPLEMENT",
            bias: 0,
          },
          {
            type: "output",
            uuid: "output-0",
            squash: "IDENTITY",
            bias: 0,
            tags: [{ name: "role", value: "final-output" }],
          },
        ],
        synapses: [
          { fromUUID: "input-0", toUUID: "complement-0", weight: 0.5 },
          { fromUUID: "complement-0", toUUID: "output-0", weight: 0.8 },
        ],
        input: 1,
        output: 1,
      },
    },
  ] as const;
  return cases.map(({ name, testName, operation, input }) => {
    // Keep source literals intact: loading and raw helpers can add IDs.
    const working = structuredClone(input);
    const pipeline = operation === "compactCreature";
    const creature = pipeline ? Creature.fromJSON(working) : undefined;
    const before = creature ? creature.exportJSON() : structuredClone(working);
    let result: unknown;
    let after;
    if (creature) {
      const compacted = compactCreature(creature, false);
      result = compacted ? "candidate" : null;
      after = compacted?.exportJSON() ?? null;
      // Only the final original test explicitly normalises the returned export.
      if (after && name === "complement_output") normaliseCreatureExport(after);
    } else {
      result = operation === "cleanupOrphanedNeurons"
        ? cleanupOrphanedNeurons(working)
        : mergeParallelBridges(working);
      after = working;
    }
    return {
      name,
      revision,
      test: "test/compact/CompactTagPreservation.ts",
      testName,
      operation,
      preparation: pipeline ? "Creature.fromJSON" : "none",
      outputNormalization: name === "complement_output"
        ? "normaliseCreatureExport"
        : "none",
      feedbackLoop: pipeline ? false : null,
      input,
      before,
      result,
      after,
    };
  });
}
