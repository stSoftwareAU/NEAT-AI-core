# Ikaruga/NEAT-AI vs `neat-core`: feature comparison and adoption candidates

**Research issue:** [#21](https://github.com/stSoftwareAU/NEAT-AI-core/issues/21) (part of #20)
**Upstream project:** [Ikaruga/NEAT-AI](https://github.com/Ikaruga/NEAT-AI) — pure-Rust NEAT for retro games (BizHawk + MAME), ~5,143 lines, **MIT-licensed**, WIP.
**This crate:** [`neat-core`](https://github.com/stSoftwareAU/NEAT-AI-core) — shared native Rust library for the NEAT-AI ecosystem, **Apache-2.0**.

## Licence compatibility

Ikaruga/NEAT-AI is **MIT**; `neat-core` is **Apache-2.0**. MIT is permissive and compatible with Apache-2.0 for **inbound** adaptation: code or ideas may be ported into `neat-core` provided the upstream copyright notice and MIT licence text are preserved for any verbatim transcriptions (e.g., in a `NOTICE` or header). Paraphrased re-implementations of published techniques (speciation, tournament selection, innovation numbers) do not require attribution but we will cite Ikaruga as the inspiration where we directly mirror its structure.

No code from Ikaruga/NEAT-AI has been transcribed into this repository as of the date of this research. Any future port that copies non-trivial fragments must carry the MIT notice.

## Scope reminder

`neat-core` is intentionally narrow: it provides the **compute kernels** (compiled forward pass, topological backprop, SIMD accumulation, loss functions, safe-zone clamping, streaming training-data I/O, topology validation). At the time of this research it also carried a predictive-coding engine; that was **removed in Issue #414** as it had no caller. Evolutionary operators — mutation, crossover, speciation, the generation loop — live upstream in the **NEAT-AI** Deno/TypeScript repository, not here. The comparison below classifies each Ikaruga feature against that scope boundary.

## Upstream inventory (Ikaruga/NEAT-AI, `bizhawk-neat` variant)

| File | Approx. LOC | Purpose |
|------|------------:|---------|
| `src/neat/genome.rs` | 547 | `NodeGene`, `ConnectionGene`, `Genome`; 3 mutation operators; 4 activation functions; serde on all structs; global innovation counter. |
| `src/neat/species.rs` | 304 | Compatibility distance, adjusted fitness (`fitness / species_size`), stagnation counter with top-2 elitism. |
| `src/neat/population.rs` | 381 | Tournament selection, crossover + mutation paths, interspecies mating, offspring-per-species allocation. |
| `src/neat/config.rs` | — | `NeatConfig` with ~15 tuning knobs; JSON persistence (`from_file`/`to_file`). |
| `src/network/neural_net.rs` | 301 | Feed-forward via topological sort + cycle skip; `BatchEvaluator` present but sequential; Burn tensors imported but unused. |
| `src/visualization/network_view.rs` | 855 | Live `egui`/`eframe` renderer against `Arc<RwLock<VisualizationState>>`; no DOT/JSON export; not headless. |
| `src/emulator/connection.rs` | 558 | TCP bridge to Lua scripts running inside BizHawk/MAME. |
| `src/game/fitness.rs` | 579 | Per-game scoring. |
| `lua/bizhawk_bridge.lua` | — | In-emulator bridge script. |
| **Cargo deps of note** | | `burn = "0.16"` (wgpu), `egui`/`eframe = "0.29"`, `tokio = "1"`, `serde`, `rand`, `thiserror`, `tracing`. |

The `mame-neat` sibling crate mirrors this layout against MAME.

## `neat-core` inventory (relevant modules)

| Module | Role |
|--------|------|
| `squash` | **38 activation variants** (Identity, ReLU, ReLU6, LeakyReLU, SELU, ELU, Logistic, Tanh, HardTanh, Softsign, Softplus, Swish, Mish, GELU, Sine, Cosine, Tan, ArcTan, Gaussian, BentIdentity, BipolarSigmoid, Bipolar, Step, Complement, Absolute, Square, Cube, Sqrt, StdInverse, Exponential, LogSigmoid, ISRU, Minimum, Maximum, If, Hypotenuse, HypotenuseV2, Mean). |
| `creature` | `CreatureExport` / `NeuronExport` / `SynapseExport` — derive **both `Deserialize` and `Serialize`** (round-trip landed, Issue #30), plus `compile_creature` to `CompiledNetwork`. |
| `network` | `CompiledNetwork`, `NeuronData`, `SynapseData` — compiled forward-pass evaluator. |
| `topological_backprop` | Topologically ordered backprop loop (lifted from `wasm_activation` per #9). |
| `topology_ops` | Cycle detection, reverse topological order, structural validation, batch validation. |
| `accumulate`, `simd`, `simd_native` | 4-way and 8-way SIMD multi-record weighted-sum / bias accumulation; AVX2/FMA on x86_64 and NEON on aarch64 (#12). |
| `loss` | MSE/MAE/MAPE/MSLE/cross-entropy/hinge packed-batch reducers + `mse_record` / `mse_mean_record` / `mse_mean_streaming`. |
| `training_bin_stream` | Chunked double-buffered `.bin` scan API with env-tunable modes (#13). |
| `training_data` | `.bin` reader / iterator / seeking record reader. |
| `training_state` | Persistent per-neuron / per-synapse state for online training. |
| `safe_zone` | Range-clamping for unbounded activations. |
| `score_scan`, `elastic_distribution`, `fused_error`, `error`, `derivative`, `range`, `unsquash` | Supporting numerics. |

The table lists the modules present in `neat-core/src/` today. `pc_inference` / `pc_learning` (the predictive-coding inference engine and learning rule) were in this inventory when the research was written and were **removed in Issue #414**; `wasm_dataset` was likewise **removed in Issue #415**.

Source footprint: **~26,200** LOC across `neat-core/src/`, measured on `Develop` at NEAT-AI#3832. `tests/scripts/research_docs_removed_modules.bats` fails loud once the tree drifts more than 10% from that figure — refresh the number here when it does. No evolutionary operator code is present — by design.

## Feature-by-feature comparison

Each row below covers one feature area. Classification legend:

- ✅ **already present** in `neat-core`
- 🌐 **owned by parent NEAT-AI** (Deno/TypeScript repo)
- ⛔ **out of scope** for `neat-core`
- 🎯 **worth adopting** (with priority + effort)

### 1. Genome representation and mutation operators — 🌐 parent repo

Ikaruga models genomes as `HashMap<NodeId, NodeGene>` + `HashMap<InnovationId, ConnectionGene>`, with a global innovation counter and three mutation operators (weight perturb/replace, add-connection with DFS cycle check, add-node by splitting a connection). `neat-core` contains no `Genome` type and no mutation operators — genome state lives in the TypeScript `Creature` type in the NEAT-AI repo, which is already richer (UUID-based neurons, condition/positive/negative synapse types, constant neurons, aggregate activations). `neat-core`'s responsibility ends at **compiling** a creature (via `compile_creature`) and **validating** its topology (`validate_structural_integrity`, `validate_topology`). Adopting Rust-side mutation here would duplicate the source of truth and is explicitly out of this crate's scope.

### 2. Speciation (distance, fitness sharing, stagnation) — 🌐 parent repo

Ikaruga implements compatibility distance with three coefficients (excess, disjoint, weight), fitness sharing via `adjusted_fitness = fitness / species_size`, and a stagnation counter with top-2 preservation. The parent NEAT-AI repo already provides speciation at the population level; `neat-core` does not see populations. Nothing to adopt here.

### 3. Population management, tournament selection, crossover, generation loop — 🌐 parent repo

Ikaruga's `population.rs` (381 LOC) runs the evolution loop including tournament selection, elitism, interspecies mating, and re-speciation. Again, this is parent-repo territory. `neat-core` does not hold population state.

### 4. Config struct with JSON persistence — 🌐 parent repo / partially ✅

Ikaruga's `NeatConfig` persists ~15 evolutionary knobs via serde JSON. `neat-core`'s only config-shaped input is `TrainingDataConfig` for the binary streaming reader, already present and tested. Evolutionary hyper-parameters belong in the parent repo, where the generation loop reads them.

### 5. Neural-network feed-forward evaluation — ✅ already present (and richer)

Ikaruga computes a topological order with DFS + cycle skip and iterates nodes sequentially applying one of **four** activation functions (Sigmoid, Tanh, ReLU, Linear). `neat-core` provides:

- `CompiledNetwork` with pre-compiled evaluation order and **38** activation variants — a strict superset.
- `topological_backprop` for training, which Ikaruga lacks entirely (Ikaruga is inference-only).
- SIMD 4-way / 8-way batch accumulation across records (AVX2/FMA + NEON).
- (At the time of research: predictive-coding inference, novel vs Ikaruga — **removed in Issue #414**, having gained no caller.)
- Aggregate activations (MIN/MAX/IF/HYPOT/MEAN) for conditional branching inside a network — not present in Ikaruga.

Ikaruga's `BatchEvaluator` imports `burn::tensor` but the batched path is still sequential `inputs.iter().map(…)`. Nothing to port.

### 6. Topology visualisation — ✅ adopted (data-only export, not the GUI)

Ikaruga ships an 855-line `network_view.rs` built on `egui`/`eframe`, live-coupled to `Arc<RwLock<VisualizationState>>`. It is **not headless** and does **not export** DOT, JSON, or any other machine-readable graph format. At the time of research `neat-core` had no export path.

The useful capability was **deterministic topology export** (DOT and/or topology JSON) from `CompiledNetwork`, so downstream tools (Graphviz, web viewers, snapshot diffs) can render networks without linking a GUI stack into `neat-core`. That has **landed**: `neat-core/src/topology_export.rs` provides `to_dot` and `to_topology_json` from `CompiledNetwork` (Issue #22), with tests in `neat-core/tests/topology_export.rs`. The live `egui` renderer itself remains out of scope — any interactive viewer belongs in [NEAT-AI-Explore](https://github.com/stSoftwareAU/NEAT-AI-Explore) or a sibling tool.

- **Priority:** medium.
- **Effort:** small (1 PR, ~300 LOC + tests).
- **Status:** **done** — shipped via issue [#22 — Add CompiledNetwork topology export (DOT/JSON) for debugging and visualisation](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22) (`neat-core/src/topology_export.rs`).

### 7. GPU acceleration (Burn + WGPU) — ⛔ out of scope

Ikaruga declares `burn = "0.16"` with WGPU, but `neural_net.rs` openly admits the GPU path is unimplemented (`"For now, we'll use the simple network evaluation"`). Even the aspirational use-case — batched forward-pass for a single population generation — does not fit `neat-core`'s constraints: this crate must build on `wasm32-unknown-unknown` for the NEAT-AI WASM path, and pulling Burn + WGPU would break that. The SIMD native path (#12) plus the chunked streaming reader (#13) already give `neat-core` a competitive CPU throughput story. GPU experimentation belongs in a separate crate or in the NEAT-AI-Discovery repository, not here.

No adoption. Record as **explicitly rejected** to avoid re-litigating.

### 8. Serialisation format — ✅ adopted (round-trip JSON on `CreatureExport`)

Ikaruga derives both `Serialize` and `Deserialize` on its `Genome`, enabling full round-trip JSON persistence. At the time of research `neat-core`'s `CreatureExport` derived **only** `Deserialize` — networks could be loaded but not written back out. For the topology-export work in #22, and for snapshot diffing, cache priming, and WASM bridging, a symmetric serialise path was worth adding. The JSON shape is fixed by the TypeScript `CreatureExport` contract, so there was no schema design required — only matching `Serialize` derives, `#[serde(rename = …)]` attributes, and a deterministic field ordering test.

That has **landed**: `CreatureExport`, `NeuronExport`, and `SynapseExport` (`neat-core/src/creature.rs`) now derive both `Deserialize` and `Serialize`, so networks round-trip out to JSON as well as in (Issue #30).

Since Issue #550 the round trip also enforces the **observation-width contract**: the top-level `input` (authoritative observation count — not derivable from `neurons`, which lists only non-input neurons) and `output` (target count) must both be `>= 1`. `parse_creature_json`, `compile_creature` and `creature_to_json` all reject `input < 1` / `output < 1` with the typed `CreatureError::InvalidInputCount` / `InvalidOutputCount`, and neither field carries a `#[serde(default)]`.

- **Priority:** medium.
- **Effort:** small (~150 LOC + tests: round-trip, deterministic field order, numerical precision on f64 weights).
- **Status:** **done** — shipped via issue [#30](https://github.com/stSoftwareAU/NEAT-AI-core/issues/30) (`neat-core/src/creature.rs`).

### 9. Emulator bridge / environment interface (TCP + Lua) — ⛔ out of scope

Ikaruga's `connection.rs` (558 LOC) plus two Lua scripts wire a BizHawk or MAME instance over TCP to feed screen pixels in and button presses out. This is entirely application-specific and has no place in a shared compute library. The parent NEAT-AI repo defines its own environment abstraction; game-specific drivers belong there or in an example crate.

No adoption.

### 10. Fitness evaluation — 🌐 parent repo / ⛔ out of scope for `neat-core`

Ikaruga's `fitness.rs` (579 LOC) encodes per-game scoring heuristics. Fitness is defined at the application layer, not in a compute crate.

## Summary table

| # | Feature area | Ikaruga | `neat-core` | Classification | Follow-up |
|---|---|---|---|---|---|
| 1 | Genome + 3 mutation ops | `genome.rs` 547 LOC | — | 🌐 parent repo | — |
| 2 | Speciation | `species.rs` 304 LOC | — | 🌐 parent repo | — |
| 3 | Population, tournament, crossover | `population.rs` 381 LOC | — | 🌐 parent repo | — |
| 4 | NeatConfig JSON | `config.rs` | `TrainingDataConfig` only | 🌐 parent repo | — |
| 5 | Feed-forward evaluation | 4 activations, no SIMD, no backprop | 38 activations + backprop + SIMD | ✅ already richer | — |
| 6 | Topology visualisation | 855-LOC egui GUI, no export | `topology_export` (DOT/JSON) | ✅ adopted (data-only export) | [#22](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22) (landed) |
| 7 | GPU (Burn + WGPU) | declared, unused | — | ⛔ out of scope (breaks WASM) | — |
| 8 | Genome JSON round-trip | `Serialize` + `Deserialize` | `Serialize` + `Deserialize` | ✅ adopted | [#30](https://github.com/stSoftwareAU/NEAT-AI-core/issues/30) (landed) |
| 9 | Emulator TCP + Lua bridge | 558 LOC + Lua scripts | — | ⛔ out of scope | — |
| 10 | Fitness scoring | 579 LOC per game | — | ⛔ out of scope | — |

## Adoption candidates (outcome)

Both candidates identified by this research have since **shipped** on `Develop`:

1. ✅ **CompiledNetwork topology export (DOT / topology JSON)** — **done**. Landed via issue **[#22](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22)** as `neat-core/src/topology_export.rs` (`to_dot`, `to_topology_json`), tested in `neat-core/tests/topology_export.rs`.
2. ✅ **`CreatureExport` round-trip JSON (`Serialize` on all three export types)** — **done**. Landed via issue **[#30](https://github.com/stSoftwareAU/NEAT-AI-core/issues/30)**; `CreatureExport` / `NeuronExport` / `SynapseExport` in `neat-core/src/creature.rs` now derive both `Serialize` and `Deserialize`.

Everything else is either already covered more completely by `neat-core`, owned by the parent NEAT-AI repo, or deliberately outside this crate's scope.

## Attribution

This document summarises the public source of [Ikaruga/NEAT-AI](https://github.com/Ikaruga/NEAT-AI) (MIT, © contributors). No code was transcribed; file names, approximate line counts, dependency versions, and high-level designs were read from the upstream repository.
