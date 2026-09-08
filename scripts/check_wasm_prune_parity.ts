// check_wasm_prune_parity.ts — native vs WASM parity for the pruning rewrite
// APIs (Issue #592).
//
// Usage:
//   deno run --allow-read scripts/check_wasm_prune_parity.ts <pkg-dir> [golden.json]
//
// There is one Rust implementation (`prune_neuron` / `prune_synapse`) behind
// two entry surfaces: the native `prune_neuron_json` / `prune_synapse_json`,
// and the `#[wasm_bindgen]` renames over them. This gate is what stops the two
// drifting: `neat-core/tests/golden/prune_wasm_parity.json` records the request
// and the answer **native** gives it (regenerate with `UPDATE_PRUNE_GOLDEN=1
// cargo test -p neat-core --test prune_json`), and this script drives the built
// bundle through the same requests and compares.
//
// # What "parity" means here, exactly
//
// Everything a representation can carry exactly is compared exactly: the keys
// present, the array lengths, every uuid, role, squash name, reason code,
// message, boolean and integer. A single changed character fails.
//
// Floating-point values are compared to {@link FLOAT_TOLERANCE} rather than bit
// for bit, and that is deliberate rather than slack. A fold of a fixed neuron's
// activation runs its squash, and a transcendental (`exp`, `tanh`) is resolved
// by the host libm natively and by the wasm implementation in the bundle; the
// two are each correctly rounded to within an ulp but need not agree on the
// last bit. A bit-exact gate there would fail on the toolchain rather than on a
// defect. Every structural claim — which neuron survived, which role, which
// error — is still exact, so a rewrite that differs by anything more than
// rounding fails loudly.

/** Which entry point a case drives. */
export type PruneOp = "neuron" | "synapse";

/** One recorded request and the answer the native ABI gives it. */
export interface GoldenCase {
  name: string;
  note: string;
  op: PruneOp;
  // deno-lint-ignore no-explicit-any
  request: any;
  // deno-lint-ignore no-explicit-any
  response: any;
}

/**
 * Relative slack allowed on a float. Tight enough that a changed weight or a
 * differently-ordered sum fails; loose enough that a last-ulp difference
 * between two libm implementations does not.
 */
export const FLOAT_TOLERANCE = 1e-9;

/** Where the golden record lives, relative to the repository root. */
export const GOLDEN_PATH = "neat-core/tests/golden/prune_wasm_parity.json";

/** Parse the golden record, refusing one that would grade nothing. */
export function parseGolden(text: string, path: string = GOLDEN_PATH): GoldenCase[] {
  const cases = JSON.parse(text);
  if (!Array.isArray(cases) || cases.length === 0) {
    throw new Error(`${path} carries no cases — the parity gate would pass vacuously`);
  }
  return cases;
}

/** Read and parse the golden record. */
export async function loadGolden(path: string = GOLDEN_PATH): Promise<GoldenCase[]> {
  return parseGolden(await Deno.readTextFile(path), path);
}

/** True when two floats agree to {@link FLOAT_TOLERANCE}, relatively. */
function numbersAgree(expected: number, actual: number): boolean {
  if (Object.is(expected, actual)) return true;
  if (!Number.isFinite(expected) || !Number.isFinite(actual)) return false;
  return Math.abs(expected - actual) <= FLOAT_TOLERANCE * Math.max(1, Math.abs(expected));
}

/**
 * Compare one answer against the recorded native answer.
 *
 * Returns one line per difference, deepest path first; an empty array means the
 * two answers agree.
 */
