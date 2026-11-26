/** Capture the unmodified legacy implementation; never regenerate from core. */
const [checkout, destination] = Deno.args;
if (!checkout || !destination) {
  throw new Error(
    "Usage: capture_legacy_compaction.ts <NEAT-AI checkout> <output.json>",
  );
}
const revision = "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab";
const head = await new Deno.Command("git", {
  args: ["-C", checkout, "rev-parse", "HEAD"],
}).output();
if (
  !head.success || new TextDecoder().decode(head.stdout).trim() !== revision
) {
  throw new Error(`Legacy checkout must be at ${revision}`);
}
const diff = await new Deno.Command("git", {
  args: ["-C", checkout, "status", "--porcelain", "--untracked-files=no"],
}).output();
if (!diff.success || diff.stdout.length) {
  throw new Error("Legacy checkout must have no tracked modifications");
}
const root = new URL(`file://${await Deno.realPath(checkout)}/`);
const { Creature } = await import(new URL("src/Creature.ts", root).href);
const { compactCreature } = await import(
  new URL("src/compact/CompactCreature.ts", root).href
);
const probes = [[0, 0], [1, 1], [-1, -1], [0.25, -0.75], [-2, 1]];
const captures = [];
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
  const creature = Creature.fromJSON({
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
  });
  const before = creature.exportJSON();
  const after = compactCreature(creature, true);
  captures.push({
    name,
    revision,
    source: "src/compact/CompactCreature.ts",
    test: "test/compact/CompactCreatureComplementBypass.ts",
    before,
    after: after?.exportJSON() ?? null,
    probes,
    outputs: probes.map((probe) =>
      Array.from(creature.activate(new Float32Array(probe), false))
    ),
  });
}
await Deno.writeTextFile(destination, JSON.stringify(captures, null, 2) + "\n");
