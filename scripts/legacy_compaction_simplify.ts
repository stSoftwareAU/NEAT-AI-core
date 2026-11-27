/** Capture the frozen simplifyLargeWeights and magnitude penalty; do not implement their policy here. */
export async function captureLegacySimplify(root: URL, revision: string) {
  const { simplifyLargeWeights, calculateWeightBiasPenalty } = await import(
    new URL("src/compact/SimplifyLargeWeights.ts", root).href
  );
  const { normaliseCreatureExport } = await import(
    new URL("src/architecture/NormaliseCreatureExport.ts", root).href
  );
  const { magnitudePenalty } = await import(
    new URL("src/architecture/Score.ts", root).href
  );
  const source = "src/compact/SimplifyLargeWeights.ts";
  const preparation =
    "normaliseCreatureExport, then attach an empty memetic record";

  type Neuron = {
    uuid: string;
    type: string;
    squash: string;
    bias: number;
  };
  type Synapse = {
    fromUUID: string;
    toUUID: string;
    weight: number;
    type?: string;
  };
  type Export = {
    input: number;
    output: number;
    neurons: Neuron[];
    synapses: Synapse[];
  };

  const N = (
    uuid: string,
    type: string,
    squash: string,
    bias: number,
  ): Neuron => ({ uuid, type, squash, bias });
  const S = (
    fromUUID: string,
    toUUID: string,
    weight: number,
    type?: string,
  ): Synapse =>
    type ? { fromUUID, toUUID, weight, type } : { fromUUID, toUUID, weight };
  const outputs = [0, 1, 2, 3, 4];
  const worse = (): Export => ({
    input: 1,
    output: 5,
    neurons: [
      N("hidden-0", "hidden", "IDENTITY", 2000),
      ...outputs.map((i) => N(`output-${i}`, "output", "IDENTITY", 0)),
    ],
    synapses: [
      S("input-0", "hidden-0", 1),
      ...outputs.map((i) => S("hidden-0", `output-${i}`, 1)),
    ],
  });

  // JSON cannot hold NaN/Infinity: every weight and bias is the JS shortest string.
  const neuronsOf = (e: Export) =>
    e.neurons.map((n) => ({
      uuid: n.uuid,
      type: n.type,
      squash: n.squash,
      bias: String(n.bias),
    }));
  const synapsesOf = (e: Export) =>
    e.synapses.map((s) => ({
      fromUUID: s.fromUUID,
      toUUID: s.toUUID,
      weight: String(s.weight),
      ...(s.type === undefined ? {} : { type: s.type }),
    }));

  const cases: [string, Export, number | undefined, number | undefined][] = [
    [
      "absolute",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "ABSOLUTE", 1e6),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "relu_negative",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "ReLU", -3e6),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 2e6),
          S("hidden-0", "output-0", -4e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "tiny_inbound",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IDENTITY", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e-6),
          S("hidden-0", "output-0", 1e6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "typed_if",
      {
        input: 3,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IF", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6, "condition"),
          S("input-1", "hidden-0", 2e6, "positive"),
          S("input-2", "hidden-0", -1e6, "negative"),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "mean_hypot",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "MEAN", 5e5),
          N("hidden-1", "hidden", "HYPOTv2", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6),
          S("hidden-0", "hidden-1", 1e-6),
          S("input-0", "hidden-1", 1),
          S("hidden-1", "output-0", 1e-7),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "tanh_left",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "TANH", 1e6),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "balanced",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IDENTITY", 0.25),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1.5),
          S("hidden-0", "output-0", -0.75),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "output_skipped",
      {
        input: 1,
        output: 1,
        neurons: [N("output-0", "output", "ABSOLUTE", 1e6)],
        synapses: [
          S("input-0", "output-0", 1e6),
          S("output-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "nan_bias",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IDENTITY", NaN),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "nan_inbound_first",
      {
        input: 2,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IDENTITY", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", NaN),
          S("input-1", "hidden-0", 1e6),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "infinite_inbound",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "IDENTITY", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", Infinity),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    [
      "self_loop",
      {
        input: 1,
        output: 1,
        neurons: [
          N("hidden-0", "hidden", "ABSOLUTE", 0),
          N("output-0", "output", "IDENTITY", 0),
        ],
        synapses: [
          S("input-0", "hidden-0", 1e6),
          S("hidden-0", "hidden-0", 3),
          S("hidden-0", "output-0", 1e-6),
        ],
      },
      undefined,
      undefined,
    ],
    ["worse_greedy", worse(), undefined, undefined],
    ["worse_mcmc_accept", worse(), 1, 0],
    ["worse_mcmc_reject", worse(), 1, 0.999999],
  ];

  const captures: unknown[] = [];
  for (const [name, e, temperature, draw] of cases) {
    const input = structuredClone(e);
    const x = structuredClone(e) as Export & { memetic?: unknown };
    normaliseCreatureExport(x);
    x.memetic = { generation: 1, score: 0, biases: {}, weights: [] };
    let draws = 0;
    const real = Math.random;
    let before: number;
    let changed: boolean;
    try {
      if (draw !== undefined) {
        Math.random = () => {
          draws++;
          return draw;
        };
      }
      before = calculateWeightBiasPenalty(x);
      changed = simplifyLargeWeights(x, temperature);
    } finally {
      Math.random = real;
    }
    captures.push({
      name,
      revision,
      source,
      operation: "simplifyLargeWeights",
      preparation,
      temperature: temperature ?? null,
      draw: draw ?? null,
      draws,
      input: {
        input: input.input,
        output: input.output,
        neurons: neuronsOf(input),
        synapses: synapsesOf(input),
      },
      before,
      changed,
      memeticKept: x.memetic !== undefined,
      after: calculateWeightBiasPenalty(x),
      neurons: neuronsOf(x),
      synapses: synapsesOf(x),
    });
  }

  captures.push({
    name: "penalty_empty",
    revision,
    source,
    operation: "calculateWeightBiasPenalty",
    penalty: calculateWeightBiasPenalty({
      neurons: [],
      synapses: [],
      input: 0,
      output: 0,
    }),
  });

  const corpusPath = "test/fixtures/scoring/magnitude-penalty-corpus.json";
  const corpus = JSON.parse(
    await Deno.readTextFile(new URL(corpusPath, root)),
  );
  captures.push({
    name: "magnitude_penalty_corpus",
    revision,
    source: corpusPath,
    corpus,
    computed: corpus.cases.map((c: { magnitude: number }) => ({
      magnitude: c.magnitude,
      penalty: magnitudePenalty(c.magnitude),
    })),
  });
  return captures;
}
