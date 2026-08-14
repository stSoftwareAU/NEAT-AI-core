// check_wasm64_bundle.ts — CI/build gate over a *built* wasm_activation pkg/
// directory (Issue #541).
//
// Usage:
//   deno run --allow-read scripts/check_wasm64_bundle.ts <pkg-dir> --arch wasm64
//   deno run --allow-read scripts/check_wasm64_bundle.ts <pkg-dir> --arch wasm32
//
// What it proves, against the real artefact rather than a hand-assembled
// fixture:
//
//   * the module validates in this runtime;
//   * its linear memory carries the index type the arch demands — i64 for
//     wasm64, i32 for the wasm32 rollback asset. A wasm64 build that silently
//     regressed to i32 still validates and still scores, and would still hit
//     the 4 GiB wall the port exists to remove;
//   * the activation/backprop surface survives on **both** sides of the
//     boundary (module exports and generated glue) — wasm-bindgen 0.2.108
//     exited 0 while stripping the glue on wasm64;
//   * for wasm64 only: the artefact's own `WebAssembly.Memory` grows past
//     65536 pages, using the `BigInt` delta a 64-bit index type requires.
//
// Exits non-zero with a diagnostic on any failure — never "no failure marker,
// therefore a pass".

import {
  assertBundleExportSurface,
  assertMemory64,
  parseMemoryLimits,
  WASM32_MAX_PAGES,
  WASM_PAGE_BYTES,
} from "./wasm64_bundle_gate.ts";

/** One page past the wasm32 ceiling: the smallest growth wasm32 cannot do. */
const GROW_TARGET_PAGES = WASM32_MAX_PAGES + 16;

interface Args {
  pkgDir: string;
  arch: "wasm32" | "wasm64";
}

function parseArgs(argv: string[]): Args {
  let pkgDir = "";
  let arch: "wasm32" | "wasm64" | "" = "";
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--arch") {
      const value = argv[++i];
      if (value !== "wasm32" && value !== "wasm64") {
        throw new Error(`--arch must be wasm32 or wasm64 (got ${value ?? "nothing"})`);
      }
      arch = value;
    } else if (argv[i].startsWith("-")) {
      throw new Error(`unknown option: ${argv[i]}`);
    } else if (pkgDir === "") {
      pkgDir = argv[i];
    } else {
      throw new Error(`unexpected extra argument: ${argv[i]}`);
    }
  }
  if (pkgDir === "") throw new Error("usage: check_wasm64_bundle.ts <pkg-dir> --arch <wasm32|wasm64>");
  if (arch === "") throw new Error("--arch <wasm32|wasm64> is required");
  return { pkgDir, arch };
}

/**
 * Grow the artefact's own linear memory one page past the wasm32 ceiling.
 * V8 reserves pages lazily, so this costs address space, not RAM.
 */
async function assertGrowsPastWasm32Ceiling(pkgDir: string, wasmBytes: Uint8Array): Promise<void> {
  const glueUrl = new URL(`file://${await Deno.realPath(`${pkgDir}/wasm_activation.js`)}`);
  const mod = await import(glueUrl.href);
  const exports = await mod.default({ module_or_path: wasmBytes });
  const memory: WebAssembly.Memory = exports.memory ?? mod.memory;
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new Error("initialised bundle exposes no WebAssembly.Memory export");
  }

  const currentPages = BigInt(memory.buffer.byteLength / WASM_PAGE_BYTES);
  const delta = BigInt(GROW_TARGET_PAGES) - currentPages;
  // A 64-bit memory demands a BigInt delta and answers with a BigInt; a Number
  // throws. That signature *is* the observable proof of the i64 index type.
  const previous = memory.grow(delta as unknown as number);
  if (typeof previous !== "bigint") {
    throw new Error(
      `WebAssembly.Memory.grow returned ${typeof previous}, not bigint — ` +
        `this memory is not 64-bit indexed`,
    );
  }
  const pagesNow = memory.buffer.byteLength / WASM_PAGE_BYTES;
  if (pagesNow <= WASM32_MAX_PAGES) {
    throw new Error(
      `linear memory grew to ${pagesNow} pages, which does not pass the wasm32 ` +
        `ceiling of ${WASM32_MAX_PAGES} pages (4 GiB)`,
    );
  }
  console.log(`✅ grew linear memory to ${pagesNow} pages (> ${WASM32_MAX_PAGES} = 4 GiB)`);
}

async function main(): Promise<void> {
  const { pkgDir, arch } = parseArgs(Deno.args);
  const wasmPath = `${pkgDir}/wasm_activation_bg.wasm`;
  const gluePath = `${pkgDir}/wasm_activation.js`;

  const wasmBytes = await Deno.readFile(wasmPath);
  const glueSource = await Deno.readTextFile(gluePath);

  if (!WebAssembly.validate(wasmBytes)) {
    throw new Error(`${wasmPath}: WebAssembly.validate rejected the module`);
  }
  console.log(`✅ ${wasmPath} validates (${wasmBytes.length} bytes)`);

  if (arch === "wasm64") {
    assertMemory64(wasmBytes, wasmPath);
    console.log("✅ linear memory declares the i64 index type");
  } else {
    const limits = parseMemoryLimits(wasmBytes);
    if (limits.isMemory64) {
      throw new Error(
        `${wasmPath}: --arch wasm32 was requested but the module declares a ` +
          `64-bit memory (flags 0x${limits.flags.toString(16)}); the dual-ship ` +
          `rollback asset must stay wasm32`,
      );
    }
    console.log("✅ linear memory declares the i32 index type (wasm32 rollback asset)");
  }

  assertBundleExportSurface(wasmBytes, glueSource);
  console.log("✅ activation/backprop export surface present in module and glue");

  if (arch === "wasm64") await assertGrowsPastWasm32Ceiling(pkgDir, wasmBytes);

  console.log(`✅ ${pkgDir} passes the ${arch} bundle gate`);
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(`check_wasm64_bundle: ${error instanceof Error ? error.message : error}`);
    Deno.exit(1);
  }
}
