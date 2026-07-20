// learn_oome_repro.ts — wasm64 lane (a) gating diagnostic (Issue #296).
//
// Reproduces the two *distinct* out-of-memory ceilings a Deno learn job can hit
// and attributes a peak to the pool that actually binds:
//
//   1. V8 old-space (the JS heap)     — abort signature "Reached heap limit",
//      process exit 133. Raising `--v8-flags=--max-old-space-size=<N>` lifts it.
//   2. WASM linear memory on wasm32    — RangeError from `WebAssembly.Memory`
//      construction / `.grow()` at the hard 4 GiB (65536-page) address-space
//      cap. `--max-old-space-size` cannot lift this; only a wasm64 (Memory64)
//      build can.
//
// The harness is DATA-set free on purpose: both ceilings are architectural, so
// it drives them synthetically and captures the same crash signatures a real
// `Learn.ts` job emits. It is gated behind `LEARN_OOME_REPRO=1` so it never runs
// in default CI — only on an 8 GB+ host (see the finding in
// docs/research/wasm64-lane-a-4gb-ceiling-attribution.md).
//
// Run examples (Deno >= 2.9.3):
//   LEARN_OOME_REPRO=1 deno run --allow-env tests/perf/learn_oome_repro.ts wasm
//   LEARN_OOME_REPRO=1 deno run --allow-env \
//     --v8-flags=--max-old-space-size=256 tests/perf/learn_oome_repro.ts heap 2000
//
// On the crash path the harness prints a `[learn] FAIL:` marker so GRQ node.sh
// does not downgrade a marker-less non-zero exit to success (GRQ#2391).

/** Hard wasm32 linear-memory ceiling: 65536 pages x 64 KiB = exactly 4 GiB. */
export const WASM32_MAX_PAGES = 65536;
/** wasm32 linear-memory ceiling in bytes (4 GiB). */
export const WASM32_MAX_BYTES = WASM32_MAX_PAGES * 64 * 1024;

/** Which memory pool a crash signature or peak snapshot is attributed to. */
export type Pool =
  | "v8-heap"
  | "wasm-linear-memory"
  | "external-buffers"
  | "unknown";

/**
 * Classify a raw crash / abort string as a V8 JS-heap ceiling or a WASM
 * linear-memory ceiling. The two are unambiguous in the log text, which is the
 * whole point of lane (a): exit 133 + "Reached heap limit" is V8 old-space;
 * a `WebAssembly.Memory` RangeError / out-of-bounds trap is WASM memory.
 */
export function classifyCrashSignature(text: string): Pool {
  const t = text.toLowerCase();
  // WASM linear-memory signatures (checked first: they can also mention "memory").
  if (
    t.includes("webassembly.memory") ||
    t.includes("maximum memory size exceeded") ||
    t.includes("memory access out of bounds") ||
    t.includes("out of bounds memory access")
  ) {
    return "wasm-linear-memory";
  }
  // V8 JS-heap (old-space) abort signatures.
  if (
    t.includes("reached heap limit") ||
    t.includes("javascript heap out of memory") ||
    t.includes("allocation failed - javascript heap") ||
    (t.includes("fatal") && t.includes("heap"))
  ) {
    return "v8-heap";
  }
  return "unknown";
}

/** Dual-probe memory snapshot: BOTH probes required by the acceptance criteria. */
export interface ProbeSnapshot {
  /** Resident set size of the whole process (Deno.memoryUsage().rss). */
  rssBytes: number;
  /** V8 old-space live bytes (Deno.memoryUsage().heapUsed). */
  heapUsedBytes: number;
  /** V8 old-space reserved bytes (Deno.memoryUsage().heapTotal). */
  heapTotalBytes: number;
  /** Off-heap ArrayBuffer/TypedArray bytes tracked by V8 (external). */
  externalBytes: number;
  /** WASM linear-memory bytes (memory.buffer.byteLength), 0 if no module. */
  wasmBytes: number;
}

/**
 * Capture both probes at once. NEAT-AI#3410 shows MemoryMonitor misreads the
 * limit from a single probe, so the finding must cite `Deno.memoryUsage()` AND
 * `WebAssembly.Memory.buffer.byteLength` together.
 */
