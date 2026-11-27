/**
 * Public forest bridge regressions. The frozen helpers own the rewrites.
 * This records their observable counts and the deterministic probe; it does
 * not copy the 3.4 MB creature into core.
 */

type Synapse = { fromId?: number; toId?: number; fromUUID?: string; toUUID?: string };
type Neuron = { id?: number };
type Graph = { input: number; neurons: Neuron[]; synapses: Synapse[] };

function inputRows(inputs: number, rows: number): Float32Array[] {
  const data: Float32Array[] = [];
  for (let r = 0; r < rows; r++) {
    const row = new Float32Array(inputs);
    for (let i = 0; i < inputs; i++) {
      row[i] = Math.sin((i + 1) * 0.7 + r * 1.31);
    }
    data.push(row);
  }
  return data;
}

function positions(json: Graph): Map<number, number> {
  const map = new Map<number, number>();
  for (let i = 0; i < json.input; i++) map.set(i, i);
  let position = json.input;
  for (const neuron of json.neurons) map.set(neuron.id!, position++);
  return map;
}

function backwardSynapses(json: Graph): string[] {
  const position = positions(json);
  const backward: string[] = [];
  for (const synapse of json.synapses) {
    const from = position.get(synapse.fromId!);
    const to = position.get(synapse.toId!);
    if (from === undefined || to === undefined) {
      throw new Error("forest export is missing an endpoint id");
    }
    if (from >= to) {
      backward.push(`${synapse.fromUUID ?? from}->${synapse.toUUID ?? to}`);
    }
  }
  return backward;
}

function worstOutputDelta(
  before: { input: number; output: number; activate: (row: Float32Array, trace: boolean) => Float32Array },
  after: { activate: (row: Float32Array, trace: boolean) => Float32Array },
  rows: Float32Array[],
): number {
  let worst = 0;
  for (const row of rows) {
    const expected = before.activate(row, false);
    const actual = after.activate(row, false);
    for (let o = 0; o < before.output; o++) {
      const delta = Math.abs(actual[o] - expected[o]);
      if (delta > worst) worst = delta;
    }
  }
  return worst;
}

function mergeToFixpoint(
  json: Graph,
  mergeParallelBridges: (graph: Graph) => { removedNeurons: number },
): number {
  let removed = 0;
  for (let pass = 0; pass < 1000; pass++) {
    const result = mergeParallelBridges(json);
    if (result.removedNeurons === 0) return removed;
    removed += result.removedNeurons;
  }
  throw new Error("parallel bridge merge did not reach a fixpoint");
}

export async function captureLegacyForest(root: URL, revision: string) {
  const { Creature } = await import(new URL("src/Creature.ts", root).href);
  const { exportJSONUnchecked } = await import(
    new URL("src/creature/CreatureSerialization.ts", root).href
  );
  const { mergeParallelBridges } = await import(
    new URL("src/compact/ParallelBridgeMerge.ts", root).href
  );
  const { mergeRedundantConstants } = await import(
    new URL("src/compact/ConstantMerge.ts", root).href
  );
  const { initWasmForTests } = await import(
    new URL("test/_initWasm.ts", root).href
  );
  await initWasmForTests();

  const fixture = "test/data/grq-23-forests-constants.json";
  const bytes = await Deno.readFile(new URL(fixture, root));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const fixtureSha256 = [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  const source = JSON.parse(new TextDecoder().decode(bytes));

  const orderingCreature = Creature.fromJSON(structuredClone(source), false, "#3809");
  const orderingExport = exportJSONUnchecked(orderingCreature);
  mergeRedundantConstants(orderingExport);
  const orderingNeurons = orderingExport.neurons.length;
  const orderingSynapses = orderingExport.synapses.length;
  const orderingResult = mergeParallelBridges(orderingExport);
  const orderingReloaded = Creature.fromJSON(orderingExport, false, "#3809");
  const orderingDelta = worstOutputDelta(
    orderingCreature,
    orderingReloaded,
    inputRows(orderingCreature.input, 25),
  );

  const exactBefore = Creature.fromJSON(structuredClone(source), false);
  const exactExport = exportJSONUnchecked(exactBefore);
  const exactRemoved = mergeToFixpoint(exactExport, mergeParallelBridges);
  const exactBackward = backwardSynapses(exactExport);
  const exactAfter = Creature.fromJSON(exactExport, false, "issue-3809");

  return {
    revision,
    fixture,
    fixtureSha256,
    fixtureBytes: bytes.byteLength,
    input: source.input,
    output: source.output,
    declaredNeurons: source.neurons.length,
    declaredSynapses: source.synapses.length,
    cases: [
      {
        name: "ordering_declines_if_bridges",
        test: "test/compact/ParallelBridgeMergeOrdering.ts",
        source: "src/compact/ParallelBridgeMerge.ts",
        preparation: "mergeRedundantConstants",
        neuronsBefore: orderingNeurons,
        neuronsAfter: orderingExport.neurons.length,
        synapsesBefore: orderingSynapses,
        synapsesAfterHelper: orderingExport.synapses.length,
        synapsesAfterReload: orderingReloaded.synapses.length,
        removedNeurons: orderingResult.removedNeurons,
        probeRows: 25,
        probe: "sine",
        worstOutputDelta: orderingDelta,
      },
      {
        name: "exactness_fixpoint_stays_forward",
        test: "test/compact/ParallelBridgeMergeExactness.ts",
        source: "src/compact/ParallelBridgeMerge.ts",
        preparation: "mergeToFixpoint",
        neuronsBefore: exactBefore.neurons.length,
        neuronsAfter: exactAfter.neurons.length,
        synapsesBefore: exactBefore.synapses.length,
        synapsesAfter: exactAfter.synapses.length,
        removedNeurons: exactRemoved,
        backwardSynapses: exactBackward.length,
        outputCheck: "unseeded",
      },
    ],
  };
}
