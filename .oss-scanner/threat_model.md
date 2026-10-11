# Threat model

The memory-safety argument for the SIMD hot path, and the invariant that holds
it, is in [SECURITY.md](../SECURITY.md#memory-safety-of-compiled-network-loading)
and [AGENTS.md](../AGENTS.md#unsafe--simd-invariants). This file is the short
brief for the scanner.

## What this project does and where untrusted input enters

NEAT-AI-core is the shared Rust computation library (`neat-core`) behind the
NEAT-AI neuro-evolution family: creature parsing and validation, compiled-network
activation, scoring, backpropagation and pruning. It ships as a native `rlib`
(linked by the scorer, back-propagation and discovery crates) and as a
`wasm-bindgen` bundle the Deno library NEAT-AI loads. Treat as untrusted:

- creature JSON (`parse_creature_json`, `creature_validate`, `prune_neuron`,
  `prune_synapse`): creatures are pulled from shared repositories pushed by
  other machines in the fleet;
- compiled-network `.bin` buffers (`CompiledNetwork::new`) and the byte-packed
  `propagate_topological` / `creature_validate_packed` requests;
- packed training corpora streamed by `training_bin_stream` and
  `training_data`;
- every argument JavaScript passes to an export in
  `neat-core/src/wasm_exports.rs` or a `cfg_attr(wasm_bindgen)` item: lengths,
  indices and squash ids are not trusted to agree with each other.

## Components that matter most / least

Most important: the `unsafe` SIMD kernels (`simd_native.rs`, `simd.rs`,
`simd/scalar.rs`) and every validation that makes their unchecked indexing
sound (`network.rs`, `creature_validate*.rs`, `propagate_codec.rs`); the packed
decoders; `batch_scoring.rs` and `loss.rs`.

Lower priority: `prune_fixtures.rs` (test fixtures), `benches/`,
`neat-core/examples/`, and `wasm-bench/` (a research-only harness outside the
workspace). `scripts/` and `tests/*.ts` are repository tooling.

## How to exercise it

From `/src`: `cargo test --workspace --all-features` runs the unit and
integration suites (`neat-core/tests/`, with golden and conformance fixtures
under `neat-core/tests/fixtures/`). The wasm32 bundle, when the optional build
step succeeded, is at `neat-core/wasm_activation/pkg/`. The wasm64 (Memory64)
lane needs nightly and is not built in this image.

## How you rate severity

- Critical: memory corruption or an out-of-bounds write reachable from creature
  JSON, a `.bin` buffer or a packed request.
- High: any out-of-bounds read or other undefined behaviour in `unsafe` code
  reachable through a safe public API or a WASM export, even without a
  demonstrated exploit; a validation bypass that lets a later
  `*_unchecked` kernel run on unvalidated indices.
- Medium: a panic, abort, unbounded allocation or hang on malformed input. In
  WASM a panic aborts the module and takes the host session with it, so this is
  denial of service, not a crash of one call.
- Low: wrong numeric results with no safety impact, and panics only reachable
  by a native caller breaking a documented `# Safety` contract.

## Anything to leave alone

- The `unsafe` `*_unchecked` kernels are unsound by contract when called with
  invalid indices; report a safe path that reaches them, not the contract.
- `unsafe { std::env::set_var(..) }` in `#[cfg(test)]` code is test-only.
- Unmaintained transitive crates under the `criterion` dev-dependency are a
  recorded exception (SECURITY.md, "Orphaned transitive crates").
