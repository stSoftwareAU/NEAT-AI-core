// Memory64 (Wasm 3.0) runtime probe for wasm64 lane (b) — Issue #297.
//
// Self-contained: hand-assembles a minimal `(memory i64 ...)` module that
// exports `store(i64 addr, i32 val)` / `load(i64 addr) -> i32`, so the smoke
// test needs NO checked-in `.wasm` binary and NO nightly build step. It probes
// the *runtime* half of the spike: does this Deno/V8 instantiate a Memory64
// module, grow linear memory past the wasm32 4 GiB ceiling, and round-trip a
// 64-bit (`BigInt`) pointer across the JS↔WASM boundary?
//
// Every value below is a plain byte constant — the module is small enough to
// audit by hand against the WebAssembly binary format.

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

/** Unsigned LEB128 encoding of a non-negative integer. */
export function unsignedLeb128(value: number): number[] {
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`unsignedLeb128 expects a non-negative integer, got ${value}`);
  }
  const out: number[] = [];
  let n = value;
  do {
    let byte = n & 0x7f;
    n = Math.floor(n / 128);
    if (n !== 0) byte |= 0x80;
    out.push(byte);
  } while (n !== 0);
  return out;
}

function section(id: number, body: number[]): number[] {
  return [id, ...unsignedLeb128(body.length), ...body];
}

function nameBytes(s: string): number[] {
  return [s.length, ...[...s].map((c) => c.charCodeAt(0))];
}

/**
 * Assemble a minimal Memory64 module with a store/load boundary.
 *
 * @param maxPages declared maximum pages for the memory. Must exceed
 *   {@link WASM32_MAX_PAGES} so the module can grow past 4 GiB.
 */
export function buildMemory64Module(maxPages: number): Uint8Array<ArrayBuffer> {
  if (maxPages <= WASM32_MAX_PAGES) {
    throw new RangeError(
      `maxPages (${maxPages}) must exceed the wasm32 ceiling (${WASM32_MAX_PAGES}) to prove >4 GiB growth`,
    );
  }
  // Type section: (i64,i32)->()  and  (i64)->(i32)
  const types = section(0x01, [
    0x02,
    0x60, 0x02, 0x7e, 0x7f, 0x00, // store: (i64 addr, i32 val) -> ()
    0x60, 0x01, 0x7e, 0x01, 0x7f, // load:  (i64 addr) -> i32
  ]);
  // Function section: func0 uses type0 (store), func1 uses type1 (load)
  const funcs = section(0x03, [0x02, 0x00, 0x01]);
  // Memory section: one memory, flags 0x05 = has-maximum(0x01) | is64(0x04),
  // minimum 1 page, maximum maxPages.
  const mem = section(0x05, [
    0x01,
    MEMORY64_INDEX_FLAG | 0x01,
    ...unsignedLeb128(1),
    ...unsignedLeb128(maxPages),
  ]);
  // Export section: memory, store, load
  const exportSec = section(0x07, [
    0x03,
    ...nameBytes("mem"), 0x02, 0x00,
    ...nameBytes("store"), 0x00, 0x00,
    ...nameBytes("load"), 0x00, 0x01,
  ]);
  // Code section. i32.store/i32.load take the address from the stack as i64
  // when the memory is 64-bit; memarg is (align=2, offset=0).
  const storeBody = [0x00, 0x20, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, 0x0b];
  const loadBody = [0x00, 0x20, 0x00, 0x28, 0x02, 0x00, 0x0b];
  const code = section(0x0a, [
    0x02,
    ...unsignedLeb128(storeBody.length), ...storeBody,
    ...unsignedLeb128(loadBody.length), ...loadBody,
  ]);
  const parts = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
    ...types,
    ...funcs,
    ...mem,
    ...exportSec,
    ...code,
  ];
  // Back the module with a concrete ArrayBuffer (not ArrayBufferLike) so it
  // satisfies the `BufferSource` parameter of WebAssembly.{validate,Module}.
  const out = new Uint8Array(new ArrayBuffer(parts.length));
  out.set(parts);
  return out;
}

/** Parsed limits of the first memory declared in a module. */
export interface MemoryLimits {
  count: number;
  flags: number;
  /** True when the memory declares a 64-bit (i64) index type. */
  isMemory64: boolean;
}

/**
 * Parse the memory section (id 0x05) of a module and report its flags. Used to
 * assert — without instantiating — that the module we built really carries the
 * i64 index type.
 */
export function parseMemoryLimits(bytes: Uint8Array<ArrayBuffer>): MemoryLimits {
  let p = 8; // skip magic + version
  const readLeb = (): number => {
    let result = 0, shift = 0, byte: number;
    do {
      byte = bytes[p++];
      result |= (byte & 0x7f) << shift;
      shift += 7;
    } while (byte & 0x80);
    return result >>> 0;
  };
  while (p < bytes.length) {
    const id = bytes[p++];
    const len = readLeb();
    const start = p;
    if (id === 0x05) {
      const count = readLeb();
      const flags = bytes[p];
      return { count, flags, isMemory64: (flags & MEMORY64_INDEX_FLAG) !== 0 };
    }
    p = start + len;
  }
  throw new Error("no memory section (0x05) found in module");
}

/** Handles bound to an instantiated Memory64 store/load module. */
export interface Memory64Instance {
  memory: WebAssembly.Memory;
  store: (addr: bigint, value: number) => void;
  load: (addr: bigint) => number;
}

/** Instantiate a Memory64 store/load module built by {@link buildMemory64Module}. */
export function instantiateMemory64(bytes: Uint8Array<ArrayBuffer>): Memory64Instance {
  const inst = new WebAssembly.Instance(new WebAssembly.Module(bytes));
  return {
    memory: inst.exports.mem as WebAssembly.Memory,
    store: inst.exports.store as (addr: bigint, value: number) => void,
    load: inst.exports.load as (addr: bigint) => number,
  };
}

/**
 * Grow an i64 memory to `targetPages`. Growth on a Memory64 uses a `BigInt`
 * delta (the 64-bit index type) and returns the previous page count as a
 * `BigInt`. Returns the resulting `byteLength`.
 *
 * Pages are reserved lazily by V8, so growing past 4 GiB reserves address space
 * without committing physical RAM — only pages actually written are committed.
 */
export function growToPages(memory: WebAssembly.Memory, targetPages: number): number {
  const currentPages = memory.buffer.byteLength / WASM_PAGE_BYTES;
  const delta = targetPages - currentPages;
  if (delta > 0) {
    // BigInt delta is mandatory for a 64-bit memory — a Number throws.
    memory.grow(BigInt(delta) as unknown as number);
  }
  return memory.buffer.byteLength;
}
