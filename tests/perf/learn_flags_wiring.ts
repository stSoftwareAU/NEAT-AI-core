// learn_flags_wiring.ts — wasm64 lane (d) GRQ learn-invocation wiring model
// (Issue #299).
//
// Lane (a) (#296) attributed the GRQ#3508 learn OOME to the **V8 old-space heap**
// (exit 133 / "Reached heap limit"), liftable by
// `--v8-flags=--max-old-space-size=<N>`. Lane (d) is the GRQ-side wiring plus
// end-to-end verification: GRQ's learn invocation must select that flag
// **RAM-aware**, so a bigger heap on a roomy host never pushes a smaller host
// past its limit, and a V8 old-space abort is never silently reported as
// success.
//
// The PRODUCTION selection lives in GRQ (stSoftwareAU/GRQ):
//   * `worker/shared/memory_calc.sh` — `get_max_heap_size` / `get_heap_floor_mb`
//     size the heap from currently-available RAM after a fixed FFI/OS headroom.
//   * `worker/learn.sh` — injects `--v8-flags=--max-old-space-size=${MAX_HEAP_SIZE}`
//     into the `deno run src/Learn.ts` argv (the BEGIN_LEARN_DENO_ARGV_2950 block).
//   * `worker/shared/stage_fail_marker.sh` — the EXIT-trap emits a
//     `[learn] FAIL:` marker on any non-zero exit (incl. 133), so node.sh cannot
//     downgrade a marker-less OOM abort to success (GRQ#2391 / Issue #3234).
// It is CI-guarded in GRQ by `test/worker/MemoryCalcHeapSize.ts` and
// `test/worker/MemoryCalcHostFloor.ts`.
//
// This module is the **neat-core-side acceptance model** of that selection: a
// pure re-derivation used to verify the milestone's headroom invariants from
// this repo end-to-end (the "neat-core adoption verified end-to-end" tracked by
// #299). It must stay in **lock-step** with `memory_calc.sh`; the constants
// below mirror it exactly. A divergence between this model's numbers and GRQ's
// is itself the regression signal. See
// `docs/research/wasm64-lane-d-grq-learn-wiring-verification.md`.

/** Fixed headroom (MB) reserved for Rust FFI + OS caches before apportioning to
 * V8 — `MEMORY_FFI_OS_HEADROOM_MB` in memory_calc.sh (#1768). */
export const FFI_OS_HEADROOM_MB = 1536;
/** Share of the post-headroom budget granted to V8 (`get_max_heap_size` default). */
export const HEAP_PERCENTAGE = 65;
/** Heap ceiling (MB): large hosts keep RAM for OS/non-heap (`GRQ_MAX_HEAP_CAP_MB`). */
export const HEAP_CAP_MB = 24576;
/** Global heap floor (MB) for hosts smaller than the 8 GB tier. */
export const GLOBAL_HEAP_FLOOR_MB = 1536;
/** 8 GB sloth-class working-set floor (MB), ample-available case (#2084/#3342). */
export const EIGHT_GB_HEAP_FLOOR_MB = 3072;
/** >=16 GB working-set floor (MB), ample-available case (#2243/#3294). */
export const SIXTEEN_GB_HEAP_FLOOR_MB = 4096;
/** Minority share of *available* RAM the step-down floor may claim (`get_heap_floor_avail_pct`). */
export const HEAP_FLOOR_AVAIL_PCT = 45;
/** V8 fatal heap-limit abort code (SIGTRAP, 128 + 5) — GRQ#3508's signature. */
export const OOM_EXIT_CODE = 133;

/** A host's memory picture, in MB. */
export interface HostMemory {
  /** Total physical RAM in MB. */
  totalMb: number;
  /** Currently-available RAM in MB; defaults to `totalMb` (an idle host). */
  availableMb?: number;
}

/**
 * Mirror of `memory_calc.sh get_heap_floor_mb`. Hosts below 8 GB keep the global
 * 1536 MB floor. The 8 GB tier uses a 3072 MB working-set floor that steps
 * **down** on a constrained host (available-aware, #3342, always on for the
 * learn/teams victim): capped at a minority share (45%) of *available* RAM but
 * never below the 1536 MB global floor. >=16 GB hosts use a 4096 MB floor (the
 * available-aware step-down there is opt-in via `GRQ_HEAP_AVAILABLE_AWARE_FLOOR`
 * and off for the learn path, so the fixed floor is the learn-path value).
 */
export function heapFloorMb(host: HostMemory): number {
  const totalGb = Math.floor(host.totalMb / 1024);
  const availableMb = host.availableMb ?? host.totalMb;
  if (totalGb >= 16) return SIXTEEN_GB_HEAP_FLOOR_MB;
  if (totalGb === 8) {
    let floor = EIGHT_GB_HEAP_FLOOR_MB;
    const availCap = Math.floor((availableMb * HEAP_FLOOR_AVAIL_PCT) / 100);
    if (availCap < floor) floor = availCap;
    if (floor < GLOBAL_HEAP_FLOOR_MB) floor = GLOBAL_HEAP_FLOOR_MB;
    return floor;
  }
  return GLOBAL_HEAP_FLOOR_MB;
}

/**
 * Mirror of `memory_calc.sh get_max_heap_size`: the `--max-old-space-size` value
 * (MB) GRQ's learn invocation selects for a host.
 *
 *   heap = clamp((available - FFI_OS_HEADROOM) * 65%, floor(total) .. 24576)
 */
export function selectMaxOldSpaceSizeMb(host: HostMemory): number {
  const availableMb = host.availableMb ?? host.totalMb;
  const budget = Math.max(availableMb - FFI_OS_HEADROOM_MB, 0);
  let heap = Math.floor((budget * HEAP_PERCENTAGE) / 100);
  if (heap > HEAP_CAP_MB) heap = HEAP_CAP_MB;
  const floor = heapFloorMb(host);
  if (heap < floor) heap = floor;
  return heap;
}

/** The exact `--v8-flags` token `worker/learn.sh` injects for the learn stage. */
export function learnV8HeapFlag(host: HostMemory): string {
  return `--v8-flags=--max-old-space-size=${selectMaxOldSpaceSizeMb(host)}`;
}

/**
 * RAM-aware guard: the selected heap plus the FFI/OS headroom must fit within
 * the host's **total** RAM, so a bigger heap on a roomy host never pushes a
 * smaller host past its limit (acceptance: "no regression / new OOME on smaller
 * hosts"). Returns true iff the invocation stays within the host's budget.
 */
export function heapFitsHostBudget(host: HostMemory): boolean {
  return selectMaxOldSpaceSizeMb(host) + FFI_OS_HEADROOM_MB <= host.totalMb;
}

/**
 * Silent-failure guard (GRQ#2391 / Issue #3234). A V8 old-space OOM is a native
 * abort (exit 133): the crashed child cannot print its own marker, and node.sh
 * downgrades a marker-less non-zero exit to success. `learn.sh`'s EXIT trap must
 * therefore emit a `[learn] FAIL:` marker. Returns the marker line for a
 * marker-less non-zero exit, or `null` for a clean exit or one already marked.
 */
export function learnFailMarkerForExit(
  exitCode: number,
  alreadyMarked = false,
): string | null {
  if (exitCode === 0 || alreadyMarked) return null;
  return `[learn] FAIL: learn exited ${exitCode}`;
}
