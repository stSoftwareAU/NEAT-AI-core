// Tests for the native/WASM pruning parity record and comparator — Issue #592.
//
// The end-to-end gate needs a *built* bundle, so it runs in `wasm-bundle.yml`.
// What these tests pin is the part that can silently go vacuous: a golden
// record that stops covering the shapes the wire has to carry, or a comparator
// that reports agreement it never checked.
//
// Run: deno test --allow-read tests/wasm_prune_parity_test.ts

import { assert, assertEquals, assertRejects, assertThrows } from "@std/assert";
import {
  checkAnswers,
  compareAnswer,
  FLOAT_TOLERANCE,
  type GoldenCase,
  GOLDEN_PATH,
  loadGolden,
  parseGolden,
} from "../scripts/check_wasm_prune_parity.ts";

const golden: GoldenCase[] = await loadGolden(GOLDEN_PATH);

Deno.test("the golden record carries a request and a native answer for every case", () => {
  assert(golden.length > 0, "an empty record would grade nothing");
  for (const testCase of golden) {
    assert(testCase.name, "every case is named");
    assert(testCase.note, `${testCase.name}: every case says what it pins`);
    assert(
      testCase.op === "neuron" || testCase.op === "synapse",
      `${testCase.name}: op must name an entry point, got ${testCase.op}`,
    );
    assert(typeof testCase.request === "object", `${testCase.name}: the request is an object`);
    assert(
      typeof testCase.response === "object" && testCase.response !== null,
      `${testCase.name}: the recorded native answer is an object`,
    );
    assert("ok" in testCase.response, `${testCase.name}: an answer always says ok`);
  }
});

Deno.test("the golden record reaches both entry points and both answer shapes", () => {
  assert(golden.some((c) => c.op === "neuron"), "no neuron removal recorded");
  assert(golden.some((c) => c.op === "synapse"), "no synapse removal recorded");
  assert(golden.some((c) => c.response.ok === true), "no successful rewrite recorded");
  assert(golden.some((c) => c.response.failure?.malformed === false), "no refusal recorded");
  assert(golden.some((c) => c.response.failure?.malformed === true), "no boundary fault recorded");
  assert(golden.some((c) => c.response.transform === "exact"), "no exact transform recorded");
  assert(
    golden.some((c) => c.response.transform === "approximate"),
    "no approximate transform recorded",
  );
  // `[]` is truthy in JS, so these ask for a *non-empty* payload: a record whose
  // IF and compensation arrays were all empty would grade neither.
  assert(
    golden.some((c) =>
      (c.response.staticIfNeurons?.length ?? 0) + (c.response.downgradedIfNeurons?.length ?? 0) > 0
    ),
    "no IF rewrite recorded — the typed edge cases would go ungraded",
  );
  assert(
    golden.some((c) => (c.response.weightShares?.length ?? 0) > 0),
    "no compensation payload recorded",
  );
  assert(
    golden.some((c) => (c.response.biasFolds?.length ?? 0) > 0),
    "no bias fold recorded",
  );
  assert(
    golden.some((c) => (c.response.cascadeNeurons?.length ?? 0) > 0),
    "no cascade recorded",
  );
  assert(
    golden.some((c) => (c.response.restoredIfRoles?.length ?? 0) > 0),
    "no restored IF role recorded",
  );
});

Deno.test("every successful answer carries the creature and the transform label", () => {
  for (const testCase of golden.filter((c) => c.response.ok === true)) {
    assert(testCase.response.creature, `${testCase.name}: an ok answer carries a creature`);
    assert(testCase.response.transform, `${testCase.name}: an ok answer carries a transform`);
    assert(
      Array.isArray(testCase.response.creature.neurons),
      `${testCase.name}: the creature is the CreatureExport wire shape`,
    );
  }
});

Deno.test("every refused answer carries a reason and no creature", () => {
  for (const testCase of golden.filter((c) => c.response.ok === false)) {
    assertEquals(
      testCase.response.creature,
      undefined,
      `${testCase.name}: a refusal must return no creature`,
    );
    assert(testCase.response.failure?.reason, `${testCase.name}: a refusal names its reason`);
    if (testCase.response.failure.malformed) {
      assert(
        String(testCase.response.failure.message).startsWith("MALFORMED_REQUEST:"),
        `${testCase.name}: a boundary fault leads with MALFORMED_REQUEST:`,
      );
    }
  }
});

Deno.test("the comparator reports agreement only when the answers agree", () => {
  const answer = golden[0].response;
  assertEquals(compareAnswer(answer, structuredClone(answer)), []);
});

