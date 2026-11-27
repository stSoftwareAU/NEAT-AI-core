/** Capture the frozen removeNeuron unity-constant branch; do not implement it here. */
export async function captureLegacyUnity(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { removeNeuron } = await import(
    new URL("src/compact/CompactUnused.ts", root).href
  );
  const { COMPACT_UNITY_SLOT_TAG } = await import(
    new URL("src/compact/OrphanedNeuronCleanup.ts", root).href
  );
  const { addTag, getTag } = await import("@stsoftware/tags/mod");
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();
  type Edge = {
    fromUUID: string;
    toUUID: string;
    weight: number;
    type?: string;
  };
  type Json = {
    neurons: { type: string; uuid: string; squash?: string; bias: number }[];
    synapses: Edge[];
    input: number;
    output: number;
  };
  const neuron = (
    type: string,
    uuid: string,
    squash: string | undefined,
    bias: number,
  ) =>
    squash === undefined ? { type, uuid, bias } : { type, uuid, squash, bias };
  const edge = (
    fromUUID: string,
    toUUID: string,
    weight: number,
    type?: string,
  ): Edge =>
    type === undefined
      ? { fromUUID, toUUID, weight }
      : { fromUUID, toUUID, weight, type };
  // Input neurons carry no uuid at runtime, so they are addressed as input-N.
  const endpointUUID = (
    creature: { input: number; neurons: { uuid: string }[] },
    index: number,
  ): string =>
    index < creature.input ? `input-${index}` : creature.neurons[index].uuid;
  const cases: {
    name: string;
    input: Json;
    remove: string;
    activation: number;
    unitySlots?: Record<string, string>;
  }[] = [
    {
      name: "unity_if",
      input: {
        neurons: [
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("hidden-0", "output-0", 2),
          edge("input-1", "output-0", 1),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
    },
    {
      name: "unity_mixed_targets",
      input: {
        neurons: [
          neuron("hidden", "hidden-0", "TANH", 0.1),
          neuron("output", "output-0", "IF", 0.25),
          neuron("output", "output-1", "IDENTITY", -0.5),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("hidden-0", "output-0", 2, "positive"),
          edge("input-1", "output-0", 1, "condition"),
          edge("input-0", "output-0", -1, "negative"),
          edge("hidden-0", "output-1", 0.75),
        ],
        input: 2,
        output: 2,
      },
      remove: "hidden-0",
      activation: 0.4,
    },
    {
      name: "unity_two_roles",
      input: {
        neurons: [
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("hidden-0", "output-0", 0.5, "condition"),
          edge("hidden-0", "output-0", -2, "positive"),
          edge("input-1", "output-0", 3, "negative"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 2,
    },
    {
      name: "unity_after_existing_constants",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k1", undefined, -0.2),
          neuron("hidden", "hidden-a", "IDENTITY", 0),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("output", "output-0", "MAXIMUM", 0.1),
        ],
        synapses: [
          edge("input-0", "hidden-a", 1),
          edge("k0", "hidden-a", 0.5),
          edge("k1", "output-0", 0.25),
          edge("hidden-a", "hidden-0", 1),
          edge("hidden-a", "output-0", 1),
          edge("hidden-0", "output-0", 3),
        ],
        input: 1,
        output: 1,
      },
      remove: "hidden-0",
      activation: -0.5,
    },
    {
      name: "unity_feeder_cascade",
      input: {
        neurons: [
          neuron("hidden", "hidden-a", "IDENTITY", 0.3),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("output", "output-0", "MEAN", 0),
        ],
        synapses: [
          edge("input-0", "hidden-a", 1),
          edge("hidden-a", "hidden-0", 1),
          edge("hidden-0", "output-0", 2),
          edge("input-1", "output-0", 1),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 0.3,
    },
    // Frozen connect throws on a non-finite weight after the constant was already inserted.
    {
      name: "unity_overflow",
      input: {
        neurons: [
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("output", "output-0", "HYPOT", 0),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("hidden-0", "output-0", 10),
          edge("input-1", "output-0", 1),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1e308,
    },
    // Frozen defect: createConstantOne reuses a tagged slot constant, removes the constant it just minted, and returns it with a stale index, so removal rewires through another neuron.
    {
      name: "unity_reuse_tagged_slot",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("hidden-0", "output-0", 2, "positive"),
          edge("hidden-1", "output-0", 3, "condition"),
          edge("input-1", "output-0", 1, "negative"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-1",
      activation: 1.5,
      unitySlots: { k0: "0" },
    },
  ];
  const captures: unknown[] = [];
  const originalRandomUUID = crypto.randomUUID.bind(crypto);
  for (const c of cases) {
    const creature = Creature.fromJSON(structuredClone(c.input));
    for (const [uuid, slot] of Object.entries(c.unitySlots ?? {})) {
      addTag(
        creature.neurons.find((n: { uuid: string }) => n.uuid === uuid),
        COMPACT_UNITY_SLOT_TAG,
        slot,
      );
    }
    const target = creature.neurons.find((n: { uuid: string }) =>
      n.uuid === c.remove
    );
    // The constant's uuid is crypto.randomUUID(); pin it so the capture is stable.
    let minted = 0;
    crypto.randomUUID = () =>
      `unity-constant-${minted++}` as `${string}-${string}-${string}-${string}-${string}`;
    let result: unknown;
    try {
      result = removeNeuron(target.id, creature, c.activation);
    } catch (error) {
      result = {
        threw: error instanceof Error ? error.message : String(error),
      };
    } finally {
      crypto.randomUUID = originalRandomUUID;
    }
    captures.push({
      name: c.name,
      revision,
      test: "src/compact/CompactUnused.ts removeNeuron",
      source: "src/compact/CompactUnused.ts",
      operation: "removeNeuronUnityConstant",
      preparation: "Creature.fromJSON(input), then tag unitySlots",
      input: c.input,
      unitySlots: c.unitySlots ?? {},
      remove: c.remove,
      activation: c.activation,
      minted,
      result,
      after: {
        neurons: creature.neurons.slice(creature.input).map((
          n: { type: string; uuid: string; squash?: string; bias: number },
        ) => ({
          type: n.type,
          uuid: n.uuid,
          squash: n.squash ?? null,
          bias: Number.isFinite(n.bias) ? n.bias : String(n.bias),
          unitySlot: getTag(n, COMPACT_UNITY_SLOT_TAG) ?? null,
        })),
        synapses: creature.synapses.map((
          s: { from: number; to: number; weight: number; type?: string },
        ) => ({
          fromUUID: endpointUUID(creature, s.from),
          toUUID: endpointUUID(creature, s.to),
          weight: Number.isFinite(s.weight) ? s.weight : String(s.weight),
          type: s.type ?? null,
        })),
        memetic: creature.memetic ?? null,
      },
    });
  }
  return captures;
}
