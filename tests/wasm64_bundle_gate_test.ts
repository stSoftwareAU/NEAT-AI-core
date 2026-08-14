// Gate tests for the shipped wasm_activation bundle — Issue #541.
//
// The July 2026 spike recorded a *silent* failure mode: `wasm-bindgen` 0.2.108
// exited 0 on a wasm64 module while stripping every binding out of the
// generated glue, so the packaging step happily tarred an artefact with no
// working API. These tests pin the checks that turn that class of failure into
// a loud one, and they run against synthetic modules/glue so they need no
// nightly build.
//
// "What" tests (AGENTS.md): every case calls the real gate function with real
// bytes and asserts on the observable outcome — the returned report, or the
// thrown error.
//
// Run: deno test tests/wasm64_bundle_gate_test.ts

import { assert, assertEquals, assertThrows } from "jsr:@std/assert@1";
import {
  assertBundleExportSurface,
  assertMemory64,
  glueExportNames,
  MEMORY64_INDEX_FLAG,
  parseMemoryLimits,
  REQUIRED_GLUE_EXPORTS,
  REQUIRED_WASM_EXPORTS,
  wasmExportNames,
} from "../scripts/wasm64_bundle_gate.ts";
import { buildMemory64Module, WASM32_MAX_PAGES } from "./wasm64_memory64_smoke.ts";

/** Assemble a module carrying a 32-bit memory and the named function exports. */
function buildWasm32Module(exportNames: string[]): Uint8Array<ArrayBuffer> {
  const leb = (n: number): number[] => {
    const out: number[] = [];
    let v = n;
    do {
      let b = v & 0x7f;
      v = Math.floor(v / 128);
      if (v !== 0) b |= 0x80;
      out.push(b);
    } while (v !== 0);
    return out;
  };
  const section = (id: number, body: number[]) => [id, ...leb(body.length), ...body];
  const name = (s: string) => [s.length, ...[...s].map((c) => c.charCodeAt(0))];

  // One type: () -> (), one function using it, one 32-bit memory.
  const types = section(0x01, [0x01, 0x60, 0x00, 0x00]);
  const funcs = section(0x03, [0x01, 0x00]);
  const mem = section(0x05, [0x01, 0x00, ...leb(1)]); // flags 0x00 => i32 memory
  const exportBody: number[] = [...leb(exportNames.length + 1), ...name("memory"), 0x02, 0x00];
  for (const n of exportNames) exportBody.push(...name(n), 0x00, 0x00);
  const exportSec = section(0x07, exportBody);
  const body = [0x00, 0x0b];
  const code = section(0x0a, [0x01, ...leb(body.length), ...body]);

  const parts = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ...types,
    ...funcs,
    ...mem,
    ...exportSec,
    ...code,
  ];
  const out = new Uint8Array(new ArrayBuffer(parts.length));
  out.set(parts);
  return out;
}

/** Glue text that declares every symbol wasm-bindgen would emit for the bundle. */
function completeGlue(): string {
  const lines = REQUIRED_GLUE_EXPORTS.map((sym) =>
    sym[0] === sym[0].toUpperCase()
      ? `export class ${sym} {}`
      : `export function ${sym}() {}`
  );
  return ["export function initSync() {}", ...lines].join("\n");
}

Deno.test("parseMemoryLimits reports the i64 index bit for a Memory64 module", () => {
  const limits = parseMemoryLimits(buildMemory64Module(WASM32_MAX_PAGES + 128));
  assertEquals(limits.count, 1);
  assert(limits.isMemory64);
  assertEquals(limits.flags & MEMORY64_INDEX_FLAG, MEMORY64_INDEX_FLAG);
});

Deno.test("parseMemoryLimits reports a plain wasm32 memory as not Memory64", () => {
  const limits = parseMemoryLimits(buildWasm32Module([]));
  assertEquals(limits.count, 1);
  assertEquals(limits.isMemory64, false);
  assertEquals(limits.flags & MEMORY64_INDEX_FLAG, 0);
});

Deno.test("assertMemory64 rejects an i32-memory artefact by name", () => {
  const err = assertThrows(
    () => assertMemory64(buildWasm32Module([]), "wasm_activation_bg.wasm"),
    Error,
  );
  assert(
    err.message.includes("wasm_activation_bg.wasm"),
    `error must name the artefact: ${err.message}`,
  );
  assert(/i32|32-bit/.test(err.message), `error must say the memory is 32-bit: ${err.message}`);
});

Deno.test("assertMemory64 accepts a genuine (memory i64 ...) artefact", () => {
  const limits = assertMemory64(buildMemory64Module(WASM32_MAX_PAGES + 128), "fixture.wasm");
  assert(limits.isMemory64);
});

Deno.test("wasmExportNames lists every export declared by the module", () => {
  const names = wasmExportNames(buildWasm32Module(["propagate_topological", "__wbindgen_malloc"]));
  assertEquals(names.includes("memory"), true);
  assertEquals(names.includes("propagate_topological"), true);
  assertEquals(names.includes("__wbindgen_malloc"), true);
  assertEquals(names.includes("compilednetwork_activate"), false);
});

Deno.test("glueExportNames picks up both function and class bindings", () => {
  const names = glueExportNames(
    "export function propagate_topological(d) {}\nexport class CompiledNetwork {}\n",
  );
  assertEquals(names.sort(), ["CompiledNetwork", "propagate_topological"]);
});

Deno.test("glueExportNames sees only the bootstrap in the glue wasm-bindgen 0.2.108 emitted", () => {
  // The recorded silent failure: the CLI exits 0 and writes initSync/init only,
  // so the bootstrap is present and not one binding of the real API is.
  const names = glueExportNames("export function initSync(module) {}\nexport default init;\n");
  assertEquals(names, ["initSync"]);
  for (const sym of REQUIRED_GLUE_EXPORTS) assertEquals(names.includes(sym), false);
});

Deno.test("assertBundleExportSurface passes a bundle carrying the full surface", () => {
  const report = assertBundleExportSurface(buildWasm32Module(REQUIRED_WASM_EXPORTS), completeGlue());
  assertEquals(report.missingWasm, []);
  assertEquals(report.missingGlue, []);
});

Deno.test("assertBundleExportSurface fails loud when the glue is a silent stub", () => {
  const err = assertThrows(
    () =>
      assertBundleExportSurface(
        buildWasm32Module(REQUIRED_WASM_EXPORTS),
        "export function initSync(module) {}\n",
      ),
    Error,
  );
  assert(/glue/i.test(err.message), `error must blame the glue: ${err.message}`);
  for (const sym of REQUIRED_GLUE_EXPORTS) {
    assert(err.message.includes(sym), `error must list the missing binding ${sym}`);
  }
});

Deno.test("assertBundleExportSurface fails loud when the wasm drops a required export", () => {
  const kept = REQUIRED_WASM_EXPORTS.filter((n) => n !== "__wbindgen_malloc");
  const err = assertThrows(
    () => assertBundleExportSurface(buildWasm32Module(kept), completeGlue()),
    Error,
  );
  assert(err.message.includes("__wbindgen_malloc"), `error must name the export: ${err.message}`);
});

Deno.test("the required surface covers the activation and backprop entry points", () => {
  // Guards the list itself: dropping the network or propagate entry points
  // would leave the gate green on a bundle NEAT-AI cannot use.
  for (const sym of ["compilednetwork_new", "compilednetwork_activate", "propagate_topological"]) {
    assert(REQUIRED_WASM_EXPORTS.includes(sym), `${sym} must be gated`);
  }
  assert(REQUIRED_GLUE_EXPORTS.includes("CompiledNetwork"));
});
