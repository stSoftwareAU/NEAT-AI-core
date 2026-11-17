// Issue #765 — pins the persisted-creature presentation contract.
//
// `neat-core/tests/fixtures/creature_presentation/canonical/` holds creature
// JSON in the one byte layout NEAT-AI's TypeScript exporter writes: one-space
// indent, ECMAScript number text, `-0` kept, the per-kind key order, and no
// trailing newline. This gate fails when a canonical file drifts from the
// writer's output, when a variant stops meaning the same document, or when a
// file loses a feature the corpus exists to cover (so the checks cannot go
// vacuous). The Rust side (`neat-core/tests/creature_presentation.rs`) checks
// the same files parse and compile.
//
// Run: deno test --allow-read tests/creature_presentation_test.ts

import { assert, assertEquals, assertNotEquals } from "@std/assert";

const CORPUS = new URL(
  "../neat-core/tests/fixtures/creature_presentation/",
  import.meta.url,
);

const VARIANT_SHAPES = [
  "serde_typed_pretty",
  "serde_value_pretty",
  "jq_indent_1",
] as const;

// ---------------------------------------------------------------------------
// The canonical writer.

// `JSON.rawJSON` is in the runtime (Deno 2.9) but not yet in the bundled lib types.
const rawJSON = (JSON as unknown as { rawJSON(text: string): unknown }).rawJSON;

const negativeZeroReplacer = (_key: string, value: unknown) =>
  Object.is(value, -0) ? rawJSON("-0") : value;

/** Canonical bytes of a document: one-space indent, no trailing newline. */
function canonicalText(text: string): string {
  return JSON.stringify(JSON.parse(text), negativeZeroReplacer, 1);
}

// ---------------------------------------------------------------------------
// Document comparison.

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

/**
 * First path at which `actual` differs from `expected`, or null when they are
 * the same document. Key order is ignored, array order is not, and numbers
 * compare with `Object.is` so `-0` and `0` differ.
 */
function sameDocument(
  expected: Json,
  actual: Json,
  path = "$",
): string | null {
  if (Array.isArray(expected) || Array.isArray(actual)) {
    if (!Array.isArray(expected) || !Array.isArray(actual)) return path;
    if (expected.length !== actual.length) return `${path}.length`;
    for (let i = 0; i < expected.length; i++) {
      const diff = sameDocument(expected[i], actual[i], `${path}[${i}]`);
      if (diff !== null) return diff;
    }
    return null;
  }
  if (
    expected !== null && actual !== null && typeof expected === "object" &&
    typeof actual === "object"
  ) {
    for (const key of Object.keys(expected)) {
      if (!(key in actual)) return `${path}.${key}`;
    }
    for (const key of Object.keys(actual)) {
      if (!(key in expected)) return `${path}.${key}`;
    }
    for (const key of Object.keys(expected)) {
      const diff = sameDocument(expected[key], actual[key], `${path}.${key}`);
      if (diff !== null) return diff;
    }
    return null;
  }
  return Object.is(expected, actual) ? null : path;
}

// ---------------------------------------------------------------------------
// Corpus access.

function listJson(dir: string): string[] {
  const names: string[] = [];
  for (const entry of Deno.readDirSync(new URL(dir, CORPUS))) {
    if (entry.isFile && entry.name.endsWith(".json")) names.push(entry.name);
  }
  return names.sort();
}

function readCorpus(path: string): string {
  return Deno.readTextFileSync(new URL(path, CORPUS));
}

// ---------------------------------------------------------------------------
// The per-file manifest: what each canonical file must keep covering.

type UuidForm = "input" | "output" | "rfc4122" | "forest";

interface Features {
  /** Number literals that must appear verbatim in the canonical text. */
  literals: string[];
  /** Exact count of `-0` literals. */
  negativeZeros: number;
  /** A literal with a negative mantissa and a negative exponent. */
  negativeNegativeExponent: boolean;
  /** An integer-valued number in a bias or weight field. */
  integerValuedField: boolean;
  creatureTags: boolean;
  neuronTags: boolean;
  synapseTags: boolean;
  /** No `tags` or `frozen` key anywhere. */
  noTagsOrFrozen: boolean;
  unsortedNeurons: boolean;
  unsortedSynapses: boolean;
  ifNode: boolean;
  uuidForms: UuidForm[];
  memeticRows: boolean;
  memeticById: boolean;
  /** One id-keyed map holding both "2" and "10". */
  idMapTwoAndTen: boolean;
  escapedString: boolean;
  emptyContainers: boolean;
}

