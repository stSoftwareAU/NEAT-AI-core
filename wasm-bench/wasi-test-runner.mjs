// Issue #509 — cargo test runner for `wasm32-wasip1`.
//
// The wasm32 kernels cannot be executed by the native test suite, so the parity
// tests are compiled to wasm and run on a real runtime instead:
//
//   CARGO_TARGET_WASM32_WASIP1_RUNNER="node wasm-bench/wasi-test-runner.mjs" \
//     cargo test -p neat-core --target wasm32-wasip1 --test simd_weighted_sums
//
// Same route PR #448 used. Exits with the test binary's own status, so a
// failure fails the command rather than reading as a clean run (Issue #3234).

import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";

const [modulePath, ...args] = process.argv.slice(2);
if (!modulePath) {
  console.error("usage: node wasi-test-runner.mjs <module.wasm> [args…]");
  process.exit(2);
}

const wasi = new WASI({
  version: "preview1",
  args: [modulePath, ...args],
  env: { RUST_BACKTRACE: "1" },
  returnOnExit: true,
});

const module = await WebAssembly.compile(await readFile(modulePath));
const imports = wasi.getImportObject();
// Stub anything the linker left behind that WASI does not provide (unreachable
// wasm-bindgen glue), but make an unexpected call fail loud rather than silent.
for (const { module: m, name } of WebAssembly.Module.imports(module)) {
  imports[m] ??= {};
  imports[m][name] ??= () => {
    throw new Error(`test binary called unexpected import ${m}.${name}`);
  };
}

const instance = await WebAssembly.instantiate(module, imports);
process.exit(wasi.start(instance) ?? 0);
