/** Original behaviour-guard tests, retaining their conditional measurement. */
export async function captureLegacyBehaviour(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { compactCreature } = await import(
    new URL("src/compact/CompactCreature.ts", root).href
  );
  const {
    measureBehaviourDeviation,
    isExactBehaviourPreserved,
    buildProbeInputs,
    probeOutputs,
    BEHAVIOUR_NOISE_FLOOR,
    EXACT_BEHAVIOUR_TOLERANCE,
  } = await import(new URL("src/architecture/BehaviourGuard.ts", root).href);
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  // Use the frozen checkout's declared tags package, just as the original does.
  const config = JSON.parse(
    await Deno.readTextFile(new URL("deno.json", root)),
  );
  const { addTag } = await import(config.imports["@stsoftware/tags"] + "/mod");
  await initWasmForTests();
  const logistic = {
    input: 1,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "chain-a", squash: "LOGISTIC", bias: 0.5 },
      { type: "hidden", uuid: "chain-b", squash: "LOGISTIC", bias: -0.75 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "chain-a", weight: 1.5 },
      { fromUUID: "chain-a", toUUID: "chain-b", weight: 2 },
      { fromUUID: "chain-b", toUUID: "output-0", weight: 1 },
    ],
  };
  const identity = {
    input: 1,
    output: 1,
    neurons: [
      { type: "hidden", uuid: "id-a", squash: "IDENTITY", bias: 0 },
      { type: "hidden", uuid: "id-b", squash: "IDENTITY", bias: 0 },
      { type: "output", uuid: "output-0", squash: "IDENTITY", bias: 0.25 },
    ],
    synapses: [
      { fromUUID: "input-0", toUUID: "id-a", weight: 1.5 },
      { fromUUID: "id-a", toUUID: "id-b", weight: 2 },
      { fromUUID: "id-b", toUUID: "output-0", weight: 0.5 },
    ],
  };
  const cases = [
    {
      name: "logistic_chain",
      input: logistic,
      testName:
        "safe compaction never returns a creature that changed behaviour",
    },
    {
      name: "identity_chain",
      input: identity,
      testName: "safe compaction still folds an exactly foldable creature",
    },
    {
      name: "provenance",
      input: identity,
      testName: "safe compaction carries the creature's provenance tags",
    },
  ];
  const captures: unknown[] = [];
  for (const spec of cases) {
    const creature = Creature.fromJSON(structuredClone(spec.input), false);
    if (spec.name === "provenance") {
      addTag(creature, "trainID", "T-3840");
      addTag(creature, "error", "0.6310");
    }
    const before = creature.exportJSON();
    const candidate = compactCreature(creature, false);
    let guard = null;
    // The first original is conditional; the third checks tags only.
    if (candidate && spec.name !== "provenance") {
      const deviation = measureBehaviourDeviation(creature, candidate);
      const preserved = isExactBehaviourPreserved(deviation);
      const probes = buildProbeInputs(creature.input);
      guard = {
        deviation,
        preserved,
        noiseFloor: BEHAVIOUR_NOISE_FLOOR,
        relativeTolerance: EXACT_BEHAVIOUR_TOLERANCE,
        probes: probes.map((row: Float32Array) => Array.from(row)),
        beforeOutputs: probeOutputs(creature, probes).map((row: Float32Array) =>
          Array.from(row)
        ),
        afterOutputs: probeOutputs(candidate, probes).map((row: Float32Array) =>
          Array.from(row)
        ),
      };
    }
    captures.push({
      ...spec,
      revision,
      test: "test/compact/CompactBehaviourGuard.ts",
      operation: "compactCreature",
      feedbackLoop: false,
      preparation: spec.name === "provenance"
        ? "Creature.fromJSON(input, false), then add original provenance tags"
        : "Creature.fromJSON(input, false)",
      before,
      candidate: candidate?.exportJSON() ?? null,
      guard,
    });
  }
  return captures;
}