const NONE: Features = {
  literals: [],
  negativeZeros: 0,
  negativeNegativeExponent: false,
  integerValuedField: false,
  creatureTags: false,
  neuronTags: false,
  synapseTags: false,
  noTagsOrFrozen: false,
  unsortedNeurons: false,
  unsortedSynapses: false,
  ifNode: false,
  uuidForms: [],
  memeticRows: false,
  memeticById: false,
  idMapTwoAndTen: false,
  escapedString: false,
  emptyContainers: false,
};

const MANIFEST: Record<string, Features> = {
  "number_forms.json": {
    ...NONE,
    literals: [
      "0.000001",
      "1e-7",
      "100000000000000000000",
      "1e+21",
      "123456789012345680000",
      "5e-324",
      "1.7976931348623157e+308",
      "2.2985736498644322e-8",
      "-1.5e-9",
    ],
    negativeZeros: 1,
    negativeNegativeExponent: true,
    integerValuedField: true,
    creatureTags: true,
    unsortedNeurons: true,
    unsortedSynapses: true,
    uuidForms: ["input", "output", "rfc4122"],
    memeticRows: true,
  },
  "if_forest_tagged.json": {
    ...NONE,
    creatureTags: true,
    neuronTags: true,
    synapseTags: true,
    unsortedNeurons: true,
    unsortedSynapses: true,
    ifNode: true,
    uuidForms: ["input", "output", "forest"],
    memeticRows: true,
    escapedString: true,
    emptyContainers: true,
  },
  "memetic_by_id.json": {
    ...NONE,
    negativeZeros: 1,
    integerValuedField: true,
    noTagsOrFrozen: true,
    unsortedNeurons: true,
    unsortedSynapses: true,
    uuidForms: ["input", "output", "rfc4122"],
    memeticById: true,
    idMapTwoAndTen: true,
  },
};

// ---------------------------------------------------------------------------
// Number literals read from the canonical text.

// Disjoint pieces throughout, so a hostile line cannot backtrack catastrophically.
const NUMBER_LINE =
  /^(?:"(?:[^"\\]|\\.)*": )?(-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)$/;

/** The number literal a single canonical line holds, or null. */
function numberLiteral(line: string): string | null {
  let trimmed = line.trim();
  if (trimmed.endsWith(",")) trimmed = trimmed.slice(0, -1);
  const match = NUMBER_LINE.exec(trimmed);
  return match === null ? null : match[1];
}

function literalsOf(text: string): string[] {
  const found: string[] = [];
  for (const line of text.split("\n")) {
    const literal = numberLiteral(line);
    if (literal !== null) found.push(literal);
  }
  return found;
}

// ---------------------------------------------------------------------------
// Walking the document.

type Obj = { [k: string]: Json };

