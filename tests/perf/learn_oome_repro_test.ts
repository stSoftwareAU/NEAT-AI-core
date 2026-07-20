// Tests for the wasm64 lane (a) OOME reproducer (Issue #296).
// "What" tests: they call the real classifier / probe / attribution functions
// and assert on observable outcomes — not on how those functions are wired.
//
// Run: deno test --allow-env tests/perf/learn_oome_repro_test.ts

import { assertEquals } from "jsr:@std/assert@1";
import {
  attributePeak,
  classifyCrashSignature,
  growWasmToCeiling,
  markerForExit,
  type ProbeSnapshot,
  snapshotProbes,
  WASM32_MAX_BYTES,
  WASM32_MAX_PAGES,
} from "./learn_oome_repro.ts";

Deno.test("classifyCrashSignature maps the exit-133 V8 abort to v8-heap", () => {
  assertEquals(
    classifyCrashSignature(
      "Fatal JavaScript out of memory: Reached heap limit",
    ),
    "v8-heap",
  );
  assertEquals(
    classifyCrashSignature("FATAL ERROR: ... JavaScript heap out of memory"),
    "v8-heap",
  );
});

Deno.test("classifyCrashSignature maps the wasm grow failure to wasm-linear-memory", () => {
  assertEquals(
    classifyCrashSignature(
      "RangeError: WebAssembly.Memory.grow(): Maximum memory size exceeded",
    ),
    "wasm-linear-memory",
  );
  assertEquals(
    classifyCrashSignature("RuntimeError: memory access out of bounds"),
    "wasm-linear-memory",
  );
});

Deno.test("classifyCrashSignature returns unknown for unrelated errors", () => {
  assertEquals(
    classifyCrashSignature("TypeError: x is not a function"),
    "unknown",
  );
});

Deno.test("attributePeak picks the dominant pool", () => {
  const wasmBound: ProbeSnapshot = {
    rssBytes: 4_300_000_000,
    heapUsedBytes: 120_000_000,
    heapTotalBytes: 160_000_000,
    externalBytes: 5_000_000,
    wasmBytes: WASM32_MAX_BYTES,
  };
  assertEquals(attributePeak(wasmBound), "wasm-linear-memory");

  const heapBound: ProbeSnapshot = {
    rssBytes: 4_300_000_000,
    heapUsedBytes: 4_100_000_000,
    heapTotalBytes: 4_200_000_000,
    externalBytes: 5_000_000,
    wasmBytes: 0,
  };
  assertEquals(attributePeak(heapBound), "v8-heap");

  const externalBound: ProbeSnapshot = {
    rssBytes: 4_300_000_000,
    heapUsedBytes: 50_000_000,
    heapTotalBytes: 80_000_000,
    externalBytes: 4_000_000_000,
    wasmBytes: 0,
  };
  assertEquals(attributePeak(externalBound), "external-buffers");
});

Deno.test("attributePeak returns unknown when every pool is empty", () => {
  const empty: ProbeSnapshot = {
    rssBytes: 0,
    heapUsedBytes: 0,
    heapTotalBytes: 0,
    externalBytes: 0,
    wasmBytes: 0,
  };
  assertEquals(attributePeak(empty), "unknown");
});

Deno.test("snapshotProbes reports both probes together", () => {
  const mem = new WebAssembly.Memory({ initial: 2, maximum: 4 });
  const snap = snapshotProbes(mem);
  // WASM probe reflects the live buffer (2 pages x 64 KiB).
  assertEquals(snap.wasmBytes, 2 * 64 * 1024);
  // V8 probe is present and positive on any running process.
  assertEquals(snap.rssBytes > 0, true);
  assertEquals(snap.heapTotalBytes > 0, true);
  // With no module the WASM probe is zero — never silently omitted.
  assertEquals(snapshotProbes(null).wasmBytes, 0);
});

Deno.test("wasm32 constant is exactly 4 GiB", () => {
  assertEquals(WASM32_MAX_PAGES, 65536);
  assertEquals(WASM32_MAX_BYTES, 4 * 1024 ** 3);
});

Deno.test("growWasmToCeiling hits the hard 4 GiB cap when asked for more", () => {
  // Ask for 4 GiB + 1 page. wasm32 cannot represent it: the grow must fail with
  // a RangeError classified as a WASM linear-memory ceiling.
  const r = growWasmToCeiling(WASM32_MAX_PAGES + 1);
  assertEquals(r.signature.includes("RangeError"), true);
  assertEquals(classifyCrashSignature(r.signature), "wasm-linear-memory");
  assertEquals(r.peakBytes <= WASM32_MAX_BYTES, true);
});

Deno.test("markerForExit synthesises a FAIL marker for a marker-less exit 133", () => {
  // The GRQ#2391 gap: V8 aborts natively, so the child prints no marker.
  assertEquals(
    markerForExit(133, "some gc log\n#\n# Fatal JavaScript out of memory\n"),
    "[learn] FAIL: v8-heap | child exited 133 (Reached heap limit) with no marker",
  );
});

Deno.test("markerForExit stays silent when the child already flagged failure", () => {
  assertEquals(
    markerForExit(1, "[learn] FAIL: wasm-linear-memory | ..."),
    null,
  );
});

Deno.test("markerForExit stays silent on a clean exit", () => {
  assertEquals(markerForExit(0, "[learn] OK: done"), null);
});

Deno.test("markerForExit flags any other marker-less non-zero exit", () => {
  assertEquals(
    markerForExit(2, "boom"),
    "[learn] FAIL: unknown | child exited 2 with no marker",
  );
});

Deno.test("growWasmToCeiling reaches a small target cleanly", () => {
  const r = growWasmToCeiling(8); // 512 KiB
  assertEquals(r.signature, "");
  assertEquals(r.peakPages, 8);
  assertEquals(r.peakBytes, 8 * 64 * 1024);
});
