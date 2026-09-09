// wasm64_bundle_gate.ts — the checks that decide whether a built
// wasm_activation bundle is shippable (Issue #541).
//
// Two failure modes this repo has actually hit, both silent:
//
//   1. `wasm-bindgen` 0.2.108 exited 0 on a `wasm64-unknown-unknown` module
//      while stripping every binding out of the generated glue — the packaging
//      step tarred a bundle whose JS surface was `initSync`/`init` and nothing
//      else (docs/research/wasm64-lane-b-build-lane-feasibility.md).
//   2. A wasm64 build that silently regressed to a 32-bit memory would still
//      validate, instantiate and score — and would still hit the 4 GiB wall the
//      Memory64 port exists to remove.
//
// Every function here fails **loud**: it throws with the artefact name and the
// exact symbols or flags that are wrong, so the build script exits non-zero
// instead of publishing a broken bundle. This module holds no I/O so it can be
// unit-tested against synthetic modules; `scripts/check_wasm64_bundle.ts` is
// the CLI that reads a real `pkg/` directory and calls it.

/** Bytes in one WASM page (64 KiB). */
export const WASM_PAGE_BYTES = 64 * 1024;

/** wasm32 linear memory is capped at 65536 pages = exactly 4 GiB. */
export const WASM32_MAX_PAGES = 65536;

/** The hard wasm32 address-space ceiling in bytes (4 GiB). */
export const WASM32_MAX_BYTES = WASM32_MAX_PAGES * WASM_PAGE_BYTES;

/**
 * Memory-type flag bit that marks a 64-bit (i64) index type. A wasm32 memory
 * has this bit clear; a Memory64 memory has it set. (Bit 0 = has-maximum.)
 */
export const MEMORY64_INDEX_FLAG = 0x04;

/**
 * Exports the `_bg.wasm` must declare for NEAT-AI to drive the bundle: the
 * allocator pair the glue marshals slices through, the `CompiledNetwork`
 * lifecycle and activation surface, the backprop entry points, and the squash
 * helpers. A bundle missing any of these is unusable regardless of how large
 * the `.wasm` blob is, which is what the byte-size threshold alone cannot see.
 */
export const REQUIRED_WASM_EXPORTS: string[] = [
  "memory",
  "__wbindgen_malloc",
  "__wbindgen_free",
  "__wbg_compilednetwork_free",
  "compilednetwork_new",
  "compilednetwork_activate",
  "compilednetwork_activate_into",
  "compilednetwork_activate_and_trace",
  "propagate_topological",
  "mse_sum_batch_packed",
  "squash",
  "unsquash",
  "derivative",
];

/** Bindings the generated glue must re-export for the same surface. */
export const REQUIRED_GLUE_EXPORTS: string[] = [
  "CompiledNetwork",
  "propagate_topological",
  "mse_sum_batch_packed",
  "squash",
  "unsquash",
  "derivative",
];

/** Parsed limits of the first memory declared in a module. */
export interface MemoryLimits {
  count: number;
  flags: number;
  /** True when the memory declares a 64-bit (i64) index type. */
  isMemory64: boolean;
}

/** Result of a successful export-surface check. */
export interface ExportSurfaceReport {
  missingWasm: string[];
  missingGlue: string[];
}

/** Read one unsigned LEB128 integer starting at `cursor.p`, advancing it. */
function readLeb(bytes: Uint8Array, cursor: { p: number }): number {
  let result = 0;
  let shift = 0;
  let byte: number;
  do {
    byte = bytes[cursor.p++];
    if (byte === undefined) {
      throw new Error("truncated LEB128: ran off the end of the module");
    }
    result |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);
  return result >>> 0;
}

/** Read a length-prefixed UTF-8 name starting at `cursor.p`, advancing it. */
function readName(bytes: Uint8Array, cursor: { p: number }): string {
  const len = readLeb(bytes, cursor);
  const start = cursor.p;
  cursor.p += len;
  return new TextDecoder().decode(bytes.subarray(start, start + len));
}

/**
 * Parse the memory section (id 0x05) of a module and report its flags, so a
 * caller can assert the index type **without** instantiating.
 *
 * @throws when the module declares no memory section.
 */