Deno.test("the comparator catches a changed uuid, role, reason and array length", () => {
  const base = { ok: true, creature: { neurons: [{ uuid: "h-1", type: "hidden" }] } };

  const renamed = structuredClone(base);
  renamed.creature.neurons[0].uuid = "h-2";
  assertEquals(compareAnswer(base, renamed).length, 1);
  assert(compareAnswer(base, renamed)[0].includes("$.creature.neurons[0].uuid"));

  const shortened = { ok: true, creature: { neurons: [] } };
  assert(compareAnswer(base, shortened)[0].includes("1 entries"));

  const refused = { ok: false, failure: { reason: "UNKNOWN_NEURON" } };
  const other = { ok: false, failure: { reason: "PROTECTED_NEURON" } };
  assertEquals(compareAnswer(refused, other).length, 1);
});

Deno.test("the comparator catches a key one side does not carry", () => {
  const withKey = { ok: true, transform: "exact" };
  const without = { ok: true };
  assert(compareAnswer(withKey, without)[0].includes("absent from wasm"));
  assert(compareAnswer(without, withKey)[0].includes("absent natively"));
});

Deno.test("the comparator allows a last-ulp float difference and nothing wider", () => {
  const bias = 0.5744425168116591;
  assertEquals(compareAnswer({ bias }, { bias }), []);
  // One ulp of an f64 near 0.57 is ~1e-16 — inside the tolerance.
  assertEquals(compareAnswer({ bias }, { bias: bias + Number.EPSILON * bias }), []);
  // A changed weight is not.
  assertEquals(compareAnswer({ bias }, { bias: bias + 1e-6 }).length, 1);
  // The tolerance is relative, so a large value keeps proportional slack…
  assertEquals(compareAnswer({ w: 1e6 }, { w: 1e6 + 1e-4 }), []);
  // …and an absolute floor of 1 keeps a near-zero comparison from being exact.
  assertEquals(compareAnswer({ w: 0 }, { w: FLOAT_TOLERANCE / 2 }), []);
  assertEquals(compareAnswer({ w: 0 }, { w: 1e-3 }).length, 1);
});

Deno.test("the comparator does not mistake a type change for agreement", () => {
  assertEquals(compareAnswer({ ok: true }, { ok: "true" }).length, 1);
  assertEquals(compareAnswer({ passes: 2 }, { passes: null }).length, 1);
  assertEquals(compareAnswer({ list: [] }, { list: {} }).length, 1);
});

Deno.test("an empty or missing record fails loudly rather than passing vacuously", async () => {
  assertThrows(() => parseGolden("[]"), Error, "carries no cases");
  assertThrows(() => parseGolden("{}"), Error, "carries no cases");
  await assertRejects(() => loadGolden(`${GOLDEN_PATH}.missing`));
});

// The comparison the CI gate rests on — routing each request to its entry
// point, reading the answer and reporting the differences — is graded here
// against a stub surface, so it is covered everywhere and not only in the one
// place a built bundle exists. `checkBundle` adds the bundle load on top of
// this and nothing else.
function stub(answer: (request: string) => string) {
  return { prune_neuron: answer, prune_synapse: answer };
}

Deno.test("the driver reports no difference when the surface answers the record", () => {
  const answers = new Map(golden.map((c) => [JSON.stringify(c.request), c.response]));
  assertEquals(
    checkAnswers(stub((request) => JSON.stringify(answers.get(request))), golden),
    [],
  );
});

Deno.test("the driver reports a difference when the surface answers something else", () => {
  const failures = checkAnswers(stub(() => JSON.stringify({ ok: false })), golden);
  assert(failures.length > 0, "a surface answering nothing like the record must fail the gate");
  assert(failures.some((f) => f.includes("$.ok")), failures.join("\n"));
});

Deno.test("the driver fails loudly when the surface carries no pruning exports", () => {
  assertThrows(() => checkAnswers({}, golden), Error, "exports no prune_neuron()");
  assertThrows(
    () => checkAnswers({ prune_neuron: () => "{}" }, golden),
    Error,
    "exports no prune_synapse()",
  );
});

Deno.test("the driver fails loudly when the surface answers something that is not JSON", () => {
  const failures = checkAnswers(stub(() => "not json"), golden);
  assertEquals(failures.length, golden.length);
  assert(failures.every((f) => f.includes("not JSON")), failures.join("\n"));
});

Deno.test("the driver refuses a record naming no entry point", () => {
  const broken = [{ ...golden[0], op: "both" as unknown as GoldenCase["op"] }];
  const failures = checkAnswers(stub(() => "{}"), broken);
  assertEquals(failures.length, 1);
  assert(failures[0].includes("names no entry point"), failures[0]);
});
