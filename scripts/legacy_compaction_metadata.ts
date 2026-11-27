/** Capture original runtime repair tests without normalizing away their inputs. */
export async function captureLegacyMetadata(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { creatureValidate } = await import(
    new URL("src/architecture/CreatureValidate.ts", root).href
  );
  const { pruneOrphanMemeticReferences } = await import(
    new URL("src/compact/MemeticCleanup.ts", root).href
  );
  const { fix } = await import(
    new URL("src/creature/CreatureMutation.ts", root).href
  );
  const { removeNeuron } = await import(
    new URL("src/compact/CompactUnused.ts", root).href
  );
  const {
    createSeededRng,
    getRandomNumberGenerator,
    setRandomNumberGenerator,
  } = await import(new URL("src/utils/RandomNumberGenerator.ts", root).href);
  type LegacyCreature = ReturnType<typeof Creature.fromJSON>;
  function runtimeSnapshot(creature: LegacyCreature) {
    // exportJSON validates DEBUG creatures and converts memetic IDs to wire
    // labels. Preserve the invalid runtime state without calling that exporter.
    return structuredClone({
      neurons: creature.neurons.map((n: {
        id: number;
        index: number;
        type: string;
        squash?: string;
        bias: number;
      }) => ({
        id: n.id,
        index: n.index,
        type: n.type,
        squash: n.squash,
        // Input neurons carry Infinity; JSON must not silently turn it into null.
        bias: Number.isFinite(n.bias) ? n.bias : String(n.bias),
      })),
      synapses: creature.synapses.map((s: {
        from: number;
        to: number;
        weight: number;
        type?: string;
      }) => ({ from: s.from, to: s.to, weight: s.weight, type: s.type })),
      memetic: creature.memetic,
      forwardOnly: creature.forwardOnly,
    });
  }
  function validate(
    creature: LegacyCreature,
    options?: { forwardOnly: boolean },
  ) {
    try {
      creatureValidate(creature, options);
      return { valid: true };
    } catch (error) {
      return {
        valid: false,
        name: error instanceof Error ? error.name : "Error",
        message: error instanceof Error ? error.message : String(error),
      };
    }
  }
  // Both originals use this same literal, then attach memetic after fromJSON.
  const input = {
    input: 2,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "hidden-a", bias: 0, squash: "IDENTITY" },
      { type: "output", uuid: "output-0", bias: 0, squash: "IDENTITY" },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "hidden-a", weight: 1 },
      { fromUUID: "hidden-a", toUUID: "output-0", weight: 1 },
    ],
  };
  const captures: unknown[] = [];
  for (const name of ["creature_memetic_prune", "creature_memetic_fix"]) {
    const creature = Creature.fromJSON(structuredClone(input), true);
    const hiddenId = creature.neurons[creature.input].id;
    const prune = name === "creature_memetic_prune";
    creature.memetic = {
      generation: 1,
      score: 0,
      biases: prune
        ? { [hiddenId]: 0.1, 1_998_066_541: 0.5 }
        : { 1_998_066_541: 0.5 },
      weights: {},
    };
    const before = runtimeSnapshot(creature);
    // Only the first original test probes validation before its operation.
    const beforeValidation = prune ? validate(creature) : null;
    const options = prune ? undefined : { forwardOnly: true };
    const result = prune
      ? pruneOrphanMemeticReferences(creature)
      : fix(creature, options);
    const afterValidation = validate(creature, options);
    if (!afterValidation.valid) {
      throw new Error(`${name}: original post-repair validation failed`);
    }
    captures.push({
      name,
      revision,
      test: "test/compact/PruneOrphanMemeticReferences.ts",
      source: prune
        ? "src/compact/MemeticCleanup.ts"
        : "src/creature/CreatureMutation.ts",
      operation: prune ? "pruneOrphanMemeticReferences" : "fix",
      preparation: "Creature.fromJSON(input, true), then attach memetic",
      input,
      hiddenId,
      options,
      before,
      beforeValidation,
      result: result ?? null,
      after: runtimeSnapshot(creature),
      afterValidation,
      afterExport: creature.exportJSON(),
    });
  }
  // The four original literals share topology; only these scalar fields differ.
  for (
    const [name, hiddenBias, outputBias, outwardWeight, activation] of [
      ["finite_normal", 0.1, 0.5, 0.3, 0.5],
      ["finite_overflow", 0, 0, 1, 1],
      ["finite_nan", 0, 0, 0.5, NaN],
      ["finite_atomic", 0, 0, 0.5, Number.MAX_VALUE],
    ] as const
  ) {
    const atomic = name === "finite_atomic";
    const input = {
      neurons: [
        {
          type: "hidden",
          uuid: "hidden-0",
          squash: "IDENTITY",
          bias: hiddenBias,
        },
        {
          type: "output",
          uuid: "output-0",
          squash: "IDENTITY",
          bias: outputBias,
        },
        ...(atomic
          ? [{ type: "output", uuid: "output-1", squash: "IDENTITY", bias: 0 }]
          : []),
      ],
      synapses: [
        { fromUUID: "input-0", toUUID: "hidden-0", weight: 1.0 },
        { fromUUID: "hidden-0", toUUID: "output-0", weight: outwardWeight },
        ...(atomic
          ? [{ fromUUID: "hidden-0", toUUID: "output-1", weight: 1.0 }]
          : []),
      ],
      input: 2,
      output: atomic ? 2 : 1,
    };
    const creature = Creature.fromJSON(structuredClone(input));
    const hidden = creature.neurons.find((n: { type: string }) =>
      n.type === "hidden"
    );
    const loaded = runtimeSnapshot(creature);
    if (name === "finite_overflow" || atomic) {
      const output = creature.neurons.find((n: { id: number }) =>
        n.id === (atomic ? -2 : -1)
      );
      if (!atomic) output.bias = Number.MAX_VALUE / 2;
      const outward = creature.synapses.find((
        s: { from: number; to: number },
      ) => s.from === hidden.index && s.to === output.index);
      outward.weight = Number.MAX_VALUE;
    }
    const before = runtimeSnapshot(creature);
    // Successful removal chooses a random input for a temporary zero edge.
    // Seed only that capture; the original test does not require a given input.
    const captureSeed = name === "finite_normal" ? 1910 : null;
    const previousRng = getRandomNumberGenerator();
    if (captureSeed !== null) {
      setRandomNumberGenerator(createSeededRng(captureSeed));
    }
    let result;
    try {
      result = removeNeuron(hidden.id, creature, activation);
    } finally {
      if (captureSeed !== null) setRandomNumberGenerator(previousRng);
    }
    captures.push({
      name,
      revision,
      test: "test/compact/CompactUnusedFiniteGuard.ts",
      source: "src/compact/CompactUnused.ts",
      operation: "removeNeuron",
      preparation: name === "finite_overflow" || atomic
        ? "Creature.fromJSON(input), then original runtime injections"
        : "Creature.fromJSON(input)",
      input,
      hiddenId: hidden.id,
      loaded,
      before,
      activation: Number.isFinite(activation) ? activation : String(activation),
      captureSeed,
      result,
      after: runtimeSnapshot(creature),
    });
  }
  // Issue #4071 originals use seed 4071 around each operation and restore it.
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  const { cleanupOrphanedNeurons, cleanupOrphanedNeuronsInCreature } =
    await import(
      new URL("src/compact/OrphanedNeuronCleanup.ts", root).href
    );
  await initWasmForTests();
  const uuidInputs = [{
    semanticVersion: "4.0.0",
    input: 2,
    output: 1,
    neurons: [
      // Legacy-labelled constant: its uuid is what a uuid-less conversion
      // of the hidden neuron with id 1777777 would have collided with.
      { type: "constant", uuid: "legacy-neuron-1777777", bias: 0.5 },
      // Hidden neuron with no inward synapses: converted to a constant.
      {
        type: "hidden",
        uuid: "U",
        id: 1777777,
        squash: "IDENTITY",
        bias: 0.3,
      },
      { type: "hidden", uuid: "H2", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "H2", weight: 0.5 },
      { fromUUID: "legacy-neuron-1777777", toUUID: "output-0", weight: 0.7 },
      { fromUUID: "U", toUUID: "output-0", weight: 0.9 },
      { fromUUID: "H2", toUUID: "output-0", weight: 0.4 },
    ],
  }, {
    neurons: [
      {
        type: "hidden",
        uuid: "hidden-no-inward",
        squash: "LOGISTIC",
        bias: 0.5,
      },
      {
        type: "output",
        uuid: "output-0",
        squash: "LOGISTIC",
        bias: 0.1,
      },
    ],
    synapses: [
      { fromUUID: "hidden-no-inward", toUUID: "output-0", weight: 1.0 },
      { fromUUID: "input-0", toUUID: "output-0", weight: 1.0 },
    ],
    input: 2,
    output: 1,
  }];
  for (const [index, input] of uuidInputs.entries()) {
    const runtime = index === 0;
    const previousRng = getRandomNumberGenerator();
    setRandomNumberGenerator(createSeededRng(4071));
    try {
      const target = runtime
        ? Creature.fromJSON(structuredClone(input), false)
        : structuredClone(input);
      // The loaded creature is DEBUG=false, as in the original. This export
      // records its intermediate graph without validating or repairing it.
      const before = runtime ? target.exportJSON() : structuredClone(target);
      const result = runtime
        ? cleanupOrphanedNeuronsInCreature(target)
        : cleanupOrphanedNeurons(target);
      const after = runtime ? target.exportJSON() : structuredClone(target);
      const reloaded = runtime
        ? Creature.fromJSON(structuredClone(after), true)
        : null;
      const validation = runtime ? validate(reloaded) : null;
      if (validation && !validation.valid) {
        throw new Error("Converted constant reload failed");
      }
      captures.push({
        name: runtime ? "converted_uuid_roundtrip" : "converted_uuid_raw",
        revision,
        test: "test/compact/ConvertedConstantKeepsUuid_test.ts",
        testName: runtime
          ? "Issue #4071 - a converted constant survives an export round trip beside a legacy-labelled constant"
          : "Issue #4071 - cleanupOrphanedNeurons keeps the source uuid on a hidden converted to a constant",
        operation: runtime
          ? "cleanupOrphanedNeuronsInCreature"
          : "cleanupOrphanedNeurons",
        preparation: runtime
          ? "Creature.fromJSON(input, false)"
          : "raw export; no loader",
        seed: 4071,
        input,
        before,
        result,
        after,
        reloaded: reloaded?.exportJSON() ?? null,
        validation,
        constants: reloaded
          ? reloaded.neurons
            .filter((n: { type: string }) => n.type === "constant")
            .map((n: { uuid: string; index: number }) => ({
              uuid: n.uuid,
              outward: reloaded.outwardConnections(n.index).length,
            })).sort((a: { uuid: string }, b: { uuid: string }) =>
              a.uuid < b.uuid ? -1 : a.uuid > b.uuid ? 1 : 0
            )
          : null,
        runtimeIds: reloaded
          ? reloaded.neurons.map((n: { uuid: string; id: number }) => ({
            uuid: n.uuid,
            id: n.id,
          }))
          : null,
      });
    } finally {
      setRandomNumberGenerator(previousRng);
    }
  }
  return captures;
}
