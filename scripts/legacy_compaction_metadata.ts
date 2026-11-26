/** Capture the original creature-based memetic tests without load-time cleanup. */
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
  return captures;
}