function isObj(value: Json | undefined): value is Obj {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function objects(value: Json | undefined): Obj[] {
  return Array.isArray(value) ? value.filter(isObj) : [];
}

/** Every value in the document, depth first. */
function* walk(value: Json): Generator<Json> {
  yield value;
  if (Array.isArray(value)) {
    for (const item of value) yield* walk(item);
  } else if (isObj(value)) {
    for (const item of Object.values(value)) yield* walk(item);
  }
}

function uuidsOf(doc: Obj): string[] {
  const uuids: string[] = [];
  for (const neuron of objects(doc.neurons)) {
    if (typeof neuron.uuid === "string") uuids.push(neuron.uuid);
  }
  for (const synapse of objects(doc.synapses)) {
    for (const key of ["fromUUID", "toUUID"]) {
      const uuid = synapse[key];
      if (typeof uuid === "string") uuids.push(uuid);
    }
  }
  return uuids;
}

const UUID_FORMS: Record<UuidForm, RegExp> = {
  input: /^input-\d+$/,
  output: /^output-\d+$/,
  rfc4122:
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  forest: /^forest-.+-if\d+$/,
};

function isUnsorted<T>(items: T[], key: (item: T) => string): boolean {
  const keys = items.map(key);
  const sorted = [...keys].sort();
  return keys.some((k, i) => k !== sorted[i]);
}

/** Integer-valued numbers sitting in a bias or weight position. */
function integerValuedFields(doc: Obj): number[] {
  const found: number[] = [];
  const take = (value: Json | undefined) => {
    if (typeof value === "number" && Number.isInteger(value)) found.push(value);
  };
  for (const neuron of objects(doc.neurons)) take(neuron.bias);
  for (const synapse of objects(doc.synapses)) take(synapse.weight);
  const visitMemetic = (record: Json | undefined) => {
    if (!isObj(record)) return;
    if (isObj(record.biases)) Object.values(record.biases).forEach(take);
    const weights = record.weights;
    if (Array.isArray(weights)) {
      for (const row of objects(weights)) take(row.weight);
    } else if (isObj(weights)) {
      for (const entries of Object.values(weights)) {
        for (const entry of objects(entries)) take(entry.weight);
      }
    }
    objects(record.ancestry).forEach(visitMemetic);
  };
  visitMemetic(doc.memetic);
  return found;
}

function memeticHasHistory(memetic: Obj): boolean {
  return typeof memetic.generation === "number" &&
    typeof memetic.score === "number" && Array.isArray(memetic.ancestry);
}

function isRowForm(weights: Json | undefined): boolean {
  return Array.isArray(weights) && weights.length > 0 &&
    objects(weights).every((r) =>
      typeof r.fromUUID === "string" && typeof r.toUUID === "string" &&
      typeof r.weight === "number"
    );
}

function isIdKeyedForm(weights: Json | undefined): boolean {
  if (!isObj(weights)) return false;
  const entries = Object.values(weights);
  return entries.length > 0 &&
    entries.every((list) =>
      Array.isArray(list) &&
      objects(list).every((e) =>
        typeof e.toId === "number" && typeof e.weight === "number"
      )
    );
}

function hasEmptyContainerLine(text: string, empty: "[]" | "{}"): boolean {
  return text.split("\n").some((line) => {
    const trimmed = line.trim().replace(/,$/, "");
    return trimmed.endsWith(`: ${empty}`);
  });
}

// ---------------------------------------------------------------------------
// Key order.

const INDEX_KEY = /^(?:0|[1-9]\d*)$/;

const KEY_TABLES = {
  creature: [
    "uuid",
    "semanticVersion",
    "forwardOnly",
    "neurons",
    "synapses",
    "input",
    "output",
    "tags",
    "memetic",
  ],
  neuron: ["type", "uuid", "bias", "squash", "frozen", "tags", "id"],
  synapse: ["weight", "fromUUID", "toUUID", "type", "frozen", "tags"],
  tag: ["name", "value"],
  memetic: ["generation", "weights", "biases", "score", "ancestry"],
  weightRow: ["fromUUID", "toUUID", "weight"],
  idEntry: ["toId", "weight"],
  map: [],
} as const;

type Kind = keyof typeof KEY_TABLES;

/** Array-index keys ascending, then table keys in table order, then the rest ascending. */
function expectedKeyOrder(keys: string[], table: readonly string[]): string[] {
  const index = keys.filter((k) => INDEX_KEY.test(k)).sort((a, b) =>
    Number(a) - Number(b)
  );
  const rest = keys.filter((k) => !INDEX_KEY.test(k));
  const tabled = table.filter((k) => rest.includes(k));
  const others = rest.filter((k) => !table.includes(k)).sort((a, b) =>
    a < b ? -1 : a > b ? 1 : 0
  );
  return [...index, ...tabled, ...others];
}

/** First path whose keys are out of contract order, or null. */
function keyOrderViolation(
  value: Json | undefined,
  kind: Kind,
  path: string,
): string | null {
  if (!isObj(value)) return null;
  const actual = Object.keys(value);
  const expected = expectedKeyOrder(actual, KEY_TABLES[kind]);
  if (actual.join("\u0000") !== expected.join("\u0000")) {
    return `${path}: keys [${actual.join(", ")}] should be [${
      expected.join(", ")
    }]`;
  }
  const children: [Json | undefined, Kind, string][] = [];
  const listOf = (items: Json | undefined, child: Kind, name: string) => {
    if (Array.isArray(items)) {
      items.forEach((item, i) =>
        children.push([item, child, `${path}.${name}[${i}]`])
      );
    }
  };
  switch (kind) {
    case "creature":
      listOf(value.neurons, "neuron", "neurons");
      listOf(value.synapses, "synapse", "synapses");
      listOf(value.tags, "tag", "tags");
      children.push([value.memetic, "memetic", `${path}.memetic`]);
      break;
    case "neuron":
    case "synapse":
      listOf(value.tags, "tag", "tags");
      break;
    case "memetic": {
      const weights = value.weights;
      if (Array.isArray(weights)) {
        listOf(weights, "weightRow", "weights");
      } else if (isObj(weights)) {
        children.push([weights, "map", `${path}.weights`]);
        for (const [id, entries] of Object.entries(weights)) {
          listOf(entries, "idEntry", `weights[${id}]`);
        }
      }
      children.push([value.biases, "map", `${path}.biases`]);
      listOf(value.ancestry, "memetic", "ancestry");
      break;
    }
    default:
      break;
  }
  for (const [child, childKind, childPath] of children) {
    const violation = keyOrderViolation(child, childKind, childPath);
    if (violation !== null) return violation;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Tests.

const canonicalNames = listJson("canonical/");

Deno.test("corpus is registered: canonical files match the manifest and every variant shape is populated", () => {
  assertEquals(canonicalNames, Object.keys(MANIFEST).sort());
  for (const shape of VARIANT_SHAPES) {
    assert(
      listJson(`variants/${shape}/`).length >= 1,
      `variants/${shape} holds no files`,
    );
  }
});

Deno.test("canonical files are byte-identical to the canonical writer's output", () => {
  assert(canonicalNames.length > 0, "no canonical files found");
  for (const name of canonicalNames) {
    const text = readCorpus(`canonical/${name}`);
    assertEquals(text, canonicalText(text), `${name} is not canonical`);
    assert(!text.endsWith("\n"), `${name} ends in a newline`);
    assert(text.endsWith("}"), `${name} does not end with "}"`);
  }
});

Deno.test("variants are the same document as their canonical file but different bytes", () => {
  let checked = 0;
  for (const shape of VARIANT_SHAPES) {
    for (const name of listJson(`variants/${shape}/`)) {
      const canonical = readCorpus(`canonical/${name}`);
      const variant = readCorpus(`variants/${shape}/${name}`);
      assertEquals(
        sameDocument(JSON.parse(canonical), JSON.parse(variant)),
        null,
        `variants/${shape}/${name} differs from canonical/${name}`,
      );
      assertNotEquals(
        variant,
        canonical,
        `variants/${shape}/${name} is byte-identical to canonical`,
      );
      checked++;
    }
  }
  assert(checked >= VARIANT_SHAPES.length, "too few variants compared");
});

for (const [name, want] of Object.entries(MANIFEST)) {
  Deno.test(`${name} keeps every feature the manifest requires`, () => {
    const text = readCorpus(`canonical/${name}`);
    const doc = JSON.parse(text) as Obj;
    const literals = literalsOf(text);

    for (const literal of want.literals) {
      assert(literals.includes(literal), `${name}: literal ${literal} missing`);
    }
    assertEquals(
      literals.filter((l) => l === "-0").length,
      want.negativeZeros,
      `${name}: count of -0 literals`,
    );
    if (want.negativeNegativeExponent) {
      assert(
        literals.some((l) => /^-[0-9.]+e-\d+$/.test(l)),
        `${name}: no negative literal with a negative exponent`,
      );
    }
    if (want.integerValuedField) {
      const ints = integerValuedFields(doc);
      assert(ints.length > 0, `${name}: no integer-valued bias or weight`);
      assert(
        ints.some((n) => literals.includes(String(n))),
        `${name}: integer-valued field is not written as an integer literal`,
      );
    }

    const neurons = objects(doc.neurons);
    const synapses = objects(doc.synapses);
    if (want.creatureTags) {
      assert(objects(doc.tags).length > 0, `${name}: no creature tags`);
    }
    if (want.neuronTags) {
      assert(
        neurons.some((n) => objects(n.tags).length > 0),
        `${name}: no neuron tags`,
      );
    }
    if (want.synapseTags) {
      assert(
        synapses.some((s) => objects(s.tags).length > 0),
        `${name}: no synapse tags`,
      );
    }
    if (want.noTagsOrFrozen) {
      for (const node of walk(doc)) {
        if (isObj(node)) {
          assert(
            !("tags" in node) && !("frozen" in node),
            `${name}: carries tags or frozen`,
          );
        }
      }
    }
    if (want.unsortedNeurons) {
      assert(
        isUnsorted(neurons, (n) => String(n.uuid)),
        `${name}: neurons are sorted`,
      );
    }
    if (want.unsortedSynapses) {
      assert(
        isUnsorted(synapses, (s) => `${s.fromUUID}\u0000${s.toUUID}`),
        `${name}: synapses are sorted`,
      );
    }
    if (want.ifNode) {
      const gates = neurons.filter((n) => n.squash === "IF");
      assert(
        gates.some((gate) => {
          const roles = new Set(
            synapses.filter((s) => s.toUUID === gate.uuid).map((s) => s.type),
          );
          return ["condition", "positive", "negative"].every((r) =>
            roles.has(r)
          );
        }),
        `${name}: no IF node receives condition, positive and negative synapses`,
      );
    }
    const uuids = uuidsOf(doc);
    for (const form of want.uuidForms) {
      assert(
        uuids.some((u) => UUID_FORMS[form].test(u)),
        `${name}: no ${form} uuid`,
      );
    }

    const memetic = isObj(doc.memetic) ? doc.memetic : null;
    if (want.memeticRows) {
      assert(
        memetic !== null && isRowForm(memetic.weights),
        `${name}: no row-form memetic`,
      );
    }
    if (want.memeticById) {
      assert(
        memetic !== null && isIdKeyedForm(memetic.weights),
        `${name}: no id-keyed memetic`,
      );
    }
    if (want.memeticRows || want.memeticById) {
      assert(
        memetic !== null && memeticHasHistory(memetic),
        `${name}: memetic lacks generation, score or ancestry`,
      );
    }
    if (want.idMapTwoAndTen) {
      assert(
        [...walk(memetic ?? null)].some((node) =>
          isObj(node) && "2" in node && "10" in node
        ),
        `${name}: no id-keyed map holds both "2" and "10"`,
      );
    }
    if (want.escapedString) {
      const awkward = [...walk(doc)].some((v) =>
        typeof v === "string" && /["\\\n]/.test(v)
      );
      assert(awkward, `${name}: no string needing an escape`);
      assert(/\\["\\n]/.test(text), `${name}: no escape in the canonical text`);
    }
    if (want.emptyContainers) {
      assert(hasEmptyContainerLine(text, "[]"), `${name}: no empty array`);
      assert(hasEmptyContainerLine(text, "{}"), `${name}: no empty object`);
    }
  });
}

Deno.test("number literal extraction is linear on a hostile line", () => {
  assertEquals(numberLiteral('   "weight": -1.5e-9,'), "-1.5e-9");
  assertEquals(numberLiteral("0.000001"), "0.000001");
  assertEquals(numberLiteral('"name": "1e-7"'), null);
  const hostile = '"' + "\\\\".repeat(50_000) + "x";
  assertEquals(hostile.length, 100_002);
  assertEquals(numberLiteral(hostile), null);
});

Deno.test("every object in every canonical file follows the contract key order", () => {
  assert(canonicalNames.length > 0, "no canonical files found");
  for (const name of canonicalNames) {
    const doc = JSON.parse(readCorpus(`canonical/${name}`));
    assertEquals(keyOrderViolation(doc, "creature", "$"), null, name);
  }
});

Deno.test("key order check rejects a misplaced table key and a misordered map", () => {
  const swapped = { weight: 1, fromUUID: "a", toUUID: "b" };
  assert(
    keyOrderViolation(
      { neurons: [], synapses: [{ fromUUID: "a", weight: 1, toUUID: "b" }] },
      "creature",
      "$",
    )?.startsWith("$.synapses[0]"),
  );
  assertEquals(keyOrderViolation(swapped, "synapse", "$"), null);
  assert(keyOrderViolation({ b: 1, a: 2 }, "map", "$.biases") !== null);
  assertEquals(
    keyOrderViolation({ "2": 1, "10": 1, "-1": 1, a: 1 }, "map", "$"),
    null,
  );
});

Deno.test("sameDocument accepts reordered keys and reports the path of every real difference", () => {
  const base: Json = {
    weight: 0.5,
    tags: [{ name: "a", value: "one" }, { name: "b", value: "two" }],
    list: [1, 2],
    neg: -0,
  };
  const reordered: Json = {
    neg: -0,
    list: [1, 2],
    tags: [{ value: "one", name: "a" }, { value: "two", name: "b" }],
    weight: 0.5,
  };
  assertEquals(sameDocument(base, reordered), null);

  const mutate = (change: (copy: { [k: string]: Json }) => void) => {
    const copy = structuredClone(base) as { [k: string]: Json };
    change(copy);
    return copy;
  };
  assertEquals(
    sameDocument(base, mutate((c) => c.weight = 0.25)),
    "$.weight",
  );
  assertEquals(sameDocument(base, mutate((c) => c.neg = 0)), "$.neg");
  assertEquals(
    sameDocument(
      base,
      mutate((c) => (c.tags as { [k: string]: Json }[])[1].value = "changed"),
    ),
    "$.tags[1].value",
  );
  assertEquals(
    sameDocument(base, mutate((c) => c.list = [2, 1])),
    "$.list[0]",
  );
  assertEquals(
    sameDocument(base, mutate((c) => delete c.weight)),
    "$.weight",
  );
  assertEquals(
    sameDocument(base, mutate((c) => c.extra = true)),
    "$.extra",
  );
  assertEquals(
    sameDocument(base, mutate((c) => c.list = [1])),
    "$.list.length",
  );
});
