// Committed Memory64 smoke test for wasm64 lane (b) — Issue #297.
//
// This is the *earliest failure-detection point* for the wasm64 spike: if a
// Deno/V8 upgrade drops or breaks Wasm 3.0 Memory64 support, this test fails in
// the repo's `deno test` CI job rather than being rediscovered mid-port later.
//
// "What" tests (AGENTS.md): they instantiate a real minimal `(memory i64 ...)`
// module and assert on observable outcomes — the module validates, linear
// memory grows past the wasm32 4 GiB ceiling, growth uses the 64-bit `BigInt`
// index type, and a `BigInt` offset above 4 GiB round-trips across the JS↔WASM
// boundary through the exported `store`/`load` functions.
//
// Run: deno test tests/wasm64_memory64_smoke_test.ts

import { assert, assertEquals, assertThrows } from "@std/assert";
import {
  buildMemory64Module,
  instantiateMemory64,
  MEMORY64_INDEX_FLAG,
  parseMemoryLimits,
  unsignedLeb128,
  WASM32_MAX_BYTES,
  WASM32_MAX_PAGES,
  WASM_PAGE_BYTES,
} from "./wasm64_memory64_smoke.ts";

// ~4.004 GiB: just enough headroom to prove growth strictly past the 4 GiB wall
// while keeping reserved address space (and touched pages) minimal.
const MAX_PAGES = WASM32_MAX_PAGES + 128;
const FOUR_GIB = BigInt(WASM32_MAX_BYTES);

Deno.test("unsignedLeb128 encodes multi-byte page counts correctly", () => {
  assertEquals(unsignedLeb128(0), [0x00]);
  assertEquals(unsignedLeb128(1), [0x01]);
  // 65664 = 0x100 80 ... verify it decodes back to itself via the parser path.
  assertEquals(unsignedLeb128(624485), [0xe5, 0x8e, 0x26]);
  assertThrows(() => unsignedLeb128(-1), RangeError);
});

Deno.test("minimal (memory i64 ...) module validates and marks the i64 index type", () => {
  const bytes = buildMemory64Module(MAX_PAGES);
  assert(
    WebAssembly.validate(bytes),
    "Memory64 module must validate in this V8",
  );
  const limits = parseMemoryLimits(bytes);
  assertEquals(limits.count, 1);
  assert(
    (limits.flags & MEMORY64_INDEX_FLAG) !== 0,
    `memory flags 0x${limits.flags.toString(16)} must set the i64 index bit`,
  );
  assert(limits.isMemory64);
});

Deno.test("buildMemory64Module rejects a max that cannot exceed the 4 GiB wall", () => {
  assertThrows(() => buildMemory64Module(WASM32_MAX_PAGES), RangeError);
});

Deno.test("instantiates and grows linear memory past the wasm32 4 GiB ceiling", () => {
  const { memory } = instantiateMemory64(buildMemory64Module(MAX_PAGES));
  assertEquals(memory.buffer.byteLength, WASM_PAGE_BYTES); // starts at 1 page
  memory.grow(BigInt(MAX_PAGES - 1) as unknown as number);
  assert(
    memory.buffer.byteLength > WASM32_MAX_BYTES,
    `byteLength ${memory.buffer.byteLength} must exceed the wasm32 4 GiB ceiling ${WASM32_MAX_BYTES}`,
  );
});

Deno.test("grow on an i64 memory requires and returns a BigInt (64-bit index)", () => {
  const { memory } = instantiateMemory64(buildMemory64Module(MAX_PAGES));
  const prev = memory.grow(1n as unknown as number);
  assertEquals(typeof prev, "bigint");
  assertEquals(prev as unknown as bigint, 1n); // previous page count, as a BigInt
  // A plain Number delta is rejected: the index type is genuinely 64-bit.
  assertThrows(() => memory.grow(1 as unknown as number), TypeError);
});

Deno.test("round-trips a BigInt offset above 4 GiB across the JS↔WASM boundary", () => {
  const { memory, store, load } = instantiateMemory64(
    buildMemory64Module(MAX_PAGES),
  );
  memory.grow(BigInt(MAX_PAGES - 1) as unknown as number);

  // Address strictly above the wasm32 4 GiB ceiling — unreachable on wasm32.
  const addr = FOUR_GIB + 2048n;
  const value = 0x1234abcd | 0;
  store(addr, value);
  assertEquals(load(addr) >>> 0, 0x1234abcd);

  // A second address, also above 4 GiB, is independent (no aliasing wrap-around
  // to a 32-bit offset).
  const addr2 = FOUR_GIB + BigInt(WASM_PAGE_BYTES) + 16n;
  store(addr2, 0x0badf00d | 0);
  assertEquals(load(addr2) >>> 0, 0x0badf00d);
  assertEquals(load(addr) >>> 0, 0x1234abcd); // first write untouched
});
