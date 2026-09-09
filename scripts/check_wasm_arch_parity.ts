// check_wasm_arch_parity.ts — bit-exact numeric parity between the wasm32 and
// wasm64 wasm_activation bundles (Issue #541).
//
// Usage:
//   deno run --allow-read scripts/check_wasm_arch_parity.ts <wasm32-pkg> <wasm64-pkg>
//
// Widening `cfg(target_arch = "wasm32")` to the wasm family recompiles the
// SIMD128 kernels for a second address size. The kernels themselves are
// unchanged, so every overlapping result must be **bit-identical** — not close.
// A tolerance here would hide exactly the failure this gate exists to catch: a
// pointer-width change that silently perturbs a gather stride or a remainder.
//
// Both bundles are driven through the committed fixture in
// `tests/wasm_arch_parity_fixture.ts`, and the comparison is on raw IEEE-754
// bit patterns, so a sign-of-zero or last-ulp difference fails loudly instead of
// comparing equal.

import {
  bitPatterns,
  buildParityInput,
  buildParityNetwork,
  buildParityRecords,
  NUM_INPUTS,
  NUM_OUTPUTS,
  NUM_RECORDS,
  scalarBitPattern,
} from "../tests/wasm_arch_parity_fixture.ts";

/** Named observations taken from one bundle, keyed identically across arches. */
export type Observations = Map<string, string[]>;

/** Loss entry points compared as f64 sums over the packed record batch. */
const LOSS_ENTRY_POINTS = [
  "mse_sum_batch_packed",
  "mae_sum_batch_packed",
  "msle_sum_batch_packed",
  "hinge_sum_batch_packed",
  "cross_entropy_sum_batch_packed",
  "categorical_error_sum_batch_packed",
] as const;

// deno-lint-ignore no-explicit-any
async function loadBundle(pkgDir: string): Promise<any> {
  const gluePath = await Deno.realPath(`${pkgDir}/wasm_activation.js`);
  const wasmBytes = await Deno.readFile(`${pkgDir}/wasm_activation_bg.wasm`);
  const mod = await import(new URL(`file://${gluePath}`).href);
  await mod.default({ module_or_path: wasmBytes });
  return mod;
}

async function observe(pkgDir: string): Promise<Observations> {
  const api = await loadBundle(pkgDir);
  const out: Observations = new Map();

  const network = new api.CompiledNetwork(buildParityNetwork());
  out.set("shape", [
    String(network.num_neurons),
    String(network.num_inputs),
    String(network.num_synapses),
  ]);

  for (let r = 0; r < NUM_RECORDS; r++) {
    const input = buildParityInput(r);
    out.set(
      `activate[${r}]`,
      bitPatterns(network.activate(input, NUM_OUTPUTS)),
    );
    out.set(
      `activate_view[${r}]`,
      bitPatterns(network.activate_view(input, NUM_OUTPUTS)),
    );
    out.set(
      `activate_and_trace[${r}]`,
      bitPatterns(network.activate_and_trace(input, NUM_OUTPUTS)),
    );
  }

  const records = buildParityRecords();
  for (const name of LOSS_ENTRY_POINTS) {
    const sum = api[name](network, records, NUM_INPUTS, NUM_OUTPUTS, true);
    out.set(name, [scalarBitPattern(sum)]);
  }

  // Squash/derivative tables: cheap, and they cover the scalar fall-through the
  // aggregate neurons above do not reach.
  const squashSweep: string[] = [];
  for (const squashType of [0, 1, 6, 7, 13, 18, 27, 29]) {
    for (const x of [-2.5, -0.125, 0, 0.125, 2.5]) {
      squashSweep.push(scalarBitPattern(api.squash(squashType, x)));
      squashSweep.push(scalarBitPattern(api.derivative(squashType, x)));
    }
  }
  out.set("squash_derivative_sweep", squashSweep);

  return out;
}

/**
 * Every place the two arches disagree, as human-readable lines. An empty list
 * means bit-for-bit agreement across every observation taken.
 */
export function compare(left: Observations, right: Observations): string[] {
  const failures: string[] = [];
  const keys = new Set([...left.keys(), ...right.keys()]);
  for (const key of [...keys].sort()) {
    const a = left.get(key);
    const b = right.get(key);
    if (a === undefined || b === undefined) {
      failures.push(`${key}: observed on only one arch`);
      continue;
    }
    if (a.length !== b.length) {
      failures.push(`${key}: length ${a.length} vs ${b.length}`);
      continue;
    }
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) failures.push(`${key}[${i}]: 0x${a[i]} vs 0x${b[i]}`);
    }
  }
  return failures;
}

async function main(): Promise<void> {
  const [pkg32, pkg64] = Deno.args;
  if (!pkg32 || !pkg64) {
    throw new Error(
      "usage: check_wasm_arch_parity.ts <wasm32-pkg> <wasm64-pkg>",
    );
  }

  const left = await observe(pkg32);
  const right = await observe(pkg64);
  const failures = compare(left, right);

  const compared = [...left.values()].reduce((acc, v) => acc + v.length, 0);
  if (failures.length > 0) {
    throw new Error(
      `wasm32/wasm64 numeric parity broken on ${failures.length} of ${compared} ` +
        `compared values:\n  ${failures.slice(0, 40).join("\n  ")}`,
    );
  }
  console.log(`✅ wasm32 and wasm64 agree bit-for-bit on ${compared} values`);
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(
      `check_wasm_arch_parity: ${
        error instanceof Error ? error.message : error
      }`,
    );
    Deno.exit(1);
  }
}
