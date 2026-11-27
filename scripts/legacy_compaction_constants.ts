/** Original raw constant-merge fixtures; the frozen helper owns all rewriting. */
function ifWithThreeConstants() {
  return {
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
      // A varying condition input keeps the IF alive (no constant collapse).
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
  };
}

export async function captureLegacyConstants(root: URL, revision: string) {
  const { mergeRedundantConstants } = await import(
    new URL("src/compact/ConstantMerge.ts", root).href
  );
  const { normaliseCreatureExport } = await import(
    new URL("src/architecture/NormaliseCreatureExport.ts", root).href
  );
  // Original constructor and raw literals from ConstantMerge.ts at the frozen revision.
  const cases = [
    {
      name: "if_roles",
      testName: "mergeRedundantConstants - one canonical constant per IF role",
      normalize: false,
      repeat: false,
      input: ifWithThreeConstants(),
    },
    {
      name: "fixpoint",
      testName:
        "mergeRedundantConstants - is a fixpoint (re-running changes nothing)",
      normalize: false,
      repeat: true,
      input: ifWithThreeConstants(),
    },
    {
      name: "same_role",
      testName: "mergeRedundantConstants - sums constants sharing one IF role",
      normalize: false,
      repeat: false,
      input: {
        semanticVersion: "4.0.0",
        forwardOnly: true,
        input: 2,
        output: 1,
        neurons: [
          { type: "constant", uuid: "const-a", bias: 0.5 },
          { type: "constant", uuid: "const-b", bias: 4 },
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
            fromUUID: "const-a",
            toUUID: "if-neuron",
            weight: 0.6,
            type: "positive",
          },
          // Same role as const-a: an IF sums the values within a role, so both fold
          // into one edge from one canonical constant.
          {
            fromUUID: "const-b",
            toUUID: "if-neuron",
            weight: 0.25,
            type: "positive",
          },
          {
            fromUUID: "input-1",
            toUUID: "if-neuron",
            weight: 1,
            type: "negative",
          },
          { fromUUID: "if-neuron", toUUID: "output-0", weight: 1 },
        ],
      },
    },
    {
      name: "maximum_slots",
      testName:
        "mergeRedundantConstants - never sums into a MAXIMUM, and stops at three slots",
      normalize: false,
      repeat: false,
      input: {
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
    },
    {
      name: "frozen",
      testName:
        "mergeRedundantConstants - leaves frozen constants and frozen synapses alone",
      normalize: false,
      repeat: false,
      input: {
        semanticVersion: "4.0.0",
        forwardOnly: true,
        input: 1,
        output: 1,
        neurons: [
          { type: "constant", uuid: "const-frozen", bias: 0.5, frozen: true },
          { type: "constant", uuid: "const-locked-edge", bias: 0.75 },
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
            fromUUID: "const-frozen",
            toUUID: "if-neuron",
            weight: 0.6,
            type: "positive",
          },
          {
            fromUUID: "const-locked-edge",
            toUUID: "if-neuron",
            weight: 0.3,
            type: "negative",
            frozen: true,
          },
          { fromUUID: "if-neuron", toUUID: "output-0", weight: 1 },
        ],
      },
    },
    {
      name: "reuse_slot",
      testName:
        "mergeRedundantConstants - reuses a canonical constant already present",
      normalize: true,
      repeat: false,
      input: {
        semanticVersion: "4.0.0",
        forwardOnly: true,
        input: 1,
        output: 1,
        neurons: [
          { type: "constant", uuid: "constant-0", bias: 1 },
          { type: "constant", uuid: "const-extra", bias: 0.5 },
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
            fromUUID: "constant-0",
            toUUID: "if-neuron",
            weight: 0.6,
            type: "positive",
          },
          {
            fromUUID: "const-extra",
            toUUID: "if-neuron",
            weight: 0.3,
            type: "negative",
          },
          { fromUUID: "if-neuron", toUUID: "output-0", weight: 1 },
        ],
      },
    },
  ] as const;
  return cases.map(({ name, testName, normalize, repeat, input }) => {
    const working = structuredClone(input);
    // Only the reuse test explicitly normalises before calling the helper.
    if (normalize) normaliseCreatureExport(working);
    const before = structuredClone(working);
    const result = mergeRedundantConstants(working);
    const after = structuredClone(working);
    let second = null;
    if (repeat) {
      const beforeJson = JSON.stringify(working);
      const secondBefore = structuredClone(working);
      const secondResult = mergeRedundantConstants(working);
      second = {
        before: secondBefore,
        result: secondResult,
        after: structuredClone(working),
        beforeJson,
        afterJson: JSON.stringify(working),
      };
    }
    return {
      name,
      revision,
      test: "test/compact/ConstantMerge.ts",
      testName,
      operation: "mergeRedundantConstants",
      preparation: normalize ? "normaliseCreatureExport" : "none",
      weightTolerance: 1e-12,
      input,
      before,
      result,
      after,
      second,
    };
  });
}