export function compareAnswer(
  // deno-lint-ignore no-explicit-any
  expected: any,
  // deno-lint-ignore no-explicit-any
  actual: any,
  path = "$",
): string[] {
  if (expected === null || actual === null || typeof expected !== typeof actual) {
    return expected === actual ? [] : [`${path}: native ${show(expected)}, wasm ${show(actual)}`];
  }

  if (typeof expected === "number") {
    return numbersAgree(expected, actual as number)
      ? []
      : [`${path}: native ${expected}, wasm ${actual}`];
  }

  if (Array.isArray(expected) || Array.isArray(actual)) {
    if (!Array.isArray(expected) || !Array.isArray(actual)) {
      return [`${path}: native ${show(expected)}, wasm ${show(actual)}`];
    }
    if (expected.length !== actual.length) {
      return [`${path}: native has ${expected.length} entries, wasm has ${actual.length}`];
    }
    return expected.flatMap((entry, i) => compareAnswer(entry, actual[i], `${path}[${i}]`));
  }

  if (typeof expected === "object") {
    const differences: string[] = [];
    const keys = new Set([...Object.keys(expected), ...Object.keys(actual)]);
    for (const key of [...keys].sort()) {
      if (!(key in expected)) {
        differences.push(`${path}.${key}: absent natively, wasm ${show(actual[key])}`);
      } else if (!(key in actual)) {
        differences.push(`${path}.${key}: native ${show(expected[key])}, absent from wasm`);
      } else {
        differences.push(...compareAnswer(expected[key], actual[key], `${path}.${key}`));
      }
    }
    return differences;
  }

  return expected === actual ? [] : [`${path}: native ${show(expected)}, wasm ${show(actual)}`];
}

// deno-lint-ignore no-explicit-any
function show(value: any): string {
  const text = JSON.stringify(value) ?? String(value);
  return text.length > 120 ? `${text.slice(0, 117)}…` : text;
}

// deno-lint-ignore no-explicit-any
async function loadBundle(pkgDir: string): Promise<any> {
  const gluePath = await Deno.realPath(`${pkgDir}/wasm_activation.js`);
  const wasmBytes = await Deno.readFile(`${pkgDir}/wasm_activation_bg.wasm`);
  const mod = await import(new URL(`file://${gluePath}`).href);
  await mod.default({ module_or_path: wasmBytes });
  return mod;
}

/** The pruning surface a bundle must expose. */
export interface PruneApi {
  prune_neuron(request: string): string;
  prune_synapse(request: string): string;
}

/**
 * Drive every golden request through an already-loaded bundle and report the
 * differences.
 *
 * Separate from {@link checkBundle} so the comparison — which is the part that
 * decides whether the gate passes — is graded without a built bundle.
 */
export function checkAnswers(api: Partial<PruneApi>, golden: GoldenCase[]): string[] {
  for (const name of ["prune_neuron", "prune_synapse"] as const) {
    if (typeof api[name] !== "function") {
      throw new Error(
        `the bundle exports no ${name}() — the pruning surface never reached wasm`,
      );
    }
  }

  const failures: string[] = [];
  for (const testCase of golden) {
    if (testCase.op !== "neuron" && testCase.op !== "synapse") {
      // A record naming no entry point must not be answered by whichever one
      // happened to be the fallback.
      failures.push(`${testCase.name}: op '${testCase.op}' names no entry point`);
      continue;
    }
    const request = JSON.stringify(testCase.request);
    const raw = testCase.op === "neuron"
      ? (api as PruneApi).prune_neuron(request)
      : (api as PruneApi).prune_synapse(request);
    let answer: unknown;
    try {
      answer = JSON.parse(raw);
    } catch (error) {
      failures.push(`${testCase.name}: wasm answered something that is not JSON: ${error}`);
      continue;
    }
    for (const difference of compareAnswer(testCase.response, answer)) {
      failures.push(`${testCase.name}: ${difference}`);
    }
  }
  return failures;
}

/** Load the bundle at `pkgDir` and grade its answers against the record. */
export async function checkBundle(pkgDir: string, golden: GoldenCase[]): Promise<string[]> {
  return checkAnswers(await loadBundle(pkgDir), golden);
}

if (import.meta.main) {
  const [pkgDir, goldenPath] = Deno.args;
  if (!pkgDir) {
    console.error(
      "usage: deno run --allow-read scripts/check_wasm_prune_parity.ts <pkg-dir> [golden.json]",
    );
    Deno.exit(2);
  }

  const golden = await loadGolden(goldenPath ?? GOLDEN_PATH);
  const failures = await checkBundle(pkgDir, golden);
  if (failures.length > 0) {
    console.error(`❌ native/WASM pruning parity failed on ${failures.length} difference(s):`);
    for (const failure of failures) console.error(`   ${failure}`);
    Deno.exit(1);
  }
  console.log(`✅ native/WASM pruning parity: ${golden.length} cases agree`);
}
