# NEAT-AI-core

<p align="center">
  <img width="720" src="https://raw.githubusercontent.com/stSoftwareAU/NEAT-AI/Develop/docs/brand/social-previews/neat-ai-core.png" alt="NEAT-AI-core — the shared native Rust core of NEAT-AI">
</p>

**Native shared Rust** for [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI) — an implementation of [**NEAT** (NeuroEvolution of Augmenting Topologies)](https://en.wikipedia.org/wiki/Neuroevolution_of_augmenting_topologies): the **`neat-core`** crate (tests included) lives here as a Cargo workspace member.

## Glossary

First-use definitions for the project's core terms, an acronym, and its
internal automation name. Standard terms link out; the project's own
vocabulary carries a plain-English gloss.

- <a id="glossary-neat"></a>**NEAT** — [NeuroEvolution of Augmenting Topologies](https://en.wikipedia.org/wiki/Neuroevolution_of_augmenting_topologies), the algorithm that evolves both the weights *and* the topology of a neural network. NEAT-AI is a project built on this idea; this repo is its shared native core.
- <a id="glossary-creature"></a>**creature** — the project's term for a single evolved individual: a [genome](https://en.wikipedia.org/wiki/Artificial_neural_network) compiled to a runnable network. Scoring pushes many records through one creature.
- <a id="glossary-squash"></a>**squash** — a neuron's [activation function](https://en.wikipedia.org/wiki/Activation_function) (the non-linearity applied to its weighted input sum). "Standard-squash" neurons use the project's default activation.
- <a id="glossary-vibe-coder"></a>**Vibe Coder** — the automated agent that raises the routine dependency-bump and quality PRs (it runs `bump-deps.sh` on every such PR; the CI `quality` job then applies the same gates `quality.sh` runs locally).

## Test-driven development

Development in this repository follows **TDD**: do not merge behaviour changes unless **`cargo test --workspace`** already covers them (extend tests first when fixing bugs or adding APIs). Run **`./quality.sh`** before every commit/PR.

## WebAssembly

**`wasm_activation`** and **`pkg/`** remain in the **NEAT-AI** repo on `Develop` — not in this repository.

### wasm64 (Memory64) bundle — dual-ship, Issue #541

Every push to `Develop` publishes **two** bundles on the per-commit
`wasm-bundle-<sha>` Release:

| Asset | Target | Role |
|-------|--------|------|
| `wasm_activation-wasm64-pkg.tar.gz` | `wasm64-unknown-unknown` | Genuine Memory64 `(memory i64 …)` build — **the pin new `neatCore.rev` revisions take**. Linear memory grows past the wasm32 65536-page (4 GiB) ceiling. |
| `wasm_activation-pkg.tar.gz` | `wasm32-unknown-unknown` | Unchanged wasm32 build, retained as the rollback window until NEAT-AI's Memory64 loader lands. |

Each carries its own `.sha256` sidecar and a CycloneDX SBOM resolved against
its own target triple, and both are covered by the build-provenance attestation.

The July 2026 spike recorded the production `wasm-bindgen` path as a wasm64
**NO-GO**. That was **CLI skew, not a permanent gap**: CLI `0.2.108` exited 0
while stripping the bindings, and CLI ≥ `0.2.120` ships Memory64 codegen
([wasm-bindgen#5004](https://github.com/wasm-bindgen/wasm-bindgen/pull/5004)).
Re-measured **2026-08-14** against CLI `0.2.127` — the production path emits the
full activation/backprop surface. See
[`docs/research/wasm64-lane-b-build-lane-feasibility.md`](docs/research/wasm64-lane-b-build-lane-feasibility.md).

This does **not** fix V8 exit-133 JS-heap aborts: that ceiling is the JS heap,
not WASM linear memory, and `--max-old-space-size` remains its lever (lane (a),
Issue #296).

```mermaid
flowchart TD
    S["neat-core sources<br/>cfg(target_family = &quot;wasm&quot;)"] --> A["wasm-pack<br/>wasm32"]
    S --> B["cargo +nightly -Z build-std<br/>wasm64 (Tier 3)"]
    B --> C["wasm-bindgen CLI<br/>pinned = Cargo.lock"]
    A --> G1["check_wasm64_bundle.ts<br/>memory type + export surface"]
    C --> G1
    G1 --> G2["check_wasm_arch_parity.ts<br/>bit-identical f32/f64"]
    G2 --> R["Release wasm-bundle-&lt;sha&gt;<br/>wasm64 = pin · wasm32 = rollback"]
```

Building locally:

```bash
# wasm32 (needs wasm-pack)
./scripts/build-wasm-bundle.sh --arch wasm32 --rev "$(git rev-parse HEAD)"

# wasm64 (needs nightly + rust-src + the wasm-bindgen CLI matching Cargo.lock)
rustup toolchain install nightly --profile minimal --component rust-src
./scripts/build-wasm-bundle.sh --arch wasm64 --rev "$(git rev-parse HEAD)" \
  --out wasm_activation-wasm64-pkg.tar.gz
```

Both invocations gate the built `pkg/` before packaging, so a stripped glue or
a wrong-arch memory type fails the build instead of shipping.

## Training-data offload (WASM linear memory) — removed, Issue #415

wasm64 milestone #295, lane (c) — Issue #298 — shipped a `wasm_dataset` module
that held the large numeric training arrays in neat-core's own linear memory,
handing JS a `u32` handle instead of the bytes. The `Learn.ts` adoption that
would have consumed it was never done: the milestone and its upstream adoption
issue both closed with the seven `training_data_*` exports unbound, so the module
was **removed as dead code in Issue #415**. Nothing in NEAT-AI or
NEAT-AI-scorer referenced it.

The performance work it carried lives on independently: the flat batched scoring
path (Issue #386, [below](#flat-record-input-issue-386)) is the contract any
future offload lane would drive, and lane (a)'s finding stands — the ~4 GB Learn
ceiling is the **V8 JS heap** (exit-133 / "Reached heap limit"), not WASM linear
memory. Re-adopting the offload means re-landing the module against a live
consumer, not resurrecting an unbound export surface.

Lane (d) — Issue #299 — verifies the downstream adoption end-to-end: the
production trainer's launch script injects a **RAM-aware**
`--v8-flags=--max-old-space-size` (sized by its memory-budget helper, floored
safely below the 8 GB tier) and fails **loud** on a V8 heap abort (exit 133).
The neat-core acceptance model
[`tests/perf/learn_flags_wiring.ts`](tests/perf/learn_flags_wiring.ts) re-derives
that selection lock-step with the production selector and pins the budget-fit /
safe-fall-back invariants; see
[`docs/research/wasm64-lane-d-learn-wiring-verification.md`](docs/research/wasm64-lane-d-learn-wiring-verification.md).

## Layout

| Path | Role |
|------|------|
| `neat-core/` | Shared computation library; **140+** unit tests in `src/**/*.rs` plus integration tests in `neat-core/tests/` (>350 total). |
| `Cargo.toml` | Virtual workspace root; `[workspace.package]` holds semver for release automation. |
| `RELEASING.md` | Single source of truth for the versioning/release policy (Issue #251) — semver, what counts as breaking, and `v<version>` tags/releases on `Develop`. |
| `deny.toml` | `cargo deny` (licences, advisories, bans). |
| `neat-core/benches/` | Opt-in Criterion harnesses: `hot_paths` (core hot paths) and `parallel_scoring` (data-parallel scoring, needs `--features parallel`); see `neat-core/benches/README.md`. |
| `quality.sh` | Local gate (fmt, clippy, tests, doc, deny, bats). |
| `.github/workflows/ci.yml` | CI gate. The `rust-gates` job runs the lint (`cargo clippy -D warnings`) and compile/syntax (`cargo check --all-targets`) gates on **every push to `Develop`** (and `workflow_dispatch`); on pull requests the `quality` job — the full PR pipeline — runs the same clippy gate, so `rust-gates` is skipped there rather than compiling the workspace twice (Issue #337). |
| `bump-deps.sh` | Cargo dep refresh + audit + native/WASM build ([Vibe Coder](#glossary-vibe-coder) hook). |
| `.github/dependabot.yml` | Weekly Cargo **version-updates** channel (7-day `cooldown`, 10-PR limit) — see [Dependency updates](#dependency-updates-two-channels). |
| `tests/scripts/` | `bats` suites for shell helpers (e.g. `bump-deps.sh`) and for the CI workflow contracts. Shared assertions live in `tests/scripts/helpers.bash` (loaded with `load helpers`, unit-tested by `helpers_shared.bats`) — put a new assertion there rather than copying it between suites (Issue #477). |
| `LICENSE`, `.gitleaks.toml` | Inherited from NEAT-AI `Develop`. |

## Build

```bash
export RUSTFLAGS="-D warnings"
cargo test --workspace
# or full gate:
./quality.sh
# opt-in performance benchmarks (not part of the test gate):
cargo bench -p neat-core --bench hot_paths
```

The committed TypeScript helpers under `tests/` carry their own basic-validity
gate (Issue #307): `./scripts/typescript-check.sh` type-checks every `.ts` file
with `deno check`. It runs inside `./quality.sh` and as the CI `typescript-gate`
job on every push and pull request, so a syntax or type error fails the build.
Basic validity only — it is not a style or lint gate, and it requires
[Deno](https://docs.deno.com/runtime/getting_started/installation/).

### Build profiles (Issue #546)

Fleet decision (`stSoftwareAU/VibeCoding#4159`): **dev builds compile as fast as
possible; release builds produce the most optimised artefact possible and
compile time is irrelevant.** Stable Rust only — no nightly, no `-Zthreads`, no
Cranelift. The root `Cargo.toml` carries both profiles workspace-wide:

| Profile | Settings | Why |
|---------|----------|-----|
| `[profile.dev]` | `debug = "line-tables-only"` (plus cargo's defaults: `opt-level = 0`, incremental on) | Keeps panic and backtrace `file:line`, drops the rest of the DWARF. Measured here: rebuild after a one-line edit **1.87s → 1.50s**, clean `cargo build --workspace --all-targets` **8.37s → 6.72s**, `target/debug` **1.4G → 1.1G**. |
| `[profile.release]` | `opt-level = 3`, `lto = "fat"`, `codegen-units = 1` | One codegen unit per crate with fat LTO across the whole graph — the most optimised artefact, whatever it costs to compile. |

**Cargo profiles come from the crate being built.** `neat-core` is a library, so
its `[profile.*]` tables govern only this workspace's own tests, benches and
`cargo build --release` — they never reach a consumer. Every binary crate
downstream (the scorer, the Lamarck and backpropagation consumers, the private
trainer) must carry the same release settings in its own manifest, or it links
`neat-core` unoptimised no matter what is written here.

```mermaid
flowchart LR
    P["root Cargo.toml<br/>[profile.dev] + [profile.release]"] --> W["this workspace's builds<br/>tests, benches, release"]
    P -. "never inherited" .-> C["consumer binary crate"]
    C --> O["consumer's own [profile.release]<br/>+ its own target-cpu choice"]
```

**`-C target-cpu=native` is consumer-owned and deliberately absent here.**
Recommended for a binary built and run on the same host (the fleet pattern:
`cargo build --release` on the machine that runs the artefact); never for a
published crate and never for the `wasm32`/`wasm64` bundles, which must stay
portable. Stable Cargo has no per-profile rustflags, so a consumer sets it in
its own `.cargo/config.toml` under a target-scoped key —
`[target.'cfg(not(target_arch = "wasm32"))'] rustflags = ["-C", "target-cpu=native"]`
— or in the build invocation's `RUSTFLAGS`; note that an exported `RUSTFLAGS`
**replaces** config rustflags entirely rather than adding to them.

`tests/scripts/rust_build_profiles.bats` is the gate: it asserts the manifest's
values and then splices the live `[profile.*]` tables into a throwaway crate to
check the flags **cargo itself** passes rustc (`-C debuginfo=line-tables-only`;
`-C opt-level=3 -C lto=fat -C codegen-units=1`), and fails if any build input
under `.cargo/`, `scripts/` or `.github/workflows/` pins `target-cpu=native`.

## Cargo features

| Feature | Default | Effect |
|---------|---------|--------|
| `parallel` | off | Native-only data-parallel record scoring via `rayon` (Issue #179). Adds `CompiledNetwork::score_records_parallel_flat`, which chunks records across the rayon pool. Off by default, so the default build and the `wasm32` build pull in **no** `rayon` symbols and keep their single-thread path. |
| `checked-gather4` | off | Restores bounds-checked indexing in the `wasm32` `gather4` scaffold helper (Issue #509). By default that helper elides the checks under the load-time `InvalidSynapseIndex` invariant — worth ~20% on the isolated kernel and ~2% on production-sized end-to-end inference, bit-identical either way. This is the A/B control and the one-flag way back out; no effect on native builds. See [`docs/research/wasm-gather4-unchecked-loads.md`](docs/research/wasm-gather4-unchecked-loads.md). |

Scoring a production-size dataset pushes many records through one [creature](#glossary-creature) — an
embarrassingly parallel workload *across records*. Both `score_records_flat`
and `score_records_parallel_flat` drive the forward pass through the **8-record
batched SIMD path** (Issue #230): records are grouped into 8s (then a 4-record group,
then a scalar tail) and forwarded through `weighted_sum_simd_8records` /
`weighted_sum_simd_4records`, loading each synapse weight once and applying it
across the lanes. On the gather-bound production topology this cut single-core
scoring time by **~39%** (see `docs/archive/pr-summaries/pr-summary-230.md`).
With the `parallel` feature the throughput additionally scales with core count.

On the exact committed production topology the native lane beats the wasm32 lane
**1.78×** per core (NEON + FMA vs `simd128` + relaxed-madd) and **4.75×** at 12
cores versus a single-threaded wasm32 creature — so where the native `rust_scorer`
is built, production per-creature scoring should use
`score_records_parallel_flat`.
See the native-vs-wasm32 decision, numbers, and the generation-end idle-core
"win zone" in [`neat-core/benches/BASELINE.md`](neat-core/benches/BASELINE.md)
(Issue #288).

Standard-[squash](#glossary-squash) neurons now sum **across records** rather than across synapses,
so their `f32` results match the per-record `activate` reference within a small
tolerance (SIMD re-association), while the sequential and parallel paths agree
bit-for-bit. Output order always matches input order.

```rust
// Off-feature / wasm: transparently runs sequentially (still batched SIMD).
let outputs = net.score_records_flat(&inputs, stride, num_outputs);

// With `--features parallel` on native: batches scored across the rayon pool,
// same results, in input order.
let outputs = net.score_records_parallel_flat(&inputs, stride, num_outputs);
```

### Flat record input (Issue #386)

Both the input and the output of a scoring batch have a flat, contiguous
contract. Record `i`'s inputs are `inputs[i * stride .. i * stride + stride]`
and its outputs are `out[i * num_outputs .. (i + 1) * num_outputs]`. Callers
that already hold a contiguous buffer — anything reading a packed `.bin` shard,
for instance — pass it straight through,
skipping the one-heap-allocation-per-record the `&[Vec<f32>]` signature forces
(4,096 allocations for a ~40 MiB production shard). A `stride` narrower than the
network's input arity zero-fills the uncovered inputs, exactly as a short `Vec`
does.

```rust
// One contiguous buffer of records * stride values — no per-record Vec.
let outputs = net.score_records_flat(&inputs, stride, num_outputs);

// Same, into a caller-owned buffer, or across the rayon pool.
net.score_records_flat_into(&inputs, stride, num_outputs, &mut out);
let outputs = net.score_records_parallel_flat(&inputs, stride, num_outputs);
```

The flat layout is the **only** one supported. The per-record `&[Vec<f32>]`
wrappers (`score_records`, `score_records_parallel`), deprecated in `0.2.28` by
Issue #408, were **removed** in `0.3.0` (Issue #409) — a breaking change; pack
your records into one contiguous buffer and call the `_flat` entry points.
Scoring is asserted against an independent per-record reference (each record
scored on its own through `activate`) across the 8-record group boundary and
both dispatch arms by
[`tests/flat_record_scoring_parity.rs`](neat-core/tests/flat_record_scoring_parity.rs).
A malformed batch (zero `stride`, or a buffer that is not a whole number of
records) **panics** rather than silently mis-slicing every record.

```mermaid
flowchart LR
    R[records] --> S{parallel feature?}
    S -- off / wasm32 --> Q[score_records_flat<br/>sequential, batched SIMD]
    S -- on, native --> P[score_records_parallel_flat<br/>rayon chunks]
    P --> W1[worker 1<br/>own lane scratch]
    P --> Wn[worker N<br/>own lane scratch]
    subgraph B[batched forward per chunk]
        direction TB
        E8[8-record SIMD] --> E4[4-record SIMD] --> E1[scalar tail]
    end
    Q --> B
    W1 --> B
    Wn --> B
    B --> O[outputs in input order]
```

### Fused-MSE record tile (Issue #530)

`mse_sum_batch_packed` — the fused activate + MSE entry point production
scoring calls — walks records in **record-interleaved tiles** of
`loss::MSE_TILE_LANES` (`32`), not the 8 the scoring lane uses. The interleaved
kernel re-streams the network's whole synapse array once per *tile*, so a wider
tile divides per-record synapse traffic by `MSE_TILE_LANES / 8`; the cost is a
proportionally larger `mse_inter` scratch buffer,
`num_neurons * MSE_TILE_LANES * 4` bytes **per compiled network** (~528 KB on
the production creature at 32 lanes, ~132 KB at 8). Directory scoring holds one
compiled network per worker, so budget that against the worker-count RAM ceiling
before raising it.

`MSE_TILE_LANES` is a single constant, must be a non-zero multiple of 8 and at
most `simd::MAX_INTERLEAVED_LANES` (`64`) — both checked at compile time — and
every width is **bit-identical**: each lane accumulates its own
`bias + Σ w·a` in synapse order and every tier reduces in record order.
The `< MSE_TILE_LANES` remainder steps down the unchanged ladder.

```mermaid
flowchart LR
    R["packed records"] --> T{"records left"}
    T -- "&ge; MSE_TILE_LANES" --> A["R-record interleaved tile<br/>one synapse sweep per R records"]
    T -- "8..R-1" --> B["8-record interleaved tile"]
    T -- "4..7" --> C["4-record scattered group"]
    T -- "&lt; 4" --> D["scalar tail — exact activate"]
    A --> T
    B --> T
    C --> S["running f64 sum_error<br/>in record order"]
    D --> S
    A --> S
    B --> S
```

### Streaming directory MSE (Issue #538)

`mse_mean_streaming` scores a whole `.bin` training directory without loading
it into memory, so consumers stop re-implementing the chunk → packed-buffer →
`mse_sum_batch_packed` loop around this crate's MSE maths. It returns
`(mean_mse, record_count)` for
`(1/N) * Σ_records mse_record(targets, outputs)` — the same semantics as
`mse_mean_record` and NEAT-AI's `Costs.MSE` — and `(0.0, 0)` when the directory
yields no whole records, leaving the fail-loud decision to the caller. A
malformed corpus is **not** silent: a missing directory, an unreadable file, or
a trailing partial record returns `Err`.

Records are buffered into packed chunks so the fused SIMD tile above still
does the work; a record straddling a read-chunk or file boundary is carried in
a residual buffer and scored with the next chunk. `max_records` truncates both
the file list and the final chunk, so a capped scan never opens a file it
cannot use.

`mse_record` is the per-record reduction those paths share — the mean over a
record's outputs of `(target - output)^2`, accumulated in `f64`. Both scalar
MSE closures in `loss.rs` call it, and it is exported for consumers (such as a
backpropagation trace pass) that already hold activations and cannot use a
fused batch path. The SIMD tile kernels keep their own bit-parity-critical
reductions. Neither helper is on the `wasm_bindgen` export surface — they are
native-host conveniences.

```rust
use neat_core::mse_mean_streaming;

// (mean per-record MSE, records scored); `None` = the whole corpus.
let (mse, records) = mse_mean_streaming(
    &mut network,
    std::path::Path::new("./training"),
    input_size,
    num_outputs,
    /* forward_only */ true,
    /* max_records */ None,
)?;
```

Chunk boundaries decide which records land in an 8-way SIMD group, so the
result matches `mse_sum_batch_packed(all_records) / N` to floating-point
tolerance rather than bit-for-bit.

```mermaid
flowchart LR
    D["training .bin directory"] --> F["find_bin_files<br/>numeric order"]
    F --> C["for_each_read_chunk_with_mode"]
    C --> P["pending residual<br/>+ whole records → packed f32"]
    P --> M["mse_sum_batch_packed<br/>per chunk — fused SIMD"]
    M --> A["Σ per-record MSE / N"]
    P -. "cap reached" .-> T["truncate final chunk"]
    T --> A
```

### Struct-of-arrays hot synapse view (Issue #533)

The interleaved gather reads only two of `SynapseData`'s three fields, so
`CompiledNetwork` also carries a parallel **struct-of-arrays** view of them —
`hot_weights: Vec<f32>` and `hot_from: Vec<u16>`, in the same order as
`synapses`. The interleaved kernels take those two slices, streaming 6 B per
synapse instead of the struct's 8 B; `synapse_type` stays on `SynapseData` for
the aggregate/IF and single-record paths, which are unchanged. Values and
accumulation order are identical, so every result stays bit-identical.

Both views are built by `hot_synapse_soa` at every construction path
(`CompiledNetwork::new`, `compile_creature`), and cost **+6 B per synapse per
compiled network** — ~126 KB on the production creature, cloned once per
directory-scoring worker. Because the fields are public they can be made to
drift; `debug_assert_hot_soa` runs at every interleaved entry point and panics
in debug builds if they have.

```mermaid
flowchart LR
    C["compile_creature / CompiledNetwork::new"] --> S["synapses: Vec&lt;SynapseData&gt;<br/>weight + from_index + synapse_type"]
    S --> H["hot_synapse_soa"]
    H --> W["hot_weights: Vec&lt;f32&gt;"]
    H --> F["hot_from: Vec&lt;u16&gt;"]
    W --> G["weighted_sum_interleaved::&lt;R&gt;<br/>6 B per synapse"]
    F --> G
    S --> A["aggregate / IF / single-record paths<br/>unchanged, 8 B per synapse"]
```

```bash
# Run the data-parallel scoring throughput bench (1 vs all cores).
cargo bench -p neat-core --features parallel --bench parallel_scoring

# A/B the fused-MSE tile width (edit MSE_TILE_LANES, rebuild, compare medians).
cargo bench -p neat-core --bench hot_paths -- mse_sum_production
```

### Creature JSON observation-width contract (Issue #550)

`CreatureExport` (`neat-core/src/creature.rs`) is the Rust mirror of the
TypeScript `CreatureExport` interface: top-level `input` / `output` integers,
then `neurons` and `synapses`. The two counts are the **width contract** for
the whole fleet — `input` is the observation count and `output` the target
count — and `neurons` deliberately lists only *non-input* neurons, so **`input`
cannot be re-derived from the neuron list**: lose it and the width is gone.

`input < 1` or `output < 1` is therefore never accepted anywhere in this crate.
`validate_creature_width` is the single home of the rule and every entry point
calls it:

| Entry point | `input: 0` / `output: 0` | missing key | `-1` |
|-------------|--------------------------|-------------|------|
| `parse_creature_json` | `CreatureError::InvalidInputCount { found }` / `InvalidOutputCount { found }` | `CreatureError::Json` (no `#[serde(default)]`) | `CreatureError::Json` |
| `compile_creature` | same typed error, checked **before** any other validation | — | — |
| `creature_to_json` / `creature_to_json_pretty` | same typed error — a widthless creature is never *written* | — | — |

The Display text (`Must have at least one input neurons was: 0`) mirrors
NEAT-AI `src/architecture/CreatureValidate.ts` so logs line up across the TS
and Rust stacks. A valid creature round-trips `input` / `output` byte-identically
through parse → serialise (`neat-core/tests/creature_width_contract.rs`).

Consumers (NEAT-AI-scorer, -Backpropagation, -Lamarck, -Discovery and the
downstream trainers) read
the top-level counts and never re-derive them from `neurons`; a hand-built
`CreatureExport` should go through `validate_creature_width` at the boundary.

```mermaid
flowchart LR
    J["creature JSON"] --> P["parse_creature_json<br/>serde → validate_creature_width"]
    E["CreatureExport (hand-built)"] --> C["compile_creature<br/>validate_creature_width first"]
    P --> C
    E --> S["creature_to_json / _pretty<br/>validate_creature_width first"]
    P --> S
    P -. "input &lt; 1 / output &lt; 1" .-> X["Err(InvalidInputCount / InvalidOutputCount)"]
    C -. "input &lt; 1 / output &lt; 1" .-> X
    S -. "input &lt; 1 / output &lt; 1" .-> X
```

### Duplicate `(fromUUID, toUUID)` synapses are rejected (Issue #556)

NEAT-AI's TypeScript loader keys synapses by the `(fromUUID, toUUID)` pair, so
a creature carrying that pair twice loses every copy but one before it is
scored. `compile_creature` used to resolve each synapse independently and
**sum** them, so the same JSON scored differently under the two engines —
observed in production as `rust_scorer` 0.356183 against `Creature.scoreDir`
0.353147, with the minimal repro an `IF` neuron fed three times (condition /
positive / negative) by one constant neuron.

Which copy TypeScript keeps falls out of its map insertion order, so there is
no value this crate could reproduce and no safe way to dedupe.
`validate_no_duplicate_synapses` is the single home of the rule and
`compile_creature` calls it right after `validate_creature_width`: a repeated
pair is `CreatureError::DuplicateSynapse { from_uuid, to_uuid }`, naming the
first pair that repeats in declaration order. Distinct pairs that share one
endpoint — fan-out from a source, fan-in to a target — are untouched.

Consumers that assemble a `CreatureExport` in Rust rather than parsing one
should call `validate_no_duplicate_synapses` at their own boundary
(NEAT-AI-Forests already guards this itself).

```mermaid
flowchart LR
    J["creature JSON<br/>same (from, to) twice"] --> T["NEAT-AI TypeScript<br/>keyed by (from, to)"]
    J --> R["compile_creature"]
    T --> K["keeps one copy<br/>insertion-order dependent"]
    R --> V["validate_no_duplicate_synapses"]
    V -. "repeated pair" .-> X["Err(DuplicateSynapse)<br/>fail closed"]
    V --> C["CompiledNetwork<br/>every pair distinct"]
    K -. "divergent score" .-> X
```

### Canonical IF decision trees and the graft helper (Issue #555)

`neat-core` owns the fleet's one interpretation of a decision tree built from
the `IF` aggregate. **NEAT-AI-Forests and every other consumer read it from
here** rather than inventing their own reading of the synapse roles.

An `IF` neuron sums its `Condition` inputs. When that sum is **strictly**
greater than zero it emits the sum of its `Positive` inputs, otherwise the sum
of its `Negative` inputs; the neuron's bias is added either way. So a split test
`x > t` is a condition sum of `x * 1.0 + 1.0 * (-t)`, with a constant neuron
supplying the `1.0`. A creature may not carry two synapses between the same
ordered pair of neurons, so one node cannot take all three roles from a single
constant — each grafted node brings its own trio of `1.0` constants, leaving the
threshold and the leaf values in the trainable **weights**.

**Fixtures** — `neat-core/src/decision_tree.rs`, with documented expected
outputs beside each builder:

| Builder | Shape | Covers |
|---------|-------|--------|
| `stump_creature()` | `x > 0.5 ? 3.0 : 0.0` | single split, zero/default branch (`STUMP_CASES`) |
| `depth2_tree_creature()` | root on `x0 > 0.5`, both children on `x1 > 0.25` | nested depth-2, all four leaves (`DEPTH2_CASES`) |
| `linear_base_creature()` | `2x`, no `IF` at all | the pre-graft base |
| `residual_correction_creature()` | `2x + (x > 0.75 ? 1.5 : 0)` | non-zero residual/correction leaf (`RESIDUAL_CASES`) |

**Graft helper** — `neat-core/src/if_graft.rs`. A caller describes the node
(`IfNodeSpec`, or `IfCorrectionSpec` for the common depth-1 correction) and
`graft_if_node` / `graft_if_tree` / `graft_if_correction` return a **new**
validated `CreatureExport`; the source is never mutated. Placement is chosen so
the node is evaluated after every source and before every target, which is what
preserves the `forwardOnly` reading order the compiled forward pass relies on.
Every rejection is a typed `GraftError` and **no creature is produced** —
unknown or duplicate UUID, a missing `IF` role, no outward edge, an edge to an
input or a constant, a self edge, a duplicate edge, a non-finite weight or bias,
or no position that keeps every edge pointing forwards. `graft_if_correction`
on `linear_base_creature()` reproduces `residual_correction_creature()` exactly,
which is how the helper and the fixture keep each other honest.

`validate_creature_topology` is the shared gate both ends run: it reuses
`validate_creature_width`, `validate_topology` and
`validate_structural_integrity` rather than restating their rules. The ordering
gate only runs for `forwardOnly` creatures, because a recurrent creature
legitimately carries backward edges.

```mermaid
flowchart LR
    B["base CreatureExport"] --> V["validate_creature_topology"]
    S["IfNodeSpec / IfCorrectionSpec"] --> K{"names new?<br/>all three roles?<br/>edges resolve?"}
    V --> K
    K -- no --> E["Err(GraftError) — no creature"]
    K -- yes --> P{"position after every source,<br/>before every target?"}
    P -- none exists --> E
    P -- yes --> G["build creature"]
    G --> V2["validate_creature_topology"]
    V2 -- fails --> E
    V2 -- passes --> O["Ok(CreatureExport)"]
```

### Creature validation contract (Issue #559)

`neat-core/src/creature_validate.rs` is the Rust home of NEAT-AI's
`src/architecture/CreatureValidate.ts` — the fleet's one definition of a valid
creature. NEAT-AI-Forests shipped an invalid creature that `Creature.validate()`
would have caught; the fix is a single definition Rust consumers call natively
and NEAT-AI calls over the existing WASM boundary.

**This issue lands the contract only** — the types, the error model, the rule
order and the input format — so the two rule ports can be worked in parallel
against a fixed interface.

```rust
pub fn creature_validate(creature: &CreatureExport, options: &ValidateOptions)
    -> Result<ValidationStats, ValidationFailure>;
```

| Piece | What it fixes |
|-------|---------------|
| `ValidateOptions` | `forward_only` forces `feedback_loop` to `Some(false)`; otherwise only an explicit `Some(false)` rejects `from > to`. `neurons: Some(0)` is **skipped** (TypeScript truthiness) while `connections: Some(0)` **is** checked (`Number.isInteger`) |
| `ValidationStats` | the `stats` object `creatureValidate` returns: `input`, `constant`, `hidden`, `output`, `connections` |
| `ValidationFailure` | `class` (`TopologyError` / `ValidationError`), the verbatim `reason`, the human-readable `message`, and the `neuron_index` / `synapse_index` it stopped on |
| `VALIDATION_REASONS` / `TOPOLOGY_REASONS` | the `ValidationErrorName` and `TopologyErrorReason` unions verbatim, so NEAT-AI rehydrates the right error type with no translation table |

The reason lists are the single home of the permitted `reason` values and are
checked at **compile time**: each list is internally distinct and the two are
disjoint, so a name copied into the wrong union fails the build. The failure
constructors reject a reason outside their class rather than sending NEAT-AI a
string it cannot rehydrate. Rule evaluation order is part of the contract — the
numbered table in the module documentation is what both ports implement, first
failure wins.

**Input format.** A creature arrives as the `CreatureExport` this crate already
parses, extended by two optional fields — `NeuronExport::id` (signed: output
neurons carry negative ids, NEAT-AI #1958) and `CreatureExport::memetic`
(`biases`, `weights`, with every other key preserved verbatim). Both default to
absent and are skipped on output, so existing `parse_creature_json` callers and
already-written creature files round trip byte-identically. The export form is
index-free, so indices are derived exactly as `compile_creature` derives them:
`0..input` are the implicit input neurons (`input-N`, `id == index`), and
`input + i` is `neurons[i]`.

**What stays host-side** (NEAT-AI#3802): `neuron.creature !== creature`, the
`neuron.index` vs loop-position check, `neuron.validate()` and the `debugWrite` diagnostics
dump all depend on JavaScript object identity or the host filesystem. The
failure's neuron/synapse index is what lets the host run those against the same
neuron the shared rules stopped on.

Both halves of the table are ported, so `creature_validate` evaluates every
rule and returns `ValidationStats` for a creature that breaks none of them.
Exposing it over the WASM boundary and replaying the TypeScript conformance
corpus is Issue #562.

### Neuron rules ported (Issue #560)

Rules 1–22 — everything `CreatureValidate.ts` evaluates before its synapse walk
— now run in Rust, in the TypeScript's own order, with the message text
reproduced verbatim so NEAT-AI's error-message tests keep passing across the
boundary. Rules 23–31 (synapses, `forwardOnly`, memetic) are Issue #561 and run
straight after them.

Two derivations make the port work on the index-free, id-optional export form:

| Derivation | Rule |
|------------|------|
| Indices — `0..input` are the implicit input neurons, `input + i` is `neurons[i]` (as `compile_creature` derives them) | the walk position every rule reads |
| Ids — input `= index`, output `= -(outputIndex + 1)`, everything else its exported `id` or a deterministic hash of its UUID | rules 4–7, mirroring NEAT-AI's loader, which assigns ids *before* `creatureValidate` runs |

Without the id derivation every modern export — NEAT-AI writes UUIDs, not ids —
would fail rule 4. Because inputs and outputs take derived ids, rules 7, 10 and
21 cannot fail for a `CreatureExport`, and rule 19 is unreachable in both stacks
behind rule 8; all four are ported and covered anyway so the two files stay
line-for-line comparable.

**One implementation per invariant.** `creature_validate` and
`validate_structural_integrity` ask the same wiring questions but answer them
differently — a TypeScript message here, a numeric code and neuron index there —
so neither can call the other. What they share now lives in
`neat-core/src/topology_invariants.rs`: `ConnectionIndex` (inward/outward degree
and the inward synapse list, built once in `O(neurons + synapses)` so no caller
rescans the synapse list per neuron), `hidden_wiring_fault` and
`if_neuron_fault`. Mutating any of the three fails tests on **both** sides.

```mermaid
flowchart TD
    CE["CreatureExport"] --> D["derive indices + ids"]
    D --> W["neuron walk<br/>rules 1-22"]
    D --> CI["ConnectionIndex"]
    CI --> W
    CI --> VSI["validate_structural_integrity"]
    SI["topology_invariants<br/>hidden_wiring_fault / if_neuron_fault"] --> W
    SI --> VSI
    W -->|"first violated rule"| F["Err(ValidationFailure)<br/>TypeScript message"]
    VSI -->|"first violated rule"| C["[code, neuron index]"]
    W -->|"rules 1-22 pass"| N["synapse walk<br/>rules 23-31 (#561)"]
```

```mermaid
flowchart LR
    C["CreatureExport<br/>+ id, + memetic"] --> V["creature_validate"]
    O["ValidateOptions<br/>forward_only → feedback_loop = false"] --> V
    V -->|"no rule broken"| S["Ok(ValidationStats)"]
    V -->|"first violated rule"| F["Err(ValidationFailure)<br/>class + reason + message + index"]
    F --> T["NEAT-AI rehydrates<br/>TopologyError / ValidationError"]
    F --> H["host-only checks<br/>identity, neuron.validate(), debugWrite"]
```

#### Synapse, forward-only and memetic rules (Issue #561)

Rules 23–31 of that table run behind `creature_validate` and are callable on
their own:

```rust
pub fn validate_synapse_and_memetic_rules(
    creature: &CreatureExport,
    options: &ValidateOptions,
    stats: &mut ValidationStats,
) -> Result<(), ValidationFailure>;
```

`stats` is the same object the neuron walk fills — this half adds the
connection tally and leaves the neuron counters alone, exactly as the
TypeScript threads one `stats` literal through both halves. It evaluates, first
failure wins: the single synapse pass (no synapse into an input neuron; no self
connection under `forward_only`; sorted by `(from, to)`; no duplicate pair; no
`from > to` when `feedback_loop` is an explicit `Some(false)`), then the
`connections` count, then — for a forward-only creature — `topology_ops`'
`validate_topology`, `validate_structural_integrity` and `detect_cycles`, and
finally the memetic cross-references.

Two details a caller can trip over:

- **`feedback_loop` is a tri-state.** `None` and `Some(true)` both allow a
  recursive synapse; only `Some(false)` rejects one, and `forward_only: true`
  forces that `Some(false)` whatever the caller asked for.
- **Memetic entries match on neuron id, not index.** The synapse set is built
  from `neurons[s.from].id -> neurons[s.to].id` using the same derived ids as
  the neuron half, so a creature whose ids differ from its positions still
  resolves.

```mermaid
flowchart LR
    W["synapse walk<br/>rules 23–27"] --> N["connections count<br/>rule 28"]
    N --> FO{"forward_only?"}
    FO -- yes --> T["topology_ops<br/>validate_topology →<br/>validate_structural_integrity →<br/>detect_cycles"]
    FO -- no --> M["memetic rules<br/>rule 31"]
    T --> M
    M --> S["Ok(()) — stats.connections tallied"]
```

## Related Repositories

The NEAT-AI project is split across seven public repositories. Each focuses on one concern and composes with the others as shown below.

`neat-core` keeps the **per-sample** training primitives (`propagate_topological_loop`, the packed propagate ABI, `mse_mean_streaming`, the topology helpers); the `trainDir` **epoch** loop and its journal, CLI apply policy and `traceStore` layout stay in NEAT-AI-Backpropagation. The rule and the gate that enforces it live in [`AGENTS.md`](AGENTS.md#ownership-fence-issue-544) (Issue #544).

| Repository | Role |
|------------|------|
| [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI) | Primary Deno/TypeScript neural-network engine (evolution, training, WASM activation). |
| [NEAT-AI-core](https://github.com/stSoftwareAU/NEAT-AI-core) | Shared native Rust library (`neat-core`) with numerics, topology helpers, and the chunked `.bin` training stream. |
| [NEAT-AI-Discovery](https://github.com/stSoftwareAU/NEAT-AI-Discovery) | Rust discovery module invoked by NEAT-AI via Deno FFI to search architectures and hyper-parameters. |
| [NEAT-AI-Snapshot](https://github.com/stSoftwareAU/NEAT-AI-Snapshot) | Creature/genome snapshot format and fixtures produced by NEAT-AI and consumed by downstream tools. |
| [NEAT-AI-scorer](https://github.com/stSoftwareAU/NEAT-AI-scorer) | Production forward-only scoring application built on `neat-core` via a path dependency. |
| [NEAT-AI-Explore](https://github.com/stSoftwareAU/NEAT-AI-Explore) | Visualiser for creatures that reads NEAT-AI-Snapshot data. |
| [NEAT-AI-Examples](https://github.com/stSoftwareAU/NEAT-AI-Examples) | Worked examples and tutorials that depend on NEAT-AI. |

### Dependency graph

```mermaid
graph TD
    Core[NEAT-AI-core<br/>Rust shared lib]
    Main[NEAT-AI<br/>Deno/TypeScript engine]
    Discovery[NEAT-AI-Discovery<br/>Rust, via Deno FFI]
    Snapshot[NEAT-AI-Snapshot<br/>creature data]
    Scorer[NEAT-AI-scorer<br/>Rust scorer app]
    Explore[NEAT-AI-Explore<br/>visualiser]
    Examples[NEAT-AI-Examples<br/>tutorials]

    Main -->|Deno FFI| Discovery
    Main -->|produces| Snapshot
    Scorer -->|path dependency| Core
    Explore -->|reads| Snapshot
    Examples -->|depends on| Main
```

## Propagation to downstream repositories

Once an enhancement merges to `Develop` here, it flows automatically to the
next pull request raised in either consumer repository — no manual SHA bump
is required. The two consumer paths differ in mechanism but share the same
Vibe Coder hook (`bump-deps.sh` runs on every PR; the CI `quality` job then
applies the same gates `quality.sh` runs locally).

### NEAT-AI (Deno + WASM consumer)

- On every push to `Develop`, [`.github/workflows/wasm-bundle.yml`](.github/workflows/wasm-bundle.yml)
  builds `wasm_activation-pkg.tar.gz` and publishes a per-commit GitHub
  Release tagged `wasm-bundle-<SHA>`. Each Release carries three assets: the
  tarball, its `wasm_activation-pkg.tar.gz.sha256` sidecar, and the CycloneDX
  SBOM `wasm_activation-pkg.cdx.json`.
- NEAT-AI's `bump-deps.sh` invokes `./build.sh`, which downloads the matching
  bundle, verifies it against the `.sha256` sidecar, and updates `deno.json`'s
  `neatCore.rev` field in lock-step. The sidecar is the per-revision hash
  anchor: it ships with the revision it describes, so a `neatCore.rev` bump
  has a trustworthy hash for the *new* bundle rather than only the pin
  recorded for the old one.
- A fresh PR in NEAT-AI is therefore sufficient to pick up the latest
  `Develop` of NEAT-AI-core.
- The published bundle (and its CycloneDX SBOM) carries a Sigstore-backed
  build-provenance attestation, so a consumer can cryptographically confirm
  the tarball was produced by this workflow from the pinned commit before
  trusting it. Verify in the bump flow with:

  ```bash
  gh attestation verify wasm_activation-pkg.tar.gz \
    --repo stSoftwareAU/NEAT-AI-core
  ```

  Pinning by SHA and the post-publish content re-verification prove the
  bundle's *shape*; the attestation proves its *origin*, closing the
  published-artefact-substitution attack class.

### NEAT-AI-scorer (Rust + path dependency)

- NEAT-AI-scorer's CI uses `actions/checkout` to clone
  `stSoftwareAU/NEAT-AI-core@Develop` into the workspace on every PR.
- `rust_scorer/Cargo.toml`'s `path = "../../NEAT-AI-core/neat-core"` resolves
  against that fresh clone, so the next PR build always compiles against the
  current tip of `Develop`.
- No SHA pin or release artefact is involved on this path.

### End-to-end flow

```mermaid
sequenceDiagram
    actor Dev as Maintainer
    participant Core as NEAT-AI-core/Develop
    participant CI as wasm-bundle.yml
    participant Rel as GitHub Release<br/>wasm-bundle-{SHA}
    participant Main as NEAT-AI PR
    participant Scorer as NEAT-AI-scorer PR

    Dev->>Core: merge enhancement
    Core->>CI: push to Develop
    CI->>CI: attest build provenance (Sigstore, keyless)
    CI->>Rel: build & publish wasm_activation-pkg.tar.gz + .sha256 + SBOM
    CI->>Rel: re-download & sha256sum -c (fails job on mismatch)
    Dev->>Main: open PR
    Main->>Rel: bump-deps.sh -> build.sh download
    Main->>Rel: verify tarball against .sha256 sidecar
    Main->>Rel: gh attestation verify (origin proof)
    Note over Main: deno.json neatCore.rev advances
    Dev->>Scorer: open PR
    Scorer->>Core: actions/checkout @Develop
    Note over Scorer: path dep resolves fresh clone
```

### Wiring reference

| Consumer | Trigger | Script / workflow |
|----------|---------|-------------------|
| NEAT-AI-core | push to `Develop` | [`.github/workflows/wasm-bundle.yml`](.github/workflows/wasm-bundle.yml) |
| NEAT-AI | PR opened (Vibe Coder hook) | `NEAT-AI/bump-deps.sh` → `NEAT-AI/build.sh` |
| NEAT-AI-scorer | PR opened (CI) | `NEAT-AI-scorer/.github/workflows/ci.yml` (`actions/checkout` of `NEAT-AI-core@Develop`) |

### Race window

`wasm-bundle.yml` typically takes ~30–60 seconds to build and publish the
release after a merge to `Develop`. PRs raised in NEAT-AI inside that
window may transiently fail the bundle download in `build.sh` because the
release tag for the latest `Develop` SHA does not yet exist. Re-run the
PR's checks once the bundle workflow has completed, or wait a minute
before opening the PR.

## Dependency updates: two channels

Two committed channels raise routine dependency-bump PRs. Both are weekly;
the quarantine-aware workflow is the authoritative one, because it is the only
channel that applies this project's release-age quarantine before proposing a
bump:

- **Routine bump (authoritative)** — [`.github/workflows/upgrade-dependencies.yml`](.github/workflows/upgrade-dependencies.yml)
  runs `bump-deps.sh` every Monday (`cron "0 6 * * 1"`), applying the
  `VIBE_BUMP_QUARANTINE_HOURS` release-age quarantine, `cargo audit`, and
  dual native/WASM builds before raising a general upgrade PR. The same script
  runs on every PR from the `ci.yml` `version-increment` job.
- **Dependabot version updates** — [`.github/dependabot.yml`](.github/dependabot.yml)
  configures a Cargo **version-updates** entry: `interval: weekly`, a 7-day
  `cooldown` (newly published crates are not proposed until they have aged),
  and `open-pull-requests-limit: 10`. It overlaps the workflow above rather
  than replacing it; its PRs go through the same CI gates.

Dependabot **security updates** — the advisory-triggered fast lane — are a
repository-level setting rather than anything the committed tree configures, so
this README cannot state whether the repository has it switched on. See GitHub's
[about Dependabot security updates](https://docs.github.com/en/code-security/dependabot/dependabot-security-updates/about-dependabot-security-updates)
for what that setting does and how to enable it.

Advisory *detection* is committed and verifiable: it lives in
[`security.yml`](.github/workflows/security.yml) and the `ci.yml` `security`
job (`cargo audit` / `rustsec/audit-check`), which fail the build on a
`Cargo.lock` crate with a known advisory.

When an actively-exploited advisory's fix is newer than the
`VIBE_BUMP_QUARANTINE_HOURS` window, an approver can take the documented
**emergency quarantine override**. The full procedure (both bypass levers, the
runbook, and the mandatory `cargo audit` re-check) lives in
[`SECURITY.md`](SECURITY.md#emergency-quarantine-override).

```mermaid
flowchart TD
    Adv[RustSec/OSV advisory disclosed] --> Detect[cargo audit detects<br/>security.yml / ci.yml]
    Detect -->|fails PR / scheduled job| Alert[Maintainer alerted]
    Alert --> FixPR[Advisory fix PR]
    Cron[Weekly cron Mon 06:00] --> Bump[upgrade-dependencies.yml<br/>bump-deps.sh + quarantine]
    Bump -->|authoritative refresh| GenPR[Weekly upgrade PR]
    DepCron[Dependabot weekly<br/>7-day cooldown] --> DepPR[Version-update PR]
    FixPR --> Develop[Develop]
    GenPR --> Develop
    DepPR --> Develop
```

## License

Apache-2.0 — see `LICENSE`.