export function snapshotProbes(mem?: WebAssembly.Memory | null): ProbeSnapshot {
  const u = Deno.memoryUsage();
  return {
    rssBytes: u.rss,
    heapUsedBytes: u.heapUsed,
    heapTotalBytes: u.heapTotal,
    externalBytes: u.external,
    wasmBytes: mem ? mem.buffer.byteLength : 0,
  };
}

/**
 * Attribute a peak snapshot to the pool that dominates it. This is the core
 * decision lane (a) exists to make: a peak dominated by WASM linear memory sat
 * at ~4 GiB cannot be helped by `--max-old-space-size`; one dominated by V8
 * old-space can.
 */
export function attributePeak(p: ProbeSnapshot): Pool {
  const wasm = p.wasmBytes;
  const heap = p.heapUsedBytes;
  const external = p.externalBytes;
  const max = Math.max(wasm, heap, external);
  if (max === 0) return "unknown";
  if (max === wasm) return "wasm-linear-memory";
  if (max === heap) return "v8-heap";
  return "external-buffers";
}

/**
 * Grow a wasm32 `WebAssembly.Memory` towards `targetPages`, returning the peak
 * reached and any grow-failure signature. Demonstrates the hard 4 GiB cap:
 * asking for more than 65536 pages fails with a RangeError regardless of host
 * RAM.
 */
export function growWasmToCeiling(
  targetPages: number,
): {
  mem: WebAssembly.Memory;
  peakPages: number;
  peakBytes: number;
  signature: string;
} {
  // Cap the declared maximum at the wasm32 ceiling so the grow (not the
  // constructor) is what fails when target > ceiling — mirroring a real module
  // whose static `maximum` is the wasm32 limit.
  const maximum = Math.min(targetPages, WASM32_MAX_PAGES);
  const mem = new WebAssembly.Memory({ initial: 1, maximum });
  let pages = 1;
  let signature = "";
  const step = 1024; // 64 MiB per grow
  try {
    while (pages < targetPages) {
      const grow = Math.min(step, targetPages - pages);
      mem.grow(grow);
      pages += grow;
    }
  } catch (e) {
    signature = `${(e as Error).name}: ${(e as Error).message}`;
  }
  return { mem, peakPages: pages, peakBytes: mem.buffer.byteLength, signature };
}

/**
 * A V8 old-space OOM is a *native* abort (exit 133) — it cannot be caught
 * in-process, so the crashing child can never print its own `[learn] FAIL:`
 * marker. `markerForExit` is the launcher-side rule that closes that gap: given
 * a child's exit code and captured output, it returns the marker line the
 * launcher must emit so a marker-less exit 133 is not downgraded to success by
 * GRQ node.sh (GRQ#2391). Returns null when the child already emitted a marker
 * or exited cleanly.
 */
export function markerForExit(
  code: number,
  childOutput: string,
): string | null {
  if (childOutput.includes("[learn] FAIL:")) return null; // child already flagged
  if (code === 0) return null;
  if (
    code === 133 || childOutput.toLowerCase().includes("reached heap limit")
  ) {
    return "[learn] FAIL: v8-heap | child exited 133 (Reached heap limit) with no marker";
  }
  return `[learn] FAIL: unknown | child exited ${code} with no marker`;
}

