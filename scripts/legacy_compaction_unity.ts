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
    // Slots 0 and 1 are held by leading constants and the removed neuron sits at
    // the insertion point. Both stale indices name the removed neuron, so
    // removeNeuron's count loop reaches createConstantOne(creature, 2), which
    // mints a fresh slot-2 constant.
    {
      name: "unity_reuse_slot_one_held",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k1", undefined, -0.2),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("k1", "hidden-1", 0.25),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
          edge("input-1", "output-0", 1, "negative"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k1: "1" },
    },
    // Frozen defect: all three slots are held, so every createConstantOne call
    // returns a stale index naming the removed neuron, which is then treated as
    // the constant it is already connected to.
    {
      name: "unity_reuse_slot_two_held",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k1", undefined, -0.2),
          neuron("constant", "k2", undefined, 0.3),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("k1", "hidden-1", 0.25),
          edge("k2", "hidden-1", 0.125),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
          edge("input-1", "output-0", 1, "negative"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k1: "1", k2: "2" },
    },
    // The slot-0 holder is listed behind the removed neuron. Creature.fromJSON
    // moves the constants first, so the frozen code sees a leading holder and
    // takes the slot-1 reuse.
    {
      name: "unity_reuse_holder_behind",
      input: {
        neurons: [
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("constant", "k0", undefined, 0.7),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
          edge("input-1", "output-0", 1, "negative"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0" },
    },
    // Two leading constants hold slot 0. The dedupe loop re-sources the second
    // onto the first and removes it.
    {
      name: "unity_reuse_second_slot_zero_holder",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k0b", undefined, -0.2),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0b", "hidden-1", 0.25),
          edge("k0", "output-0", 0.5, "negative"),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k0b: "0" },
    },
    // A third leading constant also holds slot 0. The dedupe loop merges the
    // second holder into the first and stops, so the third is left as it is.
    {
      name: "unity_reuse_third_slot_zero_holder",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k0b", undefined, -0.2),
          neuron("constant", "k0c", undefined, 0.3),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0b", "hidden-1", 0.25),
          edge("k0c", "hidden-1", 0.125),
          edge("k0", "output-0", 0.5, "negative"),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k0b: "0", k0c: "0" },
    },
    // Slot 0 is held by one leading constant and slot 1 by two. The slot-0 reuse
    // asks for slot 1, whose dedupe loop merges the second slot-1 holder into the
    // first, so the temporary slot-1 constant survives.
    {
      name: "unity_reuse_second_slot_one_holder",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k1", undefined, -0.2),
          neuron("constant", "k1b", undefined, 0.3),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("k1b", "hidden-1", 0.25),
          edge("k1", "output-0", 0.5, "negative"),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k1: "1", k1b: "1" },
    },
    // Slots 0 and 1 are held by one leading constant each and slot 2 by two. The
    // slot-1 reuse asks for slot 2, whose dedupe loop merges the second slot-2
    // holder into the first, so the temporary slot-2 constant survives.
    {
      name: "unity_reuse_second_slot_two_holder",
      input: {
        neurons: [
          neuron("constant", "k0", undefined, 0.7),
          neuron("constant", "k1", undefined, -0.2),
          neuron("constant", "k2", undefined, 0.3),
          neuron("constant", "k2b", undefined, 0.6),
          neuron("hidden", "hidden-0", "IDENTITY", 0),
          neuron("hidden", "hidden-1", "IDENTITY", 0),
          neuron("output", "output-0", "IF", 0.25),
        ],
        synapses: [
          edge("input-0", "hidden-0", 1),
          edge("input-0", "hidden-1", 1),
          edge("k0", "hidden-1", 0.5),
          edge("k1", "hidden-1", 0.25),
          edge("k2b", "hidden-1", 0.125),
          edge("k2", "output-0", 0.5, "negative"),
          edge("hidden-0", "output-0", 3, "condition"),
          edge("hidden-1", "output-0", 2, "positive"),
        ],
        input: 2,
        output: 1,
      },
      remove: "hidden-0",
      activation: 1.5,
      unitySlots: { k0: "0", k1: "1", k2: "2", k2b: "2" },
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
