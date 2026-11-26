/** Capture original variant/selection tests; do not implement their policy here. */
export async function captureLegacyVariants(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { CreatureUtil } = await import(
    new URL("src/architecture/CreatureUtils.ts", root).href
  );
  const { compactCreature, compactCreatureVariants } = await import(
    new URL("src/compact/CompactCreature.ts", root).href
  );
  const { selectCompactVariant } = await import(
    new URL("src/compact/CompactVariants.ts", root).href
  );
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();
  const test = "test/compact/CompactCreatureVariants.ts";
  type LegacyCreature = ReturnType<typeof Creature.fromJSON>;
  type Variants = { safe?: LegacyCreature; aggressive?: LegacyCreature };
  const captures: unknown[] = [];

  // Original tinyCreature input and preparation, with input retained for review.
  function tinyCreature(bias: number, score?: number) {
    const json = {
      input: 1,
      output: 1,
      neurons: [
        { type: "output", uuid: "output-0", bias, squash: "IDENTITY" },
      ],
      synapses: [
        { fromUUID: "input-0", toUUID: "output-0", weight: 1 },
      ],
    };
    // fromJSON assigns IDs into its argument; retain the original raw literal.
    const input = structuredClone(json);
    const creature = Creature.fromJSON(json, false);
    if (score !== undefined) creature.score = score;
    return { creature, input };
  }

  function snapshot(creature: LegacyCreature | undefined, input?: unknown) {
    if (!creature) return null;
    // Capture identity after selection. Calculating UUID is not itself selection.
    const uuid = CreatureUtil.makeUUID(creature);
    return {
      input,
      uuid,
      score: creature.score ?? null,
      graph: creature.exportJSON(),
    };
  }

  function selectedRole(variants: Variants) {
    const selected = selectCompactVariant(variants);
    if (selected === undefined) return "none";
    if (selected === variants.safe) return "safe";
    if (selected === variants.aggressive) return "aggressive";
    throw new Error("Selection returned neither original candidate");
  }

  // These two pipeline inputs cover the first four original tests. The fold
  // input is shared by existence, wrapper identity and UUID-deduplication tests.
  const fixture = "test/data/constant-fold-full-removal.json";
  const foldInput = JSON.parse(await Deno.readTextFile(new URL(fixture, root)));
  for (const name of ["fold_variants", "noop_variants"]) {
    const input = name === "fold_variants"
      ? foldInput
      : tinyCreature(0.1).input;
    const creature = Creature.fromJSON(structuredClone(input), false);
    const before = creature.exportJSON();
    const variants = compactCreatureVariants(creature, false);
    const selected = selectedRole(variants);
    const safe = snapshot(variants.safe);
    const aggressive = snapshot(variants.aggressive);
    const uuids = new Set(
      [safe, aggressive].filter((c) => c !== null).map((c) => c.uuid),
    );
    // The original wrapper test constructs a fresh creature from the same JSON.
    const wrapperSafe = name === "fold_variants"
      ? snapshot(
        compactCreature(
          Creature.fromJSON(structuredClone(input), false),
          false,
        ),
      )
      : undefined;
    captures.push({
      name,
      revision,
      test,
      source: "src/compact/CompactCreature.ts",
      operation: "compactCreatureVariants",
      fixture: name === "fold_variants" ? fixture : undefined,
      feedbackLoop: false,
      input,
      before,
      safe,
      aggressive,
      wrapperSafe,
      distinctCandidates: uuids.size,
      selected,
    });
  }

  // Exact bias/score tuples from the final four original selection tests.
  // One fallback test has three calls; the final test has lower and tie calls.
  const selectionInputs: {
    name: string;
    safe?: [number, number?];
    aggressive?: [number, number?];
  }[] = [
    { name: "select_safe_only", safe: [0.1] },
    { name: "select_aggressive_only", aggressive: [0.2] },
    { name: "select_empty" },
    { name: "select_duplicate", safe: [0.1, 5], aggressive: [0.1, 999] },
    { name: "select_higher", safe: [0.1, 5], aggressive: [0.2, 6] },
    { name: "select_lower", safe: [0.1, 6], aggressive: [0.2, 5] },
    { name: "select_tie", safe: [0.1, 5], aggressive: [0.2, 5] },
  ];
  for (const spec of selectionInputs) {
    const safe = spec.safe ? tinyCreature(...spec.safe) : undefined;
    const aggressive = spec.aggressive
      ? tinyCreature(...spec.aggressive)
      : undefined;
    const variants = { safe: safe?.creature, aggressive: aggressive?.creature };
    const selected = selectedRole(variants);
    captures.push({
      name: spec.name,
      revision,
      test,
      source: "src/compact/CompactVariants.ts",
      operation: "selectCompactVariant",
      sameInstance: !!safe && !!aggressive &&
        safe.creature === aggressive.creature,
      safe: snapshot(safe?.creature, safe?.input),
      aggressive: snapshot(aggressive?.creature, aggressive?.input),
      selected,
    });
  }
  return captures;
}