function fmtMiB(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(0)}MiB`;
}

function reportPeak(label: string, p: ProbeSnapshot): void {
  console.log(
    `[learn] PEAK(${label}): rss=${fmtMiB(p.rssBytes)} ` +
      `heapUsed=${fmtMiB(p.heapUsedBytes)} heapTotal=${
        fmtMiB(p.heapTotalBytes)
      } ` +
      `external=${fmtMiB(p.externalBytes)} wasm=${fmtMiB(p.wasmBytes)} ` +
      `-> attributed=${attributePeak(p)}`,
  );
}

/** Fill V8 old-space with retained plain-JS objects until `targetMiB` is live. */
function runHeapMode(targetMiB: number): number {
  const retained: number[][] = [];
  let iter = 0;
  try {
    while (true) {
      const chunk = new Array<number>(128 * 1024); // ~1 MiB of boxed doubles
      for (let i = 0; i < chunk.length; i++) chunk[i] = i * 1.0001;
      retained.push(chunk);
      iter++;
      if ((iter & 63) === 0) {
        const snap = snapshotProbes(null);
        if (snap.heapUsedBytes / 1024 ** 2 >= targetMiB) {
          reportPeak("heap", snap);
          console.log(
            `[learn] OK: heap job reached ${targetMiB} MiB old-space`,
          );
          return 0;
        }
      }
    }
  } catch (e) {
    // A soft OOM surfaced as a catchable error (rare — V8 usually hard-aborts).
    console.error(
      `[learn] FAIL: ${classifyCrashSignature(String(e))} | ${
        (e as Error).message
      }`,
    );
    return 1;
  }
}

/** Grow wasm32 linear memory towards `targetGiB` and report the ceiling hit. */
function runWasmMode(targetGiB: number): number {
  const targetPages = Math.ceil((targetGiB * 1024 ** 3) / (64 * 1024));
  const { mem, peakPages, peakBytes, signature } = growWasmToCeiling(
    targetPages,
  );
  reportPeak("wasm", snapshotProbes(mem));
  if (signature) {
    console.error(
      `[learn] FAIL: ${classifyCrashSignature(signature)} | ` +
        `peakPages=${peakPages} peakBytes=${peakBytes} (${
          (peakBytes / 1024 ** 3).toFixed(3)
        } GiB) | ${signature}`,
    );
    return 1;
  }
  console.log(
    `[learn] OK: wasm reached ${peakPages} pages (${
      (peakBytes / 1024 ** 3).toFixed(3)
    } GiB)`,
  );
  return 0;
}

/**
 * Launcher demonstrator: run the `heap` child under a chosen old-space cap and
 * synthesise the `[learn] FAIL:` marker if it aborts with exit 133 but printed
 * no marker of its own. This is the pattern GRQ node.sh / the learn wrapper
 * (lane (d), #299) must adopt so a V8 heap OOM is never seen as success.
 */
async function runGuardMode(
  targetMiB: number,
  maxOldSpaceMiB: number,
): Promise<number> {
  const cmd = new Deno.Command(Deno.execPath(), {
    args: [
      "run",
      "--quiet",
      "--allow-env",
      `--v8-flags=--max-old-space-size=${maxOldSpaceMiB}`,
      new URL(import.meta.url).pathname,
      "heap",
      String(targetMiB),
    ],
    env: { LEARN_OOME_REPRO: "1" },
    stdout: "piped",
    stderr: "piped",
  });
  const out = await cmd.output();
  const text = new TextDecoder().decode(out.stdout) +
    new TextDecoder().decode(out.stderr);
  const marker = markerForExit(out.code, text);
  if (marker) {
    console.error(marker);
    return 1;
  }
  console.log(`[learn] OK: guarded heap child exited ${out.code}`);
  return out.code;
}

async function main(): Promise<number> {
  if (Deno.env.get("LEARN_OOME_REPRO") !== "1") {
    console.error(
      "[learn] SKIP: set LEARN_OOME_REPRO=1 to run (8 GB+ host only; not for default CI).",
    );
    return 0;
  }
  const mode = Deno.args[0] ?? "wasm";
  const arg = Number(Deno.args[1] ?? "");
  const arg2 = Number(Deno.args[2] ?? "");
  switch (mode) {
    case "heap":
      return runHeapMode(Number.isFinite(arg) && arg > 0 ? arg : 4096);
    case "wasm":
      return runWasmMode(Number.isFinite(arg) && arg > 0 ? arg : 6);
    case "guard":
      return await runGuardMode(
        Number.isFinite(arg) && arg > 0 ? arg : 2000,
        Number.isFinite(arg2) && arg2 > 0 ? arg2 : 256,
      );
    case "probe":
      reportPeak("probe", snapshotProbes(null));
      return 0;
    default:
      console.error(
        `[learn] FAIL: unknown mode '${mode}' (use heap|wasm|guard|probe)`,
      );
      return 2;
  }
}

if (import.meta.main) {
  Deno.exit(await main());
}