export function parseMemoryLimits(bytes: Uint8Array): MemoryLimits {
  const cursor = { p: 8 }; // skip magic + version
  while (cursor.p < bytes.length) {
    const id = bytes[cursor.p++];
    const len = readLeb(bytes, cursor);
    const start = cursor.p;
    if (id === 0x05) {
      const count = readLeb(bytes, cursor);
      const flags = bytes[cursor.p];
      return { count, flags, isMemory64: (flags & MEMORY64_INDEX_FLAG) !== 0 };
    }
    cursor.p = start + len;
  }
  throw new Error("no memory section (0x05) found in module");
}

/**
 * Every name in the module's export section (id 0x07), in declaration order.
 * Returns an empty list when the module declares no exports.
 */
export function wasmExportNames(bytes: Uint8Array): string[] {
  const cursor = { p: 8 };
  while (cursor.p < bytes.length) {
    const id = bytes[cursor.p++];
    const len = readLeb(bytes, cursor);
    const start = cursor.p;
    if (id === 0x07) {
      const count = readLeb(bytes, cursor);
      const names: string[] = [];
      for (let i = 0; i < count; i++) {
        names.push(readName(bytes, cursor));
        cursor.p++; // export kind
        readLeb(bytes, cursor); // export index
      }
      return names;
    }
    cursor.p = start + len;
  }
  return [];
}

/**
 * Names bound by `export function` / `export class` declarations in generated
 * glue. `initSync` and `init` are wasm-bindgen's own bootstrap and are counted
 * like any other binding — the caller decides which names are load-bearing.
 */
export function glueExportNames(source: string): string[] {
  const names: string[] = [];
  const pattern =
    /^export\s+(?:async\s+)?(?:function|class)\s+([A-Za-z_$][\w$]*)/gm;
  for (const match of source.matchAll(pattern)) names.push(match[1]);
  return names;
}

/**
 * Assert the artefact declares a 64-bit linear memory.
 *
 * @param artefact path or file name, quoted back in the failure so a CI log
 *   names the file that is wrong.
 * @throws when the memory is 32-bit — the exact regression the Memory64 port
 *   exists to prevent, and one that is otherwise invisible (an i32 bundle
 *   validates, instantiates and scores correctly right up to 4 GiB).
 */
export function assertMemory64(
  bytes: Uint8Array,
  artefact: string,
): MemoryLimits {
  const limits = parseMemoryLimits(bytes);
  if (!limits.isMemory64) {
    throw new Error(
      `${artefact}: linear memory declares a 32-bit (i32) index type ` +
        `(flags 0x${limits.flags.toString(16).padStart(2, "0")}); ` +
        `a Memory64 bundle must set the i64 bit 0x${
          MEMORY64_INDEX_FLAG.toString(16)
        } ` +
        `and is capped at ${WASM32_MAX_PAGES} pages (4 GiB) without it`,
    );
  }
  return limits;
}

/**
 * Assert the built bundle exposes the activation/backprop surface on both
 * sides of the boundary: the compiled module's exports and the generated glue.
 *
 * Checking both is the point. A stripped glue with an intact `.wasm` is exactly
 * what wasm-bindgen 0.2.108 produced on wasm64 — and it is invisible to a
 * byte-size threshold, because the `.wasm` blob is full-sized.
 *
 * @throws listing every missing symbol, so one CI run reports the whole gap.
 */
export function assertBundleExportSurface(
  wasmBytes: Uint8Array,
  glueSource: string,
): ExportSurfaceReport {
  const wasmNames = new Set(wasmExportNames(wasmBytes));
  const glueNames = new Set(glueExportNames(glueSource));
  const missingWasm = REQUIRED_WASM_EXPORTS.filter((n) => !wasmNames.has(n));
  const missingGlue = REQUIRED_GLUE_EXPORTS.filter((n) => !glueNames.has(n));

  if (missingWasm.length > 0 || missingGlue.length > 0) {
    const parts: string[] = [];
    if (missingWasm.length > 0) {
      parts.push(`wasm module is missing exports: ${missingWasm.join(", ")}`);
    }
    if (missingGlue.length > 0) {
      parts.push(
        `generated glue is missing bindings: ${missingGlue.join(", ")}`,
      );
    }
    throw new Error(
      `wasm_activation bundle export surface incomplete — ${
        parts.join("; ")
      }. ` +
        `A CLI that exits 0 while stripping bindings is a silent failure, not a pass.`,
    );
  }
  return { missingWasm, missingGlue };
}
