# Ikaruga/NEAT-AI vs `neat-core`: feature comparison and adoption candidates

**Research issue:** [#21](https://github.com/stSoftwareAU/NEAT-AI-core/issues/21) (part of #20)
**Upstream project:** [Ikaruga/NEAT-AI](https://github.com/Ikaruga/NEAT-AI) — pure-Rust NEAT for retro games (BizHawk + MAME), ~5,143 lines, **MIT-licensed**, WIP.
**This crate:** [`neat-core`](https://github.com/stSoftwareAU/NEAT-AI-core) — shared native Rust library for the NEAT-AI ecosystem, **Apache-2.0**.

## Licence compatibility

Ikaruga/NEAT-AI is **MIT**; `neat-core` is **Apache-2.0**. MIT is permissive and compatible with Apache-2.0 for **inbound** adaptation: code or ideas may be ported into `neat-core` provided the upstream copyright notice and MIT licence text are preserved for any verbatim transcriptions (e.g., in a `NOTICE` or header). Paraphrased re-implementations of published techniques (speciation, tournament selection, innovation numbers) do not require attribution but we will cite Ikaruga as the inspiration where we directly mirror its structure.

No code from Ikaruga/NEAT-AI has been transcribed into this repository as of the date of this research. Any future port that copies non-trivial fragments must carry the MIT notice.

## Scope reminder

`neat-core` is intentionally narrow: it provides the **compute kernels** (compiled forward pass, topological backprop, SIMD accumulation, loss functions, safe-zone clamping, streaming training-data I/O, topology validation, predictive coding). Evolutionary operators — mutation, crossover, speciation, the generation loop — live upstream in the **NEAT-AI** Deno/TypeScript repository, not here. The comparison below classifies each Ikaruga feature against that scope boundary.

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
| `creature` | `CreatureExport` / `NeuronExport` / `SynapseExport` — **`Deserialize` only** (no `Serialize`), plus `compile_creature` to `CompiledNetwork`. |
| `network` | `CompiledNetwork`, `NeuronData`, `SynapseData` — compiled forward-pass evaluator. |
| `topological_backprop` | Topologically ordered backprop loop (lifted from `wasm_activation` per #9). |
| `topology_ops` | Cycle detection, reverse topological order, structural validation, batch validation. |
| `accumulate`, `simd`, `simd_native` | 4-way and 8-way SIMD multi-record weighted-sum / bias accumulation; AVX2/FMA on x86_64 and NEON on aarch64 (#12). |
| `loss` | MSE/MAE/MAPE/MSLE/cross-entropy/hinge packed-batch reducers + `mse_mean_record`. |
| `pc_inference` / `pc_learning` | Predictive-coding inference engine and learning rule. |
| `training_bin_stream` | Chunked double-buffered `.bin` scan API with env-tunable modes (#13). |
| `training_data` | `.bin` reader / iterator / seeking record reader. |
| `training_state` | Persistent per-neuron / per-synapse state for online training. |
| `safe_zone` | Range-clamping for unbounded activations. |
| `score_scan`, `elastic_distribution`, `fused_error`, `error`, `derivative`, `range`, `unsquash` | Supporting numerics. |

Source footprint: ~15,100 LOC across `neat-core/src/`. No evolutionary operator code is present — by design.

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
- Predictive-coding inference (`PredictiveCodingEngine`) — novel vs Ikaruga.
- Aggregate activations (MIN/MAX/IF/HYPOT/MEAN) for conditional branching inside a network — not present in Ikaruga.

Ikaruga's `BatchEvaluator` imports `burn::tensor` but the batched path is still sequential `inputs.iter().map(…)`. Nothing to port.

### 6. Topology visualisation — 🎯 worth adopting (data-only export, not the GUI)

Ikaruga ships an 855-line `network_view.rs` built on `egui`/`eframe`, live-coupled to `Arc<RwLock<VisualizationState>>`. It is **not headless** and does **not export** DOT, JSON, or any other machine-readable graph format. `neat-core` has **no export path** today.

The useful capability to pull forward is **deterministic topology export** (DOT and/or topology JSON) from `CompiledNetwork`, so downstream tools (Graphviz, web viewers, snapshot diffs) can render networks without linking a GUI stack into `neat-core`. The live `egui` renderer itself is out of scope — any interactive viewer belongs in [NEAT-AI-Explore](https://github.com/stSoftwareAU/NEAT-AI-Explore) or a sibling tool.

- **Priority:** medium.
- **Effort:** small (1 PR, ~300 LOC + tests).
- **Tracked by:** existing open issue [#22 — Add CompiledNetwork topology export (DOT/JSON) for debugging and visualisation](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22). **No new issue needed.**

### 7. GPU acceleration (Burn + WGPU) — ⛔ out of scope

Ikaruga declares `burn = "0.16"` with WGPU, but `neural_net.rs` openly admits the GPU path is unimplemented (`"For now, we'll use the simple network evaluation"`). Even the aspirational use-case — batched forward-pass for a single population generation — does not fit `neat-core`'s constraints: this crate must build on `wasm32-unknown-unknown` for the NEAT-AI WASM path, and pulling Burn + WGPU would break that. The SIMD native path (#12) plus the chunked streaming reader (#13) already give `neat-core` a competitive CPU throughput story. GPU experimentation belongs in a separate crate or in the NEAT-AI-Discovery repository, not here.

No adoption. Record as **explicitly rejected** to avoid re-litigating.

### 8. Serialisation format — 🎯 worth adopting (round-trip JSON on `CreatureExport`)

Ikaruga derives both `Serialize` and `Deserialize` on its `Genome`, enabling full round-trip JSON persistence. `neat-core`'s `CreatureExport` currently derives **only** `Deserialize` — networks can be loaded but not written back out. For the topology-export work in #22, and for snapshot diffing, cache priming, and WASM bridging, a symmetric serialise path is worth adding. The JSON shape is fixed by the TypeScript `CreatureExport` contract, so there is no schema design required — only matching `Serialize` derives, `#[serde(rename = …)]` attributes, and a deterministic field ordering test.

- **Priority:** medium.
- **Effort:** small (~150 LOC + tests: round-trip, deterministic field order, numerical precision on f64 weights).
- **Issue:** new follow-up issue created per this research.

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
| 5 | Feed-forward evaluation | 4 activations, no SIMD, no backprop | 38 activations + backprop + SIMD + PC | ✅ already richer | — |
| 6 | Topology visualisation | 855-LOC egui GUI, no export | no export path | 🎯 adopt (data-only export) | [#22](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22) (existing) |
| 7 | GPU (Burn + WGPU) | declared, unused | — | ⛔ out of scope (breaks WASM) | — |
| 8 | Genome JSON round-trip | `Serialize` + `Deserialize` | `Deserialize` only | 🎯 adopt | [#30](https://github.com/stSoftwareAU/NEAT-AI-core/issues/30) |
| 9 | Emulator TCP + Lua bridge | 558 LOC + Lua scripts | — | ⛔ out of scope | — |
| 10 | Fitness scoring | 579 LOC per game | — | ⛔ out of scope | — |

## Adoption candidates (prioritised)

1. **CompiledNetwork topology export (DOT / topology JSON)** — medium priority, small effort. Already scoped in open issue **[#22](https://github.com/stSoftwareAU/NEAT-AI-core/issues/22)**. No new issue needed.
2. **`CreatureExport` round-trip JSON (`Serialize` on all three export types)** — medium priority, small effort. Tracked by follow-up issue **[#30](https://github.com/stSoftwareAU/NEAT-AI-core/issues/30)** raised alongside this document.

Everything else is either already covered more completely by `neat-core`, owned by the parent NEAT-AI repo, or deliberately outside this crate's scope.

## Attribution

This document summarises the public source of [Ikaruga/NEAT-AI](https://github.com/Ikaruga/NEAT-AI) (MIT, © contributors). No code was transcribed; file names, approximate line counts, dependency versions, and high-level designs were read from the upstream repository.
