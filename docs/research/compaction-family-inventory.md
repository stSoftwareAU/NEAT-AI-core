# Compaction family source and dependency inventory (Issue #776)

This is a dated, reproducible source snapshot for the
[legacy compaction baseline](compaction-baseline.md), inspected on 2026-11-26.
It records dependencies and confirmed implementation boundaries across the 14
public family repositories. It is **not** the exhaustive test-to-rule matrix,
a claim of whole-family compatibility, or acceptance of #776. No pins or
production implementations are changed by this inventory.

## Frozen source revisions

Source links below resolve to these revisions, not moving branch heads. NEAT-AI
stays at the legacy capture revision even when newer commits exist.

| Repository | Inspected revision |
|---|---|
| [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI/tree/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab) | `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab` |
| [NEAT-AI-Backpropagation](https://github.com/stSoftwareAU/NEAT-AI-Backpropagation/tree/947cbb5412156e229ed2e0d34142aa18ff2f9760) | `947cbb5412156e229ed2e0d34142aa18ff2f9760` |
| [NEAT-AI-Discovery](https://github.com/stSoftwareAU/NEAT-AI-Discovery/tree/08f1f51357e10134e3da0e61d1f56354260429ee) | `08f1f51357e10134e3da0e61d1f56354260429ee` |
| [NEAT-AI-Examples](https://github.com/stSoftwareAU/NEAT-AI-Examples/tree/b8b2d87a43e54a7837baace267c10d8fde81f9a5) | `b8b2d87a43e54a7837baace267c10d8fde81f9a5` |
| [NEAT-AI-Explore](https://github.com/stSoftwareAU/NEAT-AI-Explore/tree/98376b4443b58c2e5922e24d7466f8f3943cdea6) | `98376b4443b58c2e5922e24d7466f8f3943cdea6` |
| [NEAT-AI-Forests](https://github.com/stSoftwareAU/NEAT-AI-Forests/tree/a41c1b71bc881b4a3e0dccaff5c66469c4619665) | `a41c1b71bc881b4a3e0dccaff5c66469c4619665` |
| [NEAT-AI-Lamarck](https://github.com/stSoftwareAU/NEAT-AI-Lamarck/tree/f8f8f78032fb32a7567638aaa0382b65c7db9931) | `f8f8f78032fb32a7567638aaa0382b65c7db9931` |
| [NEAT-AI-Ockham](https://github.com/stSoftwareAU/NEAT-AI-Ockham/tree/febc59d836bcb165e99aaa149a13f52bdcad431e) | `febc59d836bcb165e99aaa149a13f52bdcad431e` |
| [NEAT-AI-Predict](https://github.com/stSoftwareAU/NEAT-AI-Predict/tree/a973b7b7a8d7dd839e27ee7b27b908d20409d050) | `a973b7b7a8d7dd839e27ee7b27b908d20409d050` |
| [NEAT-AI-Rebase](https://github.com/stSoftwareAU/NEAT-AI-Rebase/tree/b2e1e983d2597647406c26acc5be43a8a3da2f40) | `b2e1e983d2597647406c26acc5be43a8a3da2f40` |
| [NEAT-AI-Refinery](https://github.com/stSoftwareAU/NEAT-AI-Refinery/tree/52e30c24bfcd253d6b2898512fd9f8d7fa4955fb) | `52e30c24bfcd253d6b2898512fd9f8d7fa4955fb` |
| [NEAT-AI-Snapshot](https://github.com/stSoftwareAU/NEAT-AI-Snapshot/tree/bf8eb5445d78812fecb1d6d4a721ce42fdc874d0) | `bf8eb5445d78812fecb1d6d4a721ce42fdc874d0` |
| [NEAT-AI-core](https://github.com/stSoftwareAU/NEAT-AI-core/tree/f055a8b7067c52e988154b96a55c52d820b4b317) | `f055a8b7067c52e988154b96a55c52d820b4b317` |
| [NEAT-AI-scorer](https://github.com/stSoftwareAU/NEAT-AI-scorer/tree/a164969ac4d96ff39eace7086ad22c0538d95705) | `a164969ac4d96ff39eace7086ad22c0538d95705` |

## Declared dependencies and lockfile resolutions

The native declarations below are in the indicated crate's `Cargo.toml`; the
resolved revision is the `neat-core` package source in the repository's root
`Cargo.lock`. A tag declaration alone is not resolution evidence. These pins
were read, not upgraded, and their compatibility with a future compactor has
not been tested. Links to manifests and locks use the snapshot above.

| Consumer | Manifest | Declared core tag | Locked core revision |
|---|---|---|---|
| NEAT-AI-Backpropagation | [backpropagation/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Backpropagation/blob/947cbb5412156e229ed2e0d34142aa18ff2f9760/backpropagation/Cargo.toml) | `v0.23.4` | [`e86c342e72e85172b2f9e557329aca3b15bf5229`](https://github.com/stSoftwareAU/NEAT-AI-Backpropagation/blob/947cbb5412156e229ed2e0d34142aa18ff2f9760/Cargo.lock) |
| NEAT-AI-Forests | [forests/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Forests/blob/a41c1b71bc881b4a3e0dccaff5c66469c4619665/forests/Cargo.toml) | `v0.23.7` | [`3759355634b1cabc8034cd76df5dbd44a867d18c`](https://github.com/stSoftwareAU/NEAT-AI-Forests/blob/a41c1b71bc881b4a3e0dccaff5c66469c4619665/Cargo.lock) |
| NEAT-AI-Lamarck | [lamarck/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Lamarck/blob/f8f8f78032fb32a7567638aaa0382b65c7db9931/lamarck/Cargo.toml) | `v0.23.7` | [`3759355634b1cabc8034cd76df5dbd44a867d18c`](https://github.com/stSoftwareAU/NEAT-AI-Lamarck/blob/f8f8f78032fb32a7567638aaa0382b65c7db9931/Cargo.lock) |
| NEAT-AI-Ockham | [ockham/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/ockham/Cargo.toml) | `v0.23.7` | [`3759355634b1cabc8034cd76df5dbd44a867d18c`](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/Cargo.lock) |
| NEAT-AI-Predict | [Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Predict/blob/a973b7b7a8d7dd839e27ee7b27b908d20409d050/Cargo.toml) | `v0.22.16` | [`58b8842066d9a1b640c4ba9551074f4432fb891f`](https://github.com/stSoftwareAU/NEAT-AI-Predict/blob/a973b7b7a8d7dd839e27ee7b27b908d20409d050/Cargo.lock) |
| NEAT-AI-Rebase | [rebase/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Rebase/blob/b2e1e983d2597647406c26acc5be43a8a3da2f40/rebase/Cargo.toml) | `v0.23.7` | [`3759355634b1cabc8034cd76df5dbd44a867d18c`](https://github.com/stSoftwareAU/NEAT-AI-Rebase/blob/b2e1e983d2597647406c26acc5be43a8a3da2f40/Cargo.lock) |
| NEAT-AI-scorer | [rust_scorer/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-scorer/blob/a164969ac4d96ff39eace7086ad22c0538d95705/rust_scorer/Cargo.toml) | `v0.23.7` | [`3759355634b1cabc8034cd76df5dbd44a867d18c`](https://github.com/stSoftwareAU/NEAT-AI-scorer/blob/a164969ac4d96ff39eace7086ad22c0538d95705/Cargo.lock) |

Predict additionally declares `version = "0.22"`. Forests also declares
`neat-ai-rebase` at `v0.1.13`; its lock resolves that package to
`b2e1e983d2597647406c26acc5be43a8a3da2f40`. Its one locked core source is
`v0.23.7`; do not independently repin the two crates without checking their
shared creature types.

| Consumer / boundary | Manifest evidence | Recorded resolution |
|---|---|---|
| NEAT-AI | [deno.json](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/deno.json) | `neatCore.ref = Develop`, revision `1d44889e405e4e0cfb553321b44cc39e7d771f5f`, memory model `wasm64`; see artifact check below |
| NEAT-AI-Examples | [deno.json](https://github.com/stSoftwareAU/NEAT-AI-Examples/blob/b8b2d87a43e54a7837baace267c10d8fde81f9a5/deno.json) | `@stsoftware/neat-ai@7.0.62`; root `deno.lock` resolves `7.0.62` |
| NEAT-AI-Predict | [parity/deno.json](https://github.com/stSoftwareAU/NEAT-AI-Predict/blob/a973b7b7a8d7dd839e27ee7b27b908d20409d050/parity/deno.json) | Parity oracle `@stsoftware/neat-ai@7.0.48`; `parity/deno.lock` resolves `7.0.48` |
| NEAT-AI-Refinery | [parity/deno.json](https://github.com/stSoftwareAU/NEAT-AI-Refinery/blob/52e30c24bfcd253d6b2898512fd9f8d7fa4955fb/parity/deno.json) | Parity oracle `@stsoftware/neat-ai@^7.0.16`; `parity/deno.lock` resolves `7.0.16` |
| NEAT-AI-core | [wasm-bench/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-core/blob/f055a8b7067c52e988154b96a55c52d820b4b317/wasm-bench/Cargo.toml) | Local path `../neat-core`; internal workspace packaging, no external family revision |
| NEAT-AI-Discovery | [Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Discovery/blob/08f1f51357e10134e3da0e61d1f56354260429ee/Cargo.toml) | No direct core dependency declared in this manifest |
| NEAT-AI-Refinery | [refinery/Cargo.toml](https://github.com/stSoftwareAU/NEAT-AI-Refinery/blob/52e30c24bfcd253d6b2898512fd9f8d7fa4955fb/refinery/Cargo.toml) | Corpus primitives; no direct core dependency declared in this manifest |
| NEAT-AI-Explore | [deno.json](https://github.com/stSoftwareAU/NEAT-AI-Explore/blob/98376b4443b58c2e5922e24d7466f8f3943cdea6/deno.json) | Browser visualisation/tooling imports; no NEAT-AI library import declared here |
| NEAT-AI-Snapshot | [README.md](https://github.com/stSoftwareAU/NEAT-AI-Snapshot/blob/bf8eb5445d78812fecb1d6d4a721ce42fdc874d0/README.md) | Published creature snapshot data, rather than a library dependency pin |

The JSR resolutions above were read from the named lockfiles. They do not
identify the JSR package's transitive core artifact revision; that requires a
separate package/artifact inspection. The absence of a direct dependency in
one manifest is not proof of no indirect consumer.

NEAT-AI's declared artifact SHA-256 is
`abc296af6cc505a20de84a81afef0df4b44a512aeaf7548be344bafeabbead7a`.
Running its existing `./build.sh --verify-only` with Deno available passed:
`wasm_activation/pkg` matched the declared core revision and the binary memory
model was `wasm64`. This verifies the frozen vendored baseline; it does not
verify a new release artifact or the consumers' future cutover.

## Confirmed callers and implementation boundaries

The table follows source paths and distinguishes graph rewriting from scoring,
candidate construction, serialization and visualisation. It is an inspected
subset, not a guarantee that every indirect caller has been found.

| Repository / source | Confirmed boundary and migration implication |
|---|---|
| NEAT-AI [src/Creature.ts](https://github.com/stSoftwareAU/NEAT-AI/blob/60d6b4e5d3af20afbaaeb3420d588ad673ab8dab/src/Creature.ts) | `compact` / `compactVariants` enter the legacy `src/compact/CompactCreature.ts` engine. Fine-tuning, synthetic-synapse removal and training use candidates; trace-dependent cleanup and evolution simplification remain separate entrypoints listed in the baseline. |
| NEAT-AI-Ockham [ockham/src/run.rs](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/ockham/src/run.rs) | The exact pre-pass calls `canonical::canonicalise`; `canonical.rs` owns fixed-point zero-edge and identity cleanup. Baseline scoring follows this pre-pass. |
| NEAT-AI-Ockham [ockham/src/collapse.rs](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/ockham/src/collapse.rs) | Identity rewrite candidate with typed-edge, aggregate, self-loop and cost guards; `sweep.rs` calls it. Uses `ablation::cleanup_cascade`, also used by approximate merge/ablation paths. The guards and caller acceptance must survive extraction. |
| NEAT-AI-Ockham [ockham/src/prune.rs](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/ockham/src/prune.rs) | Neuron, group and synapse prune requests already call core `prune_neuron` / `prune_synapse`. This existing delegation does not retire `canonical.rs`, `collapse.rs` or the ablation cleanup implementation. |
| NEAT-AI-Ockham [ockham/src/cascade.rs](https://github.com/stSoftwareAU/NEAT-AI-Ockham/blob/febc59d836bcb165e99aaa149a13f52bdcad431e/ockham/src/cascade.rs) | Estimates removal effects; not the graph rewrite implementation. Keep ranking/estimation separate from candidate construction. |
| NEAT-AI-Rebase [rebase/src/adapter.rs](https://github.com/stSoftwareAU/NEAT-AI-Rebase/blob/b2e1e983d2597647406c26acc5be43a8a3da2f40/rebase/src/adapter.rs) | The engine imports adapter `apply`; an `OckhamRemoval` dispatches to `ockham::apply`. This is a live replay path, not only a test helper. |
| NEAT-AI-Rebase [rebase/src/ockham.rs](https://github.com/stSoftwareAU/NEAT-AI-Rebase/blob/b2e1e983d2597647406c26acc5be43a8a3da2f40/rebase/src/ockham.rs) | Own identity collapse and mean ablation call own `cleanup_cascade`. Collapse rejects typed edges, aggregate targets and self-loops, clears memetic state, merges bypass edges, sorts and validates. Cleanup removes dead non-output neurons and folds inputless hidden constants, refusing aggregate/typed cases. Record its own contract before delegation. |
| NEAT-AI-Forests [forests/src/enhancements.rs](https://github.com/stSoftwareAU/NEAT-AI-Forests/blob/a41c1b71bc881b4a3e0dccaff5c66469c4619665/forests/src/enhancements.rs) | Uses Rebase `PatchLog` and patch conversion to publish accepted enhancements. The `engine::rebase` call in `run.rs` is inside its test module, not evidence that production Forests invokes Rebase identity cleanup. The dependency still creates a compile/format coupling. |
| NEAT-AI-Discovery [src/analysis/remove_neuron_constant_promotion.rs](https://github.com/stSoftwareAU/NEAT-AI-Discovery/blob/08f1f51357e10134e3da0e61d1f56354260429ee/src/analysis/remove_neuron_constant_promotion.rs) | Ranks structural/measured constant-neuron removal candidates. Structural flags and recorded-activation bias-fold acceptance are distinct. Candidate generation must not be labelled whole-creature exact compaction. |
| NEAT-AI-Discovery [src/analysis/candidate_compression/identity.rs](https://github.com/stSoftwareAU/NEAT-AI-Discovery/blob/08f1f51357e10134e3da0e61d1f56354260429ee/src/analysis/candidate_compression/identity.rs) | Groups proposed synapses into an added IDENTITY neuron and coordinated operations, subject to discounted gain. This constructs proposals; it is not removal of an existing identity neuron. |
| NEAT-AI-Examples [synthetic_synapse/synthetic_synapse_example.ts](https://github.com/stSoftwareAU/NEAT-AI-Examples/blob/b8b2d87a43e54a7837baace267c10d8fde81f9a5/synthetic_synapse/synthetic_synapse_example.ts) | `pruneCreature` disconnects only recorded synthetic edges whose absolute weight is below a threshold; the training example calls it. This is threshold pruning, not an algebraic equivalence guarantee. Its existing example tests cover membership and negative-threshold refusal. |
| NEAT-AI-Examples [stock_market/stock_market.ts](https://github.com/stSoftwareAU/NEAT-AI-Examples/blob/b8b2d87a43e54a7837baace267c10d8fde81f9a5/stock_market/stock_market.ts) | The seed factory delegates to `Creature.forDataset`; the MNIST factory does likewise. Their comments about constant-input pruning do not establish another local graph cleanup implementation. |
| NEAT-AI-Backpropagation [backpropagation/src/train.rs](https://github.com/stSoftwareAU/NEAT-AI-Backpropagation/blob/947cbb5412156e229ed2e0d34142aa18ff2f9760/backpropagation/src/train.rs) | `compact_best` is the result of `checked_json_pretty`: serialization of the trained incumbent, not evidence of topology compaction. |
| NEAT-AI-Lamarck [lamarck/src/tags.rs](https://github.com/stSoftwareAU/NEAT-AI-Lamarck/blob/f8f8f78032fb32a7567638aaa0382b65c7db9931/lamarck/src/tags.rs) | `serialize_creature_with_meta_compact` removes JSON whitespace while preserving values and metadata. Do not migrate it as a graph algorithm. |
| NEAT-AI-scorer [rust_scorer/src/stream_io.rs](https://github.com/stSoftwareAU/NEAT-AI-scorer/blob/a164969ac4d96ff39eace7086ad22c0538d95705/rust_scorer/src/stream_io.rs) | `compact_pending_if_needed` moves buffered input values; it is stream-buffer maintenance, not neuron removal. |
| NEAT-AI-Predict [src/engine.rs](https://github.com/stSoftwareAU/NEAT-AI-Predict/blob/a973b7b7a8d7dd839e27ee7b27b908d20409d050/src/engine.rs) | Parses/compiles creatures and activates rows via `CompiledNetwork::activate_into`. This is an inference dependency that needs compatibility evidence, not a demonstrated compaction caller. |
| NEAT-AI-Refinery [refinery/src/lib.rs](https://github.com/stSoftwareAU/NEAT-AI-Refinery/blob/52e30c24bfcd253d6b2898512fd9f8d7fa4955fb/refinery/src/lib.rs) | Defines fixed-width corpus reading, writing and transformations. Its TS parity dependency must be recorded separately from native graph rewriting. |
| NEAT-AI-Explore [docs/shared/aggregated_graph_model.js](https://github.com/stSoftwareAU/NEAT-AI-Explore/blob/98376b4443b58c2e5922e24d7466f8f3943cdea6/docs/shared/aggregated_graph_model.js) | Collapses observation families and low-impact nodes into a display model; `subgraph_model.js` compacts display layers. These are visualisation operations, not evidence of a creature rewrite passed to a scorer. |
| NEAT-AI-Snapshot [README.md](https://github.com/stSoftwareAU/NEAT-AI-Snapshot/blob/bf8eb5445d78812fecb1d6d4a721ce42fdc874d0/README.md) | Publishes `docs/snapshot.json.gz` for consumers such as Explore. Artifact-format acceptance is separate from implementing a compaction engine. |
| NEAT-AI-core [neat-core/src/lib.rs](https://github.com/stSoftwareAU/NEAT-AI-core/blob/f055a8b7067c52e988154b96a55c52d820b4b317/neat-core/src/lib.rs) | Exports existing pruning/cleanup APIs and JSON adapters. Reuse the documented pruning contracts; do not silently equate them with every legacy whole-creature policy. |

## Verification gaps and next acceptance work

The [registered downstream list](../../scripts/downstream-consumers.txt)
contains scorer, Backpropagation, Rebase, Forests, Ockham and Lamarck at this
snapshot. The candidate-core compile check overrides their pins for the gate;
a passing check is not a released-pin upgrade. Predict directly depends on
core but is absent from that six-repository registry. Track its compile and
inference acceptance explicitly before API changes; do not treat the six
registered builds as coverage of all seven direct native consumers.

Remaining #776 work includes:

- Finish the exhaustive original test-to-rule and caller-to-API matrix,
  including Discovery's candidate application boundary, TS trace/repair paths,
  metadata invalidation and indirect example callers. A lexical scan alone
  cannot discharge this requirement.
- Freeze outputs and refusal policies for the additional Rebase/Ockham paths,
  and classify threshold, trace-based and approximate transformations against
  their actual guards and scoring requirements.
- Resolve JSR package artifact provenance where it differs from the frozen TS
  checkout. Check release availability and compatibility in each consumer's
  eventual migration PR; do not infer either from a manifest or lockfile.
- Run required consumer suites and authoritative scorer comparisons, and
  publish reproducible performance/allocation/serialization baselines. This
  source audit did not run every family suite or any new benchmark.
- Keep private-consumer verification in its own environment. Public source
  inspection cannot prove private deployment or release acceptance.

The [baseline's remaining gates](compaction-baseline.md#outstanding-before-accepting-776)
still apply. #777/#778 wait for #776 acceptance; #779 and the final #775/#780
tracker acceptance retain their later consumer, release and TS retirement gates.
