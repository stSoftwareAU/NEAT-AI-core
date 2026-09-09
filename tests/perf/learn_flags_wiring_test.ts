// learn_flags_wiring_test.ts — "what" tests for the wasm64 lane (d)
// downstream-trainer learn-invocation wiring model (Issue #299).
//
// These pin the RAM-aware `--v8-flags=--max-old-space-size` selection the
// downstream trainer's learn launcher injects, verified from neat-core. The
// expected MB values are the SAME numbers the trainer's own CI asserts for its
// memory-sizing formula (e.g. 8 GB → 4326, 16 GB → 9651, 4 GB → 1664): if the
// production sizing formula drifts, these lock-step values catch it here too.
//
// Run: deno test tests/perf/learn_flags_wiring_test.ts

import { assert, assertEquals, assertGreater } from "@std/assert";
import {
  EIGHT_GB_HEAP_FLOOR_MB,
  FFI_OS_HEADROOM_MB,
  GLOBAL_HEAP_FLOOR_MB,
  heapFitsHostBudget,
  heapFloorMb,
  type HostMemory,
  learnFailMarkerForExit,
  learnV8HeapFlag,
  OOM_EXIT_CODE,
  selectMaxOldSpaceSizeMb,
  SIXTEEN_GB_HEAP_FLOOR_MB,
} from "./learn_flags_wiring.ts";

const GB = 1024;
const host = (totalGb: number, availableMb?: number): HostMemory => ({
  totalMb: totalGb * GB,
  availableMb,
});

// --- RAM-aware heap selection, lock-step with the production memory-sizing tests --

Deno.test("selectMaxOldSpaceSizeMb: 4 GB host gets 1664 MB (above the 1536 floor)", () => {
  // (4096 - 1536) * 65 / 100 = 1664
  assertEquals(selectMaxOldSpaceSizeMb(host(4)), 1664);
});

Deno.test("selectMaxOldSpaceSizeMb: 8 GB host gets 4326 MB (the production OOME tier)", () => {
  // (8192 - 1536) * 65 / 100 = 4326
  assertEquals(selectMaxOldSpaceSizeMb(host(8)), 4326);
});

Deno.test("selectMaxOldSpaceSizeMb: 16 GB host gets 9651 MB", () => {
  // (16384 - 1536) * 65 / 100 = 9651
  assertEquals(selectMaxOldSpaceSizeMb(host(16)), 9651);
});

Deno.test("selectMaxOldSpaceSizeMb: 64 GB host is capped at 24576 MB", () => {
  // (65536 - 1536) * 65 / 100 = 41600 → capped at the 24 GB ceiling
  assertEquals(selectMaxOldSpaceSizeMb(host(64)), 24576);
});

Deno.test("selectMaxOldSpaceSizeMb: 2 GB host is floored at 1536 MB", () => {
  // (2048 - 1536) * 65 / 100 = 332 → floored at the global 1536 MB floor
  assertEquals(selectMaxOldSpaceSizeMb(host(2)), GLOBAL_HEAP_FLOOR_MB);
});

Deno.test("selectMaxOldSpaceSizeMb: constrained 8 GB host sizes off AVAILABLE, not total (#3342)", () => {
  // Constrained host: 8 GB total but only 3865 MB available. Heap =
  // (3865 - 1536) * 65 / 100 = 1513; the available-aware floor steps down to
  // min(3072, 3865 * 45 / 100 = 1739) = 1739, so the selection is 1739 — far
  // below the fixed-3072 over-commit that fatal-OOM'd such a host, and below the
  // 4326 a roomy 8 GB gets.
  assertEquals(selectMaxOldSpaceSizeMb(host(8, 3865)), 1739);
  assertGreater(
    selectMaxOldSpaceSizeMb(host(8)),
    selectMaxOldSpaceSizeMb(host(8, 3865)),
  );
});

// --- Falls back safely below the 8 GB tier ------------------------------------

Deno.test("heapFloorMb: below-8 GB hosts keep the 1536 MB global floor, not the 8 GB tier floor", () => {
  assertEquals(heapFloorMb(host(4)), GLOBAL_HEAP_FLOOR_MB);
  assertEquals(heapFloorMb(host(8)), EIGHT_GB_HEAP_FLOOR_MB);
  assertEquals(heapFloorMb(host(16)), SIXTEEN_GB_HEAP_FLOOR_MB);
  // A 4 GB host must not inherit the 8 GB tier's higher floor.
  assertGreater(heapFloorMb(host(8)), heapFloorMb(host(4)));
});

// --- RAM-aware budget guard: no new OOME on smaller hosts ---------------------

Deno.test("heapFitsHostBudget: 4 / 8 / 16 GB hosts all stay within their total RAM", () => {
  for (const gb of [4, 8, 16, 32, 64]) {
    assert(
      heapFitsHostBudget(host(gb)),
      `heap + FFI/OS headroom must fit in ${gb} GB total RAM`,
    );
  }
});

Deno.test("heapFitsHostBudget: heap + headroom never exceeds total on the 8 GB tier", () => {
  const h = host(8);
  assert(selectMaxOldSpaceSizeMb(h) + FFI_OS_HEADROOM_MB <= h.totalMb);
});

Deno.test("selectMaxOldSpaceSizeMb: heap grows monotonically with host RAM (no smaller host over-sized)", () => {
  const h4 = selectMaxOldSpaceSizeMb(host(4));
  const h8 = selectMaxOldSpaceSizeMb(host(8));
  const h16 = selectMaxOldSpaceSizeMb(host(16));
  assertGreater(h8, h4, "8 GB must not be sized below 4 GB");
  assertGreater(h16, h8, "16 GB must not be sized below 8 GB");
});

// --- The exact flag token learn.sh injects ------------------------------------

Deno.test("learnV8HeapFlag: composes the --v8-flags=--max-old-space-size token", () => {
  assertEquals(
    learnV8HeapFlag(host(8)),
    "--v8-flags=--max-old-space-size=4326",
  );
});

// --- Silent-failure guard (Issue #3234) ---------------------------------------

Deno.test("learnFailMarkerForExit: a marker-less exit-133 abort gets a [learn] FAIL: marker", () => {
  assertEquals(
    learnFailMarkerForExit(OOM_EXIT_CODE),
    "[learn] FAIL: learn exited 133",
  );
});

Deno.test("learnFailMarkerForExit: a clean exit and an already-marked exit stay silent", () => {
  assertEquals(learnFailMarkerForExit(0), null);
  assertEquals(learnFailMarkerForExit(OOM_EXIT_CODE, true), null);
});
